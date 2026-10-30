import { Balance, RAO_PER_TAO, UnitMismatchError } from './balance'
import { prepareOrderIntent } from './limit-orders'
import { prepareRecipients, registrationShieldDefault, type Recipient } from './receiving'
import { INTENT_CATALOG } from './intent-catalog'
import * as emission from './emission-weights'
import { prepareWeights, type EncryptWeightsV2Options } from './intent-weights'
import { prepareHyperparameter } from './intent-hyperparameters'
import { rootClaimChargedFeeRao, quoteRootClaimPreviews, type RootClaimPreview } from './root-claim'
import type { IntentArguments, IntentBuilders, IntentInputMap } from './intent-inputs'
export type { IntentInputMap, IntentJSON, IntentArguments, IntentBuilders, IntentAmountInput, BasketSwapLegInput, IntentTimepointInput } from './intent-inputs'

export type IntentOperation = typeof INTENT_CATALOG[number]['name']
export type IntentMoney = Balance | number | string
export type IntentSpend = bigint | null | 'unbounded'
export type IntentOrigin = 'signed' | 'subnet_owner' | 'root'
export interface IntentDescriptor {
  readonly pallet: string
  readonly call: string
  readonly params: Readonly<Record<string, unknown>>
  /** Exact semantic call required to verify a hash-only multisig wrapper. */
  readonly semanticCall?: IntentDescriptor
}
export interface IntentContext {
  accountMode?: 'legacy' | 'safe'
  /** Account that dispatches the semantic call (the proxy real or multisig when wrapped). */
  coldkey: string
  hotkey?: string
  hotkeyPublicKey?: Uint8Array
  /** Outer signer paying the extrinsic fee; defaults to the dispatch account. */
  feePayer?: string
}
export interface IntentReader {
  descriptorAddress?(descriptor: Uint8Array): string
  parseRecipient?(address: string, descriptor?: Uint8Array): Recipient
  supportsPqProfile?(profile: number): boolean | Promise<boolean>
  finalizedHead?(): Promise<string>
  composeCall?(call: IntentDescriptor): Promise<Uint8Array>
  query(pallet: string, item: string, params?: unknown[], block?: number | string | null): Promise<unknown>
  queryMap?(pallet: string, item: string, params?: unknown[], limit?: number): Promise<Array<[unknown, unknown]>>
  /** Count physical keys, including retired Alpha rows not present in runtime metadata. */
  countStorageKeys?(pallet: string, item: string, params: unknown[], limit: number): Promise<number>
  runtimeCall(api: string, method: string, params?: unknown[], block?: number | string | null): Promise<unknown>
  constant?(pallet: string, name: string): unknown | Promise<unknown>
  blockNumber?(): Promise<number>
  blockHash?(number?: number): Promise<string>
  /** Seconds per block, including fractional seconds on fast-block chains. */
  blockTime?(): Promise<number>
  estimateFee?(call: IntentDescriptor, signer: string): Promise<bigint>
  /** Resolve the actual fee account after any outer execution wrappers. */
  resolveFeePayer?(call: IntentDescriptor, signer: string): Promise<string>
  estimateWeight?(call: IntentDescriptor, signer: string): Promise<{ ref_time: bigint; proof_size: bigint }>
  decodeAddress?(address: string): Uint8Array
  encodeAddress?(publicKey: Uint8Array): string
  multisigAccount?(signatories: string[], threshold: number, safe?: boolean): { address: string; publicKey: Uint8Array }
  hashCall?(call: IntentDescriptor): Promise<Uint8Array>
  evmMirror?(h160: string): string
  encryptWeightsV2?(options: EncryptWeightsV2Options): Promise<{ commit: Uint8Array; revealRound: bigint }>
  /** Return a metadata-resolved owner hyperparameter call, preserving its paired fields. */
  hyperparameterCall?(netuid: number, name: string, value: unknown): Promise<IntentDescriptor>
}
export interface PreparedIntent {
  readonly op: string
  readonly call: IntentDescriptor
  readonly signerRole: 'coldkey' | 'hotkey'
  readonly dispatchOrigin: string
  readonly feePayer: string
  readonly origin: IntentOrigin
  readonly spendRao: IntentSpend
  /** Bounded TAO debits by actual dispatch account, including nested wrappers. */
  readonly spendByAccount: readonly { address: string; amountRao: bigint }[]
  readonly netuids: readonly number[]
  readonly affectsAllSubnets: boolean
  readonly global: boolean
  readonly mevShieldDefault: boolean
  readonly mevShieldRequired: boolean
  readonly effects: readonly string[]
  readonly warnings: readonly string[]
  readonly violations: readonly string[]
  readonly requiredFreeRao?: bigint
  readonly requiredFreeAccount?: string
  /** Independent reserve/fee headroom constraints for actual dispatch and fee accounts. */
  readonly requiredFreeByAccount: readonly { address: string; requiredRao: bigint; availableRao: bigint }[]
  readonly availableFreeRao?: bigint
  readonly estimatedFeeRao?: bigint
  readonly preview: Readonly<Record<string, unknown>>
  readonly extras: Readonly<Record<string, unknown>>
}
export interface IntentPolicy {
  maxFeeRao?: bigint | number | string | null
  maxSpendRao?: bigint | number | string | null
  allowedNetuids?: readonly number[] | null
  allowGlobal?: boolean | null
}
export interface CatalogEntry {
  name: string
  summary: string
  description: string
  signer: 'coldkey' | 'hotkey'
  origin: IntentOrigin
  verify: string | null
  wraps: readonly (readonly string[])[]
  docs_url: string
  input_schema: { properties: Readonly<Record<string, any>>; required?: readonly string[] }
  defaults: Readonly<Record<string, unknown>>
  money_fields: readonly string[]
  all_amount_fields: readonly string[]
  mev_shield_default: boolean
  mev_shield_required: boolean
}
freeze(INTENT_CATALOG)
const catalog = new Map<string, CatalogEntry>((INTENT_CATALOG as unknown as CatalogEntry[]).map(tool => [tool.name, tool]))
const preparedIntents = new WeakSet<object>()
const U64_MAX = (1n << 64n) - 1n
const encoder = new TextEncoder()
export const PROXY_TYPES = Object.freeze(['Any', 'Owner', 'NonCritical', 'NonTransfer', 'Senate', 'NonFungible', 'Triumvirate', 'Governance', 'Staking', 'Registration', 'Transfer', 'SmallTransfer', 'RootWeights', 'ChildKeys', 'SudoUncheckedSetCode', 'SwapHotkey', 'SubnetLeaseBeneficiary', 'RootClaim', 'BasketTrading', 'Validate', 'Weights', 'Lock'])

/** Machine-readable Python-compatible input schemas, defaults, safety notes and signer roles. */
export function listIntentTools(): readonly CatalogEntry[] { return INTENT_CATALOG as unknown as readonly CatalogEntry[] }
export function isPreparedIntent(value: unknown): value is PreparedIntent {
  return typeof value === 'object' && value !== null && preparedIntents.has(value)
}
export function checkIntentPolicy(intent: PreparedIntent, policy: IntentPolicy = {}, feeRao?: bigint): string[] {
  if (!isPreparedIntent(intent)) throw new TypeError('expected an SDK-prepared intent')
  const violations = [...intent.violations]
  if (policy.maxFeeRao != null) {
    const cap = unsigned(policy.maxFeeRao, 'maxFeeRao')
    if (feeRao == null) violations.push('fee estimate unavailable under maxFeeRao policy')
    else if (feeRao > cap) violations.push(`fee ${feeRao} exceeds maxFeeRao ${cap}`)
  }
  if (policy.maxSpendRao != null) {
    const cap = unsigned(policy.maxSpendRao, 'maxSpendRao')
    if (intent.spendRao === 'unbounded') violations.push('unbounded spend cannot satisfy maxSpendRao')
    else if ((intent.spendRao ?? 0n) > cap) violations.push(`spend ${intent.spendRao} exceeds maxSpendRao ${cap}`)
  }
  if (policy.allowedNetuids != null) {
    if (intent.affectsAllSubnets) violations.push('operation affects all subnets and cannot satisfy a subnet allowlist')
    if (intent.global && !policy.allowGlobal) violations.push('global operation requires allowGlobal with a subnet allowlist')
    for (const netuid of intent.netuids) if (!policy.allowedNetuids.includes(netuid)) violations.push(`netuid ${netuid} is outside allowedNetuids`)
  }
  return violations
}

