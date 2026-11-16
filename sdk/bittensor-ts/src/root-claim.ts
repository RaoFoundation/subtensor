import { evaluatePolicy } from './policy'
/** Current runtime claim fee model: runtime/src/staking_fee.rs and claim_root.rs. */
export interface RootClaimPreview {
  accrued_tao: bigint
  redeemable_tao: bigint
  forfeited_tao_est: bigint
  rows: number
  rows_to_sell: number
  dust_rows: number
  swept: number
}
export interface RootClaimFeeQuote {
  reservedFeeRao: bigint
  spentFeeFloorRao: bigint
  possibleRefundRao: bigint
  /** Total entitlement is unknown while deferred alpha/cash is outside the basket RPC. */
  accruedRao: bigint | null
  redeemableRao: bigint | null
  basketAccruedRao: bigint
  basketRedeemableRao: bigint
  previewComplete: boolean
  unquotedHotkeys: number
  forfeitedRao: bigint
  thresholdRao: bigint
  eligibleHotkeys: number
  belowThresholdHotkeys: number
  dustRows: number
  losesMoney: boolean | null
}
export interface RootClaimRelationship {
  hotkey: string
  hasRootStake: boolean
  /** Signed basket-share watermark; a negative value survives full unstaking. */
  basketClaimed: bigint
  hasDeferredDebt: boolean
  /** Signed I64F64 alpha corrections, represented as exact raw bits. */
  deferredDebts: Array<{ netuid: number; generation: bigint; amountBits: bigint }>
  /** I64F64 alpha per unit of root stake, represented as exact raw bits. */
  deferredRates: Array<{ netuid: number; generation: bigint; rateBits: bigint; debtCount: number }>
  deferredCash: { rateBits: bigint; claimedRao: bigint; reserveRao: bigint }
  selected: boolean
  /** Basket RPCs omit these independent deferred entitlements. */
  hasUnquotedDeferredEntitlement: boolean
}
interface RootClaimStateReader {
  query(pallet: string, item: string, params?: unknown[]): Promise<unknown>
  queryMap?(pallet: string, item: string, params?: unknown[], limit?: number): Promise<Array<[unknown, unknown]>>
  countStorageKeys?(pallet: string, item: string, params: unknown[], limit: number): Promise<number>
}
/** Mirror runtime root_claim_hotkeys, preserving relationships after all root stake exits. */
export async function readRootClaimRelationships(reader: RootClaimStateReader, coldkey: string, hotkeys: readonly string[], rootHotkeys: ReadonlySet<string>): Promise<RootClaimRelationship[]> {
  if (!reader.countStorageKeys || !reader.queryMap) throw new Error('root claim relationships require physical storage-key and map reads')
  const result: RootClaimRelationship[] = []
  for (let offset = 0; offset < hotkeys.length; offset += 16) {
    result.push(...await Promise.all(hotkeys.slice(offset, offset + 16).map(async hotkey => {
      const read = (item: string, pair = false) => reader.query('SubtensorModule', item, pair ? [hotkey, coldkey] : [hotkey])
      const [watermark, debtKeys, debts, rates, cashRate, cashClaimed, cashReserve] = await Promise.all([
        read('BasketClaimed', true),
        reader.countStorageKeys!('SubtensorModule', 'DeferredBasketDebts', [hotkey, coldkey], 1),
        read('DeferredBasketDebts', true),
        reader.queryMap!('SubtensorModule', 'DeferredBasketRates', [hotkey], 257),
        read('DeferredBasketCashRate'), read('DeferredBasketCashClaimed', true), read('DeferredBasketCashReserve'),
      ])
      if (debtKeys !== 0 && debtKeys !== 1) throw new RangeError('invalid deferred debt storage-key count')
      if (rates.length > 256) throw new RangeError('deferred dividend rates exceed the runtime row limit')
      const basketClaimed = signed(watermark ?? 0n), claimedRao = signed(cashClaimed ?? 0n)
      const rateBits = bits(cashRate ?? 0n), reserveRao = unsigned(cashReserve ?? 0n)
      const hasRootStake = rootHotkeys.has(hotkey), hasDeferredDebt = debtKeys === 1
      const deferredDebts = entries(debts).map(([id, value]) => {
        const debt = record(value)
        return { netuid: subnet(id), generation: unsigned(debt.generation), amountBits: bits(debt.amount) }
      })
      if (deferredDebts.length > 256) throw new RangeError('deferred dividend debts exceed the runtime row limit')
      const deferredRates = rates.map(([id, value]) => {
        const rate = record(value)
        return { netuid: subnet(id), generation: unsigned(rate.generation), rateBits: bits(rate.rate), debtCount: count(Number(unsigned(rate.debt_count))) }
      })
      const policy = evaluatePolicy<{ selected: boolean; unquoted: boolean }>('financial.root_claim.relationship', { root: hasRootStake, basket: basketClaimed, debt: hasDeferredDebt, claimed: claimedRao, rates: deferredRates.length, cash_rate: rateBits })
      return {
        hotkey, hasRootStake, basketClaimed, hasDeferredDebt, deferredDebts, deferredRates,
        deferredCash: { rateBits, claimedRao, reserveRao },
        selected: policy.selected,
        hasUnquotedDeferredEntitlement: policy.unquoted,
      }
    })))
  }
  return result
}
export function rootClaimChargedFeeRao(limit = 256): bigint { return BigInt(evaluatePolicy<string>('financial.root_claim.charged_fee', { limit })) }
export function rootClaimSpentFeeRao(reserved: bigint, hotkeys: number, redeemHoldings: number, scanHoldings: number, selectionScans: number, limit = 256): bigint { return BigInt(evaluatePolicy<string>('financial.root_claim.spent_fee', { reserved, hotkeys, redeem: redeemHoldings, scan: scanHoldings, selection: selectionScans, limit })) }
export function quoteRootClaimPreviews(previews: readonly (RootClaimPreview | null)[], reserved: bigint, threshold: bigint, selectionScans: number, limit = 256, unquotedHotkeys = 0): RootClaimFeeQuote {
  const result = evaluatePolicy<any>('financial.root_claim.preview', { previews, reserved, threshold, selection: selectionScans, limit, unquoted: unquotedHotkeys })
  for (const key of ['reservedFeeRao', 'spentFeeFloorRao', 'possibleRefundRao', 'accruedRao', 'redeemableRao', 'basketAccruedRao', 'basketRedeemableRao', 'forfeitedRao', 'thresholdRao']) if (result[key] != null) result[key] = BigInt(result[key])
  return Object.freeze(result)
}

