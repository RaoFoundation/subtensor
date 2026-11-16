import { evaluatePolicy } from '../policy'
import { type ReadView, type ChainRecord, st, api, num, integer, fixed, fixedBits, bal, blockOf, max, keyOf } from './base'
const lockApi = (name: string) => api('StakeInfoRuntimeApi', name)
async function lockRecord(view: ReadView, coldkey: string, netuid: number, hotkey: string) {
  const [state, decaying] = await Promise.all([view.runtime(lockApi('get_coldkey_lock'), [coldkey, netuid]), view.query(st('DecayingLock'), [coldkey, netuid])])
  return state ? { hotkey, netuid, locked_alpha: bal(view, state.locked_mass, netuid), is_perpetual: decaying != null && !decaying } : null
}
/** Float projection used only for estimates; on-chain lock amounts come from runtime APIs. */
export function projectLock(mass: number, conviction: number, dt: number, unlockRate: number, maturityRate: number, perpetual: boolean, owner: boolean): [number, number] { return evaluatePolicy('financial.reads.lock_project', { mass, conviction, dt, unlock: unlockRate, maturity: maturityRate, perpetual, owner }) }
type Bucket = [number, number, boolean, boolean]
export function blocksUntilConviction(buckets: Bucket[], eligible: number, unlock: number, maturity: number): number | null { return evaluatePolicy('financial.reads.lock_blocks', { buckets, eligible, unlock, maturity }) }
async function ownerLocks(view: ReadView, netuid: number): Promise<Map<boolean, ChainRecord | null>> {
  const layouts = [[true, 'OwnerLock'], [false, 'DecayingOwnerLock']] as const
  const rows = await Promise.all(layouts.map(async ([perpetual, name]) => {
    const item = st(name)
    if (view.hasStorage && await view.hasStorage(item) === false) return []
    return [[perpetual, await view.query(item, [netuid]) ?? null] as const]
  }))
  return new Map(rows.flat())
}
export async function subnetConvictions(view: ReadView, netuid: number) {
  view = await view.at()
  const [perpetual, decaying, ownerBuckets, ownerValue, out, protocol, burned, unlockRaw, maturityRaw, registered] = await Promise.all([
    view.queryMap(st('HotkeyLock'), [netuid]), view.queryMap(st('DecayingHotkeyLock'), [netuid]), ownerLocks(view, netuid), view.query(st('SubnetOwnerHotkey'), [netuid]), view.query(st('SubnetAlphaOut'), [netuid]), view.query(st('SubnetProtocolAlpha'), [netuid]), view.query(['AlphaAssets', 'AlphaBurned'], [netuid]), view.query(st('UnlockRate')), view.query(st('MaturityRate')), view.query(st('NetworkRegisteredAt'), [netuid]),
  ])
  const now = blockOf(view), unlock = num(unlockRaw) || 934866, maturity = num(maturityRaw) || 934866, owner = ownerValue ? String(ownerValue) : null
  const buckets = new Map<string, Bucket[]>()
  const add = (key: string, value: ChainRecord, perpetual: boolean, isOwner: boolean) => {
    const c = value?.conviction, conviction = typeof c === 'object' ? fixed(c) : Number(c ?? 0)
    const [mass, projected] = projectLock(Number(integer(value?.locked_mass)), conviction, Math.max(0, now - num(value?.last_update)), unlock, maturity, perpetual, isOwner)
    const group = buckets.get(key) ?? []; group.push([mass, projected, perpetual, isOwner]); buckets.set(key, group)
  }
  const rowOwner = ownerBuckets.size ? null : owner
  for (const [key, value] of perpetual) add(String(keyOf(key)), value, true, String(keyOf(key)) === rowOwner)
  for (const [key, value] of decaying) add(String(keyOf(key)), value, false, String(keyOf(key)) === rowOwner)
  for (const [perpetual, value] of ownerBuckets) if (owner && value) add(owner, value, perpetual, true)
  const eligibility = evaluatePolicy<{eligible: string; threshold: string}>('financial.reads.eligible_alpha', { out: integer(out), protocol: integer(protocol), burned: integer(burned) }), eligible = BigInt(eligibility.eligible), threshold = Number(eligible) * .18
  const entries = [...buckets].map(([hotkey, group]) => {
    const locked = group.reduce((sum, b) => sum + b[0], 0), conviction = group.reduce((sum, b) => sum + b[1], 0)
    return { hotkey, is_owner: hotkey === owner, locked_alpha: bal(view, BigInt(Math.trunc(locked)), netuid), conviction_alpha: bal(view, BigInt(Math.trunc(conviction)), netuid), pct_of_threshold: threshold ? conviction / threshold : null, blocks_to_threshold: blocksUntilConviction(group, Number(eligible), unlock, maturity) }
  }).sort((a, b) => a.conviction_alpha.rao > b.conviction_alpha.rao ? -1 : a.conviction_alpha.rao < b.conviction_alpha.rao ? 1 : 0)
  const all = [...buckets.values()].flat()
  return { netuid, block: now, alpha_out: bal(view, out, netuid), protocol_alpha: bal(view, protocol, netuid), alpha_burned: bal(view, burned, netuid), eligible_alpha: bal(view, eligible, netuid), threshold_alpha: bal(view, BigInt(eligibility.threshold), netuid), total_locked_alpha: bal(view, BigInt(Math.trunc(all.reduce((sum, b) => sum + b[0], 0))), netuid), total_conviction_alpha: bal(view, BigInt(Math.trunc(all.reduce((sum, b) => sum + b[1], 0))), netuid), leader_hotkey: entries[0]?.hotkey ?? null, leader_blocks_to_threshold: entries[0]?.blocks_to_threshold ?? null, unlock_rate: unlock, maturity_rate: maturity, owner_hotkey: owner, registered_at: num(registered), ownership_changeable_at_block: num(registered) + 2629800, hotkeys: entries }
}
export const lockReads = {
  coldkey_lock: async (view: ReadView, p: { coldkey_ss58: string; netuid: number }) => {
    const rows = await view.queryMap(st('Lock'), [p.coldkey_ss58]); const key = rows.find(([key]) => num(key[0]) === p.netuid)?.[0]
    return key ? lockRecord(view, p.coldkey_ss58, p.netuid, String(key[1])) : null
  },
  locks_for_coldkey: async (view: ReadView, p: { coldkey_ss58: string }) => {
    const rows = await view.queryMap(st('Lock'), [p.coldkey_ss58])
    const values = await Promise.all(rows.sort(([a], [b]) => num(a[0]) - num(b[0]) || String(a[1]).localeCompare(String(b[1]))).map(([key]) => lockRecord(view, p.coldkey_ss58, num(key[0]), String(key[1]))))
    return values.filter((value): value is NonNullable<typeof value> => value != null)
  },
  locks_for_hotkey: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => {
    const keys = (await view.queryMap(st('LockingColdkeys'), [p.netuid, p.hotkey_ss58])).map(([key]) => String(keyOf(key))).sort()
    const records = await Promise.all(keys.map(key => lockRecord(view, key, p.netuid, p.hotkey_ss58)))
    return records.flatMap((record, i) => record ? [{ coldkey: keys[i], ...record }] : [])
  },
  accepts_locked_alpha: async (view: ReadView, p: { coldkey_ss58: string }) => (integer(await view.query(st('AccountFlags'), [p.coldkey_ss58])) & 1n) !== 0n,
  hotkey_conviction: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => {
    const value = await view.runtime(lockApi('get_hotkey_conviction'), [p.hotkey_ss58, p.netuid])
    return value && typeof value === 'object' && 'bits' in value ? { conviction_alpha: bal(view, fixedBits(value) >> 64n, p.netuid) } : typeof value === 'object' ? value as ChainRecord : { value }
  },
  most_convicted_hotkey: async (view: ReadView, p: { netuid: number }): Promise<string | null> => { const value = await view.runtime(lockApi('get_most_convicted_hotkey_on_subnet'), [p.netuid]); return value ? String(value) : null },
  subnet_convictions: (view: ReadView, p: { netuid: number }) => subnetConvictions(view, p.netuid),
}