/** A semantic operation with immutable arguments. Money fields use decimal TAO/alpha, as in Python. */
export class Intent {
  readonly op: IntentOperation
  readonly args: Readonly<Record<string, unknown>>
  constructor(op: IntentOperation, args: Record<string, unknown> = {}) {
    const tool = catalog.get(op)
    if (tool == null) throw new RangeError(`unknown intent ${op}`)
    const normalized = { ...copy(tool.defaults), ...copy(args) }
    delete normalized.op
    for (const key of Object.keys(normalized)) if (!(key in tool.input_schema.properties)) throw new TypeError(`${op}: unknown argument ${key}`)
    for (const key of tool.input_schema.required ?? []) if (normalized[key] == null) throw new TypeError(`${op}: missing ${key}`)
    for (const [key, value] of Object.entries(normalized)) {
      if (value == null) continue
      if (tool.money_fields.includes(key)) {
        normalized[key] = amount(value, moneyNetuid(op, key, normalized), tool.all_amount_fields.includes(key))
        continue
      }
      const type = tool.input_schema.properties[key]?.type
      if (type === 'integer') {
        const integer = unsigned(value, key)
        normalized[key] = typeof value === 'number' ? Number(integer) : integer
      }
      if (type === 'boolean' && typeof value !== 'boolean') throw new TypeError(`${key} must be boolean`)
      if (type === 'string' && typeof value !== 'string') throw new TypeError(`${key} must be a string`)
      if (type === 'array' && !Array.isArray(value) && !(key === 'weights' && typeof value === 'object')) throw new TypeError(`${key} must be an array`)
      if (type === 'number' && (typeof value !== 'number' || !Number.isFinite(value))) throw new TypeError(`${key} must be finite`)
    }
    for (const key of ['netuid', 'origin_netuid', 'dest_netuid']) if (normalized[key] != null) normalized[key] = netuid(normalized[key])
    if (normalized.rate_tolerance != null && !(Number(normalized.rate_tolerance) >= 0 && Number(normalized.rate_tolerance) < 1)) throw new RangeError('rate_tolerance must be in [0, 1)')
    this.op = op
    this.args = freeze(normalized)
    Object.freeze(this)
  }
  /** Round-trip JSON representation with exact decimal money strings. */
  toJSON(): Record<string, unknown> { return { op: this.op, ...jsonArgs(this.args) } }
  async prepare(reader: IntentReader, context: IntentContext): Promise<PreparedIntent> {
    if (!context.coldkey) throw new TypeError('intent preparation needs a dispatch coldkey address')
    const result = await prepare(this, reader, context)
    preparedIntents.add(result)
    return freeze(result)
  }
}
export function buildIntent<N extends keyof IntentInputMap>(op: N, ...args: IntentArguments<N>): Intent { return new Intent(op, args[0] as Record<string, unknown> | undefined) }
/** Autocompleting builders with required arguments and exact money/quantity inputs. */
export const intentBuilders: IntentBuilders = Object.freeze(Object.fromEntries([...catalog.keys()].map(op => [op, (args?: Record<string, unknown>) => new Intent(op as IntentOperation, args)]))) as unknown as IntentBuilders
export function intentSignerRole(intent: Intent): 'coldkey' | 'hotkey' {
  if (intent.op === 'batch') {
    const children = intent.args.intents as Array<Intent | Record<string, unknown>>
    if (!children?.length) throw new RangeError('batch requires at least one intent')
    const roles = new Set(children.map(child => intentSignerRole(child instanceof Intent ? child : intentFromJSON(child))))
    if (roles.size !== 1) throw new RangeError('all batch intents must share a signer')
    return roles.values().next().value!
  }
  if (intent.op === 'add_proxy' || intent.op === 'remove_proxy') return ['Validate', 'Weights'].includes(String(intent.args.proxy_type)) ? 'hotkey' : 'coldkey'
  if (intent.op === 'remove_proxies') return intent.args.from_hotkey ? 'hotkey' : 'coldkey'
  return catalog.get(intent.op)!.signer
}
export function intentFromJSON(spec: Record<string, unknown> | Intent): Intent {
  if (spec instanceof Intent) return spec
  const { op, ...args } = spec
  if (typeof op !== 'string') throw new TypeError('intent needs an op string')
  return new Intent(op as IntentOperation, args)
}
export function isIntentDescriptor(value: unknown): value is IntentDescriptor {
  const object = rec(value)
  return typeof object.pallet === 'string' && typeof object.call === 'string' && typeof object.params === 'object' && object.params != null
}
/** Recursively encode only nested descriptors; byte strings remain opaque SCALE values. */
export async function encodeIntentCall(call: IntentDescriptor, compose: (pallet: string, fn: string, params: Record<string, unknown>) => Promise<Uint8Array>): Promise<Uint8Array> {
  async function visit(value: unknown): Promise<unknown> {
    if (isIntentDescriptor(value)) return encodeIntentCall(value, compose)
    if (Array.isArray(value)) return Promise.all(value.map(visit))
    if (value != null && typeof value === 'object' && !(value instanceof Uint8Array)) return Object.fromEntries(await Promise.all(Object.entries(value).map(async ([key, child]) => [key, await visit(child)])))
    return value
  }
  return compose(call.pallet, call.call, await visit(call.params) as Record<string, unknown>)
}

