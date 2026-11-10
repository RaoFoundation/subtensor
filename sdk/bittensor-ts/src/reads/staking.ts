import { Balance } from '../balance'
import { type ReadView, type ChainRecord, st, api, num, integer, bal, blockOf, keyOf } from './base'
import { subnetStates, type SubnetLifecycleState } from './subnets'
export interface ReadStakePosition { hotkey: string; coldkey: string; netuid: number; stake: Balance; is_registered: boolean; subnet_state: SubnetLifecycleState | null; raw: ChainRecord }
export interface StakeValuation { coldkey: string; block: number; positions: ReadStakePosition[]; stake_value: Balance; tao_per_alpha: Record<number, number>; spot_value(stake: Balance): Balance }
const stakePosition = (view: ReadView, r: ChainRecord, states: Record<number, SubnetLifecycleState>): ReadStakePosition => ({ hotkey: String(r.hotkey), coldkey: String(r.coldkey), netuid: num(r.netuid), stake: bal(view, r.stake, num(r.netuid)), is_registered: Boolean(r.is_registered), subnet_state: states[num(r.netuid)] ?? null, raw: r })
const stakeApi = (method: string) => api('StakeInfoRuntimeApi', method)
async function priceRows(view: ReadView): Promise<ChainRecord[]> { return await view.runtime(api('SwapRuntimeApi', 'current_alpha_price_all'), []) ?? [] }
async function valuation(view: ReadView, coldkeys: string[]): Promise<Record<string, StakeValuation>> {
  view = await view.at()
  const [rows, prices, states] = await Promise.all([view.runtime(stakeApi('get_stake_info_for_coldkeys'), [coldkeys]), priceRows(view), subnetStates(view)])
  const priceRao: Record<number, bigint> = Object.fromEntries(prices.map(row => [num(row.netuid), integer(row.price)]))
  const display = Object.fromEntries(Object.entries(priceRao).map(([key, price]) => [key, Number(price) / 1e9]))
  const records = new Map<string, ReadStakePosition[]>((rows ?? []).map(([key, values]: [string, ChainRecord[]]) => [String(key), (values ?? []).map(r => stakePosition(view, r, states))]))
  const spotValue = (stake: Balance) => bal(view, stake.netuid === 0 ? stake.rao : stake.rao * (priceRao[stake.netuid] ?? 0n) / 1000000000n)
  return Object.fromEntries(coldkeys.map(coldkey => {
    const positions = records.get(coldkey) ?? []
    return [coldkey, { coldkey, block: blockOf(view), positions, stake_value: bal(view, positions.reduce((sum, p) => sum + spotValue(p.stake).rao, 0n)), tao_per_alpha: display, spot_value: spotValue }]
  }))
}
async function availability(view: ReadView, coldkey: string, netuids: number[]) {
  const ids = [...new Set(netuids)].sort((a, b) => a - b)
  if (!ids.length) return []
  const raw = await view.runtime(stakeApi('get_stake_availability_for_coldkeys'), [[coldkey], ids])
  const entries = raw instanceof Map ? raw.get(coldkey) : raw?.[coldkey]
  return ids.map(netuid => {
    const r = (entries instanceof Map ? entries.get(netuid) ?? entries.get(String(netuid)) : entries?.[netuid]) ?? {}
    return { netuid, total: bal(view, r.total, netuid), locked: bal(view, r.locked, netuid), available: bal(view, r.available, netuid) }
  })
}
function quote(view: ReadView, r: ChainRecord, netuid: number) {
  return { tao: bal(view, r.tao_amount), alpha: bal(view, r.alpha_amount, netuid), tao_fee: bal(view, r.tao_fee), alpha_fee: bal(view, r.alpha_fee, netuid), tao_slippage: bal(view, r.tao_slippage), alpha_slippage: bal(view, r.alpha_slippage, netuid) }
}
export const stakingReads = {
  stake: async (view: ReadView, p: { coldkey_ss58: string; hotkey_ss58: string; netuid: number }) => {
    const value = await view.runtime(stakeApi('get_stake_info_for_hotkey_coldkey_netuid'), [p.hotkey_ss58, p.coldkey_ss58, p.netuid])
    return bal(view, value?.stake, p.netuid)
  },
  stake_for_coldkey: async (view: ReadView, p: { coldkey_ss58: string }): Promise<ReadStakePosition[]> => {
    view = await view.at()
    const [rows, states] = await Promise.all([view.runtime(stakeApi('get_stake_info_for_coldkey'), [p.coldkey_ss58]), subnetStates(view)])
    return (rows ?? []).map((row: ChainRecord) => stakePosition(view, row, states))
  },
  stake_for_coldkeys: async (view: ReadView, p: { coldkey_ss58s: string[] }): Promise<Record<string, ReadStakePosition[]>> => {
    view = await view.at()
    const [rows, states] = await Promise.all([view.runtime(stakeApi('get_stake_info_for_coldkeys'), [p.coldkey_ss58s]), subnetStates(view)])
    return Object.fromEntries((rows ?? []).map(([key, values]: [string, ChainRecord[]]) => [String(key), (values ?? []).map(row => stakePosition(view, row, states))]))
  },
  stake_value_for_coldkey: async (view: ReadView, p: { coldkey_ss58: string }) => (await valuation(view, [p.coldkey_ss58]))[p.coldkey_ss58],
  stake_value_for_coldkeys: (view: ReadView, p: { coldkey_ss58s: string[] }) => valuation(view, p.coldkey_ss58s),
  stake_availability: async (view: ReadView, p: { coldkey_ss58: string; netuid: number }) => (await availability(view, p.coldkey_ss58, [p.netuid]))[0],
  stake_availability_for_coldkey: (view: ReadView, p: { coldkey_ss58: string; netuids: number[] }) => availability(view, p.coldkey_ss58, p.netuids),
  total_alpha_staked: async (view: ReadView, p: { netuid: number }) => bal(view, await view.query(st('TotalAlphaStaked'), [p.netuid]), p.netuid),
  staking_hotkeys: async (view: ReadView, p: { coldkey_ss58: string }): Promise<string[]> => (await view.query(st('StakingHotkeys'), [p.coldkey_ss58]) ?? []).map(String),
  auto_stake: async (view: ReadView, p: { coldkey_ss58: string; netuid: number }): Promise<string | null> => {
    const hotkey = await view.query(st('AutoStakeDestination'), [p.coldkey_ss58, p.netuid]); return hotkey ? String(hotkey) : null
  },
  auto_stake_all: async (view: ReadView, p: { coldkey_ss58: string }) => (await view.queryMap(st('AutoStakeDestination'), [p.coldkey_ss58])).map(([id, hotkey]) => ({ netuid: num(keyOf(id)), hotkey: String(hotkey) })).sort((a, b) => a.netuid - b.netuid),
  quote_stake: async (view: ReadView, p: { netuid: number; amount_tao: number | string }) => quote(view, await view.runtime(api('SwapRuntimeApi', 'sim_swap_tao_for_alpha'), [p.netuid, Balance.fromTao(p.amount_tao).rao]), p.netuid),
  quote_unstake: async (view: ReadView, p: { netuid: number; amount_alpha: number | string }) => quote(view, await view.runtime(api('SwapRuntimeApi', 'sim_swap_alpha_for_tao'), [p.netuid, Balance.fromAlpha(p.amount_alpha, p.netuid).rao]), p.netuid),
  alpha_prices: async (view: ReadView): Promise<Record<number, number>> => Object.fromEntries((await priceRows(view)).map(row => [num(row.netuid), Number(integer(row.price)) / 1e9])),
  alpha_price: async (view: ReadView, p: { netuid: number }) => { const price = integer(await view.runtime(api('SwapRuntimeApi', 'current_alpha_price'), [p.netuid])); return { netuid: p.netuid, tao_per_alpha: Number(price) / 1e9, price_rao: price } },
}
