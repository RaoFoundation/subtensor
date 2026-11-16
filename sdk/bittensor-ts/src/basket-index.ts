import { evaluatePolicy } from './policy'
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

const data = { baselines: BASKET_BASELINES, series: BASKET_INDEX_SERIES, tr_series: BASKET_TR_INDEX_SERIES }
const adapted = (record: BasketPosition) => ({ ...record, nav_tao: typeof record.nav_tao === 'object' && record.nav_tao != null ? record.nav_tao.tao : record.nav_tao, spot_nav_tao: typeof record.spot_nav_tao === 'object' && record.spot_nav_tao != null ? record.spot_nav_tao.tao : record.spot_nav_tao })
const evaluate = <T>(op: string, input: unknown): T => evaluatePolicy<T>(`financial.basket.${op}`, input)
export function spotBetaPrice(record: BasketPosition): number { return evaluate('spot', { record: adapted(record) }) }
export function indexLevel(block?: number | null): number { return evaluate('level', { series: BASKET_INDEX_SERIES, block }) }
export function trIndexLevel(block?: number | null): number { return evaluate('level', { series: BASKET_TR_INDEX_SERIES, block }) }
export function fundPricing(hotkey: string, rawPrice: number): BasketDisplayPricing { return Object.freeze(evaluate<BasketDisplayPricing>('pricing', { data, hotkey, raw: rawPrice })) }
export function stakerYield(record: BasketPosition): number | null { return evaluate('yield', { data, record: adapted(record) }) }
export function stakeValue(record: BasketPosition): number | null { return evaluate('stake_value', { data, record: adapted(record) }) }
export function normalizeBasketPosition<T extends BasketPosition>(record: T): T { return Object.assign(record, evaluate('normalize', { data, record: adapted(record) })) }
export function normalizeBasketPositions(records: BasketPosition[]): number { const result = evaluate<{ level: number; records: Partial<BasketPosition>[] }>('normalize_many', { data, records: records.map(adapted) }); records.forEach((record, index) => Object.assign(record, result.records[index])); return result.level }
export function basketAgeDays(firstBlock: number | null | undefined, currentBlock: number): number | null { return evaluate('age', { first: firstBlock, current: currentBlock }) }
