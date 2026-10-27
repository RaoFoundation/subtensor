import { DEFAULT_OWNER, normalizeWeights, shares, shareToFloat, minValidatorParts, rateLimitStatus } from '../emission-weights'
import { type ReadView, type ChainRecord, st, api, num, integer, fixed, bal, blockOf, textRecord, utf8Text, absentFeature, keyOf, min } from './base'
export type SubnetLifecycleState = 'registered' | 'started' | 'pending_dissolution' | 'dissolving'
function decodeState(value: any): SubnetLifecycleState | null {
  if (value == null) return null
  const name = typeof value === 'object' ? Object.keys(value)[0] : String(value)
  const values: Record<string, SubnetLifecycleState> = { Registered: 'registered', Started: 'started', PendingDissolution: 'pending_dissolution', Dissolving: 'dissolving' }
  if (values[name]) return values[name]
  if (Object.values(values).includes(name as SubnetLifecycleState)) return name as SubnetLifecycleState
  throw new Error(`invalid SubnetLifecycleState ${name}`)
}
async function legacyState(view: ReadView, netuid: number): Promise<SubnetLifecycleState | null> {
  const [status, queue, added] = await Promise.all([view.query(st('CurrentDissolveCleanupStatus')), view.query(st('DissolveCleanupQueue')), view.query(st('NetworksAdded'), [netuid])])
  if (status && num(status.netuid) === netuid) return 'dissolving'
  if ((queue ?? []).map(num).includes(netuid)) return 'pending_dissolution'
  if (!added) return null
  if (netuid === 0) return 'started'
  const [first, enabled] = await Promise.all([view.query(st('FirstEmissionBlockNumber'), [netuid]), view.query(st('SubtokenEnabled'), [netuid])])
  return first != null || enabled ? 'started' : 'registered'
}
export async function subnetState(view: ReadView, netuid: number): Promise<SubnetLifecycleState | null> {
  view = await view.at()
  let raw: any
  try { raw = await view.query(st('SubnetState'), [netuid]) }
  catch (error) { if (!absentFeature(error)) throw error }
  if (raw != null) return decodeState(raw)
  return legacyState(view, netuid)
}
export async function subnetStates(view: ReadView): Promise<Record<number, SubnetLifecycleState>> {
  view = await view.at()
  let rows: Array<[any, any]> = []
  try { rows = await view.queryMap(st('SubnetState')) }
  catch (error) { if (!absentFeature(error)) throw error }
  if (rows.length) return Object.fromEntries(rows.flatMap(([key, raw]) => { const state = decodeState(raw); return state == null ? [] : [[num(keyOf(key)), state]] }))
  const [added, queue, status] = await Promise.all([view.queryMap(st('NetworksAdded')), view.query(st('DissolveCleanupQueue')), view.query(st('CurrentDissolveCleanupStatus'))])
  const ids = new Set<number>(added.filter(([, value]) => value).map(([key]) => num(keyOf(key))))
  for (const id of queue ?? []) ids.add(num(id))
  if (status) ids.add(num(status.netuid))
  return Object.fromEntries((await Promise.all([...ids].map(async id => [id, await legacyState(view, id)] as const))).filter(([, state]) => state != null)) as Record<number, SubnetLifecycleState>
}
export async function subnet(view: ReadView, netuid: number) {
  view = await view.at()
  const [tempo, burn, count, state, enabled] = await Promise.all([view.query(st('Tempo'), [netuid]), view.query(st('Burn'), [netuid]), view.query(st('SubnetworkN'), [netuid]), subnetState(view, netuid), view.query(st('SubnetEmissionEnabled'), [netuid])])
  return { netuid, tempo: num(tempo), burn: bal(view, burn), neuron_count: num(count), state, pool_emission_enabled: Boolean(enabled) }
}
export async function subnets(view: ReadView) {
  view = await view.at()
  const [states, ...maps] = await Promise.all([subnetStates(view), ...['Tempo', 'Burn', 'SubnetworkN', 'SubnetEmissionEnabled'].map(name => view.queryMap(st(name)).then(rows => Object.fromEntries(rows.map(([key, value]) => [num(keyOf(key)), value]))))])
  const [tempos, burns, counts, enabled] = maps as ChainRecord[]
  return Object.keys(states).map(Number).sort((a, b) => a - b).map(netuid => ({ netuid, tempo: num(tempos[netuid]), burn: bal(view, burns[netuid]), neuron_count: num(counts[netuid]), state: states[netuid] as SubnetLifecycleState, pool_emission_enabled: Boolean(enabled[netuid] ?? true) }))
}
export async function subnetEmissionWeights(view: ReadView, netuid: number) {
  view = await view.at()
  const info = await view.runtime(api('SubnetInfoRuntimeApi', 'get_subnet_emission_weights'), [netuid])
  if (info == null) return null
  const weights = normalizeWeights(info.weights)
  const [root, ain, aout, reservoir, global, last] = await Promise.all([
    view.query(st('SubnetTAO'), [0]), view.query(st('SubnetAlphaIn'), [netuid]), view.query(st('SubnetAlphaOut'), [netuid]),
    view.query(['Swap', 'BalancerAlphaReservoir'], [netuid]), view.query(st('TaoWeight')),
    view.query(st('LastRateLimitedBlock'), [{ OwnerHyperparamUpdate: [netuid, 'EmissionWeights'] }]),
  ])
  const baseline = integer(info.root_baseline_share), rootParts = min(integer(info.root_share), weights[3]), floor = minValidatorParts(baseline)
  const block = blockOf(view), lastChange = num(last)
  return { netuid: num(info.netuid), weights, shares: shares(weights), burn: integer(info.burn), recycle: integer(info.recycle), owner: integer(info.owner), validator: integer(info.validator), miner: integer(info.miner), global_tao_weight: shareToFloat(integer(global)), created_via_lease: Boolean(info.created_via_lease), root_tao: integer(root), alpha_issuance: integer(ain) + integer(aout) + integer(reservoir), root_share: shareToFloat(rootParts), baseline_root_share: shareToFloat(baseline), baseline_root_parts: baseline, validator_share_after_root: shareToFloat(weights[3] - rootParts), root_capped: baseline > weights[3], min_validator: floor, block, last_change_block: lastChange, rules: () => ({ min_validator: floor, min_owner: DEFAULT_OWNER, created_via_lease: Boolean(info.created_via_lease) }), rate_limit: () => rateLimitStatus(lastChange, block) }
}
export const subnetReads = {
  subnet_state: (view: ReadView, p: { netuid: number }) => subnetState(view, p.netuid),
  subnet_states: (view: ReadView) => subnetStates(view),
  subnet: (view: ReadView, p: { netuid: number }) => subnet(view, p.netuid),
  subnets: (view: ReadView) => subnets(view),
  subnet_emission_weights: (view: ReadView, p: { netuid: number }) => subnetEmissionWeights(view, p.netuid),
  burn: async (view: ReadView, p: { netuid: number }) => bal(view, await view.query(st('Burn'), [p.netuid])),
  commit_reveal_enabled: async (view: ReadView, p: { netuid: number }) => Boolean(await view.query(st('CommitRevealWeightsEnabled'), [p.netuid])),
  subnet_tao_flows: async (view: ReadView): Promise<Record<number, number>> => Object.fromEntries((await view.queryMap(st('SubnetEmaTaoFlow'))).filter(([, value]) => Array.isArray(value) && value.length === 2).map(([key, value]) => [num(keyOf(key)), fixed(value[1])])),
  subnet_hyperparameters: async (view: ReadView, p: { netuid: number }): Promise<ChainRecord | null> => {
    const rows = await view.runtime(api('SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3'), [p.netuid])
    if (rows == null) return null
    return Object.fromEntries(rows.map((row: ChainRecord) => {
      const [tag, value] = Object.entries(row.value)[0] as [string, any]
      return [row.name, value && typeof value === 'object' && 'bits' in value ? (tag === 'I32F32' ? integer(value.bits) >> 32n : integer(value.bits)) : value]
    }))
  },
  metagraph: async (view: ReadView, p: { netuid: number }): Promise<ChainRecord | null> => {
    const graph = await view.runtime(api('SubnetInfoRuntimeApi', 'get_metagraph'), [p.netuid])
    return graph && typeof graph === 'object' ? { ...graph, name: utf8Text(graph.name), symbol: utf8Text(graph.symbol) } : graph
  },
  subnet_identity: async (view: ReadView, p: { netuid: number }) => textRecord(await view.query(st('SubnetIdentitiesV3'), [p.netuid])),
  subnet_names: async (view: ReadView): Promise<Record<number, string>> => Object.fromEntries((await view.queryMap(st('SubnetIdentitiesV3'))).flatMap(([id, identity]) => identity?.subnet_name ? [[num(keyOf(id)), String(utf8Text(identity.subnet_name))]] : [])),
  token_symbols: async (view: ReadView): Promise<Record<number, string>> => Object.fromEntries((await view.queryMap(st('TokenSymbol'))).map(([id, symbol]) => [num(keyOf(id)), String(utf8Text(symbol))])),
  subnet_start_schedule: async (view: ReadView, p: { netuid: number }) => {
    view = await view.at()
    const [registered, delay] = await Promise.all([view.query(st('NetworkRegisteredAt'), [p.netuid]), view.query(st('StartCallDelay'))])
    return { netuid: p.netuid, registered_at: num(registered), start_call_delay: num(delay), earliest_start_block: num(registered) + num(delay), current_block: blockOf(view) }
  },
  mechanism_count: async (view: ReadView, p: { netuid: number }) => num(await view.query(st('MechanismCountCurrent'), [p.netuid])),
  mechanism_emission_split: async (view: ReadView, p: { netuid: number }): Promise<number[]> => (await view.query(st('MechanismEmissionSplit'), [p.netuid]) ?? []).map(num),
  subnet_registration_cost: async (view: ReadView) => bal(view, await view.runtime(api('SubnetRegistrationRuntimeApi', 'get_network_registration_cost'), [])),
}
