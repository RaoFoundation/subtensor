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
      return {
        hotkey, hasRootStake, basketClaimed, hasDeferredDebt, deferredDebts, deferredRates,
        deferredCash: { rateBits, claimedRao, reserveRao },
        selected: hasRootStake || basketClaimed < 0n || hasDeferredDebt || claimedRao < 0n,
        hasUnquotedDeferredEntitlement: hasDeferredDebt || claimedRao < 0n || hasRootStake && (deferredRates.length > 0 || rateBits !== 0n),
      }
    })))
  }
  return result
}
const read = 25_000_000n, write = 100_000_000n
const claim = (units: bigint) => 567_441_000n + 437_318_379n * units + (23n + 22n * units) * read + (18n + 16n * units) * write
const scan = (units: bigint) => 309_208_000n + 263_604_241n * units + (6n + 21n * units) * read
const weightFee = (refTime: bigint) => refTime / 4000n + (refTime % 4000n * 2n > 4000n ? 1n : 0n)

export function rootClaimChargedFeeRao(limit = 256): bigint {
  const units = BigInt(Math.min(count(limit), 4))
  // The runtime combines claim and queued-credit units before applying the
  // benchmark, so its fixed base cost is charged once.
  const quotes = 6n * units
  const weight = claim(2n * units) + scan(units) + (10_000_000n + 4n * read) * (quotes > 0n ? quotes : 1n)
  return weightFee(weight < 249_856_000_000n ? weight : 249_856_000_000n)
}

/** Flush work cannot be known exactly before dispatch, so this is a spent-fee floor. */
export function rootClaimSpentFeeRao(reserved: bigint, hotkeys: number, redeemHoldings: number, scanHoldings: number, selectionScans: number, limit = 256): bigint {
  if (reserved < 0n) throw new RangeError('reserved fee must be nonnegative')
  const charged = rootClaimChargedFeeRao(limit), base = reserved > charged ? reserved - charged : 0n
  const active = BigInt(Math.max(count(hotkeys), count(redeemHoldings), count(selectionScans), 1))
  const spent = base + weightFee(claim(active) + scan(BigInt(count(scanHoldings))))
  return spent < reserved ? spent : reserved
}

export function quoteRootClaimPreviews(previews: readonly (RootClaimPreview | null)[], reserved: bigint, threshold: bigint, selectionScans: number, limit = 256, unquotedHotkeys = 0): RootClaimFeeQuote {
  if (threshold < 0n) throw new RangeError('claim threshold must be nonnegative')
  count(unquotedHotkeys)
  if (unquotedHotkeys > previews.length) throw new RangeError('unquoted hotkeys exceed selected claim hotkeys')
  let accruedRao = 0n, redeemableRao = 0n, forfeitedRao = 0n
  let eligibleHotkeys = 0, belowThresholdHotkeys = 0, dustRows = 0, redeemed = 0, scanned = 0
  for (const row of previews) {
    if (row == null) continue
    accruedRao += row.accrued_tao
    forfeitedRao += row.forfeited_tao_est
    dustRows += count(row.dust_rows)
    const rows = count(row.rows), sold = count(row.rows_to_sell)
    if (sold > rows) throw new RangeError('claim preview sells more rows than it scans')
    redeemed += count(row.swept)
    if (row.redeemable_tao > 0n && row.redeemable_tao >= threshold) {
      eligibleHotkeys++; redeemableRao += row.redeemable_tao; redeemed += sold; scanned += rows - sold
    } else { belowThresholdHotkeys++; scanned += rows }
  }
  const spentFeeFloorRao = rootClaimSpentFeeRao(reserved, previews.length, redeemed, scanned, selectionScans, limit)
  const previewComplete = unquotedHotkeys === 0
  return Object.freeze({ reservedFeeRao: reserved, spentFeeFloorRao, possibleRefundRao: reserved - spentFeeFloorRao, accruedRao: previewComplete ? accruedRao : null, redeemableRao: previewComplete ? redeemableRao : null, basketAccruedRao: accruedRao, basketRedeemableRao: redeemableRao, previewComplete, unquotedHotkeys, forfeitedRao, thresholdRao: threshold, eligibleHotkeys, belowThresholdHotkeys, dustRows, losesMoney: previewComplete ? spentFeeFloorRao > redeemableRao : null })
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
