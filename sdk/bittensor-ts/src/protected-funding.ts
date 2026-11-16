import { evaluatePolicy } from './policy'
import { Balance } from './balance'
import { Intent, type IntentDescriptor } from './intents'
import { unsignedInteger } from './portable'
import { controllerAnyDelays, protectedSetupPending, type ProtectedAccountRecord, type ProtectedAnnouncementPolicyRow, type ProxyGrant } from './protected-account-config'
import { requireProtectedChain, sameProtectedAccount, type ProtectedProxyBackend, type ProtectedProxySnapshot, type ProtectedProxyReceipt } from './protected-proxy-types'

export interface ControllerFunding {
  readonly controller: string
  readonly free_rao: bigint
  readonly frozen_rao: bigint
  readonly reserved_rao: bigint
  readonly announcement_deposit_rao: bigint
  readonly next_announcement_deposit_rao: bigint
  readonly transfer_fee_rao: bigint
  readonly staking_fee_rao: bigint
  readonly target_rao: bigint
  readonly warning_rao: bigint
  readonly existential_deposit_rao: bigint
  readonly fee_payer: string
  readonly signature_estimate: string
  readonly available_rao: bigint
  readonly low: boolean
  readonly top_up_rao: bigint
  readonly can_announce: boolean
  readonly estimate: true
  readonly fee_scope: string
  readonly block_hash?: string
  readonly genesis_hash?: string
}
export interface ControllerFundingInput {
  controller: string
  account: { data: { free: bigint | number | string; reserved: bigint | number | string; frozen?: bigint | number | string } }
  announcement_count: number
  announcement_deposit: bigint | number | string
  deposit_base: bigint | number | string
  deposit_factor: bigint | number | string
  existential_deposit: bigint | number | string
  announce_fee: bigint | number | string
  transfer_execution_fee: bigint | number | string
  staking_execution_fee: bigint | number | string
  fee_payer: string
  signature_estimate?: string
}
const amount = (value: bigint | number | string, name: string) => unsignedInteger(value, 128, name)
/** Reserve refundable deposits once, covering twenty transfers or ten staking operations. */
export function fundingBudget(input: ControllerFundingInput): ControllerFunding {
  const result = evaluatePolicy<Record<string, any>>('protected.funding', input)
  return Object.freeze(Object.fromEntries(Object.entries(result).map(([key, value]) => [key, key.endsWith('_rao') ? BigInt(value) : value]))) as unknown as ControllerFunding
}
function requireRecordChain(snapshot: ProtectedProxySnapshot, record: ProtectedAccountRecord): void {
  if (!record.genesis_hash) throw new Error('bind the legacy protected account to a verified chain before using it')
  requireProtectedChain(snapshot.genesisHash, record.genesis_hash)
}
const call = (pallet: string, name: string, params: Record<string, unknown>): IntentDescriptor => ({ pallet, call: name, params })

/** Every balance, reserve, constant and dummy-signed fee uses one pinned snapshot. */
export async function controllerFunding(backend: ProtectedProxyBackend, record: ProtectedAccountRecord, options: { snapshot?: ProtectedProxySnapshot; feePayer?: string } = {}): Promise<ControllerFunding> {
  const view = options.snapshot ?? await backend.snapshot()
  requireRecordChain(view, record)
  const controller = record.controller_ss58, pure = record.protected_ss58, payer = options.feePayer ?? controller
  backend.accountId(controller); backend.accountId(pure); backend.accountId(payer)
  const [account, announcements, base, factor, ed] = await Promise.all([
    view.query('System', 'Account', [controller]), view.query('Proxy', 'Announcements', [controller]),
    view.constant('Proxy', 'AnnouncementDepositBase'), view.constant('Proxy', 'AnnouncementDepositFactor'), view.constant('Balances', 'ExistentialDeposit'),
  ])
  if (!account?.data || !Array.isArray(announcements) || announcements.length !== 2 || !Array.isArray(announcements[0])) throw new TypeError('invalid controller funding state')
  const announce = call('Proxy', 'announce', { real: pure, call_hash: '0x' + '00'.repeat(32) })
  const execution = (inner: IntentDescriptor) => call('Proxy', 'proxy_announced', {
    delegate: controller, real: pure, force_proxy_type: 'Any',
    call: call('Utility', 'batch_all', { calls: [call('Balances', 'transfer_keep_alive', { dest: controller, value: 1_000_000_000n }), inner] }),
  })
  const [announceFee, transferFee, stakingFee] = await Promise.all([
    view.fee(announce, payer),
    view.fee(execution(call('Balances', 'transfer_keep_alive', { dest: controller, value: 1_000_000_000n })), payer),
    view.fee(execution(call('SubtensorModule', 'add_stake', { hotkey: controller, netuid: 0, amount_staked: 1_000_000_000n })), payer),
  ])
  return Object.freeze({ ...fundingBudget({
    controller, account, announcement_count: announcements[0].length, announcement_deposit: announcements[1], deposit_base: base, deposit_factor: factor,
    existential_deposit: ed, announce_fee: announceFee, transfer_execution_fee: transferFee, staking_execution_fee: stakingFee, fee_payer: payer,
    signature_estimate: 'pinned payer authorization estimate; multisig members pay their own fees',
  }), block_hash: view.hash, genesis_hash: view.genesisHash })
}

