import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
import { Balance, UnitMismatchError } from './balance'
import { chainErrorFromDispatch, type ChainErrorInfo, type ModuleErrorResolver } from './chain-errors'
import type { IntentContext, IntentDescriptor, IntentReader, IntentSpend } from './intents'
import { registeredRecipient } from './receiving'
import { eventName } from './receipt'

export const GAMMA_UNITS = BigInt(POLICY_CATALOG.financial.GAMMA_UNITS)
export const MAX_GAMMA_SPENDS = POLICY_CATALOG.financial.MAX_GAMMA_SPENDS
export const MAX_GAMMA_DEPOSITS = POLICY_CATALOG.financial.MAX_GAMMA_DEPOSITS
export const MAX_SERVICE_URL_BYTES = POLICY_CATALOG.financial.MAX_SERVICE_URL_BYTES
const U64_MAX = (1n << 64n) - 1n
const encoder = new TextEncoder()

export interface GammaSpendInput {
  readonly hotkey_ss58: string
  readonly amount_gamma: number | string
  readonly reference?: string | null
}

/** Gamma is an exact decimal credit amount, never a TAO/alpha Balance or raw bigint. */
export function gammaAmount(value: unknown, allowAll = false): string {
  if (value instanceof Balance) throw new UnitMismatchError('gamma amounts are plain numbers or decimal strings, not Balance')
  if (typeof value !== 'number' && typeof value !== 'string') throw new TypeError('gamma amount must be a decimal string or number')
  return evaluatePolicy('financial.gamma.amount', { value, allow_all: allowAll })
}
export function gammaUnits(value: number | string): bigint { return BigInt(evaluatePolicy<string>('financial.gamma.units', { value })) }
export function gammaFromUnits(value: bigint | number | string): string { return evaluatePolicy('financial.gamma.from_units', { value }) }
export function minimumGammaOut(quoted: bigint, tolerance: number): bigint { return BigInt(evaluatePolicy<string>('financial.gamma.minimum', { quoted, tolerance })) }
function u64(value: unknown, name: string): bigint {
  try { return BigInt(evaluatePolicy<string>('financial.gamma.u64', { value })) }
  catch (error) { if (error instanceof Error) error.message = `${name}: ${error.message}`; throw error }
}

export class GammaQuoteError extends Error {
  readonly chainError: ChainErrorInfo
  constructor(readonly dispatchError: unknown, resolveModule?: ModuleErrorResolver) {
    const error = chainErrorFromDispatch(dispatchError, resolveModule)
    super(error.name ? `${error.name}: ${error.message}` : error.message)
    this.name = 'GammaQuoteError'
    this.chainError = error
  }
}

/** Runtime quote results retain their dispatch errors instead of becoming zero quotes. */
export function decodeGammaQuote(value: unknown, resolveModule?: ModuleErrorResolver): readonly [bigint, bigint, bigint] {
  const result = value as { Ok?: unknown; Err?: unknown } | null
  if (result && Object.hasOwn(result, 'Err')) throw new GammaQuoteError(result.Err, resolveModule)
  return evaluatePolicy<string[]>('financial.gamma.quote', { value: result?.Ok }).map(BigInt) as [bigint, bigint, bigint]
}

export async function resolveGammaQuote(value: unknown, moduleError?: (pallet: number, error: number) => Promise<{ name: string; docs: readonly string[] }>): Promise<readonly [bigint, bigint, bigint]> {
  const failure = (value as any)?.Err?.Module
  if (failure && moduleError) {
    const raw = failure.error
    const index = typeof raw === 'string' && /^0x[0-9a-f]{2}/i.test(raw) ? parseInt(raw.slice(2, 4), 16) : Number(Array.isArray(raw) || raw instanceof Uint8Array ? raw[0] : raw)
    const pallet = Number(failure.index)
    if ([pallet, index].every(n => Number.isInteger(n) && n >= 0 && n <= 255)) {
      const resolved = await moduleError(pallet, index)
      return decodeGammaQuote(value, () => resolved)
    }
  }
  return decodeGammaQuote(value)
}

/** Actual mint/spend outcomes, including execution through proxy, multisig or shielding.
 * No outcomes are invented for submissions or approvals that have not dispatched.
 * The resolver must use the metadata that executed the inclusion block.
 */
