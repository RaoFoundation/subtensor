import { Balance, UnitMismatchError } from './balance'
import { chainErrorFromDispatch, type ChainErrorInfo, type ModuleErrorResolver } from './chain-errors'
import type { IntentContext, IntentDescriptor, IntentReader, IntentSpend } from './intents'
import { registeredRecipient } from './receiving'
import { eventName } from './receipt'

export const GAMMA_UNITS = 1_000_000_000n
export const MAX_GAMMA_SPENDS = 512
export const MAX_GAMMA_DEPOSITS = 64
export const MAX_SERVICE_URL_BYTES = 256
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
  if (typeof value === 'string' && value.trim().toLowerCase() === 'all') {
    if (allowAll) return 'all'
    throw new RangeError('gamma amount does not accept all')
  }
  const [numerator, denominator] = decimalFraction(value, 'gamma amount')
  const scaled = numerator * GAMMA_UNITS
  if (scaled % denominator !== 0n) throw new RangeError('gamma amount must have at most 9 decimals')
  return gammaFromUnits(u64(scaled / denominator, 'gamma amount'))
}

/** Convert a decimal gamma amount to its exact u64 chain units. */
export function gammaUnits(value: number | string): bigint {
  const normalized = gammaAmount(value)
  const [whole, fraction = ''] = normalized.split('.')
  return BigInt(whole) * GAMMA_UNITS + BigInt(fraction.padEnd(9, '0'))
}

/** Render chain gamma units as a decimal string without rounding. */
export function gammaFromUnits(value: bigint | number | string): string {
  const units = u64(value, 'gamma units')
  const tail = (units % GAMMA_UNITS).toString().padStart(9, '0').replace(/0+$/, '')
  return (units / GAMMA_UNITS).toString() + (tail ? `.${tail}` : '')
}

/** Floor the quoted gamma after the exact decimal slippage fraction. */
export function minimumGammaOut(quoted: bigint, tolerance: number): bigint {
  const [numerator, denominator] = decimalFraction(tolerance, 'rate_tolerance')
  if (numerator >= denominator) throw new RangeError('rate_tolerance must be in [0, 1)')
  return u64(quoted, 'quoted gamma') * (denominator - numerator) / denominator
}

function decimalFraction(value: unknown, name: string): [bigint, bigint] {
  if (typeof value !== 'string' && typeof value !== 'number') throw new TypeError(`${name} must be a decimal string or number`)
  if (typeof value === 'number' && !Number.isFinite(value)) throw new RangeError(`${name} must be finite`)
  const match = /^\+?(\d*)(?:\.(\d*))?(?:[eE]([+-]?\d+))?$/.exec(String(value).trim())
  if (!match || !(match[1].length || match[2]?.length)) throw new RangeError(`${name} must be a nonnegative finite decimal`)
  const exponent = Number(match[3] ?? 0)
  if (!Number.isSafeInteger(exponent) || Math.abs(exponent) > 400 || match[1].length + (match[2]?.length ?? 0) > 500) throw new RangeError(`${name} is out of range`)
  const fraction = match[2] ?? ''
  const coefficient = BigInt(match[1] + fraction)
  const scale = fraction.length - exponent
  return scale >= 0 ? [coefficient, 10n ** BigInt(scale)] : [coefficient * 10n ** BigInt(-scale), 1n]
}

function u64(value: unknown, name: string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError(`${name} requires an exact integer`)
  if (typeof value !== 'bigint' && typeof value !== 'number' && !(typeof value === 'string' && /^\d+$/.test(value))) throw new TypeError(`${name} must be an unsigned integer`)
  const integer = BigInt(value)
  if (integer < 0n || integer > U64_MAX) throw new RangeError(`${name} exceeds u64`)
  return integer
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
  if (!result || !Array.isArray(result.Ok) || result.Ok.length !== 3) throw new TypeError('invalid Gamma runtime quote')
  return result.Ok.map((item, i) => u64(item, ['alpha burned', 'TAO value', 'gamma'][i])) as [bigint, bigint, bigint]
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

function reference(value: unknown): string {
  if (value == null) return '0x' + '00'.repeat(32)
  if (typeof value !== 'string' || !/^(?:0x)?[0-9a-f]{64}$/i.test(value)) throw new RangeError('a gamma spend reference must be 32 bytes as hex')
  return '0x' + value.replace(/^0x/i, '').toLowerCase()
}
function address(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.trim()) throw new TypeError(`${field} must be an address`)
  return value
}