export interface ControllerRefillPlan {
  readonly controller_ss58: string
  readonly amount_rao: bigint
  readonly offered: boolean
  readonly requires_delayed: boolean
  readonly reason: 'offered' | 'disabled' | 'unsupported_operation' | 'sufficient_funds' | 'external_funding_required' | 'already_pending' | 'unknown_pending_payload'
}
function pendingRefills(backend: Pick<ProtectedProxyBackend, 'accountId'>, controller: string, rows: readonly ProtectedAnnouncementPolicyRow[]): { rao: bigint; unbounded: boolean; unknown: boolean } {
  // Address and Balance objects are language adapters; Rust traverses and decides.
  const adapt = (value: any, depth = 0): any => {
    if (depth > 64) throw new RangeError('protected intent nesting exceeds 64')
    if (value instanceof Intent) return adapt(value.toJSON(), depth)
    if (value instanceof Balance) return { rao: value.rao, netuid: value.netuid }
    if (Array.isArray(value)) return value.map(child => adapt(child, depth + 1))
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, key === 'dest_ss58' && typeof child === 'string' ? backend.accountId(child) : adapt(child, depth + 1)]))
    return value
  }
  const result = evaluatePolicy<{rao: string; unbounded: boolean; unknown: boolean}>('protected.pending_refills', { controller: backend.accountId(controller), rows: adapt(rows) })
  return { ...result, rao: BigInt(result.rao) }
}
export function pendingControllerRefillRao(backend: Pick<ProtectedProxyBackend, 'accountId'>, controller: string, rows: readonly ProtectedAnnouncementPolicyRow[]): bigint { return pendingRefills(backend, controller, rows).rao }

/** Pure offer preparation; including the result is a separate explicit application decision. */
export function planControllerRefill(backend: Pick<ProtectedProxyBackend, 'accountId'>, record: ProtectedAccountRecord, intent: Intent, state: ControllerFunding, options: { pending?: readonly ProtectedAnnouncementPolicyRow[]; maxRefillRao?: bigint; enabled?: boolean } = {}): ControllerRefillPlan {
  const result = evaluatePolicy<Omit<ControllerRefillPlan, 'amount_rao'> & {amount_rao: string}>('protected.refill_plan', { controller: backend.accountId(record.controller_ss58), quote_controller: backend.accountId(state.controller), controller_address: record.controller_ss58, state, op: intent.op, enabled: options.enabled, max_refill_rao: options.maxRefillRao, pending: pendingRefills(backend, record.controller_ss58, options.pending ?? []) })
  return Object.freeze({ ...result, amount_rao: BigInt(result.amount_rao) })
}
/**
 * Commit the exact offered refill in one flat atomic batch, after TAO-releasing actions.
 * Static free-balance "all" amounts resolve before the refill and would overcommit
 * funds; use explicit amounts or fund the controller separately. Runtime-computed
 * transfer_all still spends the balance remaining after the refill.
 */
export function includeControllerRefill(intent: Intent, offer: ControllerRefillPlan): Intent {
  if (!offer.offered) return intent
  const refill = new Intent('transfer', { dest_ss58: offer.controller_ss58, amount_tao: Balance.fromRao(amount(offer.amount_rao, 'controller refill')) })
  const children = intent.op === 'batch' ? [...intent.args.intents as Array<Intent | Record<string, unknown>>] : [intent]
  const position = evaluatePolicy<number>('protected.refill_position', { children: children.map(child => child instanceof Intent ? child.toJSON() : child) })
  return new Intent('batch', { intents: [...children.slice(0, position), refill, ...children.slice(position)] })
}

/** Fund from a separate account only after migration, with optional exact approval and ceiling. */
export async function fundController(backend: ProtectedProxyBackend, record: ProtectedAccountRecord, fromAddress: string, options: { maxFundingRao?: bigint; expectedFundingRao?: bigint } = {}): Promise<{ funding: ControllerFunding; amount_rao: bigint; receipt: ProtectedProxyReceipt | null }> {
  if (protectedSetupPending(record)) throw new Error('finish protected setup before funding the controller')
  if (sameProtectedAccount(backend, fromAddress, record.controller_ss58) || sameProtectedAccount(backend, fromAddress, record.protected_ss58)) throw new Error('choose a separate funding account; protected refills must use the delayed workflow')
  const view = await backend.snapshot(); requireRecordChain(view, record)
  const [proxyState, migration, dispute] = await Promise.all([
    view.query('Proxy', 'Proxies', [record.protected_ss58]), view.query('SubtensorModule', 'ColdkeySwapAnnouncements', [record.controller_ss58]), view.query('SubtensorModule', 'ColdkeySwapDisputes', [record.controller_ss58]),
  ])
  if (!Array.isArray(proxyState) || !Array.isArray(proxyState[0])) throw new TypeError('invalid protected proxy state')
  const delays = controllerAnyDelays(backend, proxyState[0] as ProxyGrant[], record.controller_ss58)
  if (!delays.length || Math.min(...delays) <= 0) throw new Error('controller has no live delayed Any grant on the protected account')
  if (migration != null || dispute != null) throw new Error('complete the coldkey migration before topping up its controller')
  const funding = await controllerFunding(backend, record, { snapshot: view }), value = funding.top_up_rao
  if (options.expectedFundingRao != null && value !== amount(options.expectedFundingRao, 'approved funding amount')) throw new Error('controller funding amount changed since approval')
  if (options.maxFundingRao != null && value > amount(options.maxFundingRao, 'maximum controller funding')) throw new RangeError('controller funding exceeds maxFundingRao')
  const receipt = value === 0n ? null : await backend.submit(new Intent('transfer', { dest_ss58: record.controller_ss58, amount_tao: Balance.fromRao(value) }), { signerAddress: fromAddress, purpose: 'fund protected-account controller' })
  return { funding, amount_rao: value, receipt }
}
