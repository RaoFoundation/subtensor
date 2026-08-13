/** Runtime-aligned emission helpers for the documentation models. */

export const RAO_PER_TAO = 1_000_000_000;
export const TOTAL_SUPPLY_TAO = 21_000_000;
export const HALVING_REFERENCE_TAO = 10_500_000;
export const DEFAULT_BLOCK_EMISSION_TAO = 1;
export const DEFAULT_TEMPO = 360;
export const BLOCK_TIME_SEC = 12;
export const DEFAULT_KAPPA = 32_767 / 65_535;
export const DEFAULT_TAO_WEIGHT = 0.18;
export const DEFAULT_BONDS_MA = 900_000;
export const DEFAULT_BURN_HALF_LIFE = 360;
export const DEFAULT_BURN_INCREASE_MULT = 1.26;
export const DEFAULT_MIN_BURN_TAO = 500_000 / RAO_PER_TAO;
export const DEFAULT_MAX_BURN_TAO = 100;
export const DEFAULT_INITIAL_BURN_TAO = 0.1;
export const MATURITY_RATE_BLOCKS = 934_866;
export const UNLOCK_RATE_BLOCKS = 934_866;
export const ONE_YEAR_BLOCKS = 2_629_800;
export const CONVICTION_OWNERSHIP_THRESHOLD = 0.1;
export const EMA_HALVING_BLOCKS = 201_600;
export const SUBNET_MOVING_ALPHA = 0.000003;
export const DEFAULT_EMISSION_BAR_RANK = 32;
export const DEFAULT_EMISSION_BAR_QUANTILE = 0.61;
export const DEFAULT_EMISSION_GATE_EXPONENT = 3;

/** `get_block_emission_for_issuance` in block_emission.rs */
export function blockEmissionTao(issuanceTao: number): number {
  if (issuanceTao >= TOTAL_SUPPLY_TAO) return 0;

  const x = issuanceTao / (2 * HALVING_REFERENCE_TAO);
  if (x >= 1) return 0;

  const residual = Math.log2(1 / (1 - x));
  const k = Math.floor(residual);
  return DEFAULT_BLOCK_EMISSION_TAO / 2 ** k;
}

export function halvingThresholdsTao(count = 8): number[] {
  const thresholds: number[] = [];
  for (let k = 0; k < count; k++) {
    const fraction = 1 - 1 / 2 ** (k + 1);
    thresholds.push(fraction * 2 * HALVING_REFERENCE_TAO);
  }
  return thresholds;
}

export type SubnetEmissionResult = {
  demandShares: number[];
  burnAdjustedShares: number[];
  gateFactors: number[];
  shares: number[];
  gateBar: number;
};

/** `maybe_update_emission_gate_bar` in subnet_emissions.rs */
export function selectEmissionGateBar(
  demandShares: number[],
  rank = DEFAULT_EMISSION_BAR_RANK,
  quantile = DEFAULT_EMISSION_BAR_QUANTILE,
): number {
  const positive = demandShares.filter((share) => share > 0).sort((a, b) => b - a);
  if (positive.length === 0) return 0;
  if (rank > 0) return positive[Math.min(rank, positive.length) - 1];

  let cumulative = 0;
  for (const share of positive) {
    cumulative += share;
    if (cumulative >= quantile) return share;
  }
  return positive[positive.length - 1];
}

