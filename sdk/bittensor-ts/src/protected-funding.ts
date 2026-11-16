import { Balance } from './balance'
import { Intent, type IntentDescriptor } from './intents'
import { unsignedInteger } from './portable'
import { controllerAnyDelays, protectedSetupPending, PROXY_TYPE_INTENT_OPS, type ProtectedAccountRecord, type ProtectedAnnouncementPolicyRow, type ProxyGrant } from './protected-account-config'
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
const max = (left: bigint, right: bigint) => left > right ? left : right

/** Reserve refundable deposits once, covering twenty transfers or ten staking operations. */
export function fundingBudget(input: ControllerFundingInput): ControllerFunding {
  const count = unsignedInteger(input.announcement_count, 32, 'announcement count')
  const deposit = amount(input.announcement_deposit, 'announcement deposit'), base = amount(input.deposit_base, 'announcement deposit base'), factor = amount(input.deposit_factor, 'announcement deposit factor')
  const ed = amount(input.existential_deposit, 'existential deposit'), announceFee = amount(input.announce_fee, 'announcement fee')
  const transferFee = announceFee + amount(input.transfer_execution_fee, 'transfer execution fee'), stakingFee = announceFee + amount(input.staking_execution_fee, 'staking execution fee')
  const reserve = (additional: bigint) => max(0n, base + factor * (count + additional) - deposit)
  const target = ed + max(reserve(20n) + 20n * transferFee, reserve(10n) + 10n * stakingFee)
  const warning = ed + max(reserve(5n) + 5n * transferFee, reserve(2n) + 2n * stakingFee)
  const free = amount(input.account.data.free, 'free balance'), frozen = amount(input.account.data.frozen ?? 0n, 'frozen balance'), available = max(0n, free - frozen)
  return Object.freeze({
    controller: input.controller, free_rao: free, frozen_rao: frozen, reserved_rao: amount(input.account.data.reserved, 'reserved balance'),
    announcement_deposit_rao: deposit, next_announcement_deposit_rao: reserve(1n), transfer_fee_rao: transferFee, staking_fee_rao: stakingFee,
    target_rao: target, warning_rao: warning, existential_deposit_rao: ed, fee_payer: input.fee_payer,
    signature_estimate: input.signature_estimate ?? 'standard sr25519 envelope; multisig members pay their own fees',
    available_rao: available, low: available < warning, top_up_rao: max(0n, target - available),
    can_announce: available >= reserve(1n) + transferFee + ed, estimate: true,
    fee_scope: 'controller; multisig signatories also need their own fees',
  })
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
const refillOperations = new Set(['transfer', 'transfer_all', 'transfer_stake', 'transfer_stake_and_hotkey', 'move_stake_limit', 'swap_stake_limit', 'move_swap_stake', 'batch', ...PROXY_TYPE_INTENT_OPS.Staking])
function pendingRefills(backend: Pick<ProtectedProxyBackend, 'accountId'>, controller: string, rows: readonly ProtectedAnnouncementPolicyRow[]): { rao: bigint; unbounded: boolean; unknown: boolean } {
  let rao = 0n, unbounded = false, unknown = false
  const visit = (spec: Record<string, any>, depth: number) => {
    if (depth > 64) throw new RangeError('protected intent nesting exceeds 64')
    if (spec.op === 'batch') for (const child of spec.intents ?? []) visit(child instanceof Intent ? child.toJSON() : child, depth + 1)
    else if (spec.op === 'execute_proxy_announced') visit({ ...spec.inner_args, op: spec.inner_op }, depth + 1)
    else if (['transfer', 'transfer_all'].includes(spec.op) && typeof spec.dest_ss58 === 'string' && sameProtectedAccount(backend, spec.dest_ss58, controller)) {
      if (spec.op === 'transfer_all' || spec.amount_tao === 'all') unbounded = true
      else {
        try { const value = spec.amount_tao instanceof Balance ? spec.amount_tao : Balance.fromTao(spec.amount_tao); if (value.netuid !== 0 || value.rao < 0n) throw new TypeError('invalid refill'); rao += value.rao }
        catch { unknown = true }
      }
    }
  }
  for (const row of rows) {
    if (row.status != null && !['pending', 'awaiting_approval', 'submitting', 'uncertain'].includes(row.status)) continue
    if (row.plaintext_missing || !row.inner_op) { unknown = true; continue }
    visit({ ...row.inner_args, op: row.inner_op }, 0)
  }
  return { rao, unbounded, unknown }
}
export function pendingControllerRefillRao(backend: Pick<ProtectedProxyBackend, 'accountId'>, controller: string, rows: readonly ProtectedAnnouncementPolicyRow[]): bigint { return pendingRefills(backend, controller, rows).rao }

/** Pure offer preparation; including the result is a separate explicit application decision. */
export function planControllerRefill(backend: Pick<ProtectedProxyBackend, 'accountId'>, record: ProtectedAccountRecord, intent: Intent, state: ControllerFunding, options: { pending?: readonly ProtectedAnnouncementPolicyRow[]; maxRefillRao?: bigint; enabled?: boolean } = {}): ControllerRefillPlan {
  if (!sameProtectedAccount(backend, record.controller_ss58, state.controller)) throw new Error('funding quote belongs to another controller')
  const result = (reason: ControllerRefillPlan['reason'], offered = false): ControllerRefillPlan => Object.freeze({ controller_ss58: record.controller_ss58, amount_rao: offered ? state.top_up_rao : 0n, offered, requires_delayed: offered, reason })
  if (options.enabled === false) return result('disabled')
  if (!refillOperations.has(intent.op)) return result('unsupported_operation')
  const projected = state.available_rao - state.next_announcement_deposit_rao - max(state.transfer_fee_rao, state.staking_fee_rao)
  if (state.top_up_rao === 0n || !state.low && projected >= state.warning_rao) return result('sufficient_funds')
  if (!state.can_announce) return result('external_funding_required')
  const pending = pendingRefills(backend, record.controller_ss58, options.pending ?? [])
  if (pending.unknown) return result('unknown_pending_payload')
  if (pending.rao > 0n || pending.unbounded) return result('already_pending')
  if (options.maxRefillRao != null && state.top_up_rao > amount(options.maxRefillRao, 'maximum controller refill')) throw new RangeError('controller refill exceeds maxRefillRao')
  return result('offered', true)
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
  if (children.some(child => child.op === 'batch')) throw new RangeError('controller refill requires a flat atomic batch')
  const staticFreeBalance = new Set(['add_stake', 'add_stake_limit', 'stake_into_basket', 'contribute_crowdloan'])
  for (const child of children) {
    const args = child instanceof Intent ? child.args : child
    if (staticFreeBalance.has(String(child.op)) && typeof args.amount_tao === 'string' && args.amount_tao.trim().toLowerCase() === 'all') throw new TypeError('controller refill cannot be combined with a static free-balance amount_tao: all; use an explicit amount or fund the controller separately')
  }
  const release = new Set(['remove_stake', 'remove_stake_limit', 'unstake_all', 'unstake_all_alpha', 'claim_root', 'claim_root_with_hotkey'])
  let lastRelease = -1
  children.forEach((child, index) => { if (release.has(String(child.op))) lastRelease = index })
  return new Intent('batch', { intents: [...children.slice(0, lastRelease + 1), refill, ...children.slice(lastRelease + 1)] })
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
