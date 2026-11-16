import { POLICY_CATALOG } from './policy-catalog'
import { Balance, RAO_PER_TAO, UnitMismatchError } from './balance'
import { evaluatePolicy, evaluatePolicyPreservingValues } from './policy'
import { gammaAmount, normalizeGammaArguments, prepareGammaIntent } from './gamma'
import { prepareOrderIntent } from './limit-orders'
import { prepareRecipients, registrationShieldDefault, type Recipient } from './receiving'
import { quoteRegistrationDeposit, registrationDepositGuard } from './registration-deposit'
import { INTENT_CATALOG } from './intent-catalog'
import * as emission from './emission-weights'
import { prepareWeights, type EncryptWeightsV2Options } from './intent-weights'
import { prepareHyperparameter } from './intent-hyperparameters'
import { rootClaimChargedFeeRao, quoteRootClaimPreviews, readRootClaimRelationships, type RootClaimPreview } from './root-claim'
import type { IntentArguments, IntentBuilders, IntentInputMap } from './intent-inputs'
import { announcedCallRequiresDirect, decodedCallDescriptor, sameCallSemantics, announcedCallLeaves, validateAnnouncedCall } from './proxy-announcements'
import { accountRecord, subnetRecord } from './staking-records'
import { canonicalJson } from './portable'
import { committedRegistrationSponsorships } from './committed-registration'
export type { IntentInputMap, IntentJSON, IntentArguments, IntentBuilders, IntentAmountInput, BasketSwapLegInput, IntentTimepointInput } from './intent-inputs'

export type IntentOperation = typeof INTENT_CATALOG[number]['name']
export type IntentMoney = Balance | number | string
export type IntentSpend = bigint | null | 'unbounded'
export type IntentOrigin = 'signed' | 'subnet_owner' | 'root'
export interface IntentDescriptor {
  readonly pallet: string
  readonly call: string
  readonly params: Readonly<Record<string, unknown>>
  /** Exact semantic call required for hash-only approvals; optional for cancellation. */
  readonly semanticCall?: IntentDescriptor
  /** Exact saved SCALE bytes; must match re-encoding this descriptor before use. */
  readonly callData?: string | Uint8Array
  /** Explicit account choice is supported only for Proxy.add_proxy. */
  readonly signerRole?: 'coldkey' | 'hotkey'
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
  decodeCall?(data: Uint8Array): Promise<unknown>
  hashCallData?(data: Uint8Array): Uint8Array | Promise<Uint8Array>
  specVersion?(): number | Promise<number>
  query(pallet: string, item: string, params?: unknown[], block?: number | string | null): Promise<unknown>
  /** Inspect the selected block's metadata before using legacy registration constants. */
  hasStorage?(pallet: string, item: string, block: string): boolean | Promise<boolean>
  /** Test physical presence at the selected block, without applying a storage default. */
  storageExists?(pallet: string, item: string, params: unknown[], block: string): Promise<boolean>
  queryMap?(pallet: string, item: string, params?: unknown[], limit?: number): Promise<Array<[unknown, unknown]>>
  /** Count physical storage keys, including rows whose stored value is zero. */
  countStorageKeys?(pallet: string, item: string, params: unknown[], limit: number): Promise<number>
  runtimeCall(api: string, method: string, params?: unknown[], block?: number | string | null): Promise<unknown>
  moduleError?(pallet: number, error: number, block?: string): Promise<{ name: string; docs: readonly string[] }>
  constant?(pallet: string, name: string, block?: string): unknown | Promise<unknown>
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
const preparedOrigins = new WeakMap<object, string>()
const preparedSources = new WeakMap<object, string>()
const pinnedMultisigs = new WeakMap<Intent, { inner: PreparedIntent; origin: string }>()
const U64_MAX = (1n << 64n) - 1n
const encoder = new TextEncoder()
export const PROXY_TYPES: readonly string[] = Object.freeze(POLICY_CATALOG.transaction.proxy_types)

/** Machine-readable Python-compatible input schemas, defaults, safety notes and signer roles. */
export function listIntentTools(): readonly CatalogEntry[] { return INTENT_CATALOG as unknown as readonly CatalogEntry[] }
export function isPreparedIntent(value: unknown): value is PreparedIntent {
  return typeof value === 'object' && value !== null && preparedIntents.has(value)
}
export function checkIntentPolicy(intent: PreparedIntent, policy: IntentPolicy = {}, feeRao?: bigint): string[] {
  if (!isPreparedIntent(intent)) throw new TypeError('expected an SDK-prepared intent')
  const result = evaluatePolicy<{ violations: Array<{ code: string; actual?: string; cap?: string; netuid?: number }> }>('transaction.intent_policy', {
    max_fee_rao: policy.maxFeeRao == null ? null : unsigned(policy.maxFeeRao, 'maxFeeRao'), max_spend_rao: policy.maxSpendRao == null ? null : unsigned(policy.maxSpendRao, 'maxSpendRao'), allowed_netuids: policy.allowedNetuids,
    allow_global: policy.allowGlobal, fee_rao: feeRao, spend_rao: intent.spendRao, netuids: intent.netuids,
    affects_all_subnets: intent.affectsAllSubnets, global: intent.global,
  })
  return [...intent.violations, ...result.violations.map(reason => {
    switch (reason.code) {
      case 'fee_unknown': return 'fee estimate unavailable under maxFeeRao policy'
      case 'fee': return `fee ${reason.actual} exceeds maxFeeRao ${reason.cap}`
      case 'unbounded': return 'unbounded spend cannot satisfy maxSpendRao'
      case 'spend': return `spend ${reason.actual} exceeds maxSpendRao ${reason.cap}`
      case 'all_subnets': return 'operation affects all subnets and cannot satisfy a subnet allowlist'
      case 'global': return 'global operation requires allowGlobal with a subnet allowlist'
      case 'netuid': return `netuid ${reason.netuid} is outside allowedNetuids`
      default: throw new Error(`unknown transaction policy violation ${reason.code}`)
    }
  })]
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
        normalized[key] = key.endsWith('_gamma') ? gammaAmount(value, tool.all_amount_fields.includes(key)) : amount(value, moneyNetuid(op, key, normalized), tool.all_amount_fields.includes(key))
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
    if (normalized.rate_tolerance != null && !evaluatePolicy<boolean>('financial.intent.rate_valid', { rate: normalized.rate_tolerance })) throw new RangeError('rate_tolerance must be in [0, 1)')
    normalizeGammaArguments(op, normalized)
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
    preparedOrigins.set(result, context.coldkey)
    preparedSources.set(result, canonicalJson({ op: this.op, ...jsonArgs(this.args) }))
    return freeze(result)
  }
}
export function buildIntent<N extends keyof IntentInputMap>(op: N, ...args: IntentArguments<N>): Intent { return new Intent(op, args[0] as Record<string, unknown> | undefined) }
/** Internal workflow bridge: preserve an SDK-prepared call and its verified policy. */
export function wrapPreparedMultisig(op: 'multisig_execute' | 'multisig_approve' | 'multisig_threshold_1', args: Record<string, unknown>, inner: PreparedIntent): Intent {
  const origin = preparedOrigins.get(inner)
  if (!preparedIntents.has(inner) || !origin) throw new TypeError('multisig requires an SDK-prepared inner intent')
  if (canonicalJson(intentFromJSON(args.call as Record<string, unknown> | Intent).toJSON()) !== preparedSources.get(inner)) throw new TypeError('multisig semantic intent differs from its prepared call')
  const intent = new Intent(op, args)
  pinnedMultisigs.set(intent, { inner: freeze(copy(inner)), origin })
  return intent
}
/** Autocompleting builders with required arguments and exact money/quantity inputs. */
export const intentBuilders: IntentBuilders = Object.freeze(Object.fromEntries([...catalog.keys()].map(op => [op, (args?: Record<string, unknown>) => new Intent(op as IntentOperation, args)]))) as unknown as IntentBuilders
export function intentSignerRole(intent: Intent): 'coldkey' | 'hotkey' {
  if (intent.op === 'batch') {
    const children = (intent.args.intents as Array<Intent | Record<string, unknown>>).map(child => child instanceof Intent ? child : intentFromJSON(child))
    return evaluatePolicy<{ signer: 'coldkey' | 'hotkey' }>('transaction.merge_intents', { batch: true, children: children.map(child => ({ op: child.op, signer: intentSignerRole(child) })) }).signer
  }
  return intentMetadata(intent.op, intent.args).signer
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
  const encoded = await compose(call.pallet, call.call, await visit(call.params) as Record<string, unknown>)
  if (call.callData != null) {
    const saved = bytes(call.callData)
    if (hex(encoded) !== hex(saved)) throw new Error('saved call bytes do not match the decoded semantic call')
    return saved
  }
  return encoded
}