/** `get_shares` plus emission-enabled redistribution in subnet_emissions.rs */
export function subnetEmissionShares(
  prices: number[],
  options: {
    minerBurned?: number[];
    emissionEnabled?: boolean[];
    rank?: number;
    quantile?: number;
    exponent?: number;
    gateBar?: number;
  } = {},
): SubnetEmissionResult {
  const safePrices = prices.map((price) => Math.max(price, 0));
  const priceSum = safePrices.reduce((sum, price) => sum + price, 0);
  const demandShares = safePrices.map((price) => (priceSum > 0 ? price / priceSum : 0));
  const minerBurned = options.minerBurned ?? prices.map(() => 0);
  const burnWeights = demandShares.map(
    (share, index) => share * (1 - Math.min(Math.max(minerBurned[index] ?? 0, 0), 1)),
  );
  const burnWeightSum = burnWeights.reduce((sum, weight) => sum + weight, 0);
  const burnAdjustedShares =
    burnWeightSum > 0 ? burnWeights.map((weight) => weight / burnWeightSum) : demandShares;
  const gateBar =
    options.gateBar ?? selectEmissionGateBar(burnAdjustedShares, options.rank, options.quantile);
  const exponent = options.exponent ?? DEFAULT_EMISSION_GATE_EXPONENT;
  const gateFactors = burnAdjustedShares.map((share) => {
    if (share <= 0) return 0;
    if (gateBar <= 0) return 1;
    return 1 / (1 + (gateBar / share) ** exponent);
  });

  let gatedWeights = burnAdjustedShares.map((share, index) => share * gateFactors[index]);
  if (gatedWeights.reduce((sum, weight) => sum + weight, 0) === 0) {
    gatedWeights = burnAdjustedShares;
  }

  const emissionEnabled = options.emissionEnabled ?? prices.map(() => true);
  const enabledTotal = gatedWeights.reduce(
    (sum, weight, index) => sum + (emissionEnabled[index] === false ? 0 : weight),
    0,
  );
  const shares = gatedWeights.map((weight, index) =>
    emissionEnabled[index] !== false && enabledTotal > 0 ? weight / enabledTotal : 0,
  );

  return {demandShares, burnAdjustedShares, gateFactors, shares, gateBar};
}

/** `update_moving_price` smoothing factor in stake_utils.rs */
export function emaSmoothingAlpha(
  blocksSinceStart: number,
  halvingBlocks = EMA_HALVING_BLOCKS,
): number {
  return (SUBNET_MOVING_ALPHA * blocksSinceStart) / (blocksSinceStart + halvingBlocks);
}

/** `root_proportion` in block_step.rs (taoWeight is normalized fraction) */
export function rootProportion(
  rootTao: number,
  alphaIssuance: number,
  taoWeight = DEFAULT_TAO_WEIGHT,
): number {
  const scaled = rootTao * taoWeight;
  const denom = scaled + alphaIssuance;
  return denom > 0 ? scaled / denom : 0;
}

/** Per-block alpha_out split before epoch (run_coinbase.rs) */
export function alphaOutSplit(alphaOut: number, rootProp: number, ownerCutFrac = 11_796 / 65_535) {
  const owner = alphaOut * ownerCutFrac;
  const remainder = alphaOut - owner;
  const minerHalf = remainder * 0.5;
  const validatorHalf = remainder - minerHalf;
  const rootAlpha = rootProp * validatorHalf;
  const validators = validatorHalf - rootAlpha;

  return {owner, miner: minerHalf, validators, root: rootAlpha};
}

/** Stake-weighted median for one miner column (epoch/math.rs) */
export function weightedMedian(values: number[], stakes: number[], kappa: number): number {
  const totalStake = stakes.reduce((a, b) => a + b, 0);
  if (totalStake === 0) return 0;

  const target = kappa * totalStake;
  const pairs = values.map((v, i) => ({v, s: stakes[i]})).sort((a, b) => a.v - b.v);

  let cumulative = 0;
  for (const {v, s} of pairs) {
    cumulative += s;
    if (cumulative >= target) return v;
  }
  return pairs[pairs.length - 1]?.v ?? 0;
}

/** Classic Yuma incentive path for a tiny demo matrix */
export function yumaIncentives(
  weights: number[][],
  stakes: number[],
  kappa = DEFAULT_KAPPA,
): {consensus: number[]; clipped: number[][]; incentive: number[]} {
  const nMiners = weights[0]?.length ?? 0;
  const consensus: number[] = [];
  const clipped: number[][] = weights.map((row) => [...row]);

  for (let j = 0; j < nMiners; j++) {
    const col = weights.map((row) => row[j]);
    const c = weightedMedian(col, stakes, kappa);
    consensus.push(c);
    for (let i = 0; i < weights.length; i++) {
      clipped[i][j] = Math.min(weights[i][j], c);
    }
  }

  const rank = Array.from({length: nMiners}, (_, j) =>
    clipped.reduce((sum, row, i) => sum + row[j] * stakes[i], 0),
  );
  const rankSum = rank.reduce((a, b) => a + b, 0);
  const incentive = rankSum > 0 ? rank.map((r) => r / rankSum) : rank.map(() => 0);

  return {consensus, clipped, incentive};
}

