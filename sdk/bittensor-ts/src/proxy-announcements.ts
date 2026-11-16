import type { IntentDescriptor } from './intents'
import { INTENT_CATALOG } from './intent-catalog'

const SINGLE_CALL_WRAPPERS = new Set([
  'Proxy.proxy', 'Proxy.proxy_announced', 'Multisig.as_multi', 'Multisig.as_multi_safe',
  'Multisig.as_multi_threshold_1', 'Multisig.as_multi_threshold_1_safe',
  'Utility.as_derivative', 'Utility.as_safe_derivative', 'Utility.as_protected_derivative',
  'Utility.dispatch_as', 'Utility.dispatch_as_fallible', 'Utility.with_weight',
  'Sudo.sudo', 'Sudo.sudo_unchecked_weight', 'Sudo.sudo_as',
])
const BATCHES = new Set(['Utility.batch', 'Utility.batch_all', 'Utility.force_batch'])
// These are the inline descriptor wrappers supported by core/transaction/operations.rs.
// Other runtime dispatch wrappers must not become a raw-call escape hatch here.
const TRUSTED_INLINE_WRAPPERS = new Set([
  'Proxy.proxy', 'Proxy.proxy_announced', 'Multisig.as_multi', 'Multisig.as_multi_safe',
  'Multisig.as_multi_threshold_1', 'Multisig.as_multi_threshold_1_safe',
  'Utility.as_derivative', 'Utility.as_safe_derivative', 'Utility.as_protected_derivative', 'Sudo.sudo',
])
const HASH_ONLY_WRAPPERS = new Set(['Multisig.approve_as_multi', 'Multisig.approve_as_multi_safe', 'Multisig.cancel_as_multi', 'Multisig.cancel_as_multi_safe'])
const ROOT_OPERATIONS = new Set([
  'AdminUtils.sudo_set_subnet_emission_enabled', 'AdminUtils.sudo_set_subnet_gamma_emission_whitelisted',
  'SubtensorModule.sudo_set_root_claim_threshold', 'AdminUtils.sudo_set_evm_chain_id', 'EVM.disable_whitelist',
])
const TRUSTED_LEAVES = new Set<string>(INTENT_CATALOG.flatMap(tool => tool.wraps.map(([pallet, call]) => `${pallet}.${call}`)).filter(operation =>
  !SINGLE_CALL_WRAPPERS.has(operation) && !BATCHES.has(operation) && !HASH_ONLY_WRAPPERS.has(operation) && operation !== 'Crowdloan.create'))
// Constructor-supported guards and legacy payloads that have no standalone intent.
for (const operation of [
  'HashedAccounts.check_registered', 'HashedAccounts.check_registration_deposit', 'HashedAccounts.register',
  'SubtensorModule.set_weights', 'AdminUtils.sudo_set_subnet_emission_weights',
  'AdminUtils.sudo_set_evm_chain_id', 'EVM.disable_whitelist',
]) TRUSTED_LEAVES.add(operation)

/** Saved payloads must stay inside the same trusted operation boundary as native. */
export function validateAnnouncedCall(call: IntentDescriptor, depth = 0): void {
  if (depth > 64) throw new RangeError('announced call nesting exceeds 64')
  const operation = `${call.pallet}.${call.call}`
  const descend = (value: unknown) => validateAnnouncedCall(decodedCallDescriptor(value), depth + 1)
  if (operation === 'Utility.batch_all') {
    if (!Array.isArray(call.params.calls) || !call.params.calls.length) throw new TypeError('saved atomic batch requires calls')
    call.params.calls.forEach(descend)
  } else if (TRUSTED_INLINE_WRAPPERS.has(operation)) {
    const child = decodedCallDescriptor(call.params.call)
    if (operation === 'Sudo.sudo' && !rootDescriptor(child)) throw new TypeError('trusted sudo only wraps declared root operations')
    descend(child)
  } else if (operation === 'Crowdloan.create') {
    if (call.params.call == null) {
      if (call.params.target_address == null) throw new TypeError('crowdloan needs a target or call')
    } else {
      if (call.params.target_address != null) throw new TypeError('crowdloan call and target are mutually exclusive')
      descend(call.params.call)
    }
  } else if (!TRUSTED_LEAVES.has(operation)) {
    throw new TypeError(`unsupported saved announcement operation ${operation}; semantic replay requires a trusted operation`)
  }
}

function rootDescriptor(call: IntentDescriptor): boolean {
  const operation = `${call.pallet}.${call.call}`
  if (ROOT_OPERATIONS.has(operation)) return true
  return operation === 'Utility.batch_all' && Array.isArray(call.params.calls) && call.params.calls.length > 0 && call.params.calls.every(child => rootDescriptor(decodedCallDescriptor(child)))
}

