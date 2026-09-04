/**
 * The worked example every derivatives figure uses: a 100 τ cushion on a
 * 10,000 τ / 200,000 α pool (0.05 τ/α). Shorts run at 1x and lift 1% of the
 * pool; longs run at 2x and lift 2%. A short pays 6 τ/day × the pool share it
 * lifts; a long pays 0.01%/day of its TAO exposure. Both fees are scaled by
 * `1 / (1 − phi)^4` for the position's own slippage.
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
/** `short_leverage_percent` / `long_leverage_percent`, as multipliers. */
export const LEVERAGE: Record<Side, number> = {short: 1, long: 2};
/** TAO per day a short pays for borrowing the whole pool (`short_fee_per_day`). */
export const SHORT_FEE_PER_DAY = 6;
/** Fraction of TAO exposure a long pays per day (`long_rate_per_day`). */
export const LONG_RATE_PER_DAY = 0.0001;
export const OPEN_PRICE = POOL_TAO / POOL_ALPHA;

/** Share of the pool the position lifts: `L × cushion / T`. */
export function phi(side: Side): number {
  return (LEVERAGE[side] * CUSHION) / POOL_TAO;
}

/** The lifted slice and what stays in the pool. */
export function lift(side: Side): {tao: number; alpha: number; restTao: number; restAlpha: number} {
  const p = phi(side);
  return {
    tao: p * POOL_TAO,
    alpha: p * POOL_ALPHA,
    restTao: (1 - p) * POOL_TAO,
    restAlpha: (1 - p) * POOL_ALPHA,
  };
}

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

/** `1 / (1 − phi)^4`: the pallet's `size_factor`. */
export function sizeFactor(p: number): number {
  return 1 / (1 - p) ** 4;
}

/** Fee per day, fixed at open: `6 τ × phi` for a short, `0.01% × exposure` for a long, times the size factor. */
export function feePerDay(side: Side): number {
  const p = phi(side);
  const base = side === 'short' ? SHORT_FEE_PER_DAY * p : LONG_RATE_PER_DAY * lift(side).tao;
  return base * sizeFactor(p);
}

export function feeFor(side: Side, days: number): number {
  return feePerDay(side) * Math.max(1, days);
}

/** Close a position `days` after opening with alpha `movePct` away from the open price. */
export function simulate(side: Side, movePct: number, days = 1): Outcome {
  // A move of m% in price is the pool drifting so that tao/alpha scales by (1 + m).
  const k = Math.sqrt(1 + movePct / 100);
  const fee = feeFor(side, days);
  const {tao: liftTao, alpha: liftAlpha, restTao, restAlpha} = lift(side);

  if (side === 'short') {
    const proceeds = (restTao * liftAlpha) / (restAlpha + liftAlpha);
    const tao0 = restTao - proceeds;
    const alpha0 = restAlpha + liftAlpha;
    const tao = tao0 * k;
    const alpha = alpha0 / k;
    const closeLeg = (tao * liftAlpha) / (alpha - liftAlpha);
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

  const proceeds = (restAlpha * liftTao) / (restTao + liftTao);
  const tao0 = restTao + liftTao;
  const alpha0 = restAlpha - proceeds;
  const tao = tao0 * k;
  const alpha = alpha0 / k;
  const closeLeg = (tao * proceeds) / (alpha + proceeds);
  const payout = Math.max(0, CUSHION + closeLeg - liftTao - fee);
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
