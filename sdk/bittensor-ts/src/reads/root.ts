import { type ReadView, type ChainRecord, st, api, num, integer, fixed, fixedBits, bal, min, max, absentFeature } from './base'
const basketApi = (name: string) => api('BetaBasketRuntimeApi', name)
const pricingRecord = (row: ChainRecord) => ({ hotkey: String(row.hotkey), spot_price: fixed(row.spot_price), display_price: fixed(row.display_price), stake_price: fixed(row.stake_price), staker_yield: fixed(row.staker_yield), staker_twr: fixed(row.staker_twr), bag_index: fixed(row.bag_index), stake_index: fixed(row.stake_index), first_block: num(row.first_block), provisional: Boolean(row.provisional), display_shares: fixed(row.display_shares) })
function summaryRecord(view: ReadView, s: ChainRecord) {
  const nav = integer(s.nav_tao), deposited = integer(s.deposited_tao), redeemed = integer(s.redeemed_tao), shares = integer(s.shares)
  return { hotkey: String(s.hotkey), beta_total_raw: shares, beta_total: Number(shares) / 1e9, beta_price_tao: shares ? Number(nav) / Number(shares) : 0, nav_tao: bal(view, nav), spot_nav_tao: bal(view, s.spot_nav_tao), deposited_tao: bal(view, deposited), redeemed_tao: bal(view, redeemed), lifetime_return: deposited ? Number(nav + redeemed) / Number(deposited) : null, holdings: (s.holdings ?? []).map((h: ChainRecord) => ({ netuid: num(h.netuid), alpha: bal(view, h.alpha, num(h.netuid)), spot_tao: bal(view, h.spot_tao), realizable_tao: bal(view, h.realizable_tao) })) as Array<{ netuid: number; alpha: ReturnType<typeof bal>; spot_tao: ReturnType<typeof bal>; realizable_tao: ReturnType<typeof bal> }> }
}
function positionRecord(view: ReadView, p: ChainRecord) {
  const beta = integer(p.beta), total = integer(p.beta_total), nav = integer(p.nav_tao)
  return { hotkey: String(p.hotkey), beta_raw: beta, beta_total_raw: total, beta: Number(beta) / 1e9, fund_fraction: total ? Number(beta) / Number(total) : 0, beta_price_tao: total ? Number(nav) / Number(total) : 0, nav_tao: bal(view, nav), value_tao: bal(view, p.value_tao), spot_value_tao: bal(view, p.spot_value_tao) }
}
type BasketSummary = ReturnType<typeof summaryRecord> & { basket_rate: number; chain_pricing?: ReturnType<typeof pricingRecord> }
async function attachPricing(view: ReadView, records: BasketSummary[]): Promise<void> {
  if (!records.length) return
  const pricing = new Map<string, ReturnType<typeof pricingRecord>>()
  const seen = new Set<string>()
  let cursor: string | null = null
  try {
    for (;;) {
      const page = await view.runtime(basketApi('get_all_beta_pricing'), [cursor, 0])
      for (const row of page.pricing ?? []) pricing.set(String(row.hotkey), pricingRecord(row))
      cursor = page.next ?? null
      if (cursor == null) break
      const key = String(cursor)
      if (seen.has(key)) throw new Error('beta pricing pagination did not advance')
      seen.add(key)
    }
  } catch (error) { if (absentFeature(error)) return; throw error }
  for (const record of records) if (pricing.has(record.hotkey)) record.chain_pricing = pricing.get(record.hotkey)
}
export const rootReads = {
  root_basket_owed: async (view: ReadView, p: { coldkey_ss58: string }) => bal(view, await view.runtime(basketApi('get_root_basket_owed'), [p.coldkey_ss58])),
  root_basket_owed_breakdown: async (view: ReadView, p: { coldkey_ss58: string }) => {
    const rows: any[][] = await view.runtime(basketApi('get_root_basket_positions'), [p.coldkey_ss58]) ?? []
    return rows.map(([hotkey, , value]) => ({ hotkey: String(hotkey), owed_tao: bal(view, value) })).sort((a, b) => a.owed_tao.rao > b.owed_tao.rao ? -1 : a.owed_tao.rao < b.owed_tao.rao ? 1 : 0)
  },
  basket_position: async (view: ReadView, p: { hotkey_ss58: string; coldkey_ss58: string }) => { const value = await view.runtime(basketApi('get_basket_position'), [p.hotkey_ss58, p.coldkey_ss58]); return value == null ? null : positionRecord(view, value) },
  root_basket_portfolio: async (view: ReadView, p: { coldkey_ss58: string }) => {
    const rows: ChainRecord[] = await view.runtime(basketApi('get_root_basket_portfolio'), [p.coldkey_ss58]) ?? []
    return rows.map(row => positionRecord(view, row)).sort((a, b) => a.value_tao.rao > b.value_tao.rao ? -1 : a.value_tao.rao < b.value_tao.rao ? 1 : 0)
  },
  validator_basket: async (view: ReadView, p: { hotkey_ss58: string }) => {
    const rows: any[][] = await view.runtime(basketApi('get_validator_basket'), [p.hotkey_ss58]) ?? []
    return rows.map(([id, alpha, tao]) => ({ netuid: num(id), alpha: bal(view, alpha, num(id)), value_tao: bal(view, tao) }))
  },
  validator_basket_summary: async (view: ReadView, p: { hotkey_ss58: string }): Promise<BasketSummary> => {
    view = await view.at()
    const [summary, rate] = await Promise.all([view.runtime(basketApi('get_validator_basket_summary'), [p.hotkey_ss58]), view.query(st('BasketRate'), [p.hotkey_ss58])])
    const record: BasketSummary = { ...summaryRecord(view, summary ?? {}), basket_rate: fixed(rate, 32) }
    let pricing
    try { pricing = await view.runtime(basketApi('get_beta_pricing'), [p.hotkey_ss58]) }
    catch (error) { if (!absentFeature(error)) throw error }
    if (pricing) record.chain_pricing = pricingRecord(pricing)
    return record
  },
  root_baskets: async (view: ReadView): Promise<BasketSummary[]> => {
    view = await view.at()
    const rows: ChainRecord[] = await view.runtime(basketApi('get_all_validator_baskets'), []) ?? []
    const records: BasketSummary[] = rows.map(row => ({ ...summaryRecord(view, row), basket_rate: 0 }))
    const rates = await view.queryBatch(st('BasketRate'), records.map(record => [record.hotkey]))
    records.forEach((record, i) => { record.basket_rate = fixed(rates[i], 32) })
    await attachPricing(view, records)
    return records.sort((a, b) => a.nav_tao.rao > b.nav_tao.rao ? -1 : a.nav_tao.rao < b.nav_tao.rao ? 1 : 0)
  },
  validator_basket_nav: async (view: ReadView, p: { hotkey_ss58: string }) => bal(view, await view.runtime(basketApi('get_validator_basket_nav'), [p.hotkey_ss58])),
  basket_trading_status: async (view: ReadView, p: { hotkey_ss58: string }) => {
    const row = await view.runtime(basketApi('get_basket_trading_status'), [p.hotkey_ss58]) ?? {}
    const available = integer(row.tao_available), budget = integer(row.budget_tao), refill = integer(row.refill_blocks)
    return { enabled: Boolean(row.enabled), frozen: Boolean(row.frozen), refill_blocks: num(refill), refill_per_block_tao: bal(view, refill ? budget / refill : 0n), used_tao: bal(view, max(budget - available, 0n)), budget_tao: bal(view, budget), remaining_tao: bal(view, min(available, budget)) }
  },
  root_basket_total_nav: async (view: ReadView) => bal(view, await view.runtime(basketApi('get_root_basket_total_nav'), [])),
  root_claim_threshold: async (view: ReadView) => bal(view, fixedBits(await view.query(st('RootClaimableThreshold'))) >> 32n),
}
