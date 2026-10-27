import { type ReadView, type ChainRecord, st, num, integer, fixed, fixedBits, bal, max } from './base'
function collateralRecord(view: ReadView, netuid: number, hotkey: string, coldkey: string, state: ChainRecord | null) {
  if (!state || typeof state !== 'object') return null
  const locked = integer(state.locked), floor = integer(state.min_locked), headroom = max(locked - floor, 0n), drain = fixedBits(state.drain_ratio)
  return { hotkey, coldkey, netuid, locked_alpha: bal(view, locked, netuid), min_locked_alpha: bal(view, floor, netuid), earned_alpha: bal(view, state.earned, netuid), drain_ratio: fixed(state.drain_ratio), headroom_alpha: bal(view, headroom, netuid), shortfall_alpha: bal(view, max(floor - locked, 0n), netuid), releasable_work_alpha: bal(view, drain > 0n ? (headroom << 64n) / drain : 0n, netuid) }
}
export const collateralReads = {
  miner_collateral: async (view: ReadView, p: { netuid: number; hotkey_ss58: string; coldkey_ss58?: string | null }) => {
    const coldkey = p.coldkey_ss58 ?? await view.query(st('Owner'), [p.hotkey_ss58])
    if (coldkey == null) return null
    return collateralRecord(view, p.netuid, p.hotkey_ss58, String(coldkey), await view.query(st('MinerCollateral'), [p.netuid, p.hotkey_ss58, String(coldkey)]))
  },
  subnet_collateral: async (view: ReadView, p: { netuid: number }) => {
    view = await view.at()
    const records = (await view.queryMap(st('MinerCollateral'), [p.netuid])).flatMap(([key, state]) => {
      if (!Array.isArray(key) || key.length < 2) return []
      const row = collateralRecord(view, p.netuid, String(key[0]), String(key[1]), state)
      return row ? [row] : []
    })
    const uids = await view.queryBatch(st('Uids'), records.map(r => [p.netuid, r.hotkey]))
    return records.map((r, i) => ({ ...r, uid: uids[i] == null ? null : num(uids[i]) })).sort((a, b) => a.locked_alpha.rao > b.locked_alpha.rao ? -1 : a.locked_alpha.rao < b.locked_alpha.rao ? 1 : 0)
  },
  collateral_policy: async (view: ReadView, p: { netuid: number }) => {
    view = await view.at()
    const [share, ratio] = await Promise.all([view.query(st('CollateralLockShare'), [p.netuid]), view.query(st('CollateralDrainRatio'), [p.netuid])])
    return { netuid: p.netuid, lock_share: num(share) / 65535, drain_ratio: ratio == null ? 1 : fixed(ratio) }
  },
}
