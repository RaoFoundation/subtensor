import { Buffer } from 'node:buffer'

import type { ByteLike, IntegerLike, ScaleValue } from './types'

export const WIRE_TAG = '__bittensor_core_wire__' as const
const BIGINT_TAG = 'bigint'
const BYTES_TAG = 'bytes'
const DICT_TAG = 'dict'
const MAX_DEPTH = 256

type WireValue =
  | null
  | boolean
  | number
  | string
  | WireValue[]
  | { [key: string]: WireValue }

function isByteLike(value: unknown): value is ByteLike {
  return Buffer.isBuffer(value) || value instanceof Uint8Array
}

function isPlainObject(value: object): value is Record<string, unknown> {
  const prototype = Object.getPrototypeOf(value)
  return prototype === Object.prototype || prototype === null
}

export function toBuffer(value: ByteLike, name = 'value'): Buffer {
  if (!isByteLike(value)) {
    throw new TypeError(`${name} must be a Buffer or Uint8Array`)
  }
  return Buffer.from(value.buffer, value.byteOffset, value.byteLength)
}

export function toBigInt(value: IntegerLike, name = 'value'): bigint {
  if (typeof value === 'bigint') return value
  if (!Number.isSafeInteger(value)) {
    throw new RangeError(`${name} must be a safe integer or bigint`)
  }
  return BigInt(value)
}

export function coerceMessage(value: string | ByteLike, name = 'message'): Buffer {
  if (typeof value !== 'string') return toBuffer(value, name)
  if (!value.startsWith('0x')) return Buffer.from(value, 'utf8')
  const hex = value.slice(2)
  if (hex.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(hex)) {
    throw new TypeError(`${name} contains invalid 0x-prefixed hex`)
  }
  return Buffer.from(hex, 'hex')
}

export function toWire(value: ScaleValue): WireValue {
  return toWireAt(value, 0)
}

function toWireAt(value: ScaleValue, depth: number): WireValue {
  if (depth > MAX_DEPTH) throw new RangeError('value nesting exceeds 256 levels')
  if (value === null || typeof value === 'boolean' || typeof value === 'string') {
    return value
  }
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) {
      throw new RangeError('SCALE number values must be safe integers; use bigint otherwise')
    }
    // napi-rs' serde bridge represents JS numbers outside i32/u32 as f64.
    // Tag those integers so Rust receives their exact decimal representation.
    if (value < -0x8000_0000 || value > 0xffff_ffff) {
      return { [WIRE_TAG]: BIGINT_TAG, value: value.toString(10) }
    }
    return value
  }
  if (typeof value === 'bigint') {
    return { [WIRE_TAG]: BIGINT_TAG, value: value.toString(10) }
  }
  if (isByteLike(value)) {
    return { [WIRE_TAG]: BYTES_TAG, hex: toBuffer(value).toString('hex') }
  }
  if (Array.isArray(value)) {
    return value.map((item) => toWireAt(item, depth + 1))
  }
  if (value instanceof Map) {
    return {
      [WIRE_TAG]: DICT_TAG,
      entries: [...value.entries()].map(([key, item]) => [
        toWireAt(key, depth + 1),
        toWireAt(item, depth + 1),
      ]),
    }
  }
  if (!isPlainObject(value)) {
    throw new TypeError('SCALE values must use plain objects, arrays, Map, bigint, or bytes')
  }

  const entries = Object.entries(value)
  if (Object.prototype.hasOwnProperty.call(value, WIRE_TAG)) {
    return {
      [WIRE_TAG]: DICT_TAG,
      entries: entries.map(([key, item]) => [key, toWireAt(item as ScaleValue, depth + 1)]),
    }
  }

  const output: Record<string, WireValue> = {}
  for (const [key, item] of entries) {
    if (item === undefined) {
      throw new TypeError(`SCALE object field ${JSON.stringify(key)} is undefined`)
    }
    output[key] = toWireAt(item as ScaleValue, depth + 1)
  }
  return output
}

export function fromWire(value: unknown): ScaleValue {
  return fromWireAt(value, 0)
}

function fromWireAt(value: unknown, depth: number): ScaleValue {
  if (depth > MAX_DEPTH) throw new RangeError('decoded value nesting exceeds 256 levels')
  if (
    value === null ||
    typeof value === 'boolean' ||
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'bigint'
  ) {
    return value
  }
  if (isByteLike(value)) return Buffer.from(value)
  if (Array.isArray(value)) return value.map((item) => fromWireAt(item, depth + 1))
  if (typeof value !== 'object') {
    throw new TypeError(`unexpected native SCALE value type: ${typeof value}`)
  }

  const object = value as Record<string, unknown>
  const tag = object[WIRE_TAG]
  if (tag === BIGINT_TAG) {
    if (typeof object.value !== 'string') throw new TypeError('invalid native bigint wire value')
    return BigInt(object.value)
  }
  if (tag === BYTES_TAG) {
    if (typeof object.hex !== 'string' || !/^[0-9a-fA-F]*$/.test(object.hex)) {
      throw new TypeError('invalid native bytes wire value')
    }
    return Buffer.from(object.hex, 'hex')
  }
  if (tag === DICT_TAG) {
    if (!Array.isArray(object.entries)) throw new TypeError('invalid native dict wire value')
    const output = new Map<ScaleValue, ScaleValue>()
    for (const entry of object.entries) {
      if (!Array.isArray(entry) || entry.length !== 2) {
        throw new TypeError('invalid native dict entry')
      }
      output.set(fromWireAt(entry[0], depth + 1), fromWireAt(entry[1], depth + 1))
    }
    return output
  }

  const output: Record<string, ScaleValue> = {}
  for (const [key, item] of Object.entries(object)) {
    output[key] = fromWireAt(item, depth + 1)
  }
  return output
}
