import type { IntentDescriptor, IntentReader } from './intents'

export interface MultisigApprovalRequest {
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
  if (!Number.isSafeInteger(threshold) || threshold < 1 || threshold > 65535) throw new RangeError('threshold must be a positive u16')
  const signerId = hex(reader.decodeAddress(request.signerAddress))
  const unique = new Map(request.signatories.map(address => [hex(reader.decodeAddress!(address)), address]))
  if (unique.size !== request.signatories.length) throw new RangeError('duplicate multisig signatory')
  if (!unique.has(signerId)) throw new RangeError('signer is not a member of this multisig')
  if (threshold > unique.size) throw new RangeError('threshold exceeds signatory count')
  const sorted = [...unique].sort(([a], [b]) => a.localeCompare(b))
  const account = reader.multisigAccount(sorted.map(([, address]) => address), threshold)
  const hash = await reader.hashCall(request.call), callHash = hex(hash)
  if (hash.length !== 32) throw new RangeError('multisig call hash must be 32 bytes')
  if (request.expectedCallHash != null && request.expectedCallHash.toLowerCase() !== callHash) throw new Error('multisig call changed from expectedCallHash')
  const pending = threshold === 1 ? null : await reader.query('Multisig', 'Multisigs', [account.address, hash]) as { when?: { height: number; index: number }; approvals?: unknown } | null
  if (pending != null && pending.when == null) throw new Error('pending multisig entry is missing its timepoint')
  const timepoint = pending?.when ?? null
  const others = sorted.filter(([id]) => id !== signerId).map(([, address]) => address)
  const call: IntentDescriptor = threshold === 1
    ? { pallet: 'Multisig', call: 'as_multi_threshold_1', params: { other_signatories: others, call: request.call } }
    : { pallet: 'Multisig', call: 'as_multi', params: { threshold, other_signatories: others, maybe_timepoint: timepoint, call: request.call, max_weight: await reader.estimateWeight(request.call, request.signerAddress) } }
  const opening = threshold > 1 && pending == null
  const [base, factor, feeRao, accountState] = await Promise.all([
    opening ? reader.constant('Multisig', 'DepositBase') : 0n,
    opening ? reader.constant('Multisig', 'DepositFactor') : 0n,
    reader.estimateFee(call, request.signerAddress),
    reader.query('System', 'Account', [request.signerAddress]),
  ])
  const depositRao = BigInt(base as bigint) + BigInt(threshold) * BigInt(factor as bigint)
  const availableFreeRao = BigInt((accountState as { data?: { free?: bigint } } | null)?.data?.free ?? 0)
  const violations = availableFreeRao < depositRao + feeRao ? [`multisig approval needs ${depositRao + feeRao} rao for deposit and fee, has ${availableFreeRao}`] : []
  return Object.freeze({ address: account.address, callHash, call, opening, timepoint, depositRao, feeRao, availableFreeRao, violations: Object.freeze(violations) })
}
function hex(value: Uint8Array): string { return '0x' + Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('') }
