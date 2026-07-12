import nativeBinding from './native'
import { fromWire, toBuffer, toWire, WIRE_TAG } from './wire'
import type { ByteLike, ScaleValue } from './types'

export * from './crypto'
export * from './errors'
export * from './balance'
export * from './client'
export * from './keys'
export * from './ledger'
export * from './modules'
export * from './runtime'
export * from './timelock'
export * from './types'
export * from './value'
export * from './wallet'
export { fromWire, toWire, WIRE_TAG }

/**
 * Direct access to the generated Node-API module. This is intentionally
 * exported so every native entry point remains callable even before an
 * ergonomic TypeScript convenience wrapper is added.
 */
export const native = Object.freeze(nativeBinding)

export const BINDING_VERSION = nativeBinding.bindingVersion()
export const LEDGER_ENABLED = nativeBinding.ledgerEnabled()

export function wireRoundtrip(value: ScaleValue): ScaleValue {
  return fromWire(nativeBinding.wireRoundtrip(toWire(value)))
}

export function valueToCorpusJson(value: ScaleValue): unknown {
  return nativeBinding.valueToCorpusJson(toWire(value))
}

export function u256LeToDecimal(raw: ByteLike): string {
  return nativeBinding.u256LeToDecimal(toBuffer(raw, 'raw'))
}