type MutablePlan = { -readonly [K in keyof PreparedIntent]: PreparedIntent[K] }
async function prepare(intent: Intent, reader: IntentReader, context: IntentContext): Promise<MutablePlan> {
  const op = intent.op, tool = catalog.get(op)!
  const receiving = await prepareRecipients(op, intent.args, reader, context.coldkey), a = receiving.args
  const role = (op === 'add_proxy' || op === 'remove_proxy') && ['Validate', 'Weights'].includes(a.proxy_type)
    ? 'hotkey' : op === 'remove_proxies' ? a.from_hotkey ? 'hotkey' : 'coldkey' : tool.signer
  const plan: MutablePlan = {
    op, call: descriptor('', ''), signerRole: role, dispatchOrigin: role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey,
    feePayer: context.feePayer ?? (role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey), origin: tool.origin,
    spendRao: spendFor(op, a), spendByAccount: [], netuids: scopeFor(op, a), affectsAllSubnets: ['unstake_all', 'unstake_all_alpha', 'swap_coldkey_announced', 'finalize_crowdloan', 'claim_root', 'claim_root_with_hotkey', 'stake_into_basket'].includes(op) || (op === 'swap_hotkey' && a.netuid == null),
    global: globalOperation(op), mevShieldDefault: tool.mev_shield_default, mevShieldRequired: tool.mev_shield_required,
    effects: [tool.summary], warnings: [], violations: [], requiredFreeByAccount: [], preview: {}, extras: {},
  }
  const hotkey = () => {
    const address = a.hotkey_ss58 ?? context.hotkey ?? (role === 'hotkey' ? context.coldkey : undefined)
    if (typeof address !== 'string' || !address) throw new TypeError(`${op}: pass hotkey_ss58 or a context hotkey`)
    return address
  }
  const sub = (fn: string, params: Record<string, unknown> = {}) => descriptor('SubtensorModule', fn, params)
  switch (op) {
    case 'transfer': case 'transfer_all': {
      const all = op === 'transfer_all' || a.amount_tao === 'all'
      plan.call = descriptor('Balances', all ? 'transfer_all' : a.keep_alive ? 'transfer_keep_alive' : 'transfer_allow_death', all ? { dest: a.dest_ss58, keep_alive: a.keep_alive } : { dest: a.dest_ss58, value: rao(a.amount_tao) })
      break
    }
    case 'fund_evm_key': {
      if (await reader.constant?.('EVM','NativeDepositSupported') === true) {
        plan.call = descriptor('EVM','deposit',{address:h160(a.evm_address),value:a.amount_tao === 'all' ? null : rao(a.amount_tao)}); break
      }
      if (reader.supportsPqProfile && await reader.query('AdminUtils','HashedAccountsEnabled',[]) === true) throw new Error('legacy EVM mirror funding is unsafe with protected aliases; use native EVM.deposit or a descriptor-guarded transfer')
      const address = h160(a.evm_address), mirror = required(reader.evmMirror, 'evmMirror').call(reader, address)
      plan.call = descriptor('Balances', a.amount_tao === 'all' ? 'transfer_all' : 'transfer_keep_alive', a.amount_tao === 'all' ? { dest: mirror, keep_alive: true } : { dest: mirror, value: rao(a.amount_tao) })
      break
    }
    case 'evm_withdraw': {
      const publicKey = required(reader.decodeAddress, 'decodeAddress').call(reader, context.coldkey)
      const address = hex(publicKey.slice(0, 20))
      const value = a.amount_tao === 'all' ? await freeBalance(reader, required(reader.evmMirror, 'evmMirror').call(reader, address)) : rao(a.amount_tao)
      if (a.amount_tao === 'all' && value <= 0n) throw new RangeError('EVM deposit address is empty')
      plan.call = descriptor('EVM', 'withdraw', { address, value })
      break
    }
    case 'add_stake': case 'add_stake_limit': case 'stake_into_basket': {
      const amount = await freeAmount(a.amount_tao, reader, context)
      const params: Record<string, unknown> = { hotkey: hotkey(), amount_staked: amount }
      if (op !== 'stake_into_basket') params.netuid = a.netuid
      const limit = op === 'add_stake_limit' || (op === 'add_stake' && a.slippage_protection)
      if (limit) { params.limit_price = op === 'add_stake_limit' ? unsigned(a.limit_price_rao, 'limit_price_rao') : await priceLimit(reader, a.netuid, a.rate_tolerance, true); params.allow_partial = op === 'add_stake_limit' ? a.allow_partial : false }
      plan.call = sub(op === 'stake_into_basket' ? op : limit ? 'add_stake_limit' : 'add_stake', params)
      if (a.amount_tao === 'all') plan.warnings = ['uses the entire free balance minus the existential deposit and 500000 rao fee headroom']
      break
    }
    case 'remove_stake': case 'remove_stake_limit': {
      const claimAll = a.claim && a.netuid === 0 && a.amount_alpha === 'all'
      const current = await stakeAmount(a.amount_alpha, reader, context, hotkey(), a.netuid)
      const params: Record<string, unknown> = { hotkey: hotkey(), netuid: a.netuid, amount_unstaked: claimAll ? U64_MAX : current }
      const limit = !claimAll && (op === 'remove_stake_limit' || a.slippage_protection)
      if (limit) { params.limit_price = op === 'remove_stake_limit' ? unsigned(a.limit_price_rao, 'limit_price_rao') : await priceLimit(reader, a.netuid, a.rate_tolerance, false); params.allow_partial = op === 'remove_stake_limit' ? a.allow_partial : false }
      plan.call = sub(limit ? 'remove_stake_limit' : 'remove_stake', params)
      await optionalClaim(plan, a, reader, context, hotkey(), a.netuid)
      break
    }
    case 'move_stake': case 'move_swap_stake': case 'swap_stake': case 'transfer_stake': {
      const originHotkey = a.origin_hotkey_ss58 ?? hotkey()
      if (a.claim && a.origin_netuid !== 0) throw new RangeError('claim is only valid when moving from root')
      const amount = a.claim && a.amount_alpha === 'all' ? U64_MAX : await stakeAmount(a.amount_alpha, reader, context, originHotkey, a.origin_netuid)
      const params: Record<string, unknown> = { origin_netuid: a.origin_netuid, destination_netuid: a.dest_netuid, alpha_amount: amount }
      if (op === 'transfer_stake') {
        params.destination_coldkey = a.dest_coldkey_ss58
        if (a.dest_hotkey_ss58 && a.dest_hotkey_ss58 !== hotkey()) { params.origin_hotkey = hotkey(); params.destination_hotkey = a.dest_hotkey_ss58 }
        else params.hotkey = hotkey()
        plan.call = sub(params.origin_hotkey ? 'transfer_stake_and_hotkey' : 'transfer_stake', params)
        plan.warnings = ['transfers stake ownership irreversibly to another coldkey; locked alpha must land on the receiver lock hotkey']
      } else if (op === 'move_swap_stake') {
        if (originHotkey === a.dest_hotkey_ss58 || a.origin_netuid === a.dest_netuid) throw new RangeError('move_swap_stake requires different hotkeys and subnets')
        if (amount <= 1n) throw new RangeError('move_swap_stake needs more than 1 rao to cover share-pool rounding')
        const move = sub('move_stake', { ...params, destination_netuid: a.origin_netuid, origin_hotkey: originHotkey, destination_hotkey: a.dest_hotkey_ss58 })
        const swap = await swapCall(reader, a, a.dest_hotkey_ss58, amount - 1n)
        plan.call = batchCall([move, swap])
        plan.warnings = ['the swap uses amount minus 1 rao to accommodate share-pool rounding; 1 rao remains on the destination hotkey at the origin subnet']
      } else if (op === 'swap_stake') plan.call = await swapCall(reader, a, hotkey(), amount)
      else {
        params.origin_hotkey = originHotkey; params.destination_hotkey = a.dest_hotkey_ss58
        const limit = a.slippage_protection && a.origin_netuid !== a.dest_netuid
        if (limit) { params.limit_price = await ratioLimit(reader, a.origin_netuid, a.dest_netuid, a.rate_tolerance); params.allow_partial = false }
        plan.call = sub(limit ? 'move_stake_limit' : 'move_stake', params)
        await optionalClaim(plan, a, reader, context, originHotkey, a.origin_netuid)
      }
      break
    }
    case 'unstake_all': case 'unstake_all_alpha':
      plan.call = sub(op, { hotkey: hotkey() })
      plan.warnings = ['affects every subnet position on this hotkey']
      await optionalClaim(plan, a, reader, context, hotkey(), 0)
      break
    case 'swap_basket': {
      if (a.origin_netuid === a.dest_netuid) throw new RangeError('basket origin and destination must differ')
      let value: bigint
      if (a.amount === 'all') {
        const rows = list(await reader.runtimeCall('BetaBasketRuntimeApi', 'get_validator_basket', [hotkey()]))
        const row = rows.find(row => Array.isArray(row) && Number(row[0]) === a.origin_netuid) as unknown[] | undefined
        value = unsigned(row?.[1] ?? 0, 'basket holding')
        if (!value) throw new RangeError('basket has no origin holding to sell')
      } else value = rao(a.amount)
      plan.call = sub(op, { hotkey: hotkey(), origin_netuid: a.origin_netuid, destination_netuid: a.dest_netuid, amount: value, min_amount_out: rao(a.min_amount_out) })
      break
    }
    case 'swap_basket_many': {
      if (!Array.isArray(a.legs) || a.legs.length < 1 || a.legs.length > 128) throw new RangeError('swap_basket_many requires 1 to 128 legs')
      const legs = a.legs.map((leg: Record<string, unknown>) => {
        for (const key of Object.keys(leg)) if (!['origin_netuid', 'dest_netuid', 'amount', 'min_amount_out'].includes(key)) throw new TypeError(`unknown basket leg field ${key}`)
        const origin = netuid(leg.origin_netuid), dest = netuid(leg.dest_netuid)
        if (origin === dest) throw new RangeError('basket leg origin and destination must differ')
        return [origin, dest, rao(amount(leg.amount, origin)), rao(amount(leg.min_amount_out ?? 0, dest))]
      })
      plan.netuids = [...new Set<number>(legs.flatMap((leg: (number | bigint)[]) => [Number(leg[0]), Number(leg[1])]))]
      plan.call = sub(op, { hotkey: hotkey(), legs })
      break
    }
    case 'set_auto_stake': plan.call = sub('set_coldkey_auto_stake_hotkey', { netuid: a.netuid, hotkey: hotkey() }); break
    case 'set_reject_staking_hotkey_growth': case 'set_reject_locked_alpha': plan.call = sub(op, { enabled: a.enabled }); break
    case 'set_perpetual_lock': plan.call = sub(op, { netuid: a.netuid, enabled: a.enabled }); break
    case 'move_lock': plan.call = sub(op, { netuid: a.netuid, destination_hotkey: a.destination_hotkey_ss58 }); break
    case 'lock_stake': {
      const value = a.amount_alpha === 'all' ? await lockable(reader, context.coldkey, a.netuid) : rao(a.amount_alpha)
      plan.call = sub(op, { hotkey: hotkey(), netuid: a.netuid, amount: value }); break
    }
    case 'add_collateral': {
      const key = hotkey()
      let value: bigint
      if (a.amount_alpha === 'all') {
        const [info, collateral] = await Promise.all([reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid', [key, context.coldkey, a.netuid]), reader.query('SubtensorModule', 'MinerCollateral', [a.netuid, key, context.coldkey])])
        value = unsigned(rec(info).stake ?? 0, 'stake') - unsigned(rec(collateral).locked ?? 0, 'collateral')
        if (value <= 0n) throw new RangeError('no free alpha to lock as collateral')
      } else value = rao(a.amount_alpha)
      plan.call = sub(op, { netuid: a.netuid, hotkey: key, alpha: value, limit_price: await priceLimit(reader, a.netuid, a.rate_tolerance, true) }); break
    }
    case 'set_min_collateral': plan.call = sub(op, { netuid: a.netuid, hotkey: hotkey(), min_locked: rao(a.min_alpha) }); break
    case 'stake_burn': plan.call = sub('add_stake_burn', { netuid: a.netuid, hotkey: hotkey(), amount: rao(a.amount_tao), limit: a.limit_price == null ? await priceLimit(reader, a.netuid, a.rate_tolerance, true) : unsigned(a.limit_price, 'limit_price') }); break
    case 'burned_register': case 'root_register':
      plan.call = sub(op, op === 'root_register' ? { hotkey: hotkey() } : { netuid: a.netuid, hotkey: hotkey() }); break
    case 'register_subnet': plan.call = sub('register_network', { hotkey: hotkey() }); break
    case 'start_call': plan.call = sub(op, { netuid: a.netuid }); break
    case 'pow_register': {
      const work = bytes(a.work_hex)
      if (work.length !== 32 || a.netuid === 0) throw new RangeError('PoW requires a 32-byte seal and non-root netuid')
      plan.call = sub(op, { netuid: a.netuid, hotkey: hotkey(), nonce: u64(a.nonce, 'nonce'), work_block: u64(a.work_block, 'work_block'), work: Array.from(work) }); break
    }
    case 'claim_root': case 'claim_root_with_hotkey':
      if (op === 'claim_root' && (!Array.isArray(a.subnets) || !a.subnets.length)) throw new RangeError('claim_root requires a nonempty subnet list')
      plan.call = sub(op, op === 'claim_root' ? { subnets: a.subnets.map(netuid) } : { hotkey: hotkey() })
      await claimPreflight(plan, reader, context, op === 'claim_root' ? undefined : hotkey()); break
    case 'swap_hotkey': plan.call = sub(op, { hotkey: hotkey(), new_hotkey: a.new_hotkey_ss58, netuid: a.netuid }); break
    case 'associate_hotkey': plan.call = sub('try_associate_hotkey', { hotkey: hotkey() }); break
    case 'associate_evm_key': plan.call = sub(op, { netuid: a.netuid, evm_key: h160(a.evm_key), block_number: unsigned(a.block_number, 'block_number'), signature: bytes(a.signature) }); break
    case 'set_children': {
      const children = a.children.map((entry: unknown) => {
        if (!Array.isArray(entry) || entry.length !== 2 || typeof entry[1] !== 'string') throw new TypeError('child must be a [proportion, address] pair')
        return [proportion(entry[0], U64_MAX), entry[1]] as const
      })
      if (children.reduce((sum: bigint, child: readonly [bigint, string]) => sum + child[0], 0n) > U64_MAX) throw new RangeError('child proportions exceed 1')
      plan.call = sub(op, { netuid: a.netuid, hotkey: hotkey(), children }); break
    }
    case 'set_take': case 'increase_take': case 'decrease_take': case 'set_childkey_take': {
      const take = Number(proportion(a.take, 65535n)), params: Record<string, unknown> = { hotkey: hotkey(), take }
      let fn: string = op
      if (op === 'set_childkey_take') params.netuid = a.netuid
      if (op === 'set_take') {
        const current = Number(await reader.query('SubtensorModule', 'Delegates', [hotkey()]) ?? 0)
        if (current === take) throw new RangeError('delegate take is unchanged')
        fn = take < current ? 'decrease_take' : 'increase_take'
      }
      plan.call = sub(fn, params); break
    }
    case 'serve_axon': case 'serve_axon_tls': case 'serve_prometheus': case 'reset_axon': {
      const reset = op === 'reset_axon', ip = reset ? { value: 0n, version: 4 } : ipAddress(a.ip)
      const params: Record<string, unknown> = { netuid: a.netuid, version: reset ? 0 : a.version, ip: ip.value, port: reset ? 1 : small(a.port, 65535, 'port'), ip_type: ip.version }
      if (op !== 'serve_prometheus') Object.assign(params, { protocol: reset ? 4 : small(a.protocol, 255, 'protocol'), placeholder1: 0, placeholder2: 0 })
      if (op === 'serve_axon_tls') { const certificate = bytes(a.certificate); if (certificate.length < 1 || certificate.length > 65) throw new RangeError('neuron certificate must have one algorithm byte and at most 64 key bytes'); params.certificate = certificate }
      plan.call = sub(reset ? 'serve_axon' : op, params); break
    }
    case 'set_identity': case 'set_subnet_identity': {
      const params: Record<string, unknown> = {}
      for (const [name, value] of Object.entries(a)) params[name] = name === 'netuid' ? value : encoder.encode(String(value))
      plan.call = sub(op, params); break
    }
    case 'announce_coldkey_swap': plan.call = sub('announce_coldkey_swap_v2', { new_coldkey: a.new_coldkey_ss58 }); plan.warnings = ['locks the source account for the announcement delay; the destination must be controlled and backed up']; break
    case 'swap_coldkey_announced': plan.call = sub(op, { new_coldkey: a.new_coldkey_ss58 }); plan.warnings = ['irreversible: all balance, stake, and subnet ownership move to the announced destination']; break
    case 'clear_coldkey_swap_announcement': case 'dispute_coldkey_swap': case 'release_own_coldkey_swap_destination_reservation': plan.call = sub(op); break
    case 'trim_subnet': plan.call = descriptor('AdminUtils', 'sudo_trim_to_max_allowed_uids', { netuid: a.netuid, max_n: a.max_n }); break
    case 'trim_null_subnet_batch': plan.call = descriptor('AdminUtils', 'sudo_trim_null_uids_batch', { netuid: a.netuid, target: a.target }); plan.warnings = ['a successful batch may only clean pending commits; read NullPruningTarget and SubnetworkN before continuing']; break
    case 'set_mechanism_count': plan.call = descriptor('AdminUtils', 'sudo_set_mechanism_count', { netuid: a.netuid, mechanism_count: a.mechanism_count }); break
    case 'update_symbol': plan.call = sub(op, { netuid: a.netuid, symbol: encoder.encode(a.symbol) }); break
    case 'set_hyperparameter': plan.call = await prepareHyperparameter(reader, a.netuid, a.name, a.value); break
    case 'set_subnet_emission_weights': await emissionWeights(plan, reader, context, a); break
    case 'set_subnet_emission_enabled': {
      if (!Array.isArray(a.netuids) || !a.netuids.length) throw new RangeError('at least one netuid is required')
      const calls = a.netuids.map((n: unknown) => descriptor('AdminUtils', 'sudo_set_subnet_emission_enabled', { netuid: netuid(n), enabled: a.enabled }))
      plan.call = calls.length === 1 ? calls[0] : batchCall(calls); break
    }
    case 'set_root_claim_threshold': plan.call = sub('sudo_set_root_claim_threshold', { netuid: 0, new_value: rao(a.threshold) }); break
    case 'register_leased_network': plan.call = sub(op, { emissions_share: small(a.emissions_share, 100, 'emissions_share'), end_block: a.end_block }); break
    case 'terminate_lease': plan.call = sub(op, { lease_id: a.lease_id, hotkey: hotkey() }); break
    case 'add_proxy': case 'remove_proxy': case 'create_pure_proxy': case 'kill_pure_proxy': {
      if (!PROXY_TYPES.includes(a.proxy_type)) throw new RangeError(`unknown proxy type ${a.proxy_type}`)
      let params: Record<string, unknown>
      if (op === 'add_proxy' || op === 'remove_proxy') params = { delegate: a.delegate_ss58, proxy_type: a.proxy_type, delay: a.delay }
      else if (op === 'create_pure_proxy') params = { proxy_type: a.proxy_type, delay: a.delay, index: a.index }
      else params = { spawner: a.spawner_ss58, proxy_type: a.proxy_type, index: a.index, height: a.height, ext_index: a.ext_index }
      const mode = a.account_mode ?? context.accountMode ?? 'legacy'
      if (!['legacy', 'safe'].includes(mode)) throw new RangeError('account_mode must be legacy or safe')
      let fn = op === 'create_pure_proxy' ? mode === 'safe' ? 'create_safe_pure' : 'create_pure' : op === 'kill_pure_proxy' ? mode === 'safe' ? 'kill_safe_pure' : 'kill_pure' : op
      if (fn === 'create_pure' && reader.supportsPqProfile && (await reader.query('HashedAccounts','MlDsaAccounts',[context.coldkey]) || await reader.query('HashedAccounts','DerivedAuthorities',[context.coldkey]))) {
        fn = 'create_protected_pure'
        let reserve = 0n
        for (const [pallet, item] of [['HashedAccounts','DerivedRegistrationDeposit'],['Proxy','ProxyDepositBase'],['Proxy','ProxyDepositFactor']]) {
          const value = await reader.constant?.(pallet, item)
          if (value == null) throw new Error('pure proxy registration deposit unavailable')
          reserve += BigInt(value as any)
        }
        plan.requiredFreeByAccount = [{address:context.coldkey,requiredRao:reserve,availableRao:await freeBalance(reader,context.coldkey)}]
      }
      plan.call = descriptor('Proxy', fn, params); break
    }
    case 'remove_proxies': plan.call = descriptor('Proxy', op); break
    case 'execute_proxy_announced': {
      if (a.force_proxy_type != null && !PROXY_TYPES.includes(a.force_proxy_type)) throw new RangeError('unknown force_proxy_type')
      const innerIntent = new Intent(a.inner_op, a.inner_args), hotkey = intentSignerRole(innerIntent) === 'hotkey'
      const inner = await innerIntent.prepare(reader, { coldkey: a.real_ss58, hotkey: hotkey ? a.real_ss58 : undefined, hotkeyPublicKey: hotkey ? required(reader.decodeAddress, 'decodeAddress').call(reader, a.real_ss58) : undefined, feePayer: plan.feePayer, accountMode: context.accountMode })
      mergeChild(plan, inner)
      plan.dispatchOrigin = inner.dispatchOrigin
      plan.call = descriptor('Proxy', 'proxy_announced', { delegate: a.delegate_ss58, real: a.real_ss58, force_proxy_type: a.force_proxy_type, call: inner.call }); break
    }
    case 'multisig_threshold_1': case 'multisig_execute': case 'multisig_approve': case 'multisig_cancel': await multisig(plan, reader, context, a); break
    case 'batch': {
      if (!Array.isArray(a.intents) || !a.intents.length) throw new RangeError('batch requires at least one intent')
      const children: PreparedIntent[] = []
      for (const item of a.intents) {
        const child = item instanceof Intent ? item : intentFromJSON(item)
        if (child.op === 'batch') throw new RangeError('nested batches are not supported')
        children.push(await child.prepare(reader, context))
      }
      if (children.some(child => child.signerRole !== children[0].signerRole)) throw new RangeError('all batch intents must share a signer')
      plan.signerRole = children[0].signerRole; plan.dispatchOrigin = children[0].dispatchOrigin; plan.feePayer = children[0].feePayer
      plan.spendRao = null; plan.netuids = []; plan.global = false
      for (const child of children) mergeChild(plan, child)
      plan.call = batchCall(children.map(child => child.call)); break
    }
    case 'create_crowdloan': {
      if (Boolean(a.target_ss58) === Boolean(a.call)) throw new RangeError('provide exactly one of target_ss58 or call')
      let inner: PreparedIntent | undefined
      if (a.call) { inner = await intentFromJSON(a.call).prepare(reader, context); mergeChild(plan, inner) }
      // The attached call executes at finalization; creation only locks its deposit.
      plan.spendByAccount = [{ address: context.coldkey, amountRao: rao(a.deposit_tao) }]
      plan.call = descriptor('Crowdloan', 'create', { deposit: rao(a.deposit_tao), min_contribution: rao(a.min_contribution_tao), cap: rao(a.cap_tao), end: a.end, call: inner?.call ?? null, target_address: a.target_ss58 }); break
    }
    case 'contribute_crowdloan': plan.call = descriptor('Crowdloan', 'contribute', { crowdloan_id: a.crowdloan_id, amount: await freeAmount(a.amount_tao, reader, context) }); break
    case 'finalize_crowdloan': case 'refund_crowdloan': case 'dissolve_crowdloan': case 'withdraw_crowdloan':
      plan.call = descriptor('Crowdloan', op.replace('_crowdloan', ''), { crowdloan_id: a.crowdloan_id }); break
    case 'update_crowdloan_cap': plan.call = descriptor('Crowdloan', 'update_cap', { crowdloan_id: a.crowdloan_id, new_cap: rao(a.new_cap_tao) }); break
    case 'update_crowdloan_end': plan.call = descriptor('Crowdloan', 'update_end', { crowdloan_id: a.crowdloan_id, new_end: a.new_end }); break
    case 'update_crowdloan_min_contribution': plan.call = descriptor('Crowdloan', 'update_min_contribution', { crowdloan_id: a.crowdloan_id, new_min_contribution: rao(a.new_min_contribution_tao) }); break
    case 'set_crowdloan_max_contribution': plan.call = descriptor('Crowdloan', 'set_max_contribution', { crowdloan_id: a.crowdloan_id, new_max_contribution: a.new_max_contribution_tao == null ? null : rao(a.new_max_contribution_tao) }); break
    case 'set_weights': case 'commit_weights': case 'reveal_weights': {
      const weights = await prepareWeights(op, a, reader, context)
      plan.call = weights.call; plan.extras = weights.extras ?? {}; break
    }
    case 'authorize_order': case 'cancel_order': case 'execute_orders': case 'execute_batched_orders': {
      const result = await prepareOrderIntent(op, a, reader, context.coldkey)
      plan.call = result.call; plan.spendRao = result.spend; plan.netuids = result.netuids
      break
    }
    default: { const exhaustive: never = op; throw new RangeError(`unimplemented intent ${exhaustive}`) }
  }
  if (receiving.guards.length) plan.call = batchCall([...receiving.guards, plan.call])
  plan.mevShieldDefault ||= await registrationShieldDefault(reader,plan.call)
  if (receiving.deposit) {
    plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, { address: plan.dispatchOrigin, requiredRao: receiving.deposit, availableRao: await freeBalance(reader, plan.dispatchOrigin) }]
    plan.extras = {...plan.extras,registrationSponsorships:receiving.sponsorships.map(quote => ({...quote,payer:plan.dispatchOrigin}))}
  }
  if (!plan.spendByAccount.length && typeof plan.spendRao === 'bigint' && plan.spendRao > 0n) plan.spendByAccount = [{ address: plan.dispatchOrigin, amountRao: plan.spendRao }]
  if (plan.origin === 'root') plan.call = descriptor('Sudo', 'sudo', { call: plan.call })
  const reserves = new Map<string, { address: string; requiredRao: bigint; availableRao: bigint }>()
  for (const reserve of plan.requiredFreeByAccount) {
    const key = reader.decodeAddress ? hex(reader.decodeAddress(reserve.address)) : reserve.address
    const previous = reserves.get(key)
    reserves.set(key, { address: reserve.address, requiredRao: (previous?.requiredRao ?? 0n) + reserve.requiredRao, availableRao: previous && previous.availableRao < reserve.availableRao ? previous.availableRao : reserve.availableRao })
  }
  plan.requiredFreeByAccount = [...reserves.values()]
  if (reserves.size === 1) {
    const reserve = plan.requiredFreeByAccount[0]!
    plan.requiredFreeRao = reserve.requiredRao; plan.requiredFreeAccount = reserve.address; plan.availableFreeRao = reserve.availableRao
  } else { plan.requiredFreeRao = undefined; plan.requiredFreeAccount = undefined; plan.availableFreeRao = undefined }
  if (op === 'batch') for (const reserve of plan.requiredFreeByAccount) if (reserve.availableRao < reserve.requiredRao) plan.violations = [...plan.violations, `batch account ${reserve.address} needs ${reserve.requiredRao} rao for combined reserves and fees; has ${reserve.availableRao}`]
  return plan
}

function descriptor(pallet: string, call: string, params: Record<string, unknown> = {}): IntentDescriptor { return { pallet, call, params } }
function batchCall(calls: IntentDescriptor[]): IntentDescriptor {
  const flattened: IntentDescriptor[] = []
  const append = (call: IntentDescriptor): void => {
    // Receiving guards and other compound intents already form atomic batches.
    // Keep their order under one transaction boundary; the runtime rejects nesting.
    if (call.pallet === 'Utility' && call.call === 'batch_all') {
      for (const child of call.params.calls as IntentDescriptor[]) append(child)
    } else {
      if (proxyContainsAtomicBatch(call)) throw new RangeError('atomic batches cannot contain a proxy with an inner batch; submit the proxy intent directly')
      flattened.push(call)
    }
  }
  for (const call of calls) append(call)
  return descriptor('Utility', 'batch_all', { calls: flattened })
}
function proxyContainsAtomicBatch(call: IntentDescriptor): boolean {
  if (call.pallet !== 'Proxy' || !['proxy', 'proxy_announced'].includes(call.call)) return false
  const inner = call.params.call as IntentDescriptor
  // Proxy preserves inherited filters. Moving its guards outside would change
  // both the registration sponsor and the call hash approved by an announcement.
  return inner.pallet === 'Utility' && inner.call === 'batch_all' || proxyContainsAtomicBatch(inner)
}
function globalOperation(op: string): boolean {
  return ['transfer', 'transfer_all', 'fund_evm_key', 'evm_withdraw', 'register_subnet', 'register_leased_network', 'terminate_lease', 'set_identity', 'set_take', 'increase_take', 'decrease_take', 'set_reject_staking_hotkey_growth', 'set_reject_locked_alpha', 'associate_hotkey'].includes(op) || op.includes('coldkey_swap') || op.includes('crowdloan') || op.includes('proxy') || op === 'remove_proxies'
}
function scopeFor(op: string, args: Record<string, any>): number[] {
  if (op === 'root_register' || op === 'set_root_claim_threshold') return [0]
  if (op === 'set_subnet_emission_enabled') return Array.from(new Set((args.netuids ?? []).map(netuid)))
  return [...new Set(['netuid', 'origin_netuid', 'dest_netuid'].filter(key => args[key] != null).map(key => netuid(args[key])))]
}
function spendFor(op: string, args: Record<string, any>): IntentSpend {
  if (['transfer_all', 'burned_register', 'register_subnet', 'register_leased_network', 'transfer_stake', 'add_collateral', 'finalize_crowdloan'].includes(op)) return 'unbounded'
  const field = op === 'create_crowdloan' ? 'deposit_tao' : ['transfer', 'fund_evm_key', 'add_stake', 'add_stake_limit', 'stake_into_basket', 'stake_burn', 'contribute_crowdloan'].includes(op) ? 'amount_tao' : undefined
  return field == null ? null : args[field] === 'all' ? 'unbounded' : rao(args[field])
}
function moneyNetuid(op: string, key: string, args: Record<string, any>): number {
  if (key === 'limit_price' || key.endsWith('_tao') || key === 'threshold') return 0
  if (key === 'min_amount_out') return netuid(args.dest_netuid)
  if (key.includes('alpha') || (op === 'swap_basket' && key === 'amount')) return netuid(args.origin_netuid ?? args.netuid)
  return 0
}
function amount(value: unknown, unit: number, allowAll = false): Balance | 'all' {
  if (typeof value === 'string' && value.trim().toLowerCase() === 'all') {
    if (!allowAll) throw new RangeError('this amount does not accept all')
    return 'all'
  }
  let parsed: Balance
  if (value instanceof Balance) {
    if (value.netuid !== unit) throw new UnitMismatchError(`expected netuid ${unit} amount, received netuid ${value.netuid}`)
    parsed = value
  } else {
    if (typeof value !== 'number' && typeof value !== 'string') throw new TypeError('money must be an exact decimal string, a number, or Balance')
    parsed = Balance.fromAmount(value, unit)
  }
  if (parsed.rao < 0n) throw new RangeError('money must be nonnegative')
  return parsed
}
function rao(value: unknown): bigint {
  if (!(value instanceof Balance)) throw new TypeError('expected a normalized Balance')
  return value.rao
}
export function unsigned(value: unknown, name: string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError(`${name} requires a safe integer; use bigint or decimal string`)
  if (typeof value !== 'number' && typeof value !== 'bigint' && !(typeof value === 'string' && /^\d+$/.test(value))) throw new TypeError(`${name} must be a nonnegative integer`)
  const result = BigInt(value)
  if (result < 0n) throw new RangeError(`${name} must be nonnegative`)
  return result
}
function u64(value: unknown, name: string): bigint { const result = unsigned(value, name); if (result > U64_MAX) throw new RangeError(`${name} exceeds u64`); return result }
function small(value: unknown, max: number, name: string): number { const result = unsigned(value, name); if (result > BigInt(max)) throw new RangeError(`${name} exceeds ${max}`); return Number(result) }
function netuid(value: unknown): number { return small(value, 65535, 'netuid') }
function rec(value: unknown): Record<string, any> { return value != null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, any> : {} }
function list(value: unknown): unknown[] { if (!Array.isArray(value)) throw new TypeError('expected a chain list'); return value }
function required<T>(value: T | undefined, name: string): T { if (value == null) throw new TypeError(`intent reader must implement ${name}`); return value }
function copy<T>(value: T): T {
  if (value instanceof Balance || value instanceof Intent) return value
  if (value instanceof Uint8Array) return value.slice() as T
  if (Array.isArray(value)) return value.map(copy) as T
  if (value != null && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, copy(child)])) as T
  return value
}
function freeze<T>(value: T): T {
  if (value != null && typeof value === 'object' && !(value instanceof Uint8Array)) { for (const child of Object.values(value)) freeze(child); Object.freeze(value) }
  return value
}
function jsonArgs(args: Readonly<Record<string, unknown>>): Record<string, unknown> {
  function convert(value: unknown): unknown {
    if (value instanceof Balance) return value.amountString
    if (value instanceof Intent) return value.toJSON()
    if (typeof value === 'bigint') return value.toString()
    if (Array.isArray(value)) return value.map(convert)
    if (value != null && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, convert(child)]))
    return value
  }
  return convert(args) as Record<string, unknown>
}
function bytes(value: unknown): Uint8Array {
  if (value instanceof Uint8Array) return value.slice()
  if (typeof value !== 'string' || !/^(?:0x)?(?:[a-fA-F0-9]{2})*$/.test(value)) throw new TypeError('expected even-length hexadecimal bytes')
  const text = value.replace(/^0x/, '')
  return Uint8Array.from(text.match(/../g) ?? [], byte => parseInt(byte, 16))
}
function hex(value: Uint8Array): string { return '0x' + Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('') }
function h160(value: unknown): string { if (typeof value !== 'string' || !/^0x[a-fA-F0-9]{40}$/.test(value)) throw new TypeError('expected 0x-prefixed H160 address'); return value.toLowerCase() }
function decimalRatio(value: number | string): [bigint, bigint] {
  const match = /^\+?(?:(\d+)(?:\.(\d*))?|\.(\d+))(?:e([+-]?\d+))?$/i.exec(String(value).trim())
  if (!match) throw new RangeError('expected a finite nonnegative decimal')
  const fraction = match[2] ?? match[3] ?? '', power = Number(match[4] ?? 0) - fraction.length
  if (!Number.isSafeInteger(power) || Math.abs(power) > 1000) throw new RangeError('decimal exponent out of range')
  const numerator = BigInt((match[1] ?? '0') + fraction)
  return power < 0 ? [numerator, 10n ** BigInt(-power)] : [numerator * 10n ** BigInt(power), 1n]
}
function proportion(value: unknown, max: bigint): bigint {
  if (typeof value !== 'number' && typeof value !== 'string' && typeof value !== 'bigint') throw new TypeError('proportion must be a fraction, percentage, or raw integer')
  const raw = typeof value === 'string' ? value.trim().replace(/^\+/, '') : value
  if (typeof raw === 'number' && Number.isInteger(raw) || typeof raw === 'bigint' || typeof raw === 'string' && /^\d+$/.test(raw)) {
    const result = unsigned(raw, 'proportion'); if (result > max) throw new RangeError('proportion out of range'); return result
  }
  const text = String(raw).trim(), percent = text.endsWith('%')
  let [numerator, denominator] = decimalRatio(percent ? text.slice(0, -1) : text)
  if (percent) denominator *= 100n
  if (numerator > denominator) throw new RangeError('proportion exceeds 1')
  numerator *= max
  const quotient = numerator / denominator, remainder = numerator % denominator
  return quotient + (remainder * 2n > denominator || remainder * 2n === denominator && quotient % 2n === 1n ? 1n : 0n)
}
function ipAddress(value: string): { value: bigint; version: number } {
  const ipv4 = (text: string): number[] => { const parts = text.split('.'); if (parts.length !== 4 || parts.some(part => !/^(?:0|[1-9]\d{0,2})$/.test(part) || Number(part) > 255)) throw new RangeError('invalid IPv4 address'); return parts.map(Number) }
  if (!value.includes(':')) return { value: ipv4(value).reduce((n, byte) => n * 256n + BigInt(byte), 0n), version: 4 }
  if (value.includes('%')) throw new RangeError('IPv6 scope identifiers cannot be advertised on chain')
  let text = value
  if (text.includes('.')) { const index = text.lastIndexOf(':'), parts = ipv4(text.slice(index + 1)); text = text.slice(0, index + 1) + ((parts[0] << 8) + parts[1]).toString(16) + ':' + ((parts[2] << 8) + parts[3]).toString(16) }
  const halves = text.split('::')
  if (halves.length > 2) throw new RangeError('invalid IPv6 address')
  const left = halves[0] ? halves[0].split(':') : [], right = halves.length === 2 && halves[1] ? halves[1].split(':') : []
  const missing = 8 - left.length - right.length
  if (halves.length === 1 && left.length !== 8 || halves.length === 2 && missing < 1) throw new RangeError('invalid IPv6 address')
  const parts = [...left, ...(halves.length === 2 ? Array(missing).fill('0') : []), ...right]
  if (parts.some(part => !/^[a-fA-F0-9]{1,4}$/.test(part))) throw new RangeError('invalid IPv6 address')
  return { value: parts.reduce((n, part) => n * 65536n + BigInt(parseInt(part, 16)), 0n), version: 6 }
}

