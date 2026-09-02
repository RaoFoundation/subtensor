/**
 * The worked example every derivatives figure uses: a 100 τ cushion at 1x on a
 * 10,000 τ / 200,000 α pool (0.05 τ/α). A short pays 5 τ/day × the pool share it
 * lifts; a long pays 0.02%/day of its TAO exposure.
 *
 * `simulate` mirrors `pallet-derivatives`: lift `phi` of both reserves, trade one
 * half through the constant-product pool, let the market move, reverse the trade
 * against the moved pool, repay, and hand back the cushion plus or minus the
 * difference, minus the fee.
 */

export type Side = 'short' | 'long';

export const POOL_TAO = 10_000;
export const POOL_ALPHA = 200_000;
export const CUSHION = 100;
/** TAO per day a short pays for borrowing the whole pool (`short_fee_per_day`). */
export const SHORT_FEE_PER_DAY = 5;
/** Fraction of TAO exposure a long pays per day (`long_rate_per_day`). */
export const LONG_RATE_PER_DAY = 0.0002;
export const OPEN_PRICE = POOL_TAO / POOL_ALPHA;

export const PHI = CUSHION / POOL_TAO;
export const LIFT_TAO = PHI * POOL_TAO;
export const LIFT_ALPHA = PHI * POOL_ALPHA;
export const REST_TAO = POOL_TAO - LIFT_TAO;
export const REST_ALPHA = POOL_ALPHA - LIFT_ALPHA;

export interface Outcome {
  /** TAO (short) or alpha (long) the opening trade produced. */
  proceeds: number;
  /** Pool price right after the opening trade. */
  priceOpen: number;
  /** Pool price when the position closes, after the market move. */
  priceClose: number;
  /** TAO paid to rebuy the debt (short) or raised by selling the alpha (long). */
  closeLeg: number;
  /** Borrow fee for `days`, one-day minimum. */
  fee: number;
  /** TAO returned to the owner. Never below zero: the pool carries any shortfall. */
  payout: number;
  /** `payout - CUSHION`. */
  pnl: number;
}

/** Fee per day, fixed at open: `5 τ × phi` for a short, `0.02% × exposure` for a long. */
export function feePerDay(side: Side): number {
  return side === 'short' ? SHORT_FEE_PER_DAY * PHI : LONG_RATE_PER_DAY * LIFT_TAO;
}

export function feeFor(side: Side, days: number): number {
  return feePerDay(side) * Math.max(1, days);
}

/** Close a position `days` after opening with alpha `movePct` away from the open price. */
export function simulate(side: Side, movePct: number, days = 1): Outcome {
  // A move of m% in price is the pool drifting so that tao/alpha scales by (1 + m).
  const k = Math.sqrt(1 + movePct / 100);
  const fee = feeFor(side, days);

  if (side === 'short') {
    const proceeds = (REST_TAO * LIFT_ALPHA) / (REST_ALPHA + LIFT_ALPHA);
    const tao0 = REST_TAO - proceeds;
    const alpha0 = REST_ALPHA + LIFT_ALPHA;
    const tao = tao0 * k;
    const alpha = alpha0 / k;
    const closeLeg = (tao * LIFT_ALPHA) / (alpha - LIFT_ALPHA);
    const payout = Math.max(0, CUSHION + proceeds - closeLeg - fee);
    return {
      proceeds,
      priceOpen: tao0 / alpha0,
      priceClose: tao / alpha,
      closeLeg,
      fee,
      payout,
      pnl: payout - CUSHION,
    };
  }

  const proceeds = (REST_ALPHA * LIFT_TAO) / (REST_TAO + LIFT_TAO);
  const tao0 = REST_TAO + LIFT_TAO;
  const alpha0 = REST_ALPHA - proceeds;
  const tao = tao0 * k;
  const alpha = alpha0 / k;
  const closeLeg = (tao * proceeds) / (alpha + proceeds);
  const payout = Math.max(0, CUSHION + closeLeg - LIFT_TAO - fee);
  return {
    proceeds,
    priceOpen: tao0 / alpha0,
    priceClose: tao / alpha,
    closeLeg,
    fee,
    payout,
    pnl: payout - CUSHION,
  };
}

/** TAO returned to the owner, closed after one day at `movePct` from open. */
export function payout(side: Side, movePct: number): number {
  return simulate(side, movePct).payout;
}