function count(value: number): number { if (!Number.isSafeInteger(value) || value < 0) throw new RangeError('claim work count must be a nonnegative safe integer'); return value }
function signed(value: unknown): bigint {
  if (typeof value === 'bigint') return value
  if (typeof value === 'number' && Number.isSafeInteger(value)) return BigInt(value)
  if (typeof value === 'string' && /^-?\d+$/.test(value)) return BigInt(value)
  throw new TypeError('invalid signed root dividend integer')
}
function unsigned(value: unknown): bigint { const out = signed(value); if (out < 0n) throw new RangeError('root dividend value must be nonnegative'); return out }
function record(value: unknown): Record<string, unknown> { if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError('invalid deferred dividend record'); return value as Record<string, unknown> }
function bits(value: unknown): bigint { return signed(value && typeof value === 'object' ? record(value).bits : value) }
function subnet(value: unknown): number { const id = count(Number(unsigned(Array.isArray(value) && value.length === 1 ? value[0] : value))); if (id > 65535) throw new RangeError('invalid deferred dividend subnet'); return id }
function entries(value: unknown): Array<[unknown, unknown]> {
  if (value == null) return []
  if (value instanceof Map) return [...value.entries()]
  if (Array.isArray(value)) { if (!value.every(row => Array.isArray(row) && row.length === 2)) throw new TypeError('invalid deferred dividend debt map'); return value as Array<[unknown, unknown]> }
  return Object.entries(record(value))
}