async function freeBalance(reader: IntentReader, address: string): Promise<bigint> { return unsigned(rec(rec(await reader.query('System', 'Account', [address])).data).free ?? 0, 'free balance') }
async function freeAmount(value: Balance | 'all', reader: IntentReader, context: IntentContext): Promise<bigint> {
  if (value !== 'all') return rao(value)
  const [free, deposit] = await Promise.all([freeBalance(reader, context.coldkey), required(reader.constant, 'constant').call(reader, 'Balances', 'ExistentialDeposit')])
  const available = free - unsigned(deposit, 'existential deposit') - 500000n
  if (available <= 0n) throw new RangeError('free balance does not cover existential deposit and fee headroom')
  return available
}
async function stakeAmount(value: Balance | 'all', reader: IntentReader, context: IntentContext, hotkey: string, netuid: number): Promise<bigint> {
  if (value !== 'all') return rao(value)
  const info = await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid', [hotkey, context.coldkey, netuid])
  const amount = unsigned(rec(info).stake ?? 0, 'stake')
  if (amount <= 0n) throw new RangeError('no stake in the selected position')
  return amount
}
async function lockable(reader: IntentReader, coldkey: string, netuid: number): Promise<bigint> {
  const values = rec(await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_availability_for_coldkeys', [[coldkey], [netuid]]))
  const amount = unsigned(rec(rec(values[coldkey])[netuid]).available ?? 0, 'available alpha')
  if (amount <= 0n) throw new RangeError('no unlocked stake available')
  return amount
}
async function price(reader: IntentReader, netuid: number): Promise<bigint> {
  const value = await reader.runtimeCall('SwapRuntimeApi', 'current_alpha_price', [netuid])
  if (value == null) throw new Error(`alpha price unavailable for netuid ${netuid}`)
  return unsigned(value, 'alpha price')
}
function tolerance(value: number): [bigint, bigint] {
  if (typeof value !== 'number' || !(value >= 0 && value < 1)) throw new RangeError('rate_tolerance must be in [0,1)')
  return decimalRatio(value)
}
async function priceLimit(reader: IntentReader, netuid: number, rate: number, buy: boolean): Promise<bigint> {
  const spot = await price(reader, netuid), [numerator, denominator] = tolerance(rate)
  return spot * (denominator + (buy ? numerator : -numerator)) / denominator
}
async function ratioLimit(reader: IntentReader, origin: number, destination: number, rate: number): Promise<bigint> {
  const [left, right] = await Promise.all([price(reader, origin), price(reader, destination)])
  if (right <= 0n) throw new RangeError('destination alpha price is zero')
  const [numerator, denominator] = tolerance(rate)
  return (left * RAO_PER_TAO / right) * (denominator - numerator) / denominator
}
async function swapCall(reader: IntentReader, a: Record<string, any>, hotkey: string, amount: bigint): Promise<IntentDescriptor> {
  const params: Record<string, unknown> = { hotkey, origin_netuid: a.origin_netuid, destination_netuid: a.dest_netuid, alpha_amount: amount }
  if (a.slippage_protection) Object.assign(params, { limit_price: await ratioLimit(reader, a.origin_netuid, a.dest_netuid, a.rate_tolerance), allow_partial: false })
  return descriptor('SubtensorModule', a.slippage_protection ? 'swap_stake_limit' : 'swap_stake', params)
}
function mergeChild(plan: MutablePlan, child: PreparedIntent): void {
  plan.spendRao = plan.spendRao === 'unbounded' || child.spendRao === 'unbounded' ? 'unbounded' : plan.spendRao == null && child.spendRao == null ? null : (plan.spendRao ?? 0n) + (child.spendRao ?? 0n)
  plan.spendByAccount = [...plan.spendByAccount, ...child.spendByAccount]
  plan.netuids = [...new Set([...plan.netuids, ...child.netuids])].sort((a, b) => a - b)
  plan.affectsAllSubnets ||= child.affectsAllSubnets
  plan.global ||= child.global
  plan.mevShieldDefault ||= child.mevShieldDefault
  plan.mevShieldRequired ||= child.mevShieldRequired
  plan.warnings = [...plan.warnings, ...child.warnings]
  plan.violations = [...plan.violations, ...child.violations]
  plan.effects = [...plan.effects, ...child.effects]
  plan.extras = { ...plan.extras, [child.op]: child.extras, registrationSponsorships:[...((plan.extras.registrationSponsorships as unknown[]) ?? []),...((child.extras.registrationSponsorships as unknown[]) ?? [])] }
  plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, ...child.requiredFreeByAccount]
}