export async function gammaReceiptData(events: readonly unknown[], moduleError?: (pallet: number, error: number) => Promise<{ name: string; docs: readonly string[] }>): Promise<Record<string, unknown>> {
  const data: Record<string, unknown> = {}, mints: Record<string, unknown>[] = [], spent: Record<string, unknown>[] = [], skipped: Record<string, unknown>[] = []
  for (const event of events) {
    const name = eventName(event)
    if (!['SubtensorModule.GammaMinted', 'SubtensorModule.GammaSpent', 'SubtensorModule.GammaSpendSkipped'].includes(name)) continue
    const row = event as any, attributes = row.attributes ?? row.event?.attributes
    if (attributes == null || typeof attributes !== 'object' || Array.isArray(attributes)) throw new TypeError('invalid Gamma event attributes')
    const netuid = Number(u64(attributes.netuid, 'Gamma event netuid'))
    if (netuid > 65535) throw new RangeError('Gamma event netuid exceeds u16')
    if (name === 'SubtensorModule.GammaMinted') {
      const minted = {
        hotkey_ss58: String(attributes.hotkey), spender_ss58: String(attributes.spender),
        daily_spend_cap_gamma: attributes.daily_spend_cap == null ? null : gammaFromUnits(attributes.daily_spend_cap),
        alpha_burned: Balance.fromRao(u64(attributes.alpha, 'alpha burned'), netuid).amountString,
        tao_value: Balance.fromRao(u64(attributes.value, 'TAO value')).amountString,
        gamma: gammaFromUnits(attributes.gamma),
      }
      data.minted ??= minted
      mints.push({ netuid, ...minted })
      continue
    }
    const rawReference = attributes.reference
    const item: Record<string, unknown> = {
      netuid, hotkey_ss58: String(attributes.hotkey), amount_gamma: gammaFromUnits(attributes.amount),
      reference: reference(rawReference instanceof Uint8Array || Array.isArray(rawReference) ? Array.from(rawReference as ArrayLike<number>, byte => byte.toString(16).padStart(2, '0')).join('') : rawReference),
    }
    if (name === 'SubtensorModule.GammaSpent') spent.push(item)
    else {
      // Receipt availability must not depend on a second metadata RPC succeeding.
      let error = chainErrorFromDispatch(attributes.reason)
      try { await resolveGammaQuote({ Err: attributes.reason }, moduleError) }
      catch (cause) { if (cause instanceof GammaQuoteError) error = cause.chainError }
      skipped.push({ ...item, reason: error.name ?? error.message })
    }
  }
  if (mints.length) data.mints = mints
  if (spent.length || skipped.length) Object.assign(data, { spent, skipped })
  return data
}

function reference(value: unknown): string { return evaluatePolicy('financial.gamma.reference', { value }) }

function address(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.trim()) throw new TypeError(`${field} must be an address`)
  return value
}

/** Validate fields not expressible by the portable JSON catalog before any RPC calls. */
export function normalizeGammaArguments(op: string, args: Record<string, any>): void {
  const normalized = evaluatePolicy<Record<string, any>>('financial.gamma.arguments', { op, args })
  if (op === 'spend_gamma') args.spends = normalized.spends
}

interface GammaPlan {
  call: IntentDescriptor
  spendRao: IntentSpend
  warnings: string[]
  violations: string[]
  preview: Record<string, unknown>
}
type Deposit = { coldkey: string; spender: string; balance: bigint | number | string; daily_spend_cap?: { cap: bigint | number | string } | null }
const cap = (deposit: Deposit): bigint | null => deposit.daily_spend_cap == null ? null : u64(deposit.daily_spend_cap.cap, 'daily spend cap')

