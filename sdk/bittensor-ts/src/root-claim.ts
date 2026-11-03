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
  accruedRao: bigint
  redeemableRao: bigint
  forfeitedRao: bigint
  thresholdRao: bigint
  eligibleHotkeys: number
  belowThresholdHotkeys: number
  dustRows: number
  losesMoney: boolean
}
const read = 25_000_000n, write = 100_000_000n
const claim = (units: bigint) => 567_441_000n + 437_318_379n * units + (23n + 22n * units) * read + (18n + 16n * units) * write
const scan = (units: bigint) => 309_208_000n + 263_604_241n * units + (6n + 21n * units) * read
const weightFee = (refTime: bigint) => refTime / 4000n + (refTime % 4000n * 2n > 4000n ? 1n : 0n)

export function rootClaimChargedFeeRao(limit = 256): bigint {
  const units = BigInt(Math.min(count(limit), 4))
  const weight = claim(units) + scan(units) + (10_000_000n + 4n * read) * 6n * units + (units === 0n ? 0n : claim(units))
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

export function quoteRootClaimPreviews(previews: readonly (RootClaimPreview | null)[], reserved: bigint, threshold: bigint, selectionScans: number, limit = 256): RootClaimFeeQuote {
  if (threshold < 0n) throw new RangeError('claim threshold must be nonnegative')
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
  return Object.freeze({ reservedFeeRao: reserved, spentFeeFloorRao, possibleRefundRao: reserved - spentFeeFloorRao, accruedRao, redeemableRao, forfeitedRao, thresholdRao: threshold, eligibleHotkeys, belowThresholdHotkeys, dustRows, losesMoney: spentFeeFloorRao > redeemableRao })
}
function count(value: number): number { if (!Number.isSafeInteger(value) || value < 0) throw new RangeError('claim work count must be a nonnegative safe integer'); return value }
