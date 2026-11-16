import { evaluatePolicy } from './policy'
import { OWNER_HYPERPARAMETER_CATALOG } from './intent-hyperparameter-catalog'
import type { IntentDescriptor, IntentReader } from './intents'

export const OWNER_HYPERPARAMETERS = Object.freeze(Object.keys(OWNER_HYPERPARAMETER_CATALOG))
type Entry = { call: string; params: readonly string[]; kind: string; minimum: string; maximum: string }

/** Exact conversion: integer strings/bigints are raw; decimal strings are human units. */
export function hyperparameterToRaw(name: string, value: unknown): bigint | boolean | 'Yuma' | 'Null' {
  const entry = (OWNER_HYPERPARAMETER_CATALOG as Record<string, Entry>)[name]
  if (!entry) throw new RangeError(`unknown or owner-unsettable hyperparameter ${name}`)
  if (typeof value === 'number' && (!Number.isFinite(value) || Number.isInteger(value) && !Number.isSafeInteger(value))) throw new RangeError('use an exact string for a large hyperparameter')
  const human = typeof value === 'number' && !Number.isInteger(value) || typeof value === 'string' && /[.e]/i.test(value)
  const result = evaluatePolicy<string | boolean>('validation.hyperparameter', { name, ...entry, value, human, aliases: true })
  return typeof result === 'boolean' || result === 'Yuma' || result === 'Null' ? result : BigInt(result)

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
