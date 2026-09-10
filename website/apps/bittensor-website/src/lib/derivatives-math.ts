/**
 * The worked example every derivatives figure uses: a 100 τ cushion on a
 * 10,000 τ / 200,000 α pool (0.05 τ/α). Each side is shown at its leverage
 * ceiling: the short at 1x lifts 1% of the pool, the long at 2x lifts 2%. Both
 * pay the same interest: `interest_rate` (25%) of their TAO exposure per year,
 * accrued per block, so the 2x long pays twice the 1x short. There is no term.
 *
 * `simulate` mirrors `pallet-derivatives`: lift `phi` of both reserves, trade one
 * half through the constant-product pool, let the market move, reverse the trade
 * against the moved pool, repay, and hand back the cushion plus or minus the
 * difference, minus the interest.
 */

export type Side = 'short' | 'long';

export const POOL_TAO = 10_000;
export const POOL_ALPHA = 200_000;
export const CUSHION = 100;
/**
 * The leverage the figures use per side: the runtime ceilings
 * `MaxShortLeverage` / `MaxLongLeverage`, as multipliers. An owner may open at
 * anything from 0.01x up to these.
 */
export const LEVERAGE: Record<Side, number> = {short: 1, long: 2};
/** Fraction of TAO exposure either side pays per year (`interest_rate`). */
export const INTEREST_RATE = 0.25;
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
  /** Interest owed after `days`. */
  fee: number;
  /** TAO returned to the owner. Never below zero: the pool carries any shortfall. */
  payout: number;
  /** `payout - CUSHION`. */
  pnl: number;
}

/** Interest per year, fixed at the add: `interest_rate × exposure`, the same on both sides. */
export function interestPerYear(side: Side): number {
  return INTEREST_RATE * lift(side).tao;
}

/** Interest after `days` held, pro rata; nothing is booked up front. */
export function interestFor(side: Side, days: number): number {
  return (interestPerYear(side) * Math.max(0, days)) / 365;
}

/** Settle a position `days` after adding it with alpha `movePct` away from the open price. */
export function simulate(side: Side, movePct: number, days = 0): Outcome {
  // A move of m% in price is the pool drifting so that tao/alpha scales by (1 + m).
  const k = Math.sqrt(1 + movePct / 100);
  const fee = interestFor(side, days);
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

/** TAO returned to the owner, closed the same block at `movePct` from open: no interest. */
export function payout(side: Side, movePct: number): number {
  return simulate(side, movePct).payout;
}
