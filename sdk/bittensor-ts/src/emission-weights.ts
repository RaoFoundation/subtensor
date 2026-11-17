import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
/** Pure emission-weight arithmetic. Exact on-chain shares use bigint u64 parts. */
export const ONE = BigInt(POLICY_CATALOG.financial.ONE)
export const SLOT_NAMES = POLICY_CATALOG.financial.SLOT_NAMES
export const ADJUSTABLE_SLOTS = [0, 1, 2, 3, 4] as const
export const DEFAULT_OWNER = BigInt(POLICY_CATALOG.financial.DEFAULT_OWNER)
export const DEFAULT_VALIDATOR = BigInt(POLICY_CATALOG.financial.DEFAULT_VALIDATOR)
export const MAX_OWNER = BigInt(POLICY_CATALOG.financial.MAX_OWNER)
export const MAX_BURN_RECYCLE = BigInt(POLICY_CATALOG.financial.MAX_BURN_RECYCLE)
export const MAX_VALIDATOR = BigInt(POLICY_CATALOG.financial.MAX_VALIDATOR)
export const MIN_MINER = BigInt(POLICY_CATALOG.financial.MIN_MINER)
export const MAX_GAMMA = BigInt(POLICY_CATALOG.financial.MAX_GAMMA)
export const MAX_GAMMA_SHARES = POLICY_CATALOG.financial.MAX_GAMMA_SHARES
export const MAX_ROUNDING_PARTS = BigInt(POLICY_CATALOG.financial.MAX_ROUNDING_PARTS)
export const OWNER_RATE_LIMIT_BLOCKS = POLICY_CATALOG.financial.OWNER_RATE_LIMIT_BLOCKS
export type EmissionSlot = typeof SLOT_NAMES[number] | number
export type ShareLike = bigint | number | string
export interface EmissionRules { min_validator?: bigint | null; min_owner?: bigint; created_via_lease?: boolean; gamma_share?: bigint; gamma_targets?: Readonly<Record<number, boolean>> | null; leased?: boolean }
export interface EmissionViolation { error: string; message: string }

const evaluate = <T>(op: string, input: unknown): T => evaluatePolicy<T>(`financial.emission.${op}`, input)
const integers = (values: string[]) => values.map(BigInt)
const integerMap = (values: Record<number, string>): Record<number, bigint> => Object.fromEntries(Object.entries(values).map(([key, value]) => [key, BigInt(value)]))
export function slotIndex(slot: EmissionSlot | string): number { return evaluate('slot', { slot }) }
export function parseShare(value: ShareLike): bigint { return BigInt(evaluate<string>('parse_share', { value, fraction_number: typeof value === 'number' && !Number.isInteger(value) })) }
export const shareToFloat = (parts: bigint): number => evaluate('float', { parts })
export const formatShare = (parts: bigint, places = 2): string => `${(shareToFloat(parts) * 100).toFixed(places)}%`
export function normalizeWeights(weights: Iterable<bigint | number | string>): bigint[] { return integers(evaluate('normalize', { weights: Array.from(weights) })) }
export function defaultWeights(owner = DEFAULT_OWNER): bigint[] { return integers(evaluate('default', { owner })) }
export function mergeWeights(current: Iterable<bigint>, updates: Record<string, bigint>): bigint[] { return integers(evaluate('merge', { weights: Array.from(current), updates })) }
export function absorbRounding(weights: Iterable<bigint>, gammaShare = 0n): bigint[] { return integers(evaluate('round', { weights: Array.from(weights), gamma: gammaShare })) }
export function gammaEmissionShares(rows: unknown): Record<number, bigint> { return integerMap(evaluate('gamma_shares', { rows })) }
export function gammaTarget(value: unknown): number { return evaluate('gamma_target', { value }) }
export function parseGammaUpdates(updates: unknown): Record<number, bigint> { return integerMap(evaluate('gamma_updates', { updates })) }
export function mergeGamma(current: Readonly<Record<number, bigint>>, updates: Readonly<Record<number, bigint>>): Record<number, bigint> { return integerMap(evaluate('merge_gamma', { current, updates })) }
export function gammaEmissionShare(rows: unknown): bigint { return BigInt(evaluate<string>('gamma_total', { rows })) }
export function renormalize(current: Iterable<bigint>, fixed: Record<string, bigint>, gammaShare = 0n): bigint[] { return integers(evaluate('renormalize', { weights: Array.from(current), fixed, gamma: gammaShare })) }
export function validate(weights: Iterable<bigint>, rules: EmissionRules = {}): EmissionViolation[] { return evaluate('validate', { weights: Array.from(weights), rules }) }
export function validateGamma(netuid: number, shares: Readonly<Record<number, bigint>>, rules: EmissionRules = {}): EmissionViolation[] { return evaluate<Array<EmissionViolation & { target?: number }>>('validate_gamma', { netuid, shares, rules }).map(({ error, message }) => ({ error, message })) }
export const minValidatorParts = (baseline: bigint): bigint => BigInt(evaluate<string>('min_validator', { baseline }))
export const rootSharePaid = (validator: bigint, baseline: bigint): number => evaluate('root_paid', { validator, baseline })
export function rootEmissionShare(validator: number, taoWeight: number, rootTao: number, alphaIssuance: number): number { return evaluate('root_share', { validator, tao_weight: taoWeight, root_tao: rootTao, alpha_issuance: alphaIssuance }) }
export const baselineRootShare = (taoWeight: number, rootTao: number, alphaIssuance: number): number => rootEmissionShare(shareToFloat(DEFAULT_VALIDATOR), taoWeight, rootTao, alphaIssuance)
export const validatorFloor = baselineRootShare
export const shares = (weights: Iterable<bigint>): Record<string, number> => Object.fromEntries(normalizeWeights(weights).map((value, index) => [SLOT_NAMES[index], shareToFloat(value)]))
export const describe = (weights: Iterable<bigint>): Record<string, string> => Object.fromEntries(normalizeWeights(weights).map((value, index) => [SLOT_NAMES[index], formatShare(value)]))
export function rateLimitStatus(lastChange: number, current: number, limit = OWNER_RATE_LIMIT_BLOCKS) { return evaluate<{ limit_blocks: number; last_change_block: number; next_allowed_block: number; blocks_until_allowed: number; owner_can_change_now: boolean }>('rate_limit', { last: lastChange, current, limit }) }
export function preview(before: Iterable<bigint>, after: Iterable<bigint>, inputs: { baseline_parts: bigint; global_tao_weight: number; root_tao: bigint; alpha_issuance: bigint }) {
  const result = evaluate<any>('preview', { before: Array.from(before), after: Array.from(after), ...inputs })
  result.before.weights = integers(result.before.weights); result.after.weights = integers(result.after.weights)
  result.root.root_tao = BigInt(result.root.root_tao); result.root.alpha_issuance = BigInt(result.root.alpha_issuance)
  return result
}
