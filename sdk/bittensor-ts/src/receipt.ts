import { Balance } from './balance'
import { hexBytes } from './portable'
import type { Runtime } from './browser'
const WIRE_TAG = '__bittensor_type'
export interface BlockHeader { number: number; parentHash?: string; hash?: string; raw: unknown }
function headerNumber(value: unknown): number { const n = Number((value as {number?: unknown})?.number); if (!Number.isSafeInteger(n) || n < 0) throw new Error('invalid block header'); return n }
export function normalizeHeader(raw: unknown): BlockHeader {
  const value = raw as { number?: string | number; parentHash?: string; hash?: string }
  return { number: headerNumber(raw), parentHash: value.parentHash, hash: value.hash, raw }
}

export function normalizeStatus(status: unknown): Record<string, unknown> {
  if (typeof status === 'string') return { [status.toLowerCase()]: null }
  const out: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(status as Record<string, unknown>)) out[key.toLowerCase()] = value
  return out
}

export function eventName(event: unknown): string {
  const value = event as { module_id?: string; event_id?: string; event?: { module_id?: string; event_id?: string; section?: string; method?: string } }
  const nested = value.event
  return `${value.module_id ?? nested?.module_id ?? nested?.section ?? ''}.${value.event_id ?? nested?.event_id ?? nested?.method ?? ''}`
}

export function eventExtrinsicIndex(event: unknown): number | null {
  const value = event as { extrinsic_idx?: unknown; phase?: unknown }
  const index = eventIndexNumber(value.extrinsic_idx)
  if (index != null) return index
  const phase = value.phase as Record<string, unknown> | undefined
  const apply = phase?.ApplyExtrinsic ?? phase?.applyExtrinsic
  return eventIndexNumber(apply)
}

function eventIndexNumber(value: unknown): number | null {
  if (value == null) return null
  if (typeof value === 'number') return Number.isSafeInteger(value) && value >= 0 ? value : null
  if (typeof value === 'bigint') {
    const number = Number(value)
    return Number.isSafeInteger(number) && number >= 0 ? number : null
  }
  if (typeof value === 'string') {
    if (!/^(?:0x[0-9a-fA-F]+|\d+)$/.test(value)) return null
    const number = Number(value)
    return Number.isSafeInteger(number) && number >= 0 ? number : null
  }
  if (typeof value !== 'object' || value instanceof Uint8Array) return null

  const object = value as Record<string, unknown>
  if (object[WIRE_TAG] === 'bigint' && typeof object.value === 'string') {
    return eventIndexNumber(object.value)
  }
  if (Array.isArray(value) && value.length === 1) return eventIndexNumber(value[0])

  const entries = Object.entries(object).filter(([key]) => key !== WIRE_TAG)
  if (entries.length !== 1) return null
  return eventIndexNumber(entries[0][1])
}

export function feeFromEvent(event: unknown): Balance | undefined {
  const attrs = (event as { attributes?: unknown }).attributes
  if (attrs == null || typeof attrs !== 'object') return undefined
  const amount = (attrs as Record<string, unknown>).actual_fee ?? (attrs as Record<string, unknown>).actualFee ?? (attrs as Record<string, unknown>).fee
  const rao = decimalAmount(amount)
  return rao == null ? undefined : Balance.fromRao(rao)
}

function decimalAmount(value: unknown): string | undefined {
  if (value == null) return undefined
  if (typeof value === 'bigint') return value >= 0n ? value.toString(10) : undefined
  if (typeof value === 'number') {
    return Number.isSafeInteger(value) && value >= 0 ? value.toString(10) : undefined
  }
  if (typeof value === 'string') {
    if (/^\d+$/.test(value)) return value
    if (/^0x[0-9a-fA-F]+$/.test(value)) return BigInt(value).toString(10)
    return undefined
  }
  if (typeof value !== 'object' || value instanceof Uint8Array) return undefined

  const object = value as Record<string, unknown>
  if (object[WIRE_TAG] === 'bigint' && typeof object.value === 'string') {
    return decimalAmount(object.value)
  }
  if (Array.isArray(value) && value.length === 1) return decimalAmount(value[0])

  const entries = Object.entries(object).filter(([key]) => key !== WIRE_TAG)
  if (entries.length !== 1) return undefined
  return decimalAmount(entries[0][1])
}