type MutablePlan = { -readonly [K in keyof PreparedIntent]: PreparedIntent[K] }
async function prepare(intent: Intent, reader: IntentReader, context: IntentContext): Promise<MutablePlan> {
  const op = intent.op, tool = catalog.get(op)!
  const receiving = await prepareRecipients(op, intent.args, reader, context.coldkey), a = receiving.args
  const role = intentSignerRole(intent), metadata = intentMetadata(op, a)
  const plan: MutablePlan = {
    op, call: descriptor('', ''), signerRole: role, dispatchOrigin: role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey,
    feePayer: context.feePayer ?? (role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey), origin: tool.origin,
    spendRao: metadata.spend_rao == null || metadata.spend_rao === 'unbounded' ? metadata.spend_rao : BigInt(metadata.spend_rao), spendByAccount: [], netuids: metadata.netuids, affectsAllSubnets: metadata.affects_all_subnets,
    global: metadata.global, mevShieldDefault: tool.mev_shield_default, mevShieldRequired: tool.mev_shield_required,
    effects: [tool.summary], warnings: [], violations: [], requiredFreeByAccount: [], preview: {}, extras: {},
  }
  const hotkey = () => {
    const address = a.hotkey_ss58 ?? context.hotkey ?? (role === 'hotkey' ? context.coldkey : undefined)
    if (typeof address !== 'string' || !address) throw new TypeError(`${op}: pass hotkey_ss58 or a context hotkey`)
    return address
  }
  const sub = (fn: string, params: Record<string, unknown> = {}) => descriptor('SubtensorModule', fn, params)
  switch (op) {
    case 'mint_gamma': case 'mint_gamma_with_stake': case 'move_gamma': case 'close_gamma_deposit':
    case 'set_gamma_deposit_spender': case 'set_gamma_deposit_daily_spend_cap': case 'spend_gamma': case 'set_service': {
      const gamma = await prepareGammaIntent(op, a, reader, context)
      plan.call = gamma.call; plan.spendRao = gamma.spendRao
      plan.warnings = gamma.warnings; plan.violations = gamma.violations; plan.preview = gamma.preview
      break
    }
    case 'transfer': case 'transfer_all': {
      plan.call = simpleCall(op, a)
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
      const amount = await freeAmount(a.amount_tao, reader, context), key = hotkey()
      const route = stakingDecision(op, a, { hotkey: key })
      plan.call = stakingCall(op, a, { hotkey: key, amount, limit_price: await stakingLimit(route, a, reader) })
      if (a.amount_tao === 'all') plan.warnings = ['uses the entire free balance minus the existential deposit and 500000 rao fee headroom']
      break
    }
    case 'remove_stake': case 'remove_stake_limit': {
      const key = hotkey(), route = stakingDecision(op, a, { hotkey: key })
      // Still require an actual position before choosing the claim-all sentinel.
      const current = await stakeAmount(a.amount_alpha, reader, context, key, a.netuid)
      plan.call = stakingCall(op, a, { hotkey: key, amount: route.amount ?? current, limit_price: await stakingLimit(route, a, reader) })
      await optionalClaim(plan, a, reader, context, key, a.netuid)
      break
    }
    case 'move_stake': case 'move_swap_stake': case 'swap_stake': case 'transfer_stake': {
      const key = a.origin_hotkey_ss58 ?? hotkey(), originHotkey = key
      const route = stakingDecision(op, a, { hotkey: key, origin_hotkey: originHotkey })
      const amount = route.amount ?? await stakeAmount(a.amount_alpha, reader, context, originHotkey, a.origin_netuid)
      plan.call = stakingCall(op, a, { hotkey: key, origin_hotkey: originHotkey, amount, limit_price: await stakingLimit(route, a, reader) })
      if (op === 'transfer_stake') plan.warnings = ['transfers stake ownership irreversibly to another coldkey; locked alpha must land on the receiver lock hotkey']
      else if (op === 'move_swap_stake') plan.warnings = ['the swap uses amount minus 1 rao to accommodate share-pool rounding; 1 rao remains on the destination hotkey at the origin subnet']
      else if (op === 'move_stake') await optionalClaim(plan, a, reader, context, originHotkey, a.origin_netuid)
      break
    }
    case 'unstake_all': case 'unstake_all_alpha':
      plan.call = stakingCall(op, a, { hotkey: hotkey() })
      plan.warnings = ['affects every subnet position on this hotkey']
      await optionalClaim(plan, a, reader, context, hotkey(), 0)
      break
    case 'swap_basket': {
      let value: bigint
      if (a.amount === 'all') {
        const rows = list(await reader.runtimeCall('BetaBasketRuntimeApi', 'get_validator_basket', [hotkey()]))
        const row = rows.find(row => Array.isArray(row) && Number(row[0]) === a.origin_netuid) as unknown[] | undefined
        value = unsigned(row?.[1] ?? 0, 'basket holding')
        if (!value) throw new RangeError('basket has no origin holding to sell')
      } else value = rao(a.amount)
      plan.call = stakingCall(op, a, { hotkey: hotkey(), amount: value })
      break
    }
    case 'swap_basket_many': {
      const adapted = Array.isArray(a.legs) ? a.legs.map((leg: Record<string, unknown>) => {
        if (leg == null || typeof leg !== 'object' || Array.isArray(leg)) throw new TypeError('basket leg must be an object')
        const origin = netuid(leg.origin_netuid), dest = netuid(leg.dest_netuid)
        return { origin_netuid: origin, dest_netuid: dest, amount_rao: rao(amount(leg.amount, origin)), min_out_rao: rao(amount(leg.min_amount_out ?? 0, dest)), fields: Object.keys(leg) }
      }) : a.legs
      const selected = evaluatePolicy<{ legs: [number, number, string, string][]; netuids: number[] }>('financial.basket_legs', { legs: adapted })
      plan.netuids = selected.netuids
      plan.call = sub(op, { hotkey: hotkey(), legs: selected.legs.map(([origin, dest, amount, minOut]) => [origin, dest, BigInt(amount), BigInt(minOut)]) })
      break
    }
    case 'set_auto_stake': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'set_reject_staking_hotkey_growth': case 'set_reject_locked_alpha': plan.call = simpleCall(op, a); break
    case 'set_perpetual_lock': plan.call = simpleCall(op, a); break
    case 'move_lock': plan.call = simpleCall(op, a); break
    case 'lock_stake': {
      const value = a.amount_alpha === 'all' ? await lockable(reader, context.coldkey, a.netuid) : rao(a.amount_alpha)
      plan.call = stakingCall(op, a, { hotkey: hotkey(), amount: value }); break
    }
    case 'add_collateral': {
      const key = hotkey()
      let value: bigint
      if (a.amount_alpha === 'all') {
        const [info, collateral] = await Promise.all([reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid', [key, context.coldkey, a.netuid]), reader.query('SubtensorModule', 'MinerCollateral', [a.netuid, key, context.coldkey])])
        value = BigInt(evaluatePolicy<string>('financial.staking_amount', { kind: 'collateral', amount: unsigned(rec(info).stake ?? 0, 'stake'), locked: unsigned(rec(collateral).locked ?? 0, 'collateral') }))
      } else value = rao(a.amount_alpha)
      plan.call = stakingCall(op, a, { hotkey: key, amount: value, limit_price: await priceLimit(reader, a.netuid, a.rate_tolerance, true) }); break
    }
    case 'set_min_collateral': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'stake_burn': plan.call = stakingCall(op, a, { hotkey: hotkey(), amount: rao(a.amount_tao), limit_price: a.limit_price == null ? await priceLimit(reader, a.netuid, a.rate_tolerance, true) : unsigned(a.limit_price, 'limit_price') }); break
    case 'burned_register': case 'root_register':
      plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'register_subnet': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'start_call': plan.call = simpleCall(op, a); break
    case 'pow_register': {
      const work = bytes(a.work_hex)
      if (work.length !== 32 || a.netuid === 0) throw new RangeError('PoW requires a 32-byte seal and non-root netuid')
      plan.call = sub(op, { netuid: a.netuid, hotkey: hotkey(), nonce: u64(a.nonce, 'nonce'), work_block: u64(a.work_block, 'work_block'), work: Array.from(work) }); break
    }
    case 'claim_root': case 'claim_root_with_hotkey':
      plan.call = stakingCall(op, a, op === 'claim_root' ? {} : { hotkey: hotkey() })
      await claimPreflight(plan, reader, context, op === 'claim_root' ? undefined : hotkey()); break
    case 'swap_hotkey': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'associate_hotkey': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'associate_evm_key': plan.call = sub(op, { netuid: a.netuid, evm_key: h160(a.evm_key), block_number: unsigned(a.block_number, 'block_number'), signature: bytes(a.signature) }); break
    case 'set_children': {
      const normalized = evaluatePolicy<Array<[string, string]>>('transaction.children', { children: a.children.map((entry: unknown) => Array.isArray(entry) ? [proportionInput(entry[0]), ...entry.slice(1)] : entry) })
      const children = normalized.map(([share, address]) => [BigInt(share), address])
      plan.call = sub(op, { netuid: a.netuid, hotkey: hotkey(), children }); break
    }
    case 'set_take': case 'increase_take': case 'decrease_take': case 'set_childkey_take': {
      const current = op === 'set_take' ? Number(await reader.query('SubtensorModule', 'Delegates', [hotkey()]) ?? 0) : undefined
      const selected = evaluatePolicy<{ call: string; take: string }>('transaction.take', { op, take: proportionInput(a.take), current })
      const fn = selected.call, params: Record<string, unknown> = { hotkey: hotkey(), take: Number(selected.take) }
      if (op === 'set_childkey_take') params.netuid = a.netuid
      plan.call = sub(fn, params); break
    }
    case 'serve_axon': case 'serve_axon_tls': case 'serve_prometheus': case 'reset_axon': {
      const call = evaluatePolicy<IntentDescriptor>('transaction.serving', { op, args: { ...a, ...(op === 'serve_axon_tls' ? { certificate: bytes(a.certificate) } : {}) } })
      plan.call = { ...call, params: { ...call.params, ip: BigInt(call.params.ip as string), ...(op === 'serve_axon_tls' ? { certificate: bytes(call.params.certificate) } : {}) } }
      break
    }
    case 'set_identity': case 'set_subnet_identity': {
      const params: Record<string, unknown> = {}
      for (const [name, value] of Object.entries(a)) params[name] = name === 'netuid' ? value : encoder.encode(String(value))
      plan.call = sub(op, params); break
    }
    case 'announce_coldkey_swap': plan.call = simpleCall(op, a); plan.warnings = ['locks the source account for the announcement delay; the destination must be controlled and backed up']; break
    case 'swap_coldkey_announced': plan.call = simpleCall(op, a); plan.warnings = ['irreversible: all balance, stake, and subnet ownership move to the announced destination']; break
    case 'clear_coldkey_swap_announcement': case 'dispute_coldkey_swap': case 'release_own_coldkey_swap_destination_reservation': plan.call = simpleCall(op, a); break
    case 'trim_subnet': plan.call = simpleCall(op, a); break
    case 'trim_null_subnet_batch': plan.call = simpleCall(op, a); plan.warnings = ['a successful batch may only clean pending commits; read NullPruningTarget and SubnetworkN before continuing']; break
    case 'set_mechanism_count': plan.call = simpleCall(op, a); break
    case 'update_symbol': plan.call = sub(op, { netuid: a.netuid, symbol: encoder.encode(a.symbol) }); break
    case 'set_hyperparameter': plan.call = await prepareHyperparameter(reader, a.netuid, a.name, a.value); break
    case 'set_subnet_emission_weights': await emissionWeights(plan, reader, context, a); break
    case 'set_subnet_emission_enabled': {
      if (!Array.isArray(a.netuids) || !a.netuids.length) throw new RangeError('at least one netuid is required')
      const calls = a.netuids.map((n: unknown) => descriptor('AdminUtils', 'sudo_set_subnet_emission_enabled', { netuid: netuid(n), enabled: a.enabled }))
      plan.call = calls.length === 1 ? calls[0] : batchCall(calls); break
    }
    case 'set_subnet_gamma_emission_whitelisted': {
      plan.call = simpleCall(op, a)
      break
    }
    case 'set_root_claim_threshold': plan.call = simpleCall(op, a); break
    case 'register_leased_network': plan.call = simpleCall(op, a, { emissions_share: small(a.emissions_share, 100, 'emissions_share') }); break
    case 'terminate_lease': plan.call = simpleCall(op, a, { hotkey: hotkey() }); break
    case 'add_proxy': case 'remove_proxy': case 'create_pure_proxy': case 'kill_pure_proxy': {
      evaluatePolicy('transaction.proxy_type', { proxy_type: a.proxy_type })
      let params: Record<string, unknown>
      if (op === 'add_proxy' || op === 'remove_proxy') params = { delegate: a.delegate_ss58, proxy_type: a.proxy_type, delay: a.delay }
      else if (op === 'create_pure_proxy') params = { proxy_type: a.proxy_type, delay: a.delay, index: a.index }
      else params = { spawner: a.spawner_ss58, proxy_type: a.proxy_type, index: a.index, height: a.height, ext_index: a.ext_index }
      const mode = a.account_mode ?? context.accountMode ?? 'legacy'
      evaluatePolicy('transaction.account_mode', { mode })
      let fn = op === 'create_pure_proxy' || op === 'kill_pure_proxy' ? evaluatePolicy<{ call: string }>('transaction.pure_proxy', { op, mode, index: a.index }).call : op
      let depositGuard: IntentDescriptor | undefined
      if (fn === 'create_pure' && reader.supportsPqProfile && (await reader.query('HashedAccounts','MlDsaAccounts',[context.coldkey]) || await reader.query('HashedAccounts','DerivedAuthorities',[context.coldkey]))) {
        fn = evaluatePolicy<{ call: string }>('transaction.pure_proxy', { op, mode, index: a.index, pq_required: true }).call
        const quote = await quoteRegistrationDeposit(reader, 'DerivedRegistrationDeposit')
        if (quote.configurable) depositGuard = registrationDepositGuard(quote)
        let reserve = quote.amountRao
        for (const [pallet, item] of [['Proxy','ProxyDepositBase'],['Proxy','ProxyDepositFactor']]) {
          const value = await reader.constant?.(pallet, item)
          if (value == null) throw new Error('pure proxy registration deposit unavailable')
          reserve += BigInt(value as any)
        }
        plan.requiredFreeByAccount = [{address:context.coldkey,requiredRao:reserve,availableRao:await freeBalance(reader,context.coldkey)}]
      }
      const call = { ...descriptor('Proxy', fn, params), ...(op === 'add_proxy' && a.from_hotkey != null ? { signerRole: role } : {}) }
      plan.call = depositGuard ? batchCall([depositGuard, call]) : call; break
    }
    case 'remove_proxies': plan.call = simpleCall(op, a); break
    case 'announce_proxy_call': case 'remove_proxy_announcement': case 'reject_proxy_announcement': {
      if (typeof a.call_hash !== 'string' || !/^0x[0-9a-f]{64}$/i.test(a.call_hash)) throw new TypeError('call_hash must be a 0x-prefixed Blake2-256 hash')
      plan.call = simpleCall(op, a)
      if (op === 'announce_proxy_call') plan.warnings = [...plan.warnings, 'reserves an announcement deposit, returned on execution, removal or rejection']
      break
    }
    case 'execute_proxy_announced': {
      if (a.force_proxy_type != null && !PROXY_TYPES.includes(a.force_proxy_type)) throw new RangeError('unknown force_proxy_type')
      const innerIntent = new Intent(a.inner_op, a.inner_args), hotkey = intentSignerRole(innerIntent) === 'hotkey'
      let call: IntentDescriptor
      if (a.announced_call_data != null) {
        if (typeof a.announced_call_data !== 'string' || !/^0x(?:[0-9a-f]{2})+$/i.test(a.announced_call_data)) throw new TypeError('announced_call_data must be nonempty 0x-prefixed byte hex')
        const data = bytes(a.announced_call_data)
        const hash = await required(reader.hashCallData, 'hashCallData').call(reader, data)
        if (hash.length !== 32) throw new RangeError('call hash must be 32 bytes')
        if (a.expected_call_hash != null && (typeof a.expected_call_hash !== 'string' || a.expected_call_hash.toLowerCase() !== hex(hash))) throw new Error('saved call bytes do not match the selected announcement hash')
        const decoded = await required(reader.decodeCall, 'decodeCall').call(reader, data)
        if (a.announced_spec_version != null && unsigned(await required(reader.specVersion, 'specVersion').call(reader), 'runtime spec version') !== unsigned(a.announced_spec_version, 'announced spec version')) {
          if (a.announced_call_semantics == null) throw new Error('runtime changed since announcement without original decoded semantics; cancel and re-announce')
          if (!sameCallSemantics(decoded, a.announced_call_semantics)) throw new Error('runtime metadata now decodes the announced bytes differently; cancel and re-announce')
        }
        call = { ...decodedCallDescriptor(decoded), callData: hex(data) }
        validateAnnouncedCall(call)
        const registration = await committedRegistrationSponsorships(call, reader, a.real_ss58)
        plan.requiredFreeByAccount = registration.reserves
        // Replay exact approved bytes. Never infer bounded spending or subnet scope
        // from the user's descriptive intent, which can differ from those bytes.
        plan.spendRao = 'unbounded'; plan.affectsAllSubnets = true; plan.global = true
        plan.dispatchOrigin = a.real_ss58
        const shielding = announcedShielding(innerIntent, call)
        plan.mevShieldDefault ||= shielding.default; plan.mevShieldRequired ||= shielding.required
        plan.extras = { ...plan.extras, announced_call_hash: hex(hash), registrationSponsorships: registration.sponsorships }
      } else {
        if (a.expected_call_hash != null || a.announced_spec_version != null || a.announced_call_semantics != null) throw new TypeError('saved announcement checks require announced_call_data')
        const inner = await innerIntent.prepare(reader, { coldkey: a.real_ss58, hotkey: hotkey ? a.real_ss58 : undefined, hotkeyPublicKey: hotkey ? required(reader.decodeAddress, 'decodeAddress').call(reader, a.real_ss58) : undefined, feePayer: plan.feePayer, accountMode: context.accountMode })
        mergeChild(plan, inner)
        plan.dispatchOrigin = inner.dispatchOrigin
        call = inner.call
      }
      if (announcedCallRequiresDirect(call)) plan.extras = { ...plan.extras, requires_direct_submission: true }
      plan.call = descriptor('Proxy', 'proxy_announced', { delegate: a.delegate_ss58, real: a.real_ss58, force_proxy_type: a.force_proxy_type, call }); break
    }
    case 'multisig_threshold_1': case 'multisig_execute': case 'multisig_approve': case 'multisig_cancel': await multisig(plan, reader, context, a, pinnedMultisigs.get(intent)); break
    case 'batch': {
      if (!Array.isArray(a.intents)) throw new TypeError('batch intents must be an array')
      const children: PreparedIntent[] = []
      for (const item of a.intents) {
        const child = item instanceof Intent ? item : intentFromJSON(item)
        evaluatePolicy('transaction.merge_intents', { batch: true, children: [{ op: child.op, signer: intentSignerRole(child) }] })
        children.push(await child.prepare(reader, context))
      }
      evaluatePolicy('transaction.merge_intents', { batch: true, children: children.map(child => ({ op: child.op, signer: child.signerRole, requires_direct: Boolean(child.extras.requires_direct_submission) })) })
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
      plan.call = simpleCall(op, a); break
    case 'update_crowdloan_cap': plan.call = simpleCall(op, a); break
    case 'update_crowdloan_end': plan.call = simpleCall(op, a); break
    case 'update_crowdloan_min_contribution': plan.call = simpleCall(op, a); break
    case 'set_crowdloan_max_contribution': plan.call = simpleCall(op, a); break
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
  const paths = evaluatePolicy<number[][]>('protected.flatten_batch_paths', { calls })
  // Rust selects safe leaves; retain the original descriptors and their
  // semanticCall, callData, signerRole, and opaque language values.
  const leaves = paths.map(path => {
    let leaf = calls[path[0]!]!
    for (const index of path.slice(1)) leaf = (leaf.params.calls as IntentDescriptor[])[index]!
    return leaf
  })
  return descriptor('Utility', 'batch_all', { calls: leaves })
}

function intentPolicyArgs(args: Readonly<Record<string, unknown>>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(args).map(([key, value]) => [key, value instanceof Balance ? value.rao : value]))
}
function intentMetadata(op: string, args: Readonly<Record<string, unknown>>) {
  return evaluatePolicy<{ signer: 'coldkey' | 'hotkey'; netuids: number[]; global: boolean; affects_all_subnets: boolean; spend_rao: string | null }>('transaction.intent_metadata', { op, args: intentPolicyArgs(args), signer: catalog.get(op)?.signer })
}
function simpleCall(op: string, args: Readonly<Record<string, unknown>>, extra: Record<string, unknown> = {}): IntentDescriptor {
  return evaluatePolicyPreservingValues<IntentDescriptor>('transaction.simple_call', { op, args: { ...intentPolicyArgs(args), ...extra } })
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
function proportionInput(value: unknown): unknown {
  if (typeof value === 'number' && (!Number.isFinite(value) || Number.isInteger(value) && !Number.isSafeInteger(value))) throw new RangeError('unsafe proportion input; use a decimal string or bigint')
  return value
}
async function freeBalance(reader: IntentReader, address: string): Promise<bigint> { return unsigned(rec(rec(await reader.query('System', 'Account', [address])).data).free ?? 0, 'free balance') }
async function freeAmount(value: Balance | 'all', reader: IntentReader, context: IntentContext): Promise<bigint> {
  if (value !== 'all') return rao(value)
  const [free, deposit] = await Promise.all([freeBalance(reader, context.coldkey), required(reader.constant, 'constant').call(reader, 'Balances', 'ExistentialDeposit')])
  return BigInt(evaluatePolicy<string>('financial.intent.free_amount', { free, existential_deposit: unsigned(deposit, 'existential deposit'), headroom: '500000' }))
}
async function stakeAmount(value: Balance | 'all', reader: IntentReader, context: IntentContext, hotkey: string, netuid: number): Promise<bigint> {
  if (value !== 'all') return rao(value)
  const info = await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid', [hotkey, context.coldkey, netuid])
  const amount = unsigned(rec(info).stake ?? 0, 'stake')
  return BigInt(evaluatePolicy<string>('financial.staking_amount', { kind: 'stake', amount }))
}
async function lockable(reader: IntentReader, coldkey: string, netuid: number): Promise<bigint> {
  const values = await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_availability_for_coldkeys', [[coldkey], [netuid]])
  const entries = accountRecord(values, coldkey, reader.decodeAddress?.bind(reader))
  const amount = unsigned(rec(subnetRecord(entries, netuid)).available ?? 0, 'available alpha')
  return BigInt(evaluatePolicy<string>('financial.staking_amount', { kind: 'lock', amount }))
}
async function price(reader: IntentReader, netuid: number): Promise<bigint> {
  const value = await reader.runtimeCall('SwapRuntimeApi', 'current_alpha_price', [netuid])
  if (value == null) throw new Error(`alpha price unavailable for netuid ${netuid}`)
  return unsigned(value, 'alpha price')
}
async function priceLimit(reader: IntentReader, netuid: number, rate: number, buy: boolean): Promise<bigint> {
  return BigInt(evaluatePolicy<string>('financial.intent.price_limit', { price: await price(reader, netuid), rate, buy }))
}
async function ratioLimit(reader: IntentReader, origin: number, destination: number, rate: number): Promise<bigint> {
  const [left, right] = await Promise.all([price(reader, origin), price(reader, destination)])
  return BigInt(evaluatePolicy<string>('financial.intent.ratio_limit', { left, right, rate }))
}
interface StakingRoute { amount: string | null; needs_limit: boolean; explicit_limit: boolean; limit_direction: 'buy' | 'sell' | 'ratio' | null }
function stakingDecision(op: string, args: Record<string, any>, facts: Record<string, unknown>): StakingRoute {
  return evaluatePolicy<StakingRoute>('financial.staking_decision', { op, args: intentPolicyArgs(args), ...facts })
}
function stakingCall(op: string, args: Record<string, any>, facts: Record<string, unknown>): IntentDescriptor {
  const selected = evaluatePolicy<{ call: IntentDescriptor; integer_paths: (string | number)[][] }>('financial.staking_call', { op, args: intentPolicyArgs(args), ...facts })
  for (const path of selected.integer_paths) {
    let target: any = selected.call
    for (const key of path.slice(0, -1)) target = target[key]
    const key = path[path.length - 1]!
    target[key] = BigInt(target[key])
  }
  return selected.call
}
async function stakingLimit(route: StakingRoute, args: Record<string, any>, reader: IntentReader): Promise<bigint | undefined> {
  if (!route.needs_limit) return undefined
  if (route.explicit_limit) return unsigned(args.limit_price_rao, 'limit_price_rao')
  if (route.limit_direction === 'ratio') return ratioLimit(reader, args.origin_netuid, args.dest_netuid, args.rate_tolerance)
  return priceLimit(reader, args.netuid, args.rate_tolerance, route.limit_direction === 'buy')
}
function mergeChild(plan: MutablePlan, child: PreparedIntent): void {
  const facts = (value: PreparedIntent) => ({ spend_rao: value.spendRao, netuids: value.netuids, affects_all_subnets: value.affectsAllSubnets, global: value.global, shield_default: value.mevShieldDefault, shield_required: value.mevShieldRequired })
  const merged = evaluatePolicy<{ spend_rao: string | null; netuids: number[]; affects_all_subnets: boolean; global: boolean; shield_default: boolean; shield_required: boolean }>('transaction.merge_intents', { children: [facts(plan), facts(child)] })
  plan.spendRao = merged.spend_rao == null || merged.spend_rao === 'unbounded' ? merged.spend_rao : BigInt(merged.spend_rao)
  plan.spendByAccount = [...plan.spendByAccount, ...child.spendByAccount]
  plan.netuids = merged.netuids; plan.affectsAllSubnets = merged.affects_all_subnets; plan.global = merged.global
  plan.mevShieldDefault = merged.shield_default; plan.mevShieldRequired = merged.shield_required
  plan.warnings = [...plan.warnings, ...child.warnings]
  plan.violations = [...plan.violations, ...child.violations]
  plan.effects = [...plan.effects, ...child.effects]
  plan.extras = { ...plan.extras, [child.op]: child.extras, registrationSponsorships:[...((plan.extras.registrationSponsorships as unknown[]) ?? []),...((child.extras.registrationSponsorships as unknown[]) ?? [])] }
  if (child.extras.requires_direct_submission) plan.extras = { ...plan.extras, requires_direct_submission: true }
  plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, ...child.requiredFreeByAccount]
}

async function optionalClaim(plan: MutablePlan, args: Record<string, any>, reader: IntentReader, context: IntentContext, hotkey: string, netuid: number): Promise<void> {
  const claim = evaluatePolicy<{ claim: boolean; error?: string }>('financial.staking_claim', { claim: args.claim, netuid })
  if (claim.error === 'non_root') throw new RangeError('claim is only valid on root')
  if (!claim.claim) return
  const hold = unsigned(await reader.query('SubtensorModule', 'RootStakeUnlockInterval', []) ?? 0, 'root hold interval')
  if (evaluatePolicy<{ error?: string }>('financial.staking_claim', { claim: true, netuid, interval: hold }).error === 'hold') throw new RangeError('claim then unstake/move is unavailable while RootStakeUnlockInterval is nonzero; claim first and wait')
  plan.call = batchCall([descriptor('SubtensorModule', 'claim_root_with_hotkey', { hotkey }), plan.call])
  plan.affectsAllSubnets = true
  plan.warnings = [...plan.warnings, 'claims the entire validator basket entitlement, not a proportional slice']
  await claimPreflight(plan, reader, context, hotkey)
}

/** Fail-closed admission checks: failed state reads never permit an unverifiable root claim. */
async function claimPreflight(plan: MutablePlan, reader: IntentReader, context: IntentContext, hotkey?: string): Promise<void> {
  const positions = list(await reader.runtimeCall('StakeInfoRuntimeApi', 'get_stake_info_for_coldkey', [context.coldkey]))
  const root = new Set(positions.map(rec).filter(row => Number(row.netuid) === 0 && unsigned(row.stake, 'stake') > 0n).map(row => String(row.hotkey)))
  let selected: string[], scans = 0
  let relationships: Awaited<ReturnType<typeof readRootClaimRelationships>>
  if (hotkey) {
    selected = [hotkey]
    relationships = await readRootClaimRelationships(reader, context.coldkey, selected, root)
  } else {
    const keys = list(await reader.query('SubtensorModule', 'StakingHotkeys', [context.coldkey]) ?? []).map(String)
    const credits = await required(reader.queryMap, 'queryMap').call(reader, 'SubtensorModule', 'HotkeySwapBasketCredits', [context.coldkey], 257)
    const creditKeys = new Set(credits.map(([key]) => String(Array.isArray(key) ? key[0] : key)))
    relationships = await readRootClaimRelationships(reader, context.coldkey, [...new Set([...keys, ...creditKeys])], root)
    selected = relationships.filter(row => row.selected || creditKeys.has(row.hotkey)).map(row => row.hotkey)
    scans = keys.length + credits.length
  }
  const unquotedHotkeys = relationships.filter(row => selected.includes(row.hotkey) && row.hasUnquotedDeferredEntitlement).length
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
      // The current runtime disables LegacyAlphaCompatibility and counts AlphaV2 only.
      const [current, queued] = await Promise.all([
        countKeys('SubtensorModule', 'AlphaV2', [key, escrow], 257),
        countKeys('SubtensorModule', 'PendingBasketDeposits', [key], 513),
      ])
      return { holdings: small(current, 257, 'holding count'), queued: small(queued, 513, 'pending deposit count') }
    }))
    holdingCounts.push(...baskets.map(basket => basket.holdings))
    holdings += baskets.reduce((sum, basket) => sum + basket.holdings, 0)
    queuedCredits += baskets.reduce((sum, basket) => sum + basket.queued, 0)
  }
  const limit = hotkey ? 130 : 256
  const admission = evaluatePolicy<{ too_heavy: boolean; queued_exceeded: boolean }>('financial.root_claim.admission', { hotkeys: selected.length, holdings, selection: scans, limit, queued: queuedCredits })
  if (admission.too_heavy) plan.violations = [...plan.violations, `root claim exceeds ${limit}-unit admission limit (${selected.length} hotkeys + ${holdings} holdings, ${scans} selection scans); claim validators separately or reduce basket work`]
  if (admission.queued_exceeded) plan.violations = [...plan.violations, `root claim exceeds the 512-row pending deposit flush allowance (${queuedCredits} queued rows)`]
  const reserveAccount = reader.resolveFeePayer ? await reader.resolveFeePayer(plan.call, plan.feePayer) : plan.feePayer
  const free = await freeBalance(reader, reserveAccount)
  let reserved: bigint, exact = false
  try { reserved = unsigned(await required(reader.estimateFee, 'estimateFee').call(reader, plan.call, plan.feePayer), 'fee'); exact = true }
  catch { reserved = rootClaimChargedFeeRao(limit); plan.warnings = [...plan.warnings, 'exact fee estimate unavailable; claim reserve uses the runtime allowance fee floor'] }
  if (!evaluatePolicy<{ affordable: boolean }>('transaction.multisig_funding', { free, deposit: 0, fee: reserved }).affordable) plan.violations = [...plan.violations, `free TAO ${free} is below reserved claim fee ${reserved}`]
  plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, { address: reserveAccount, requiredRao: reserved, availableRao: free }]
  if (exact) plan.estimatedFeeRao = reserved
  plan.preview = { ...plan.preview, rootClaim: { selectedHotkeys: selected, relationships, holdings, holdingCounts, queuedCredits, selectionScans: scans, admissionLimit: limit, reservedFeeRao: reserved } }
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
    const quote = quoteRootClaimPreviews(previews, reserved, storedThreshold > 0n ? storedThreshold : 500000n, scans, limit, unquotedHotkeys)
    plan.preview = { ...plan.preview, rootClaim: { ...rec(plan.preview.rootClaim), ...quote } }
    if (!quote.previewComplete) plan.warnings = [...plan.warnings, 'basket-share preview excludes independently redeemable deferred alpha and cash; total redeemable value requires runtime settlement, so accruedRao and redeemableRao are unknown']
    if (quote.redeemableRao === 0n) plan.warnings = [...plan.warnings, 'claim has no currently redeemable yield; a scan fee is still charged']
    else if (quote.losesMoney) plan.warnings = [...plan.warnings, 'estimated spent fee exceeds redeemable yield']
    if (quote.belowThresholdHotkeys > 0) plan.warnings = [...plan.warnings, `${quote.belowThresholdHotkeys} basket-share claims are below the payout threshold and keep accruing`]
    if (quote.dustRows > 0) plan.warnings = [...plan.warnings, `${quote.dustRows} dust rows are skipped; estimated forfeited slices total ${quote.forfeitedRao} rao`]
    if (quote.possibleRefundRao > 0n) plan.warnings = [...plan.warnings, 'spent fee is a floor; pending deposit flush work can reduce the estimated refund']
  } catch { plan.warnings = [...plan.warnings, 'basket yield preview unavailable; admission and fee reserve were checked separately'] }
}
function signedInteger(value: unknown): bigint { if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('unsafe signed integer'); if (typeof value !== 'number' && typeof value !== 'bigint' && !(typeof value === 'string' && /^-?\d+$/.test(value))) throw new TypeError('invalid signed integer'); return BigInt(value) }
function announcedShielding(intent: Intent, call: IntentDescriptor): { default: boolean; required: boolean } {
  const result = { default: false, required: false }
  const add = (tool: CatalogEntry) => { result.default ||= tool.mev_shield_default; result.required ||= tool.mev_shield_required }
  const semantic = (value: Intent, depth = 0): void => {
    if (depth > 64) throw new RangeError('announced intent nesting exceeds 64')
    add(catalog.get(value.op)!)
    if (value.op === 'batch') for (const child of value.args.intents as Array<Intent | Record<string, unknown>>) semantic(intentFromJSON(child), depth + 1)
    else if (['multisig_approve', 'multisig_execute', 'multisig_threshold_1'].includes(value.op)) semantic(intentFromJSON(value.args.call as Intent | Record<string, unknown>), depth + 1)
    else if (value.op === 'execute_proxy_announced') semantic(new Intent(value.args.inner_op as IntentOperation, value.args.inner_args as Record<string, unknown>), depth + 1)
  }
  semantic(intent)
  for (const leaf of announcedCallLeaves(call)) {
    const tools = [...catalog.values()].filter(tool => tool.wraps.some(([pallet, fn]) => pallet === leaf.pallet && fn === leaf.call))
    // Opaque-to-the-catalog calls cannot weaken shielding declared by an inner call.
    if (!tools.length) result.default = result.required = true
    else tools.forEach(add)
  }
  return result
}
async function emissionWeights(plan: MutablePlan, reader: IntentReader, context: IntentContext, a: Record<string, any>): Promise<void> {
  if (!Array.isArray(a.slots) || !Array.isArray(a.values) || a.slots.length !== a.values.length) throw new RangeError('emission slots and values must have the same length')
  const gammaUpdates = emission.parseGammaUpdates(a.gamma ?? {})
  if (!a.slots.length && !Object.keys(gammaUpdates).length) throw new RangeError('name at least one emission slot or Gamma share to set')
  const slots = a.slots.map(emission.slotIndex)
  if (new Set(slots).size !== slots.length) throw new RangeError('each emission slot may appear only once')
  const updates: Record<string, bigint> = Object.fromEntries(slots.map((slot: number, index: number) => [slot, emission.parseShare(a.values[index])]))
  if (a.netuid === 0) throw new Error('root has no emission vector')
  const current = rec(await reader.runtimeCall('SubnetInfoRuntimeApi', 'get_subnet_emission_weights', [a.netuid]))
  if (!Array.isArray(current.weights)) throw new Error('subnet emission vector unavailable; root has no emission vector')
  const before = current.weights.map((value: unknown) => unsigned(value, 'emission share'))
  const gammaBefore = emission.gammaEmissionShares(await reader.query('SubtensorModule', 'SubnetGammaEmission', [a.netuid]))
  const gammaAfter = emission.mergeGamma(gammaBefore, gammaUpdates)
  const gammaShare = Object.values(gammaAfter).reduce((sum, value) => sum + value, 0n)
  const after = a.renormalize ? emission.renormalize(before, updates, gammaShare) : emission.absorbRounding(emission.mergeWeights(before, updates), gammaShare)
  const [whitelist, enabled, lease] = await Promise.all([
    required(reader.queryMap, 'queryMap').call(reader, 'SubtensorModule', 'SubnetGammaEmissionWhitelisted', []),
    required(reader.queryMap, 'queryMap').call(reader, 'SubtensorModule', 'SubnetGammaEnabled', []),
    reader.query('SubtensorModule', 'SubnetUidToLeaseId', [a.netuid]),
  ])
  const key = (value: unknown) => emission.gammaTarget(Array.isArray(value) && value.length === 1 ? value[0] : value)
  const active = new Set(enabled.filter(([, value]) => Boolean(value)).map(([id]) => key(id)))
  const targets = Object.fromEntries(whitelist.filter(([, value]) => Boolean(value)).map(([id]) => [key(id), active.has(key(id))]))
  const baseline = unsigned(current.root_baseline_share, 'root baseline share')
  const rules: emission.EmissionRules = { min_validator: emission.minValidatorParts(baseline), min_owner: emission.DEFAULT_OWNER, created_via_lease: Boolean(current.created_via_lease), gamma_share: gammaShare, gamma_targets: targets, leased: lease != null }
  const violations = [...emission.validate(after, rules), ...emission.validateGamma(a.netuid, gammaAfter, rules)]
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
  const split: Array<[unknown, bigint]> = emission.SLOT_NAMES.map((name, index) => [name[0].toUpperCase() + name.slice(1), after[index]])
  split.push(...Object.entries(gammaAfter).map(([target, share]): [unknown, bigint] => [{ Gamma: { netuid: Number(target) } }, share]))
  plan.call = descriptor('AdminUtils', 'sudo_set_subnet_emission_weights_v2', { netuid: a.netuid, split })
  plan.netuids = [...new Set([...plan.netuids, ...Object.keys(gammaAfter).map(Number), ...Object.keys(gammaUpdates).map(Number)])].sort((a, b) => a - b)
  if (Object.entries(gammaBefore).some(([target, share]) => share > 0n && !Object.hasOwn(gammaAfter, target))) plan.warnings = [...plan.warnings, 'removed Gamma targets return their unconverted alpha to miners']
  plan.preview = { before, after, gamma_share: gammaShare, gamma: { before: gammaBefore, after: gammaAfter }, gamma_targets: targets, leased: lease != null, baselineRootShare: baseline, rateLimit, changed: emission.SLOT_NAMES.filter((_, index) => before[index] !== after[index]) }
}

