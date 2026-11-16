import type { Intent, IntentContext, IntentDescriptor, PreparedIntent } from './intents'
import { ShieldedTrackingError } from './shielded-receipt'

/** One immutable chain view used throughout preparation and policy checks. */
export interface ProtectedProxySnapshot {
  readonly genesisHash: string
  readonly hash: string
  readonly block: number
  readonly specVersion: number
  readonly blockTime: number
  query(pallet: string, item: string, params?: unknown[]): Promise<any>
  queryMap(pallet: string, item: string, params?: unknown[]): Promise<Array<[unknown, any]>>
  constant(pallet: string, item: string): Promise<any>
  prepare(intent: Intent, context: IntentContext): Promise<PreparedIntent>
  compose(call: IntentDescriptor): Promise<Uint8Array>
  decodeCall(data: Uint8Array): unknown | Promise<unknown>
  hashCallData(data: Uint8Array): Uint8Array
  /** Quote a dummy-signed envelope at this hash, without unlocking or reserving a nonce. */
  fee(call: IntentDescriptor, payer: string): Promise<bigint>
}

export interface ProtectedProxySubmission {
  /** Account that authorizes this submission; it may itself be a multisig. */
  signerAddress: string
  proxyFor?: string
  forceProxyType?: string
  /** Human-readable context passed through to application submitters. */
  purpose?: string
}

export interface ProtectedProxyEvent {
  module_id: string
  event_id: string
  attributes: any
  extrinsic_idx?: number | null
}

export interface ProtectedProxyReceipt {
  status: string
  success: boolean | null
  blockHash?: string | null
  blockNumber?: number | null
  extrinsicHash?: string | null
  extrinsicIndex?: number | null
  events: readonly ProtectedProxyEvent[]
  message?: string
  /** Multisig approval is not evidence that the protected action executed. */
  awaitingApproval?: boolean
  shielded?: boolean
  innerExtrinsicHash?: string
  carrierExtrinsicHash?: string
  data?: Record<string, unknown>
}

/** Retain Shield reconciliation coordinates without attributing carrier inclusion to its inner call. */
export function shieldedUncertainReceipt(error: unknown): ProtectedProxyReceipt | undefined {
  if (!(error instanceof ShieldedTrackingError)) return undefined
  return {
    status: 'unknown', success: null, events: [], message: error.message,
    extrinsicHash: error.innerExtrinsicHash, shielded: true,
    innerExtrinsicHash: error.innerExtrinsicHash, carrierExtrinsicHash: error.carrierExtrinsicHash,
    data: { shielded: true, submission_uncertain: true, carrier_only: true,
      inner_extrinsic_hash: error.innerExtrinsicHash, carrier_extrinsic_hash: error.carrierExtrinsicHash,
      carrier_block_hash: error.carrierReceipt.blockHash, carrier_block_number: error.carrierReceipt.blockNumber },
  }
}

/** Applications may supply a multisig submitter without exposing any private keys. */
export interface ProtectedProxyBackend {
  snapshot(): Promise<ProtectedProxySnapshot>
  submit(intent: Intent, options: ProtectedProxySubmission): Promise<ProtectedProxyReceipt>
  accountId(address: string): Uint8Array
  ss58Encode(account: Uint8Array): string
  hash(data: Uint8Array): Uint8Array
  receipt?(extrinsicHash: string, blockHash?: string): Promise<ProtectedProxyReceipt | null>
  events?(block: number | string): Promise<readonly ProtectedProxyEvent[]>
  /** Decode an inclusion-block call using the runtime that executed that block. */
  extrinsicCall?(block: number | string, index: number): Promise<unknown | null>
  blockHash?(height: number): Promise<string>
}

export function sameProtectedAccount(backend: Pick<ProtectedProxyBackend, 'accountId'>, left: string, right: string): boolean {
  const a = backend.accountId(left), b = backend.accountId(right)
  return a.length === b.length && a.every((value, index) => value === b[index])
}

export function requireProtectedChain(actual: string, expected: string): void {
  if (!/^0x[0-9a-f]{64}$/i.test(actual) || actual.toLowerCase() !== expected.toLowerCase()) throw new Error('protected account belongs to another chain')
}