async function optionalClaim(plan: MutablePlan, args: Record<string, any>, reader: IntentReader, context: IntentContext, hotkey: string, netuid: number): Promise<void> {
  if (!args.claim) return
  if (netuid !== 0) throw new RangeError('claim is only valid on root')
  const hold = unsigned(await reader.query('SubtensorModule', 'RootStakeUnlockInterval', []) ?? 0, 'root hold interval')
  if (hold > 0n) throw new RangeError('claim then unstake/move is unavailable while RootStakeUnlockInterval is nonzero; claim first and wait')
  plan.call = batchCall([descriptor('SubtensorModule', 'claim_root_with_hotkey', { hotkey }), plan.call])
  plan.affectsAllSubnets = true
  plan.warnings = [...plan.warnings, 'claims the entire validator basket entitlement, not a proportional slice']
  await claimPreflight(plan, reader, context, hotkey)
}

/** Fail-closed admission checks: failed state reads never permit an unverifiable root claim. */
async function claimPreflight(plan: MutablePlan, reader: IntentReader, context: IntentContext, hotkey?: string): Promise<void> {
  let selected: string[], scans = 0
  if (hotkey) selected = [hotkey]
  else {
    const keys = list(await reader.query('SubtensorModule', 'StakingHotkeys', [context.coldkey]) ?? []).map(String)
    const positions = list(await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_coldkey', [context.coldkey]))
    const root = new Set(positions.map(rec).filter(row => Number(row.netuid) === 0 && unsigned(row.stake, 'stake') > 0n).map(row => String(row.hotkey)))
    const [watermarks, credits] = await Promise.all([
      Promise.all(keys.map(key => reader.query('SubtensorModule', 'BasketClaimed', [key, context.coldkey]))),
      required(reader.queryMap, 'queryMap').call(reader, 'SubtensorModule', 'HotkeySwapBasketCredits', [context.coldkey], 257),
    ])
    selected = [...new Set([...keys.filter((key, index) => root.has(key) || signedInteger(watermarks[index] ?? 0) < 0n), ...credits.map(([key]) => String(Array.isArray(key) ? key[0] : key))])]
    scans = keys.length + credits.length
  }
  const palletId = await required(reader.constant, 'constant').call(reader, 'SubtensorModule', 'SubtensorPalletId')
  const id = palletId instanceof Uint8Array ? palletId : Array.isArray(palletId) ? Uint8Array.from(palletId) : bytes(palletId)
  if (id.length !== 8) throw new Error('SubtensorPalletId must be eight bytes')
  const accountId = new Uint8Array(32); accountId.set(encoder.encode('modl')); accountId.set(id, 4); accountId.set(encoder.encode('beta/esc'), 12)
  const escrow = required(reader.encodeAddress, 'encodeAddress').call(reader, accountId)
  const countKeys = required(reader.countStorageKeys, 'countStorageKeys').bind(reader)
  let holdings = 0, queuedCredits = 0
  const holdingCounts: number[] = []
  // Bound concurrency even for legacy oversize staking-hotkey lists.
  for (let index = 0; index < selected.length; index += 16) {
    const baskets = await Promise.all(selected.slice(index, index + 16).map(async key => {
      const [legacy, current, queued] = await Promise.all([
        countKeys('SubtensorModule', 'Alpha', [key, escrow], 257),
        countKeys('SubtensorModule', 'AlphaV2', [key, escrow], 257),
        countKeys('SubtensorModule', 'PendingBasketDeposits', [key], 513),
      ])
      return { holdings: small(legacy, 257, 'legacy holding count') + small(current, 257, 'holding count'), queued: small(queued, 513, 'pending deposit count') }
    }))
    holdingCounts.push(...baskets.map(basket => basket.holdings))
    holdings += baskets.reduce((sum, basket) => sum + basket.holdings, 0)
    queuedCredits += baskets.reduce((sum, basket) => sum + basket.queued, 0)
  }
  const limit = hotkey ? 130 : 256
  if (selected.length + holdings > limit || scans > limit) plan.violations = [...plan.violations, `root claim exceeds ${limit}-unit admission limit (${selected.length} hotkeys + ${holdings} holdings, ${scans} selection scans); claim validators separately or reduce basket work`]
  if (queuedCredits > 512) plan.violations = [...plan.violations, `root claim exceeds the 512-row pending deposit flush allowance (${queuedCredits} queued rows)`]
  const reserveAccount = reader.resolveFeePayer ? await reader.resolveFeePayer(plan.call, plan.feePayer) : plan.feePayer
  const free = await freeBalance(reader, reserveAccount)
  let reserved: bigint, exact = false
  try { reserved = unsigned(await required(reader.estimateFee, 'estimateFee').call(reader, plan.call, plan.feePayer), 'fee'); exact = true }
  catch { reserved = rootClaimChargedFeeRao(limit); plan.warnings = [...plan.warnings, 'exact fee estimate unavailable; claim reserve uses the runtime allowance fee floor'] }
  if (free < reserved) plan.violations = [...plan.violations, `free TAO ${free} is below reserved claim fee ${reserved}`]
  plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, { address: reserveAccount, requiredRao: reserved, availableRao: free }]
  if (exact) plan.estimatedFeeRao = reserved
  plan.preview = { ...plan.preview, rootClaim: { selectedHotkeys: selected, holdings, holdingCounts, queuedCredits, selectionScans: scans, admissionLimit: limit, reservedFeeRao: reserved } }
  try {
    const raw = hotkey ? await reader.runtimeCall('BetaBasketRuntimeApi', 'get_basket_claim_preview', [hotkey, context.coldkey]) : await reader.runtimeCall('BetaBasketRuntimeApi', 'get_root_basket_claim_previews', [context.coldkey])
    const byHotkey = hotkey ? new Map([[hotkey, raw]]) : new Map(list(raw).map(rec).map(row => [String(row.hotkey), row]))
    const previews = selected.map(key => {
      const value = byHotkey.get(key)
      if (value == null) return null
      const row = rec(value)
      return {
        accrued_tao: unsigned(row.accrued_tao, 'accrued'), redeemable_tao: unsigned(row.redeemable_tao, 'redeemable'), forfeited_tao_est: unsigned(row.forfeited_tao_est, 'forfeited'),
        rows: small(row.rows, 0xffffffff, 'rows'), rows_to_sell: small(row.rows_to_sell, 0xffffffff, 'rows to sell'), dust_rows: small(row.dust_rows, 0xffffffff, 'dust rows'), swept: small(row.swept, 0xffffffff, 'swept'),
      } satisfies RootClaimPreview
    })
    const rawThreshold = await reader.query('SubtensorModule', 'RootClaimableThreshold', [0])
    const storedThreshold = rawThreshold == null ? 0n : signedInteger(rec(rawThreshold).bits ?? rawThreshold) >> 32n
    const quote = quoteRootClaimPreviews(previews, reserved, storedThreshold > 0n ? storedThreshold : 500000n, scans, limit)
    plan.preview = { ...plan.preview, rootClaim: { ...rec(plan.preview.rootClaim), ...quote } }
    if (quote.redeemableRao === 0n) plan.warnings = [...plan.warnings, 'claim has no currently redeemable yield; a scan fee is still charged']
    else if (quote.losesMoney) plan.warnings = [...plan.warnings, 'estimated spent fee exceeds redeemable yield']
    if (quote.belowThresholdHotkeys > 0) plan.warnings = [...plan.warnings, `${quote.belowThresholdHotkeys} validator claims are below the payout threshold and keep accruing`]
    if (quote.dustRows > 0) plan.warnings = [...plan.warnings, `${quote.dustRows} dust rows are skipped; estimated forfeited slices total ${quote.forfeitedRao} rao`]
    if (quote.possibleRefundRao > 0n) plan.warnings = [...plan.warnings, 'spent fee is a floor; pending deposit flush work can reduce the estimated refund']
  } catch { plan.warnings = [...plan.warnings, 'basket yield preview unavailable; admission and fee reserve were checked separately'] }
}
function signedInteger(value: unknown): bigint { if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('unsafe signed integer'); if (typeof value !== 'number' && typeof value !== 'bigint' && !(typeof value === 'string' && /^-?\d+$/.test(value))) throw new TypeError('invalid signed integer'); return BigInt(value) }
async function emissionWeights(plan: MutablePlan, reader: IntentReader, context: IntentContext, a: Record<string, any>): Promise<void> {
  if (!Array.isArray(a.slots) || !a.slots.length || !Array.isArray(a.values) || a.slots.length !== a.values.length) throw new RangeError('emission slots and values must have the same nonzero length')
  const slots = a.slots.map(emission.slotIndex)
  if (new Set(slots).size !== slots.length) throw new RangeError('each emission slot may appear only once')
  const updates: Record<string, bigint> = Object.fromEntries(slots.map((slot: number, index: number) => [slot, emission.parseShare(a.values[index])]))
  if ((updates[0] ?? 0n) !== 0n || (updates[1] ?? 0n) !== 0n) throw new RangeError('burn and recycle slots must remain zero')
  const current = rec(await reader.runtimeCall('SubnetInfoRuntimeApi', 'get_subnet_emission_weights', [a.netuid]))
  if (!Array.isArray(current.weights)) throw new Error('subnet emission vector unavailable; root has no emission vector')
  const before = current.weights.map((value: unknown) => unsigned(value, 'emission share'))
  const after = a.renormalize ? emission.renormalize(before, updates) : emission.mergeWeights(before, updates)
  const baseline = unsigned(current.root_baseline_share, 'root baseline share')
  const violations = emission.validate(after, { min_validator: emission.minValidatorParts(baseline), min_owner: emission.DEFAULT_OWNER, created_via_lease: Boolean(current.created_via_lease) })
  plan.violations = [...plan.violations, ...violations.map(violation => `${violation.error}: ${violation.message}`)]
  const [owner, last, block] = await Promise.all([
    reader.query('SubtensorModule', 'SubnetOwner', [a.netuid]),
    reader.query('SubtensorModule', 'LastRateLimitedBlock', [{ OwnerHyperparamUpdate: [a.netuid, 'EmissionWeights'] }]),
    required(reader.blockNumber, 'blockNumber').call(reader),
  ])
  const isOwner = owner != null && (reader.decodeAddress ? hex(reader.decodeAddress(String(owner))) === hex(reader.decodeAddress(context.coldkey)) : String(owner) === context.coldkey)
  if (!isOwner) plan.violations = [...plan.violations, `dispatch account is not the owner of subnet ${a.netuid}`]
  const rateLimit = emission.rateLimitStatus(Number(unsigned(last ?? 0, 'last emission change')), block)
  if (!rateLimit.owner_can_change_now) plan.violations = [...plan.violations, `owner emission update rate-limited until block ${rateLimit.next_allowed_block}`]
  const ids: number[] = a.renormalize ? [0, 1, 2, 3, 4] : slots.slice().sort((left: number, right: number) => left - right)
  plan.call = descriptor('AdminUtils', 'sudo_set_subnet_emission_weights', { netuid: a.netuid, uids: ids, values: ids.map(id => a.renormalize ? after[id] : updates[id]) })
  plan.preview = { before, after, baselineRootShare: baseline, rateLimit, changed: emission.SLOT_NAMES.filter((_, index) => before[index] !== after[index]) }
}

