import { OWNER_HYPERPARAMETER_CATALOG } from './intent-hyperparameter-catalog'
import type { IntentDescriptor, IntentReader } from './intents'

export const OWNER_HYPERPARAMETERS = Object.freeze(Object.keys(OWNER_HYPERPARAMETER_CATALOG))
type Entry = { call: string; params: readonly string[]; kind: string; minimum: string; maximum: string }

/** Exact conversion: integer strings/bigints are raw; decimal strings are human units. */
export function hyperparameterToRaw(name: string, value: unknown): bigint | boolean | 'Yuma' | 'Null' {
  const entry = (OWNER_HYPERPARAMETER_CATALOG as Record<string, Entry>)[name]
  if (!entry) throw new RangeError(`unknown or owner-unsettable hyperparameter ${name}`)
  if (name === 'epoch_consensus') {
    if (value !== 'Yuma' && value !== 'Null') throw new RangeError('epoch_consensus must be Yuma or Null')
    return value
  }
  if (entry.kind === 'bool') {
    if ([true, 1, 1n, '1', 'true', 'yes', 'on'].includes(value as never)) return true
    if ([false, 0, 0n, '0', 'false', 'no', 'off'].includes(value as never)) return false
    throw new TypeError(`${name} accepts true, false, 0, or 1`)
  }
  if (typeof value === 'boolean') throw new TypeError(`${name} does not accept booleans`)
  if (typeof value !== 'number' && typeof value !== 'bigint' && typeof value !== 'string') throw new TypeError(`${name} needs a numeric value`)
  if (typeof value === 'number' && (!Number.isFinite(value) || Number.isInteger(value) && !Number.isSafeInteger(value))) throw new RangeError('use an exact string for a large hyperparameter')
  const text = String(value).trim().replace(/^\+/, '')
  const match = /^(\d+)(?:\.(\d*))?(?:e([+-]?\d+))?$/i.exec(text)
  if (!match) throw new TypeError(`${name} requires a nonnegative numeric value`)
  let result: bigint
  const human = typeof value === 'number' && !Number.isInteger(value) || typeof value === 'string' && /[.e]/i.test(text)
  if (!human) result = BigInt(text)
  else {
    const power = Number(match[3] ?? 0) - (match[2]?.length ?? 0)
    if (!Number.isSafeInteger(power) || Math.abs(power) > 1000) throw new RangeError('hyperparameter exponent out of range')
    let numerator = BigInt(match[1] + (match[2] ?? '')), denominator = 1n
    if (power < 0) denominator = 10n ** BigInt(-power)
    else numerator *= 10n ** BigInt(power)
    const scales: Record<string, bigint> = { u16: 65535n, u64: (1n << 64n) - 1n, per_million: 1000000n, rao: 1000000000n, fixed128: 1n << 64n }
    if (['u16', 'u64', 'per_million'].includes(entry.kind) && numerator > denominator) throw new RangeError(`${name} fraction must be between zero and one`)
    const scale = scales[entry.kind]
    if (scale == null && numerator % denominator !== 0n) throw new RangeError(`${name} requires an integral value`)
    numerator *= scale ?? 1n
    const quotient = numerator / denominator, remainder = numerator % denominator
    result = quotient + (remainder * 2n > denominator || remainder * 2n === denominator && quotient % 2n === 1n ? 1n : 0n)
  }
  if (result < BigInt(entry.minimum) || result > BigInt(entry.maximum)) throw new RangeError(`${name} must be within ${entry.minimum}..${entry.maximum}`)
  return result
}

export async function prepareHyperparameter(reader: IntentReader, netuid: number, name: string, input: unknown): Promise<IntentDescriptor> {
  const value = hyperparameterToRaw(name, input)
  const entry = (OWNER_HYPERPARAMETER_CATALOG as Record<string, Entry>)[name]
  const params: Record<string, unknown> = { netuid }
  if (name === 'alpha_low' || name === 'alpha_high') {
    const pair = await reader.query('SubtensorModule', 'AlphaValues', [netuid])
    if (!Array.isArray(pair) || pair.length !== 2) throw new Error('current alpha pair unavailable')
    params.alpha_low = name === 'alpha_low' ? value : pair[0]
    params.alpha_high = name === 'alpha_high' ? value : pair[1]
  } else params[entry.params[1]] = entry.kind === 'fixed128' ? { bits: value } : value
  return { pallet: 'AdminUtils', call: entry.call, params }
}