/** Registration burn decay + bump (registration.rs) */
export function simulateBurnPrice(
  blocks: number,
  registrations: number[],
  halfLife = DEFAULT_BURN_HALF_LIFE,
  increaseMult = DEFAULT_BURN_INCREASE_MULT,
  minBurn = DEFAULT_MIN_BURN_TAO,
  maxBurn = DEFAULT_MAX_BURN_TAO,
  initialBurn = DEFAULT_INITIAL_BURN_TAO,
): number[] {
  const prices: number[] = [];
  let price = initialBurn;
  const decayPerBlock = 0.5 ** (1 / halfLife);

  for (let b = 0; b < blocks; b++) {
    if (registrations.includes(b)) {
      price = Math.min(maxBurn, price * increaseMult);
    }
    prices.push(price);
    price = Math.max(minBurn, price * decayPerBlock);
  }
  return prices;
}

/** Perpetual conviction curve (staking docs) */
export function perpetualConviction(
  lockedMass: number,
  startConviction: number,
  deltaBlocks: number,
  tau = MATURITY_RATE_BLOCKS,
): number {
  return lockedMass - (lockedMass - startConviction) * Math.exp(-deltaBlocks / tau);
}

/** exp(-dt/tau) — matches ConvictionModel::exp_decay */
export function expDecay(deltaBlocks: number, tau: number): number {
  if (tau === 0 || deltaBlocks === 0) return deltaBlocks === 0 ? 1 : 0;
  return Math.exp(-deltaBlocks / tau);
}

/** Roll a lock forward — mirrors calculate_decayed_mass_and_conviction in lock.rs */
export function rollForwardLock(
  lockedMass: number,
  conviction: number,
  deltaBlocks: number,
  options: {
    perpetual?: boolean;
    ownerLock?: boolean;
    unlockRate?: number;
    maturityRate?: number;
  } = {},
): {lockedMass: number; conviction: number} {
  const {
    perpetual = true,
    ownerLock = false,
    unlockRate = UNLOCK_RATE_BLOCKS,
    maturityRate = MATURITY_RATE_BLOCKS,
  } = options;

  if (deltaBlocks === 0) {
    return {
      lockedMass,
      conviction: ownerLock ? lockedMass : conviction,
    };
  }

  const unlockDecay = expDecay(deltaBlocks, unlockRate);
  const maturityDecay = expDecay(deltaBlocks, maturityRate);

  const newLockedMass = perpetual ? lockedMass : lockedMass * unlockDecay;

  let convictionFromMass = 0;
  if (perpetual) {
    convictionFromMass = lockedMass * (1 - maturityDecay);
  } else if (unlockRate === maturityRate) {
    convictionFromMass = lockedMass * (deltaBlocks / maturityRate) * maturityDecay;
  } else if (unlockRate > 0 && maturityRate > 0) {
    const gamma = (unlockRate * (unlockDecay - maturityDecay)) / (unlockRate - maturityRate);
    convictionFromMass = lockedMass * Math.max(0, gamma);
  }

  const newConviction = conviction * maturityDecay + convictionFromMass;

  if (ownerLock) {
    return {lockedMass: newLockedMass, conviction: newLockedMass};
  }

  return {lockedMass: newLockedMass, conviction: newConviction};
}

export function convictionOwnershipThreshold(alphaOut: number): number {
  return alphaOut * CONVICTION_OWNERSHIP_THRESHOLD;
}

export function formatAlpha(value: number, digits = 0): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M α`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(digits || 1)}k α`;
  return `${value.toLocaleString()} α`;
}

export function formatTao(value: number, digits = 4): string {
  if (value >= 1) return `${value.toFixed(Math.min(digits, 2))} τ`;
  if (value >= 0.001) return `${value.toFixed(4)} τ`;
  return `${(value * RAO_PER_TAO).toFixed(0)} rao`;
}

export function formatPct(value: number, digits = 1): string {
  return `${(value * 100).toFixed(digits)}%`;
}

export function formatBlocks(blocks: number): string {
  const hours = (blocks * BLOCK_TIME_SEC) / 3600;
  if (hours < 48) return `${hours.toFixed(1)} h`;
  return `${(hours / 24).toFixed(1)} d`;
}
