/** Mirrors runtime/src/fee_filters.rs. These signing-hotkey calls charge its owner. */
export const COLDKEY_PAYS_FEE_CALLS: ReadonlySet<string> = new Set([
  'set_weights', 'set_mechanism_weights', 'batch_set_weights', 'commit_weights',
  'commit_mechanism_weights', 'batch_commit_weights', 'commit_crv3_mechanism_weights',
  'commit_timelocked_weights', 'commit_timelocked_mechanism_weights', 'reveal_weights',
  'reveal_mechanism_weights', 'batch_reveal_weights', 'serve_axon', 'serve_axon_tls',
  'serve_prometheus', 'associate_evm_key',
].map(name => `SubtensorModule.${name}`).concat(['Commitments.set_commitment']))
export const COLDKEY_FEE_WARNING = "transaction fee is charged to the hotkey owner's coldkey, not the signing hotkey"

type CallRecord = Record<string, any>
function callRecord(value: unknown): CallRecord | null {
  if (Array.isArray(value) && value.length >= 2 && typeof value[0] === 'string' && typeof value[1] === 'string') return { call_module: value[0], call_function: value[1], params: value[2] ?? {} }
  if (!value || typeof value !== 'object') return null
  const row = value as CallRecord
  if (row.call_module && row.call_function) return row
  if (typeof row.module === 'string' && typeof row.function === 'string') return { call_module: row.module, call_function: row.function, params: row.params ?? {} }
  if (typeof row.pallet === 'string' && typeof (row.fn ?? row.call) === 'string') return { call_module: row.pallet, call_function: row.fn ?? row.call, params: row.params ?? {} }
  if (row.value?.call_module && row.value?.call_function) return row.value
  // Runtime SCALE call-enum shape: { Utility: { batch: { calls: [...] } } }.
  const entries = Object.entries(row)
  if (entries.length === 1 && entries[0][1] && typeof entries[0][1] === 'object') {
    const [pallet, nested] = entries[0], methods = Object.entries(nested)
    if (methods.length === 1) return { call_module: pallet, call_function: methods[0][0], params: methods[0][1] }
  }
  return null
}
export function* iterCallLeaves(call: unknown): Generator<readonly [string, string]> {
  const pending: Array<{ value: unknown; exit?: boolean }> = [{ value: call }]
  const seen = new Set<object>()
  while (pending.length) {
    const { value: current, exit } = pending.pop()!
    if (exit) { seen.delete(current as object); continue }
    if (current && typeof current === 'object') {
      if (seen.has(current)) throw new TypeError('cyclic call structure')
      seen.add(current)
      pending.push({ value: current, exit: true })
    }
    const row = callRecord(current)
    if (!row) continue
    const module = String(row.call_module), fn = String(row.call_function)
    const arg = (name: string) => row.params?.[name] ?? row.call_args?.find((a: any) => a.name === name)?.value
    if (module === 'Utility' && ['batch', 'batch_all', 'force_batch'].includes(fn)) { pending.push(...[...(arg('calls') ?? [])].reverse().map(value => ({ value }))); continue }
    if ((module === 'Proxy' && ['proxy','proxy_announced'].includes(fn)) || (module === 'Multisig' && ['as_multi', 'as_multi_threshold_1','as_multi_safe','as_multi_threshold_1_safe'].includes(fn)) || (module === 'Utility' && ['as_derivative','as_safe_derivative','as_protected_derivative'].includes(fn))) { const inner = arg('call'); if (inner != null) pending.push({ value: inner }); continue }
    yield [module, fn]
  }
}
export function chargesColdkeyFee(call: unknown): boolean {
  for (const [pallet, fn] of iterCallLeaves(call)) if (COLDKEY_PAYS_FEE_CALLS.has(`${pallet}.${fn}`)) return true
  return false
}
export const charges_coldkey_fee = chargesColdkeyFee
export const iter_call_leaves = iterCallLeaves
