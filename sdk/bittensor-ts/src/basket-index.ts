import { BASKET_BASELINES, BASKET_INDEX_SERIES, BASKET_TR_INDEX_SERIES } from './basket-index-data'
export { BASKET_INDEX_EPOCH_BLOCK, BASKET_INDEX_GENERATED_BLOCK, BASKET_INDEX_SAMPLE_STEP, BASKET_INDEX_SERIES, BASKET_TR_INDEX_SERIES, BASKET_BASELINES } from './basket-index-data'

export interface BasketDisplayPricing { readonly baseline: number; readonly first_block: number | null; readonly provisional: boolean; readonly baseline_rate: number }
export interface BasketChainPricing {
  display_price: number; bag_index: number; stake_price: number; stake_index: number; spot_price: number
  first_block: number; provisional: boolean; staker_yield: number; staker_twr: number
}
export interface BasketPosition {
  hotkey: string; beta_price_tao?: number | null; nav_tao?: number | { readonly tao: number } | null
  spot_nav_tao?: number | { readonly tao: number } | null; beta?: number; basket_rate?: number | null
  chain_pricing?: BasketChainPricing | null
  display_price_tao?: number; display_beta?: number; vs_index?: number; index_first_block?: number | null
  index_provisional?: boolean; basket_index?: number; staker_yield?: number | null
  stake_value?: number | null; stake_vs_index?: number | null; stake_index?: number; staker_twr?: number
}
const MIN_INDEX_NAV_TAO = 0.1
function asTao(value: BasketPosition['nav_tao']): number { return value == null ? 0 : typeof value === 'number' ? value : Number(value.tao) }
/** Equal spot marking avoids penalizing large funds for a full-fund liquidation haircut. */
export function spotBetaPrice(record: BasketPosition): number {
  const real = record.beta_price_tao || 0, nav = asTao(record.nav_tao), spot = asTao(record.spot_nav_tao)
  return nav > 0 && spot > 0 ? real * (spot / nav) : real
}
function seriesLevel(series: readonly (readonly [number, number])[], block?: number | null): number {
  if (!series.length) return 1
  if (block == null) return series[series.length - 1][1]
  let level = 1
  for (const [height, value] of series) { if (height > block) break; level = value }
  return level
}
export function indexLevel(block?: number | null): number { return seriesLevel(BASKET_INDEX_SERIES, block) }
export function trIndexLevel(block?: number | null): number { return seriesLevel(BASKET_TR_INDEX_SERIES, block) }
/** Frozen first-sighting divisor, or a provisional fund anchored at the latest index. */
export function fundPricing(hotkey: string, rawPrice: number): BasketDisplayPricing {
  const frozen = Object.hasOwn(BASKET_BASELINES, hotkey) ? (BASKET_BASELINES as Readonly<Record<string, readonly [number, number, number]>>)[hotkey] : undefined
  if (frozen) return Object.freeze({ baseline: frozen[0], first_block: frozen[1], provisional: false, baseline_rate: frozen[2] })
  const level = indexLevel()
  return Object.freeze({ baseline: (level ? rawPrice / level : rawPrice) || 1, first_block: null, provisional: true, baseline_rate: 0 })
}
/** Pending entitlement marked at today's spot price; this is not a return series. */
export function stakerYield(record: BasketPosition): number | null {
  if (record.basket_rate == null) return null
  const raw = spotBetaPrice(record)
  return Math.max(record.basket_rate - fundPricing(record.hotkey, raw).baseline_rate, 0) * raw
}
/** Approximate pre-v3 total-return stake price. Canonical snapshots take precedence in normalization. */
export function stakeValue(record: BasketPosition): number | null {
  const value = stakerYield(record)
  return value == null ? null : (1 + value) * trIndexLevel(fundPricing(record.hotkey, spotBetaPrice(record)).first_block)
}
function applyChain(record: BasketPosition, chain: BasketChainPricing): void {
  record.display_price_tao = chain.display_price
  if (record.beta != null) record.display_beta = record.beta * (chain.display_price ? chain.spot_price / chain.display_price : 1)
  record.vs_index = chain.bag_index ? chain.display_price / chain.bag_index - 1 : 0
  record.index_first_block = chain.first_block || null
  record.index_provisional = chain.provisional
  record.basket_index = chain.bag_index
  record.staker_yield = chain.staker_yield
  record.stake_value = chain.stake_price
  record.stake_vs_index = chain.stake_index ? chain.stake_price / chain.stake_index - 1 : null
  record.stake_index = chain.stake_index
  record.staker_twr = chain.staker_twr
}
function applyPricing(record: BasketPosition, raw: number, level: number, trLevel: number): void {
  const pricing = fundPricing(record.hotkey, raw), display = pricing.baseline ? raw / pricing.baseline : raw
  record.display_price_tao = display
  if (record.beta != null) record.display_beta = record.beta * pricing.baseline
  record.vs_index = level ? display / level - 1 : 0
  record.index_first_block = pricing.first_block
  record.index_provisional = pricing.provisional
  record.basket_index = level
  record.staker_yield = record.basket_rate == null ? null : Math.max(record.basket_rate - pricing.baseline_rate, 0) * raw
  const value = stakeValue(record)
  record.stake_value = value
  record.stake_vs_index = value != null && trLevel ? value / trLevel - 1 : null
  record.stake_index = trLevel
}
/** Add display fields in place, passing through the chain's canonical v3 pricing when present. */
export function normalizeBasketPosition<T extends BasketPosition>(record: T): T {
  if (record.chain_pricing != null) applyChain(record, record.chain_pricing)
  else applyPricing(record, spotBetaPrice(record), indexLevel(), trIndexLevel())
  return record
}
/** Normalize a complete board in place; return its spot-NAV-weighted bag index. */
export function normalizeBasketPositions(records: BasketPosition[]): number {
  const chained = records.find(record => record.chain_pricing != null)?.chain_pricing
  if (chained != null) {
    for (const record of records) {
      if (record.chain_pricing != null) applyChain(record, record.chain_pricing)
      else applyPricing(record, spotBetaPrice(record), chained.bag_index, chained.stake_index)
    }
    return chained.bag_index
  }
  let weight = 0, valueSum = 0, trWeight = 0, trValueSum = 0
  const priced = records.map(record => {
    const raw = spotBetaPrice(record), pricing = fundPricing(record.hotkey, raw), display = pricing.baseline ? raw / pricing.baseline : raw
    const spot = asTao(record.spot_nav_tao) || asTao(record.nav_tao)
    if (spot >= MIN_INDEX_NAV_TAO && display > 0) {
      weight += spot; valueSum += spot * display
      const value = stakeValue(record)
      if (value != null) { trWeight += spot; trValueSum += spot * value }
    }
    return { record, raw }
  })
  const level = weight ? valueSum / weight : indexLevel(), trLevel = trWeight ? trValueSum / trWeight : trIndexLevel()
  for (const { record, raw } of priced) applyPricing(record, raw, level, trLevel)
  return level
}
/** Python's historical display convention assumes 12-second blocks (7,200/day). */
export function basketAgeDays(firstBlock: number | null | undefined, currentBlock: number): number | null { return firstBlock == null ? null : (currentBlock - firstBlock) / 7200 }