/** Validate fields not expressible by the portable JSON catalog before any RPC calls. */
export function normalizeGammaArguments(op: string, args: Record<string, any>): void {
  if (['mint_gamma', 'mint_gamma_with_stake'].includes(op) && args.spender_ss58 != null && args.service_hotkey_ss58 != null) throw new RangeError(`${op}: name a spender or a service, not both`)
  if (op === 'move_gamma') {
    if (args.origin_hotkey_ss58 === args.destination_hotkey_ss58) throw new RangeError('move_gamma: origin and destination hotkeys are the same')
    if (args.amount_gamma !== 'all' && gammaUnits(args.amount_gamma) === 0n) throw new RangeError('move_gamma: amount must be above 0')
  }
  if (op === 'set_gamma_deposit_spender' && Number(args.spender_ss58 != null) + Number(args.service_hotkey_ss58 != null) + Number(args.default_spender === true) !== 1) throw new RangeError('set_gamma_deposit_spender: set exactly one of spender_ss58, service_hotkey_ss58 or default_spender')
  if (op === 'set_gamma_deposit_daily_spend_cap' && Number(args.daily_spend_cap_gamma != null) + Number(args.remove_cap === true) !== 1) throw new RangeError('set_gamma_deposit_daily_spend_cap: set exactly one of daily_spend_cap_gamma or remove_cap')
  if (op === 'spend_gamma') {
    if (!Array.isArray(args.spends) || args.spends.length < 1 || args.spends.length > MAX_GAMMA_SPENDS) throw new RangeError(`spend_gamma takes 1 to ${MAX_GAMMA_SPENDS} spends`)
    args.spends = args.spends.map((item: any) => {
      if (!item || typeof item !== 'object' || Array.isArray(item) || !Object.hasOwn(item, 'hotkey_ss58') || !Object.hasOwn(item, 'amount_gamma') || Object.keys(item).some(key => !['hotkey_ss58', 'amount_gamma', 'reference'].includes(key))) throw new TypeError('spend_gamma entries require hotkey_ss58, amount_gamma and optional reference')
      return { hotkey_ss58: address(item.hotkey_ss58, 'hotkey_ss58'), amount_gamma: gammaAmount(item.amount_gamma), reference: reference(item.reference) }
    })
  }
  if (op === 'set_service') {
    if (typeof args.url !== 'string') throw new TypeError('service url must be a string')
    if (encoder.encode(args.url).length > MAX_SERVICE_URL_BYTES) throw new RangeError('service url exceeds 256 UTF-8 bytes')
    if (args.url) {
      let url: URL
      try { url = new URL(args.url) } catch { throw new RangeError('service url must be an http(s) URL') }
      if (!['http:', 'https:'].includes(url.protocol) || !url.hostname) throw new RangeError('service url must be an http(s) URL')
    }
  }
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
const min = (a: bigint, b: bigint): bigint => a < b ? a : b

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
    if (!value) plan.violations.push(`there is no gamma deposit at ${hotkey} on this subnet`)
    else if (!own(value.coldkey, context.coldkey)) plan.violations.push(`the gamma deposit at ${hotkey} belongs to another coldkey`)
  }
  const newDeposit = async (hotkey: string, closesOrigin = false): Promise<void> => {
    // Owner is ValueQuery: absence and an explicitly stored zero account decode
    // identically. Only a physical-key lookup can admit an unassociated hotkey.
    const exists = reader.storageExists ? await reader.storageExists('SubtensorModule', 'Owner', [hotkey], block) : undefined
    if (exists !== false && !own(await query('Owner', [hotkey]), context.coldkey)) {
      plan.violations.push(exists === undefined
        ? `cannot verify that ${hotkey} is unassociated or owned by ${context.coldkey}; use a reader with storageExists`
        : `the hotkey ${hotkey} belongs to another coldkey; use your own or an unassociated hotkey`)
    }
    const deposits = await query('ColdkeyGammaHotkeys', [args.netuid, context.coldkey])
    if (Array.isArray(deposits) && deposits.length - Number(closesOrigin) >= MAX_GAMMA_DEPOSITS) plan.violations.push(`at most ${MAX_GAMMA_DEPOSITS} gamma deposits per subnet are allowed`)
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
      if (!staked) throw new RangeError('no stake to burn into gamma')
      sent = args.amount_alpha === 'all' ? U64_MAX : u64((args.amount_alpha as Balance).rao, 'amount_alpha')
      burned = min(sent, staked)
      plan.spendRao = 'unbounded'
      if (args.amount_alpha === 'all') plan.warnings.push('burns the entire stake on this hotkey on this subnet')
      else if (sent > staked) plan.warnings.push(`only ${Balance.fromRao(staked, args.netuid)} is staked; that is burned`)
    }
    const [quoted, existing, spender] = await Promise.all([quote(op === 'mint_gamma' ? 'sim_mint_gamma' : 'sim_mint_gamma_with_stake', burned), deposit(args.hotkey_ss58), namedSpender()])
    const [alpha, value, gamma] = quoted
    const minimum = minimumGammaOut(gamma, args.rate_tolerance)
    let dailyCap = args.daily_spend_cap_gamma == null ? null : gammaUnits(args.daily_spend_cap_gamma)
    if (existing) {
      ownDeposit(existing, args.hotkey_ss58)
      if (spender != null && !own(existing.spender, spender)) plan.violations.push(`the gamma deposit at ${args.hotkey_ss58} is spent by ${existing.spender}; set its spender first`)
      if (dailyCap != null && dailyCap !== cap(existing)) plan.violations.push('the gamma deposit has another daily spend cap; set its cap first')
      if (u64(existing.balance, 'deposit balance') + gamma > U64_MAX) plan.violations.push('the gamma deposit balance would exceed u64')
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
    const moved = min(amount, balance)
    if (origin && balance === 0n) plan.violations.push('the origin gamma deposit holds no gamma')
    if (origin && moved === balance) plan.warnings.push('moves the entire balance, which closes the origin deposit')
    if (destination) {
      ownDeposit(destination, args.destination_hotkey_ss58)
      if (u64(destination.balance, 'deposit balance') + moved > U64_MAX) plan.violations.push('the destination gamma deposit balance would exceed u64')
      if (origin && !own(origin.spender, destination.spender)) plan.warnings.push(`the moved gamma becomes spendable by ${destination.spender}, the destination deposit's spender, instead of ${origin.spender}`)
      if (origin && cap(origin) !== cap(destination)) plan.warnings.push('the moved gamma falls under the destination deposit daily spend cap instead of the origin cap')
    } else await newDeposit(args.destination_hotkey_ss58, !!origin && moved === balance && balance > 0n)
    call({ netuid: args.netuid, origin_hotkey: args.origin_hotkey_ss58, destination_hotkey: args.destination_hotkey_ss58, amount })
    plan.spendRao = 'unbounded'
    plan.preview = { moved_gamma: gammaFromUnits(moved), closes_origin: !!origin && moved === balance }
  } else if (['close_gamma_deposit', 'set_gamma_deposit_spender', 'set_gamma_deposit_daily_spend_cap'].includes(op)) {
    const existing = await deposit(args.hotkey_ss58)
    ownDeposit(existing, args.hotkey_ss58)
    const params: Record<string, unknown> = { netuid: args.netuid, hotkey: args.hotkey_ss58 }
    if (op === 'close_gamma_deposit' && existing && u64(existing.balance, 'deposit balance') !== 0n) plan.violations.push(`the deposit holds ${gammaFromUnits(existing.balance)} gamma; move it out or wait for its spender before closing`)
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
    const skipped: Array<{ hotkey_ss58: string; amount_gamma: string; reference: string; reason: string }> = []
    let total = 0n
    for (const [hotkey, amount, reference] of spends) {
      const current = balances.get(accountId(hotkey))
      const reason = !current ? 'GammaDepositDoesNotExist' : !own(current.spender, context.coldkey) ? 'NotGammaDepositSpender' : amount === 0n ? 'AmountTooLow' : amount > current.balance ? 'InsufficientGammaBalance' : amount > current.spendable ? 'GammaDepositDailySpendCapExceeded' : null
      if (reason) skipped.push({ hotkey_ss58: hotkey, amount_gamma: gammaFromUnits(amount), reference, reason })
      else { current!.balance -= amount; current!.spendable -= amount; total += amount }
    }
    if (skipped.length) plan.warnings.push(`${skipped.length === spends.length ? 'nothing would be spent: ' : ''}${skipped.length} of ${spends.length} gamma spends would be skipped at the selected block`)
    call({ netuid: args.netuid, spends })
    // This is credit consumption, not a debit of the signer's TAO balance.
    plan.preview = { spent_gamma_units: total, skipped }
  } else if (op === 'set_service') {
    const hotkey = address(args.hotkey_ss58 ?? context.hotkey, 'hotkey_ss58')
    const [owner, exists] = await Promise.all([query('Owner', [hotkey]), query('NetworksAdded', [args.netuid])])
    if (!exists) plan.violations.push(`subnet ${args.netuid} does not exist`)
    if (!own(owner, context.coldkey)) plan.violations.push('the dispatch coldkey does not own the service hotkey')
    if (args.url) {
      const [uid, subnetOwner, ownerHotkey, permits] = await Promise.all([query('Uids', [args.netuid, hotkey]), query('SubnetOwner', [args.netuid]), query('SubnetOwnerHotkey', [args.netuid]), query('ValidatorPermit', [args.netuid])])
      if (uid == null) plan.violations.push('the service hotkey is not registered on this subnet')
      if (!(own(subnetOwner, context.coldkey) && own(ownerHotkey, hotkey)) && !(Array.isArray(permits) && permits[Number(uid)] === true)) plan.violations.push('the service hotkey needs a validator permit or subnet ownership')
    }
    call({ netuid: args.netuid, hotkey, url: encoder.encode(args.url), spender: args.spender_ss58 ?? context.coldkey })
  } else throw new RangeError(`unknown Gamma operation ${op}`)
  return plan
}
