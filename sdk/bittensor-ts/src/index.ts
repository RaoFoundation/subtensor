import nativeBinding from './native'
import { fromWire, toBuffer, toWire, WIRE_TAG } from './wire'
import type { ByteLike, ScaleValue } from './types'
import { blake2_256 } from './crypto'
import { publicKeyFromSs58, ss58FromPublic, verify } from './keys'
import { createHttpAuth } from './http-auth'
import { createEvmAddressUtilities } from './evm'

export * from './crypto'
export * from './errors'
export * from './chain-errors'
export * from './root-claim'
export * from './endpoint-health'
export * from './basket-index'
export * from './shielded-receipt'
export * from './balance'
export * from './client'
export * from './keys'
export * from './protected-accounts'
export * from './protected-transactions'
export * from './limit-orders'
export * from './receiving'
export * from './composite-accounts'
export * from './registration-sponsorship'
export * from './evm-alias'
export * from './ledger'
export * from './modules'
export * from './runtime'
export * from './timelock'
export {
  Executor,
  IntentCall,
  NativeChainClient,
  Policy,
  RustWallet,
  SignerRole,
  isIntentCall,
  rawCall,
  signerRoleValue,
} from './transaction'
export type {
  PolicyOptions,
  RawCallOptions,
  SignerRoleLike,
  SignerRoleName,
} from './transaction'
export * from './types'
export * from './rust-bindings'
export * from './value'
export * from './wallet'
export * from './reads'
export * from './read-namespaces'
export * from './intents'
export { Metagraph, NeuronCommitment, fetchMetagraph, fetchCommitments, decodeCommitmentFields, commitmentRevealTime } from './metagraph'
export type { MetagraphNeuron, NeuronCommitmentData } from './metagraph'
export * from './node-workflows'
export * from './emission-weights'
export * from './fee-filters'
export * from './fee-payer'
export * from './pow'
export * from './pow-registration'
export * from './intent-weights'
export * from './external-signers'
export * from './http-auth'
export * from './evm'
export * from './evm-keystore'
export * from './extension-bridge'
export * from './preferences'
export * from './node-preferences'
export * from './history'
export * from './pruning'
export * from './node-pruning'
export * from './multisig-workflow'
export * from './upgrade'
export * from './node-upgrade'
export { Timelocked, TimelockError, TimelockNotReady, createTimelock, parseDuration, formatDuration, currentRound, roundAt } from './timelock-workflow'
export type { TimelockBackend, TimelockEncryptOptions, TimelockDecryptOptions } from './timelock-workflow'
export type { ExecutorOptions } from './transaction'
const portableCrypto = { blake2_256, publicKeyFromSs58, ss58FromPublic, verify }
export const httpAuth = createHttpAuth(portableCrypto)
export const evmAddresses = createEvmAddressUtilities(portableCrypto)
export { fromWire, toWire, WIRE_TAG }

/**
 * Direct access to the generated Node-API module. This is intentionally
 * exported so every native entry point remains callable even before an
 * ergonomic TypeScript convenience wrapper is added.
 */
export const native = Object.freeze(nativeBinding)

export const BINDING_VERSION = nativeBinding.bindingVersion()
export const __core_version__ = BINDING_VERSION
export const LEDGER_ENABLED = nativeBinding.ledgerEnabled()

export function wireRoundtrip(value: ScaleValue): ScaleValue {
  return fromWire(nativeBinding.wireRoundtrip(toWire(value)))
}

export function valueToCorpusJson(value: ScaleValue): unknown {
  return nativeBinding.valueToCorpusJson(toWire(value))
}

export function u256LeToDecimal(raw: ByteLike): string {
  return nativeBinding.u256LeToDecimal(toBuffer(raw, 'raw'))
}

export * from './unsigned-transaction'
