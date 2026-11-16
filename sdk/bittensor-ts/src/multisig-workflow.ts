import { evaluatePolicy } from './policy'
import { canonicalJson } from './portable'
import { type Intent, type IntentContext, type PreparedIntent, type IntentDescriptor, type IntentReader } from './intents'

/** Retain a commitment until the caller has reconciled and explicitly released it. */
export class MultisigCallCache {
  private readonly calls = new Map<string, Promise<{ prepared: PreparedIntent; hash: string; specVersion?: number }>>()
  async prepare(reader: IntentReader, intent: Intent, context: IntentContext): Promise<PreparedIntent> {
    if (!reader.hashCall) throw new Error('reader lacks multisig call hashing')
    const key = intentKey(intent), savedPromise = this.calls.get(key)
    if (savedPromise) {
      const saved = await savedPromise
      if (saved.specVersion != null && await reader.specVersion?.() !== saved.specVersion) throw new Error('runtime changed since multisig approval; verify the original commitment under the new metadata')
      const hash = await reader.hashCall(saved.prepared.call)
      if (hex(hash) !== saved.hash) throw new Error('runtime metadata changed the prepared multisig call; verify the original commitment before approving')
      // Absence from a snapshot cannot distinguish an unfinished submission
      // from a completed operation. Neither absence nor cache pressure may
      // silently replace its commitment with a newly quoted call.
      return saved.prepared
    }
    const preparing = (async () => {
      const prepared = await intent.prepare(reader, context)
      return { prepared, hash: hex(await reader.hashCall!(prepared.call)), specVersion: await reader.specVersion?.() }
    })()
    this.calls.set(key, preparing)
    try {
      const { prepared } = await preparing
      // A refused preparation cannot have authorized an opening submission.
      // Retry its state checks instead of retaining stale failure messages.
      if (prepared.violations.length && this.calls.get(key) === preparing) this.calls.delete(key)
      return prepared
    }
    catch (error) { if (this.calls.get(key) === preparing) this.calls.delete(key); throw error }
  }

  /** Start a new operation only after the named commitment is reconciled. */
  async forget(intent: Intent, expectedCallHash: string): Promise<void> {
    const key = intentKey(intent), saved = this.calls.get(key)
    if (!saved) return
    if ((await saved).hash !== expectedCallHash.toLowerCase()) throw new Error('prepared multisig call does not match expectedCallHash')
    if (this.calls.get(key) === saved) this.calls.delete(key)
  }
}

function intentKey(intent: Intent): string {
  return canonicalJson(intent.toJSON())
}

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
