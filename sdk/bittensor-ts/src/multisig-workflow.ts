import { evaluatePolicy } from './policy'
import type { IntentDescriptor, IntentReader } from './intents'

export interface MultisigApprovalRequest {
  accountMode?: 'legacy' | 'safe'
  /** A fixed call; every signatory must approve these identical encoded arguments. */
  call: IntentDescriptor
  signatories: readonly string[]
  threshold: number
  signerAddress: string
  /** Refuse if metadata or call arguments no longer produce this expected hash. */
  expectedCallHash?: string
}
export interface MultisigApproval {
  readonly address: string
  readonly callHash: string
  readonly call: IntentDescriptor
  readonly opening: boolean
  readonly timepoint: { height: number; index: number } | null
  readonly depositRao: bigint
  readonly feeRao: bigint
  readonly availableFreeRao: bigint
  readonly violations: readonly string[]
}

/** Resolve pending state and the opening deposit against one pinned reader. */
export async function prepareMultisigApproval(reader: IntentReader, request: MultisigApprovalRequest): Promise<MultisigApproval> {
  if (!reader.decodeAddress || !reader.multisigAccount || !reader.hashCall || !reader.estimateWeight || !reader.estimateFee || !reader.constant) throw new Error('reader lacks multisig capabilities')
  const threshold = request.threshold
  const members = evaluatePolicy<{ indices: number[]; other_indices: number[] }>('transaction.multisig_members', {
    threshold, members: request.signatories.map(address => reader.decodeAddress!(address)), signer: reader.decodeAddress(request.signerAddress),
  })
  const sorted = members.indices.map(index => request.signatories[index])
  const mode = evaluatePolicy<'legacy' | 'safe'>('transaction.account_mode', { mode: request.accountMode ?? 'legacy' })
  const suffix = mode === 'safe' ? '_safe' : ''
  const account = reader.multisigAccount(sorted, threshold, mode === 'safe')
  const hash = await reader.hashCall(request.call), callHash = hex(hash)
  if (hash.length !== 32) throw new RangeError('multisig call hash must be 32 bytes')
  if (request.expectedCallHash != null && request.expectedCallHash.toLowerCase() !== callHash) throw new Error('multisig call changed from expectedCallHash')
  const pending = threshold === 1 ? null : await reader.query('Multisig', 'Multisigs', [account.address, hash]) as { when?: { height: number; index: number }; approvals?: unknown } | null
  if (pending != null && pending.when == null) throw new Error('pending multisig entry is missing its timepoint')
  const timepoint = pending?.when ?? null
  const others = members.other_indices.map(index => request.signatories[index])
  const call: IntentDescriptor = threshold === 1
    ? { pallet: 'Multisig', call: 'as_multi_threshold_1' + suffix, params: { other_signatories: others, call: request.call } }
    : { pallet: 'Multisig', call: 'as_multi' + suffix, params: { threshold, other_signatories: others, maybe_timepoint: timepoint, call: request.call, max_weight: await reader.estimateWeight(request.call, request.signerAddress) } }
  const opening = threshold > 1 && pending == null
  const [base, factor, feeRao, accountState] = await Promise.all([
    opening ? reader.constant('Multisig', 'DepositBase') : 0n,
    opening ? reader.constant('Multisig', 'DepositFactor') : 0n,
    reader.estimateFee(call, request.signerAddress),
    reader.query('System', 'Account', [request.signerAddress]),
  ])
  const depositRao = BigInt(evaluatePolicy<string>('transaction.multisig_deposit', { base, factor, threshold }))
  const availableFreeRao = BigInt((accountState as { data?: { free?: bigint } } | null)?.data?.free ?? 0)
  const funding = evaluatePolicy<{ affordable: boolean }>('transaction.multisig_funding', { free: availableFreeRao, deposit: depositRao, fee: feeRao })
  const violations = !funding.affordable ? [`multisig approval needs ${depositRao + feeRao} rao for deposit and fee, has ${availableFreeRao}`] : []
  return Object.freeze({ address: account.address, callHash, call, opening, timepoint, depositRao, feeRao, availableFreeRao, violations: Object.freeze(violations) })
}
function hex(value: Uint8Array): string { return '0x' + Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('') }