async function multisig(plan: MutablePlan, reader: IntentReader, context: IntentContext, a: Record<string, any>): Promise<void> {
  const threshold = plan.op === 'multisig_threshold_1' ? 1 : small(a.threshold, 65535, 'threshold')
  const mode = a.account_mode ?? context.accountMode ?? 'legacy'
  if (!['legacy', 'safe'].includes(mode)) throw new RangeError('account_mode must be legacy or safe')
  const suffix = mode === 'safe' ? '_safe' : ''
  const decode = required(reader.decodeAddress, 'decodeAddress').bind(reader), signerId = hex(decode(context.coldkey))
  if (!Array.isArray(a.other_signatories)) throw new TypeError('other_signatories must be an array')
  const unique = new Map<string, string>()
  for (const address of a.other_signatories) {
    const key = hex(decode(address))
    if (key === signerId) throw new RangeError('other_signatories must exclude the signer')
    unique.set(key, address)
  }
  const others = [...unique].sort(([a], [b]) => a.localeCompare(b)).map(([, address]) => address)
  if (threshold < (plan.op === 'multisig_threshold_1' ? 1 : 2) || threshold > others.length + 1) throw new RangeError('invalid multisig threshold')
  const account = required(reader.multisigAccount, 'multisigAccount').call(reader, [context.coldkey, ...others], threshold, mode === 'safe')
  const semantic = intentFromJSON(a.call), innerRole = intentSignerRole(semantic)
  const inner = await semantic.prepare(reader, { coldkey: account.address, hotkey: innerRole === 'hotkey' ? account.address : undefined, hotkeyPublicKey: innerRole === 'hotkey' ? account.publicKey : undefined, feePayer: plan.feePayer, accountMode: mode })
  mergeChild(plan, inner)
  plan.dispatchOrigin = inner.dispatchOrigin
  const params: Record<string, unknown> = { other_signatories: others }
  const hash = await required(reader.hashCall, 'hashCall').call(reader, inner.call)
  if (hash.length !== 32) throw new RangeError('call hash must be 32 bytes')
  if (plan.op === 'multisig_threshold_1') {
    params.call = inner.call; plan.call = descriptor('Multisig', 'as_multi_threshold_1' + suffix, params)
  } else {
    params.threshold = threshold
    const timepoint = a.timepoint == null ? null : { height: small(a.timepoint.height, 0xffffffff, 'timepoint height'), index: small(a.timepoint.index, 0xffffffff, 'timepoint index') }
    if (plan.op === 'multisig_cancel') {
      if (!timepoint) throw new TypeError('cancel requires a timepoint')
      params.timepoint = timepoint; params.call_hash = hash
      plan.call = { ...descriptor('Multisig', 'cancel_as_multi' + suffix, params), semanticCall: inner.call }
      // Cancellation never dispatches or spends the inner call.
      plan.spendRao = null; plan.spendByAccount = []; plan.requiredFreeByAccount = []; plan.netuids = []; plan.affectsAllSubnets = false; plan.global = true; plan.mevShieldRequired = false; plan.mevShieldDefault = false; plan.violations = []
    } else {
      params.maybe_timepoint = timepoint
      params.max_weight = await required(reader.estimateWeight, 'estimateWeight').call(reader, inner.call, plan.feePayer)
      const hashOnly = plan.op === 'multisig_approve' && timepoint != null
      if (hashOnly) params.call_hash = hash
      else params.call = inner.call
      plan.call = { ...descriptor('Multisig', (hashOnly ? 'approve_as_multi' : 'as_multi') + suffix, params), ...(hashOnly ? { semanticCall: inner.call } : {}) }
      let deposit = 0n
      if (timepoint == null) {
        const constant = required(reader.constant, 'constant').bind(reader)
        const [base, factor] = await Promise.all([constant('Multisig', 'DepositBase'), constant('Multisig', 'DepositFactor')])
        deposit = unsigned(base, 'multisig deposit base') + BigInt(threshold) * unsigned(factor, 'multisig deposit factor')
      }
      const fee = await required(reader.estimateFee, 'estimateFee').call(reader, plan.call, plan.feePayer)
      const feeAccount = reader.resolveFeePayer ? await reader.resolveFeePayer(plan.call, plan.feePayer) : plan.feePayer
      const [depositFree, feeFree] = await Promise.all([freeBalance(reader, context.coldkey), freeBalance(reader, feeAccount)])
      plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, { address: context.coldkey, requiredRao: deposit, availableRao: depositFree }, { address: feeAccount, requiredRao: fee, availableRao: feeFree }]
      plan.estimatedFeeRao = fee
      const sameAccount = hex(decode(context.coldkey)) === hex(decode(feeAccount))
      if (sameAccount ? depositFree < deposit + fee : depositFree < deposit || feeFree < fee) plan.violations = [...plan.violations, `multisig dispatch account needs ${deposit} rao for deposit and fee account needs ${fee} rao for fee`]
    }
  }
  plan.extras = { ...plan.extras, multisig_call_hash: hex(hash), multisig_account: account.address }
}
