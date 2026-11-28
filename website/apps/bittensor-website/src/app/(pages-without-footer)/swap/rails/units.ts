/**
 * Amounts. The hub and the canonical tokens count in 9 decimals. The stablecoin has 6.
 * Everything is a bigint until it is shown.
 *
 * The site compiles to a target without bigint literals, so the constants are built with
 * `BigInt()`.
 */

export const ZERO = BigInt(0);
const TEN = BigInt(10);
const HUNDRED = BigInt(100);
const BPS = BigInt(10_000);

/** The decimals of the stablecoin on the spoke. */
export const STABLE_DECIMALS = 6;
/** One unit of the stablecoin in the hub's units. */
export const STABLE_SCALE = TEN ** BigInt(9 - STABLE_DECIMALS);
/** The scale of an alpha token's rate. */
export const RATE_SCALE = TEN ** BigInt(18);

/** Parses what a person typed into units of `decimals`, or `null` if it is not an amount. */
export function parseAmount(text: string, decimals: number): bigint | null {
  const trimmed = text.trim().replace(/,/g, '');
  if (!/^\d*\.?\d*$/.test(trimmed) || trimmed === '' || trimmed === '.') return null;
  const [whole, fraction = ''] = trimmed.split('.');
  if (fraction.length > decimals) return null;
  return (
    BigInt(whole || '0') * TEN ** BigInt(decimals) + BigInt(fraction.padEnd(decimals, '0') || '0')
  );
}

/** Shows `amount` of `decimals` with at most `digits` after the point, rounded down. */
export function formatAmount(amount: bigint, decimals: number, digits = 4): string {
  const scale = TEN ** BigInt(decimals);
  const whole = amount / scale;
  const shown = Math.min(digits, decimals);
  const fraction = ((amount % scale) / TEN ** BigInt(decimals - shown))
    .toString()
    .padStart(shown, '0');
  const trimmed = fraction.replace(/0+$/, '');
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return trimmed ? `${grouped}.${trimmed}` : grouped;
}

/** An amount in the hub's 9 decimals. */
export const formatUnits9 = (amount: bigint, digits = 4) => formatAmount(amount, 9, digits);

/** The exact text of an amount, to put in an input. */
export function exactAmount(amount: bigint, decimals: number): string {
  return formatAmount(amount, decimals, decimals).replace(/,/g, '');
}

/** `amount` less `bps` basis points, rounded down. */
export function lessBps(amount: bigint, bps: number): bigint {
  return (amount * (BPS - BigInt(bps))) / BPS;
}

/** The change from `from` to `to` as a signed percentage with two decimals, or `null`. */
export function percentChange(from: bigint, to: bigint): string | null {
  if (from === ZERO) return null;
  const hundredths = ((to - from) * BPS) / from;
  const sign = hundredths < ZERO ? '−' : '+';
  const magnitude = hundredths < ZERO ? -hundredths : hundredths;
  return `${sign}${magnitude / HUNDRED}.${(magnitude % HUNDRED).toString().padStart(2, '0')}%`;
}
