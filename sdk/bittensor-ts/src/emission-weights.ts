/** Pure emission-weight arithmetic. Exact on-chain shares use bigint u64 parts. */
export const ONE = (1n << 64n) - 1n
export const SLOT_NAMES = ['burn', 'recycle', 'owner', 'validator', 'miner'] as const
export const ADJUSTABLE_SLOTS = [0, 1, 2, 3, 4] as const
export const DEFAULT_OWNER = 11796n * ONE / 65535n
export const DEFAULT_VALIDATOR = (ONE - DEFAULT_OWNER) / 2n
export const MAX_OWNER = ONE * 33n / 100n
export const MAX_BURN_RECYCLE = ONE * 33n / 100n
export const MAX_VALIDATOR = ONE / 2n
export const MIN_MINER = ONE * 33n / 100n
export const MAX_GAMMA = ONE * 10n / 100n
export const MAX_GAMMA_SHARES = 16
export const MAX_ROUNDING_PARTS = 1000n
export const OWNER_RATE_LIMIT_BLOCKS = 50400
export type EmissionSlot = typeof SLOT_NAMES[number] | number
export type ShareLike = bigint | number | string
export interface EmissionRules { min_validator?: bigint | null; min_owner?: bigint; created_via_lease?: boolean; gamma_share?: bigint; gamma_targets?: Readonly<Record<number, boolean>> | null; leased?: boolean }
export interface EmissionViolation { error: string; message: string }
export function slotIndex(slot: EmissionSlot | string): number {
  const value = typeof slot === 'string' && !/^\d+$/.test(slot.trim()) ? SLOT_NAMES.indexOf(slot.trim().toLowerCase() as typeof SLOT_NAMES[number]) : Number(slot)
  if (!Number.isInteger(value) || value < 0 || value >= 5) throw new RangeError(`unknown emission slot ${slot}`)
  return value
}
export function parseShare(value: ShareLike): bigint {
  let out: bigint
  if (typeof value === 'bigint' || (typeof value === 'number' && Number.isInteger(value))) {
    if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('raw shares must use bigint above Number.MAX_SAFE_INTEGER')
    out = BigInt(value)
  } else {
    let text = String(value).trim().replaceAll('_', '')
    const percent = text.endsWith('%')
    if (percent) text = text.slice(0, -1)
    if (!percent && typeof value === 'string' && /^\d+$/.test(text)) out = BigInt(text)
    else {
      const match = /^(\d*)(?:\.(\d*))?(?:e([+-]?\d+))?$/i.exec(text)
      if (!match || !(match[1] || match[2])) throw new RangeError(`invalid emission share ${value}`)
      const power = Number(match[3] ?? 0) - (match[2]?.length ?? 0) - (percent ? 2 : 0)
      if (!Number.isSafeInteger(power) || Math.abs(power) > 1000) throw new RangeError('emission share exponent out of range')
      let numerator = BigInt((match[1] || '0') + (match[2] || ''))
      let denominator = 1n
      if (power >= 0) numerator *= 10n ** BigInt(power)
      else denominator = 10n ** BigInt(-power)
      if (numerator > denominator) throw new RangeError('emission fraction must be between zero and one')
      out = ONE * numerator / denominator
    }
  }
  if (out < 0n || out > ONE) throw new RangeError('emission share outside u64 range')
  return out
}
export const shareToFloat = (parts: bigint): number => Number(parts) / Number(ONE)
export const formatShare = (parts: bigint, places = 2): string => `${(shareToFloat(parts) * 100).toFixed(places)}%`
export function normalizeWeights(weights: Iterable<bigint | number | string>): bigint[] {
  const result = Array.from(weights, value => {
    if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('emission weight requires bigint')
    return BigInt(value)
  })
  if (result.length > 5) throw new RangeError('at most five emission slots are supported')
  while (result.length < 5) result.push(0n)
  return result
}
export function defaultWeights(owner = DEFAULT_OWNER): bigint[] {
  if (owner < 0n || owner > ONE) throw new RangeError('owner share outside u64 range')
  const validator = (ONE - owner) / 2n
  return [0n, 0n, owner, validator, ONE - owner - validator]
}
export function mergeWeights(current: Iterable<bigint>, updates: Record<string, bigint>): bigint[] {
  const result = normalizeWeights(current)
  for (const [slot, value] of Object.entries(updates)) result[slotIndex(slot)] = value
  return result
}
/** Repair only tiny rounding differences; the miner slot receives the remainder. */
export function absorbRounding(weights: Iterable<bigint>, gammaShare = 0n): bigint[] {
  const result = normalizeWeights(weights)
  const gap = ONE - gammaShare - result.reduce((sum, value) => sum + value, 0n)
  const removable = result[4] < MAX_ROUNDING_PARTS ? result[4] : MAX_ROUNDING_PARTS
  if (gap >= -removable && gap <= MAX_ROUNDING_PARTS) result[4] += gap
  return result
}
/** Decode storage shares without losing u64 precision or accepting duplicate targets. */
export function gammaEmissionShares(rows: unknown): Record<number, bigint> {
  if (rows == null) return {}
  if (!Array.isArray(rows)) throw new TypeError('invalid Gamma emission shares')
  const shares: Record<number, bigint> = {}
  for (const row of rows) {
    if (!Array.isArray(row) || row.length !== 2) throw new TypeError('invalid Gamma emission share entry')
    const [target, value] = row
    const id = gammaTarget(target)
    if (Object.hasOwn(shares, id)) throw new RangeError('duplicate Gamma emission target')
    if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('Gamma emission share requires exact integer parts')
    if (typeof value !== 'bigint' && typeof value !== 'number' && !(typeof value === 'string' && /^\d+$/.test(value))) throw new TypeError('invalid Gamma emission share parts')
    const share = BigInt(value)
    if (share < 0n || share > ONE) throw new RangeError('Gamma emission share outside u64 range')
    shares[id] = share
  }
  return shares
}
export function gammaTarget(value: unknown): number {
  if (!(typeof value === 'string' && /^\d+$/.test(value) || typeof value === 'number' && Number.isSafeInteger(value) || typeof value === 'bigint')) throw new RangeError(`Gamma target ${String(value)} is not a netuid`)
  const id = BigInt(value)
  if (id < 0n || id > 65535n) throw new RangeError(`Gamma target ${String(value)} is not a netuid`)
  return Number(id)
}
export function parseGammaUpdates(value: unknown): Record<number, bigint> {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) throw new TypeError('gamma must map target netuids to emission shares')
  const result: Record<number, bigint> = {}
  for (const [target, share] of Object.entries(value)) {
    const id = gammaTarget(target)
    if (Object.hasOwn(result, id)) throw new RangeError('each Gamma target may appear only once')
    result[id] = parseShare(share as ShareLike)
  }
  return result
}
/** Omitted targets remain unchanged; an explicit zero removes a target. */
export function mergeGamma(current: Readonly<Record<number, bigint>>, updates: Readonly<Record<number, bigint>>): Record<number, bigint> {
  return Object.fromEntries(Object.entries({ ...current, ...updates }).filter(([, share]) => share !== 0n).sort(([a], [b]) => Number(a) - Number(b)))
}
/** Total reserved for Gamma targets; the five fixed slots share the remainder. */
export function gammaEmissionShare(rows: unknown): bigint {
  const total = Object.values(gammaEmissionShares(rows)).reduce((sum, share) => sum + share, 0n)
  if (total > ONE) throw new RangeError('Gamma emission shares exceed 100%')
  return total
}
export function renormalize(current: Iterable<bigint>, fixed: Record<string, bigint>, gammaShare = 0n): bigint[] {
  if (gammaShare < 0n || gammaShare > ONE) throw new RangeError('Gamma emission share outside u64 range')
  const result = normalizeWeights(current)
  const pinned = new Map(Object.entries(fixed).map(([slot, value]) => [slotIndex(slot), value]))
  for (const value of pinned.values()) if (value < 0n || value > ONE) throw new RangeError('invalid pinned share')
  let remaining = ONE - gammaShare - [...pinned.values()].reduce((a, b) => a + b, 0n)
  if (remaining < 0n) throw new RangeError('pinned shares exceed 100%')
  const free = ADJUSTABLE_SLOTS.filter(i => !pinned.has(i))
  if (!free.length) {
    const rounded = absorbRounding(mergeWeights(result, fixed), gammaShare)
    if (rounded.reduce((sum, value) => sum + value, gammaShare) !== ONE) throw new RangeError('pinned shares do not sum to 100%')
    return rounded
  }
  const total = free.reduce((sum, index) => sum + result[index], 0n)
  for (const [index, value] of pinned) result[index] = value
  const shares = free.map(index => total ? remaining * result[index] / total : remaining / BigInt(free.length))
  if (shares.length) shares[shares.length - 1] += remaining - shares.reduce((a, b) => a + b, 0n)
  free.forEach((index, offset) => { result[index] = shares[offset] })
  return result
}
export function validate(weights: Iterable<bigint>, rules: EmissionRules = {}): EmissionViolation[] {
  const w = normalizeWeights(weights), found: EmissionViolation[] = []
  const gammaShare = rules.gamma_share ?? 0n
  const check = (fails: boolean, error: string, message: string) => { if (fails) found.push({ error, message }) }
  check(w.some(v => v < 0n || v > ONE), 'InvalidEmissionWeight', 'weights must be u64 parts')
  check(w[0] + w[1] > MAX_BURN_RECYCLE, 'BurnRecycleEmissionAboveMaximum', 'burn and recycle together exceed 33%')
  check(w[2] > MAX_OWNER, 'OwnerEmissionAboveMaximum', 'owner share exceeds 33%')
  check(Boolean(rules.created_via_lease) && w[2] < (rules.min_owner ?? 0n), 'LeasedSubnetOwnerCutBelowMinimum', 'owner share is below the lease floor')
  check(w[3] > MAX_VALIDATOR, 'ValidatorEmissionAboveMaximum', 'validator share exceeds 50%')
  check(rules.min_validator != null && w[3] < rules.min_validator, 'ValidatorEmissionTooLowToPreserveRoot', 'validator share must exceed the root baseline')
  check(w[4] < MIN_MINER, 'MinerEmissionBelowMinimum', 'miner share is below 33%')
  check(gammaShare < 0n || gammaShare > ONE, 'InvalidEmissionWeight', 'Gamma shares must be u64 parts')
  check(w.reduce((a, b) => a + b, 0n) + gammaShare !== ONE, 'EmissionWeightsDoNotSumToOne', 'fixed slots and Gamma shares must sum exactly to u64::MAX')
  return found
}
export function validateGamma(netuid: number, shares: Readonly<Record<number, bigint>>, rules: EmissionRules = {}): EmissionViolation[] {
  const found: EmissionViolation[] = [], entries = Object.entries(shares)
  const check = (fails: boolean, error: string, message: string) => { if (fails) found.push({ error, message }) }
  check(entries.length > MAX_GAMMA_SHARES, 'TooManyGammaShares', `at most ${MAX_GAMMA_SHARES} Gamma shares are allowed`)
  check(entries.length > 0 && Boolean(rules.leased), 'GammaEmissionNotSettableOnLeasedSubnet', 'Gamma emission is not settable on a leased subnet')
  for (const [target, share] of entries) {
    const id = gammaTarget(target)
    check(id === netuid, 'InvalidValue', 'Gamma emission cannot target its own subnet')
    check(share < 0n || share > ONE, 'InvalidEmissionWeight', 'Gamma shares must be u64 parts')
    check(share > MAX_GAMMA, 'GammaEmissionAboveMaximum', `Gamma share for subnet ${id} exceeds 10%`)
    if (rules.gamma_targets != null) {
      if (!Object.hasOwn(rules.gamma_targets, id)) check(true, 'GammaEmissionTargetNotWhitelisted', `subnet ${id} is not whitelisted for Gamma emission`)
      else check(!rules.gamma_targets[id], 'GammaDisabled', `Gamma is disabled on subnet ${id}`)
    }
  }
  return found
}
export const minValidatorParts = (baseline: bigint): bigint => baseline < ONE ? baseline + 1n : ONE
export const rootSharePaid = (validator: bigint, baseline: bigint): number => shareToFloat(validator < baseline ? validator : baseline)
export function rootEmissionShare(validator: number, taoWeight: number, rootTao: number, alphaIssuance: number): number {
  const weighted = rootTao * taoWeight
  return weighted + alphaIssuance > 0 ? validator * weighted / (weighted + alphaIssuance) : 0
}
export const baselineRootShare = (taoWeight: number, rootTao: number, alphaIssuance: number): number => rootEmissionShare(shareToFloat(DEFAULT_VALIDATOR), taoWeight, rootTao, alphaIssuance)
export const validatorFloor = baselineRootShare
export const shares = (weights: Iterable<bigint>): Record<string, number> => Object.fromEntries(normalizeWeights(weights).map((value, index) => [SLOT_NAMES[index], shareToFloat(value)]))
export const describe = (weights: Iterable<bigint>): Record<string, string> => Object.fromEntries(normalizeWeights(weights).map((value, index) => [SLOT_NAMES[index], formatShare(value)]))
export function rateLimitStatus(lastChange: number, current: number, limit = OWNER_RATE_LIMIT_BLOCKS) {
  const next = lastChange <= 0 ? current : lastChange + limit
  return { limit_blocks: limit, last_change_block: lastChange, next_allowed_block: next, blocks_until_allowed: Math.max(0, next - current), owner_can_change_now: next <= current }
}
export function preview(before: Iterable<bigint>, after: Iterable<bigint>, inputs: { baseline_parts: bigint; global_tao_weight: number; root_tao: bigint; alpha_issuance: bigint }) {
  const old = normalizeWeights(before), next = normalizeWeights(after), baseline = inputs.baseline_parts
  const rejected = next[3] <= baseline
  return { before: { weights: old, shares: shares(old) }, after: { weights: next, shares: shares(next) }, changed: SLOT_NAMES.filter((_, i) => old[i] !== next[i]), root: {
    share_before: rootSharePaid(old[3], baseline), share_after: rejected ? null : rootSharePaid(next[3], baseline), baseline_share: shareToFloat(baseline),
    validators_before: shareToFloat(old[3] - (old[3] < baseline ? old[3] : baseline)), validators_after: rejected ? null : shareToFloat(next[3] - baseline),
    capped_before: baseline > old[3], capped_after: false, validator_floor: shareToFloat(baseline), global_tao_weight: inputs.global_tao_weight, root_tao: inputs.root_tao, alpha_issuance: inputs.alpha_issuance,
  } }
}