/** Prepare credit operations against one selected block; runtime dispatch rechecks the terms. */
export async function prepareGammaIntent(op: string, args: Record<string, any>, reader: IntentReader, context: IntentContext): Promise<GammaPlan> {
  const block = await reader.blockHash?.() ?? await reader.finalizedHead?.()
  if (!block) throw new Error('Gamma preparation requires a block-pinned reader')
  const query = (item: string, params: unknown[] = []) => reader.query('SubtensorModule', item, params, block)
  const runtime = (method: string, params: unknown[]) => reader.runtimeCall('GammaRuntimeApi', method, params, block)
  const quote = async (method: string, amount: bigint) => resolveGammaQuote(await runtime(method, [args.netuid, amount]), reader.moduleError ? (p, e) => reader.moduleError!(p, e, block) : undefined)
  const own = (left: unknown, right: string): boolean => {
    if (left == null) return false
    if (!reader.decodeAddress) return String(left) === right
    const a = reader.decodeAddress(String(left)), b = reader.decodeAddress(right)
    return a.length === b.length && a.every((byte, i) => byte === b[i])
  }
  const deposit = async (hotkey: string): Promise<Deposit | null> => (await query('GammaDeposits', [args.netuid, hotkey])) as Deposit | null
  const plan: GammaPlan = { call: { pallet: 'SubtensorModule', call: op, params: {} }, spendRao: null, warnings: [], violations: [], preview: {} }
  const call = (params: Record<string, unknown>) => { plan.call = { pallet: 'SubtensorModule', call: op, params } }
  const ownDeposit = (value: Deposit | null, hotkey: string): void => {
    plan.violations.push(...evaluatePolicy<string[]>('financial.gamma.own_deposit', { hotkey, exists: value != null, owned: value != null && own(value.coldkey, context.coldkey) }))
  }
  const newDeposit = async (hotkey: string, closesOrigin = false): Promise<void> => {
    const exists = reader.storageExists ? await reader.storageExists('SubtensorModule', 'Owner', [hotkey], block) : undefined
    const owner = exists === false ? null : await query('Owner', [hotkey])
    const deposits = await query('ColdkeyGammaHotkeys', [args.netuid, context.coldkey])
    plan.violations.push(...evaluatePolicy<string[]>('financial.gamma.new_deposit', { hotkey, signer: context.coldkey, owner_exists: exists, owned: own(owner, context.coldkey), count: Array.isArray(deposits) ? deposits.length : null, closes_origin: closesOrigin }))
  }
  const namedSpender = async (): Promise<string | null> => {
    if (args.service_hotkey_ss58 == null) return args.spender_ss58 ?? null
    const service = await query('Services', [args.netuid, args.service_hotkey_ss58]) as { spender: string } | null
    if (!service) throw new Error(`${args.service_hotkey_ss58} publishes no service on netuid ${args.netuid}`)
    return service.spender
  }
  if (op === 'mint_gamma' || op === 'mint_gamma_with_stake') {
    let burned: bigint, sent: bigint
    if (op === 'mint_gamma') {
      if (!(args.amount_tao instanceof Balance)) throw new TypeError('expected a normalized TAO amount')
      sent = burned = u64(args.amount_tao.rao, 'amount_tao')
      plan.spendRao = sent
    } else {
      const info = await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid', [args.origin_hotkey_ss58, context.coldkey, args.netuid], block) as { stake: unknown } | null
      const staked = u64(info?.stake ?? 0n, 'stake')
      const burn = evaluatePolicy<{ sent: string; burned: string; empty: boolean; limited: boolean; all: boolean }>('financial.gamma.burn', { stake: staked, amount: args.amount_alpha === 'all' ? 'all' : (args.amount_alpha as Balance).rao })
      if (burn.empty) throw new RangeError('no stake to burn into gamma')
      sent = BigInt(burn.sent); burned = BigInt(burn.burned)
      plan.spendRao = 'unbounded'
      if (burn.all) plan.warnings.push('burns the entire stake on this hotkey on this subnet')
      else if (burn.limited) plan.warnings.push(`only ${Balance.fromRao(staked, args.netuid)} is staked; that is burned`)
    }
    const [quoted, existing, spender] = await Promise.all([quote(op === 'mint_gamma' ? 'sim_mint_gamma' : 'sim_mint_gamma_with_stake', burned), deposit(args.hotkey_ss58), namedSpender()])
    const [alpha, value, gamma] = quoted
    const minimum = minimumGammaOut(gamma, args.rate_tolerance)
    let dailyCap = args.daily_spend_cap_gamma == null ? null : gammaUnits(args.daily_spend_cap_gamma)
    const mintFlags = evaluatePolicy<{ wrong_spender: boolean; wrong_cap: boolean; overflow: boolean }>('financial.gamma.mint_flags', { exists: existing != null, owned: existing != null && own(existing.coldkey, context.coldkey), spender_named: spender != null, same_spender: existing != null && spender != null && own(existing.spender, spender), requested_cap: dailyCap, existing_cap: existing ? cap(existing) : null, balance: existing?.balance ?? 0, minted: gamma })
    if (existing) {
      ownDeposit(existing, args.hotkey_ss58)
      if (mintFlags.wrong_spender) plan.violations.push(`the gamma deposit at ${args.hotkey_ss58} is spent by ${existing.spender}; set its spender first`)
      if (mintFlags.wrong_cap) plan.violations.push('the gamma deposit has another daily spend cap; set its cap first')
      if (mintFlags.overflow) plan.violations.push('the gamma deposit balance would exceed u64')
    } else await newDeposit(args.hotkey_ss58)
    const terms = { netuid: args.netuid, min_amount_out: minimum, spender, daily_spend_cap: dailyCap }
    call(op === 'mint_gamma' ? { hotkey: args.hotkey_ss58, amount: sent, ...terms } : { origin_hotkey: args.origin_hotkey_ss58, destination_hotkey: args.hotkey_ss58, alpha_amount: sent, ...terms })
    if (existing) dailyCap = cap(existing)
    plan.preview = { gamma: gammaFromUnits(gamma), minimum_gamma: gammaFromUnits(minimum), alpha_burned: Balance.fromRao(alpha, args.netuid).amountString, tao_value: Balance.fromRao(value).amountString, deposit_balance_gamma: existing ? gammaFromUnits(existing.balance) : null, spender_ss58: existing && spender == null ? existing.spender : spender, daily_spend_cap_gamma: dailyCap == null ? null : gammaFromUnits(dailyCap) }
  } else if (op === 'move_gamma') {
    if (own(args.origin_hotkey_ss58, args.destination_hotkey_ss58)) throw new RangeError('move_gamma: origin and destination hotkeys are the same')
    const [origin, destination] = await Promise.all([deposit(args.origin_hotkey_ss58), deposit(args.destination_hotkey_ss58)])
    const amount = args.amount_gamma === 'all' ? U64_MAX : gammaUnits(args.amount_gamma)
    ownDeposit(origin, args.origin_hotkey_ss58)
    const balance = origin ? u64(origin.balance, 'deposit balance') : 0n
    const flags = evaluatePolicy<{ moved: string; empty: boolean; closes: boolean; frees_slot: boolean; overflow: boolean; spender_changes: boolean; cap_changes: boolean }>('financial.gamma.move_flags', { amount, balance, origin_exists: origin != null, destination_exists: destination != null, destination_balance: destination?.balance ?? 0, same_spender: origin != null && destination != null && own(origin.spender, destination.spender), origin_cap: origin ? cap(origin) : null, destination_cap: destination ? cap(destination) : null })
    const moved = BigInt(flags.moved)
    if (flags.empty) plan.violations.push('the origin gamma deposit holds no gamma')
    if (flags.closes) plan.warnings.push('moves the entire balance, which closes the origin deposit')
    if (destination) {
      ownDeposit(destination, args.destination_hotkey_ss58)
      if (flags.overflow) plan.violations.push('the destination gamma deposit balance would exceed u64')
      if (flags.spender_changes) plan.warnings.push(`the moved gamma becomes spendable by ${destination.spender}, the destination deposit's spender, instead of ${origin!.spender}`)
      if (flags.cap_changes) plan.warnings.push('the moved gamma falls under the destination deposit daily spend cap instead of the origin cap')
    } else await newDeposit(args.destination_hotkey_ss58, flags.frees_slot)
    call({ netuid: args.netuid, origin_hotkey: args.origin_hotkey_ss58, destination_hotkey: args.destination_hotkey_ss58, amount })
    plan.spendRao = 'unbounded'
    plan.preview = { moved_gamma: gammaFromUnits(moved), closes_origin: flags.closes }
  } else if (['close_gamma_deposit', 'set_gamma_deposit_spender', 'set_gamma_deposit_daily_spend_cap'].includes(op)) {
    const existing = await deposit(args.hotkey_ss58)
    ownDeposit(existing, args.hotkey_ss58)
    const params: Record<string, unknown> = { netuid: args.netuid, hotkey: args.hotkey_ss58 }
    if (op === 'close_gamma_deposit' && evaluatePolicy<boolean>('financial.gamma.nonempty', { exists: existing != null, balance: existing?.balance ?? 0 })) plan.violations.push(`the deposit holds ${gammaFromUnits(existing!.balance)} gamma; move it out or wait for its spender before closing`)
    if (op === 'set_gamma_deposit_spender') { params.spender = await namedSpender(); plan.spendRao = 'unbounded' }
    if (op === 'set_gamma_deposit_daily_spend_cap') params.daily_spend_cap = args.remove_cap ? null : gammaUnits(args.daily_spend_cap_gamma)
    call(params)
  } else if (op === 'spend_gamma') {
    if (!await query('NetworksAdded', [args.netuid])) plan.violations.push(`subnet ${args.netuid} does not exist`)
    const spends = await Promise.all(args.spends.map(async (item: GammaSpendInput) => [await registeredRecipient(reader, item.hotkey_ss58), gammaUnits(item.amount_gamma), reference(item.reference)] as const))
    const accountId = (hotkey: string) => reader.decodeAddress ? Array.from(reader.decodeAddress(hotkey), byte => byte.toString(16).padStart(2, '0')).join('') : hotkey
    const balances = new Map<string, { balance: bigint; spendable: bigint; spender: string } | null>()
    const uniqueHotkeys = [...new Map(spends.map(([hotkey]) => [accountId(hotkey), hotkey])).values()]
    for (let i = 0; i < uniqueHotkeys.length; i += 16) await Promise.all(uniqueHotkeys.slice(i, i + 16).map(async hotkey => {
      const info = await runtime('get_gamma_deposit', [args.netuid, hotkey]) as { deposit: Deposit; spendable: unknown } | null
      balances.set(accountId(hotkey), info ? { balance: u64(info.deposit.balance, 'deposit balance'), spendable: u64(info.spendable, 'spendable gamma'), spender: info.deposit.spender } : null)
    }))
    const outcome = evaluatePolicy<{ total: string; skipped: Array<{ hotkey_ss58: string; amount_gamma: string; reference: string; reason: string }> }>('financial.gamma.spends', {
      deposits: Object.fromEntries([...balances].map(([key, value]) => [key, value ? { balance: value.balance, spendable: value.spendable, owned: own(value.spender, context.coldkey) } : null])),
      spends: spends.map(([hotkey, amount, reference]) => ({ hotkey, identity: accountId(hotkey), amount, reference })),
    })
    const skipped = outcome.skipped, total = BigInt(outcome.total)
    if (skipped.length) plan.warnings.push(`${skipped.length === spends.length ? 'nothing would be spent: ' : ''}${skipped.length} of ${spends.length} gamma spends would be skipped at the selected block`)
    call({ netuid: args.netuid, spends })
    // This is credit consumption, not a debit of the signer's TAO balance.
    plan.preview = { spent_gamma_units: total, skipped }
  } else if (op === 'set_service') {
    const hotkey = address(args.hotkey_ss58 ?? context.hotkey, 'hotkey_ss58')
    const [owner, exists] = await Promise.all([query('Owner', [hotkey]), query('NetworksAdded', [args.netuid])])
    let uid: unknown = null, subnetOwner: unknown, ownerHotkey: unknown, permits: unknown
    if (args.url) [uid, subnetOwner, ownerHotkey, permits] = await Promise.all([query('Uids', [args.netuid, hotkey]), query('SubnetOwner', [args.netuid]), query('SubnetOwnerHotkey', [args.netuid]), query('ValidatorPermit', [args.netuid])])
    plan.violations.push(...evaluatePolicy<string[]>('financial.gamma.service', { netuid: args.netuid, network: Boolean(exists), owned: own(owner, context.coldkey), publishing: Boolean(args.url), registered: uid != null, subnet_owner: own(subnetOwner, context.coldkey) && own(ownerHotkey, hotkey), permit: Array.isArray(permits) && permits[Number(uid)] === true }))
    call({ netuid: args.netuid, hotkey, url: encoder.encode(args.url), spender: args.spender_ss58 ?? context.coldkey })
  } else throw new RangeError(`unknown Gamma operation ${op}`)
  return plan
}