export function dispatchFailureMessage(runtime: Runtime, event: unknown): string {
  const message = formatDispatchError(runtime, dispatchErrorFromEvent(event))
  return message == null ? 'Extrinsic failed' : `Extrinsic failed: ${message}`
}

function dispatchErrorFromEvent(event: unknown): unknown {
  const outer = recordValue(event)
  if (outer == null) return undefined
  const attrs = recordValue(outer.attributes)
  if (attrs != null) {
    return attrs.dispatch_error ?? attrs.dispatchError ?? attrs.error
  }
  const nested = recordValue(outer.ExtrinsicFailed)
  return nested?.dispatch_error ?? nested?.dispatchError ?? nested
}

function formatDispatchError(runtime: Runtime, error: unknown): string | undefined {
  if (typeof error === 'string') return error
  const value = recordValue(error)
  if (value == null) return undefined
  const moduleError = recordValue(value.Module ?? value.module)
  if (moduleError != null) return formatModuleDispatchError(runtime, moduleError)
  const err = value.Err ?? value.err
  if (err != null) return formatDispatchError(runtime, err)
  const entries = Object.entries(value)
  if (entries.length !== 1) return undefined
  const [variant, payload] = entries[0]
  const nested = formatDispatchError(runtime, payload)
  return nested == null ? variant : `${variant}.${nested}`
}

function formatModuleDispatchError(runtime: Runtime, value: Record<string, unknown>): string | undefined {
  const moduleIndex = unsignedByteValue(value.index)
  const errorIndex = dispatchModuleErrorIndex(value.error)
  if (moduleIndex == null || errorIndex == null) return undefined
  try {
    const resolved = runtime.moduleError(moduleIndex, errorIndex)
    const name = Array.isArray(resolved) ? resolved[0] : resolved.name
    const rawDocs: string[] = Array.isArray(resolved) ? resolved[1] : resolved.docs
    const docs = rawDocs.map((doc: string) => doc.trim()).filter(Boolean).join(' ')
    return docs.length === 0 ? name : `${name}: ${docs}`
  } catch {
    return `Module(${moduleIndex}, ${errorIndex})`
  }
}

function dispatchModuleErrorIndex(value: unknown): number | undefined {
  if (value instanceof Uint8Array) return value[0]
  if (typeof value === 'string' && value.startsWith('0x')) {
    try {
      return hexBytes(value)[0]
    } catch {
      return undefined
    }
  }
  if (Array.isArray(value)) return unsignedByteValue(value[0])
  return unsignedByteValue(value)
}

function unsignedByteValue(value: unknown): number | undefined {
  let numeric: number
  if (typeof value === 'number') numeric = value
  else if (typeof value === 'bigint') {
    if (value < 0n || value > 255n) return undefined
    numeric = Number(value)
  } else if (typeof value === 'string') {
    numeric = value.startsWith('0x') ? Number.parseInt(value.slice(2), 16) : Number(value)
  } else {
    return undefined
  }
  return Number.isInteger(numeric) && numeric >= 0 && numeric <= 255 ? numeric : undefined
}

function recordValue(value: unknown): Record<string, unknown> | undefined {
  return value != null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}


export function nestedDispatchError(events: unknown[]): unknown {
  const fields: Record<string, string> = { 'Sudo.Sudid': 'sudo_result', 'Sudo.SudoAsDone': 'sudo_result', 'Proxy.ProxyExecuted': 'result', 'Multisig.MultisigExecuted': 'result', 'Utility.BatchInterrupted': 'error', 'Utility.ItemFailed': 'error' }
  for (const event of events) {
    const name = eventName(event), field = fields[name]
    if (!field) continue
    const row = event as any, attrs = row.attributes ?? row.event?.attributes
    const result = attrs?.[field] ?? attrs
    if (result && typeof result === 'object' && 'Err' in result) return result.Err
    if (name.startsWith('Utility.') && result != null) return result
  }
  return undefined
}