/** Convert the actual runtime decoder's call representation, never a declared intent. */
export function decodedCallDescriptor(value: unknown, depth = 0): IntentDescriptor {
  if (depth > 64) throw new RangeError('announced call nesting exceeds 64')
  if (value == null || typeof value !== 'object' || Array.isArray(value) || value instanceof Uint8Array) throw new TypeError('announced RuntimeCall cannot be inspected')
  const row = value as Record<string, any>
  let pallet: unknown, call: unknown, params: unknown
  if (typeof row.call_module === 'string' && typeof row.call_function === 'string') {
    pallet = row.call_module; call = row.call_function
    if (!Array.isArray(row.call_args) || row.call_args.some((arg: any) => !arg || typeof arg.name !== 'string')) throw new TypeError('announced call arguments unavailable')
    if (new Set(row.call_args.map((arg: any) => arg.name)).size !== row.call_args.length) throw new TypeError('duplicate announced call arguments')
    params = Object.fromEntries(row.call_args.map((arg: any) => [arg.name, arg.value]))
  } else if (typeof row.pallet === 'string' && typeof row.call === 'string') {
    pallet = row.pallet; call = row.call; params = row.params
  } else {
    const pallets = Object.entries(row)
    if (pallets.length !== 1 || !pallets[0][1] || typeof pallets[0][1] !== 'object') throw new TypeError('announced RuntimeCall has no call identity')
    const variants = Object.entries(pallets[0][1])
    if (variants.length !== 1) throw new TypeError('announced RuntimeCall has no call identity')
    pallet = pallets[0][0]; call = variants[0][0]; params = variants[0][1]
  }
  if (typeof pallet !== 'string' || !pallet || typeof call !== 'string' || !call || params == null || typeof params !== 'object' || Array.isArray(params)) throw new TypeError('announced RuntimeCall has invalid parameters')
  const args = { ...params as Record<string, unknown> }, operation = `${pallet}.${call}`
  if (SINGLE_CALL_WRAPPERS.has(operation)) args.call = decodedCallDescriptor(args.call, depth + 1)
  else if (BATCHES.has(operation)) {
    if (!Array.isArray(args.calls)) throw new TypeError('announced batch has no inspectable calls')
    args.calls = args.calls.map(child => decodedCallDescriptor(child, depth + 1))
  } else if (operation === 'Utility.if_else') {
    args.main = decodedCallDescriptor(args.main, depth + 1)
    args.fallback = decodedCallDescriptor(args.fallback, depth + 1)
  } else if (operation === 'Crowdloan.create' && args.call != null) args.call = decodedCallDescriptor(args.call, depth + 1)
  return { pallet, call, params: args }
}

function children(call: IntentDescriptor): readonly unknown[] {
  const operation = `${call.pallet}.${call.call}`
  if (SINGLE_CALL_WRAPPERS.has(operation)) return [call.params.call]
  if (BATCHES.has(operation)) {
    if (!Array.isArray(call.params.calls)) throw new TypeError('announced batch has no inspectable calls')
    return call.params.calls
  }
  if (operation === 'Utility.if_else') return [call.params.main, call.params.fallback]
  return []
}

/** Atomic calls hidden behind proxy dispatch can consume an announcement on failure. */
export function announcedCallRequiresDirect(value: unknown, depth = 0): boolean {
  try {
    if (depth > 64) return true
    const call = decodedCallDescriptor(value)
    if (call.pallet === 'Utility' && call.call === 'batch_all' || call.pallet === 'Crowdloan' && call.call === 'finalize') return true
    return children(call).some(child => announcedCallRequiresDirect(child, depth + 1))
  } catch { return true }
}

export function* announcedCallLeaves(call: IntentDescriptor, depth = 0): Generator<IntentDescriptor> {
  if (depth > 64) throw new RangeError('announced call nesting exceeds 64')
  const nested = children(call)
  if (nested.length) for (const child of nested) yield* announcedCallLeaves(decodedCallDescriptor(child), depth + 1)
  else yield call
}

/** JSON-compatible comparison also accepts bigint snapshots without losing precision. */
export function sameCallSemantics(left: unknown, right: unknown): boolean {
  const normalize = (value: unknown, depth = 0): unknown => {
    if (depth > 128) throw new RangeError('announced semantics nesting exceeds 128')
    if (typeof value === 'bigint' || typeof value === 'number' && Number.isSafeInteger(value)) return String(value)
    if (value instanceof Uint8Array) return '0x' + Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('')
    if (Array.isArray(value)) return value.map(child => normalize(child, depth + 1))
    if (value != null && typeof value === 'object') return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([key, child]) => [key, normalize(child, depth + 1)]))
    return value
  }
  return JSON.stringify(normalize(left)) === JSON.stringify(normalize(right))
}