async function multisig(plan: MutablePlan, reader: IntentReader, context: IntentContext, a: Record<string, any>, pinned?: { inner: PreparedIntent; origin: string }): Promise<void> {
  const threshold = plan.op === 'multisig_threshold_1' ? 1 : small(a.threshold, 65535, 'threshold')
  const mode = evaluatePolicy<'legacy' | 'safe'>('transaction.account_mode', { mode: a.account_mode ?? context.accountMode ?? 'legacy' })
  const suffix = mode === 'safe' ? '_safe' : ''
  const decode = required(reader.decodeAddress, 'decodeAddress').bind(reader)
  if (!Array.isArray(a.other_signatories)) throw new TypeError('other_signatories must be an array')
  const members = evaluatePolicy<{ indices: number[] }>('transaction.multisig_members', {
    members: a.other_signatories.map((address: string) => decode(address)), signer: decode(context.coldkey),
    threshold, minimum_threshold: plan.op === 'multisig_threshold_1' ? 1 : 2, others_only: true, deduplicate: true,
  })
  const others = members.indices.map(index => a.other_signatories[index])
  const account = required(reader.multisigAccount, 'multisigAccount').call(reader, [context.coldkey, ...others], threshold, mode === 'safe')
  if (plan.op === 'multisig_cancel') {
    // A semantic intent can depend on live prices, balances, or metadata. Its
    // newly prepared hash cannot identify an already committed operation.
    const pending = a.call_hash == null
      ? (await required(reader.queryMap, 'queryMap (or pass the committed call_hash)').call(reader, 'Multisig', 'Multisigs', [account.address])).map(([key, value]) => ({ call_hash: Array.isArray(key) && key.length === 1 ? key[0] : key, timepoint: rec(value).when }))
      : undefined
    const committed = evaluatePolicy<{ call_hash: string; timepoint: { height: number; index: number } }>('transaction.multisig_cancel', { call_hash: a.call_hash, timepoint: a.timepoint, pending })
    plan.call = descriptor('Multisig', 'cancel_as_multi' + suffix, { threshold, other_signatories: others, timepoint: committed.timepoint, call_hash: bytes(committed.call_hash) })
    plan.spendRao = null; plan.spendByAccount = []; plan.requiredFreeByAccount = []; plan.netuids = []; plan.affectsAllSubnets = false; plan.global = true; plan.mevShieldRequired = false; plan.mevShieldDefault = false; plan.violations = []
    plan.extras = { ...plan.extras, multisig_call_hash: committed.call_hash, multisig_account: account.address }
    return
  }
  const semantic = intentFromJSON(a.call), innerRole = intentSignerRole(semantic)
  let innerCall: IntentDescriptor
  if (pinned) {
    if (hex(decode(pinned.origin)) !== hex(account.publicKey)) throw new Error('prepared call belongs to a different multisig account')
    // Only call arguments and their verified scope are pinned. Funding is read
    // again, and any outer-member fee obligation follows the current signer.
    const original = pinned.inner
    const feeOwner = reader.resolveFeePayer ? await reader.resolveFeePayer(original.call, plan.feePayer) : plan.feePayer
    const reserves = await Promise.all(original.requiredFreeByAccount.map(async reserve => {
      const outerFee = hex(decode(reserve.address)) === hex(decode(original.feePayer)) && hex(decode(reserve.address)) !== hex(decode(original.dispatchOrigin))
      const address = outerFee ? feeOwner : reserve.address
      return { address, requiredRao: outerFee ? await required(reader.estimateFee, 'estimateFee').call(reader, original.call, plan.feePayer) : reserve.requiredRao, availableRao: await freeBalance(reader, address) }
    }))
    mergeChild(plan, { ...original, feePayer: plan.feePayer, requiredFreeByAccount: reserves })
    plan.dispatchOrigin = pinned.inner.dispatchOrigin
    innerCall = pinned.inner.call
    const registration = await committedRegistrationSponsorships(innerCall, reader, account.address)
    plan.extras = { ...plan.extras, registrationSponsorships: registration.sponsorships }
  } else if (a.call_data != null) {
    const data = bytes(a.call_data)
    if (!data.length) throw new TypeError('multisig call_data must not be empty')
    if (a.call_spec_version != null && unsigned(await required(reader.specVersion, 'specVersion').call(reader), 'runtime spec version') !== unsigned(a.call_spec_version, 'call spec version')) throw new Error('runtime changed since multisig approval; verify the original call under the new metadata')
    innerCall = { ...decodedCallDescriptor(await required(reader.decodeCall, 'decodeCall').call(reader, data)), callData: hex(data) }
    validateAnnouncedCall(innerCall)
    plan.spendRao = 'unbounded'; plan.spendByAccount = []; plan.netuids = []; plan.affectsAllSubnets = true; plan.global = true
    plan.dispatchOrigin = account.address
    const shielding = announcedShielding(semantic, innerCall)
    plan.mevShieldDefault ||= shielding.default; plan.mevShieldRequired ||= shielding.required
    const registration = await committedRegistrationSponsorships(innerCall, reader, account.address)
    plan.requiredFreeByAccount = registration.reserves
    plan.extras = { ...plan.extras, registrationSponsorships: registration.sponsorships }
    if (announcedCallRequiresDirect(innerCall)) plan.extras = { ...plan.extras, requires_direct_submission: true }
  } else {
    const inner = await semantic.prepare(reader, { coldkey: account.address, hotkey: innerRole === 'hotkey' ? account.address : undefined, hotkeyPublicKey: innerRole === 'hotkey' ? account.publicKey : undefined, feePayer: plan.feePayer, accountMode: mode })
    mergeChild(plan, inner)
    plan.dispatchOrigin = inner.dispatchOrigin
    innerCall = inner.call
  }
  const params: Record<string, unknown> = { other_signatories: others }
  const hash = await required(reader.hashCall, 'hashCall').call(reader, innerCall)
  if (hash.length !== 32) throw new RangeError('call hash must be 32 bytes')
  const pending = a.timepoint == null ? null : await reader.query('Multisig', 'Multisigs', [account.address, hash])
  evaluatePolicy('transaction.multisig_commitment', { actual_hash: hash, expected_hash: a.call_hash, timepoint: a.timepoint, pending })
  const data = innerCall.callData ?? (reader.composeCall ? hex(await reader.composeCall(innerCall)) : undefined)
  if (data != null) plan.extras = { ...plan.extras, multisig_call_data: typeof data === 'string' ? data : hex(data), ...(reader.specVersion ? { multisig_call_spec_version: await reader.specVersion() } : {}) }
  if (plan.op === 'multisig_threshold_1') {
    params.call = innerCall; plan.call = descriptor('Multisig', 'as_multi_threshold_1' + suffix, params)
  } else {
    params.threshold = threshold
    const timepoint = a.timepoint == null ? null : { height: small(a.timepoint.height, 0xffffffff, 'timepoint height'), index: small(a.timepoint.index, 0xffffffff, 'timepoint index') }
    params.maybe_timepoint = timepoint
    params.max_weight = await required(reader.estimateWeight, 'estimateWeight').call(reader, innerCall, plan.feePayer)
    const hashOnly = plan.op === 'multisig_approve' && timepoint != null
    if (hashOnly) params.call_hash = hash
    else params.call = innerCall
    plan.call = { ...descriptor('Multisig', (hashOnly ? 'approve_as_multi' : 'as_multi') + suffix, params), ...(hashOnly ? { semanticCall: innerCall } : {}) }
    let deposit = 0n
    if (timepoint == null) {
      const constant = required(reader.constant, 'constant').bind(reader)
      const [base, factor] = await Promise.all([constant('Multisig', 'DepositBase'), constant('Multisig', 'DepositFactor')])
      deposit = BigInt(evaluatePolicy<string>('transaction.multisig_deposit', { base, threshold, factor }))
    }
    const fee = await required(reader.estimateFee, 'estimateFee').call(reader, plan.call, plan.feePayer)
    const feeAccount = reader.resolveFeePayer ? await reader.resolveFeePayer(plan.call, plan.feePayer) : plan.feePayer
    const [depositFree, feeFree] = await Promise.all([freeBalance(reader, context.coldkey), freeBalance(reader, feeAccount)])
    plan.requiredFreeByAccount = [...plan.requiredFreeByAccount, { address: context.coldkey, requiredRao: deposit, availableRao: depositFree }, { address: feeAccount, requiredRao: fee, availableRao: feeFree }]
    plan.estimatedFeeRao = fee
    const depositId = hex(decode(context.coldkey)), feeId = hex(decode(feeAccount))
    const funding = evaluatePolicy<{ required: Array<{ affordable: boolean }> }>('financial.affordability', { spends: [{ account: depositId, amount: deposit }], charges: [{ account: feeId, amount: fee }], free: { [feeId]: feeFree, [depositId]: depositFree } })
    if (funding.required.some(row => !row.affordable)) plan.violations = [...plan.violations, `multisig dispatch account needs ${deposit} rao for deposit and fee account needs ${fee} rao for fee`]
  }
  plan.extras = { ...plan.extras, multisig_call_hash: hex(hash), multisig_account: account.address }
}
