import type { Balance, BalanceLike } from '../balance'
import type { ScaleValue } from '../types'

export type ReadDescriptor = readonly [string, string]
/** A decoded chain record. Field names are runtime-version dependent. */
export type ChainRecord = Record<string, any>
export interface ReadView {
  readonly block?: number | (() => Promise<number>)
  query(item: ReadDescriptor, params?: ScaleValue[]): Promise<any>
  queryMap(item: ReadDescriptor, params?: ScaleValue[]): Promise<Array<[any, any]>>
  queryBatch(item: ReadDescriptor, sets: ScaleValue[][]): Promise<any[]>
  runtime(item: ReadDescriptor, params?: ScaleValue[]): Promise<any>
  constant(item: ReadDescriptor): Promise<any>
  at(block?: number | null): Promise<ReadView>
  balance(rao: BalanceLike, netuid?: number, symbol?: string | null): Balance
  timestamp(): Promise<Date>
  blockTime(): Promise<number>
  isFastBlocks(): Promise<boolean>
  blockInfo(block?: number | null): Promise<any>
}
export const st = (name: string): ReadDescriptor => ['SubtensorModule', name]
export const api = (name: string, method: string): ReadDescriptor => [name, method]
export const integer = (value: any): bigint => {
  if (value == null) return 0n
  if (typeof value === 'boolean') return value ? 1n : 0n
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('chain integer exceeds safe precision')
  return BigInt(value)
}
export const num = (value: any): number => {
  const out = Number(value ?? 0)
  if (!Number.isSafeInteger(out)) throw new RangeError('chain integer exceeds safe precision; use bigint')
  return out
}
export const fixedBits = (value: any): bigint => integer(typeof value === 'object' && value !== null ? value.bits : value)
export const fixed = (value: any, bits = 64): number => Number(fixedBits(value)) / 2 ** bits
export const min = (a: bigint, b: bigint): bigint => a < b ? a : b
export const max = (a: bigint, b: bigint): bigint => a > b ? a : b
export const blockOf = (view: ReadView): number => {
  if (typeof view.block !== 'number') throw new Error('read requires a block-pinned view')
  return view.block
}
export const bal = (view: ReadView, value: any, netuid = 0): Balance => view.balance(integer(value), netuid)
export const keyOf = (key: any): any => Array.isArray(key) && key.length === 1 ? key[0] : key
export const textRecord = (value: ChainRecord | null | undefined): ChainRecord | null => value ? Object.fromEntries(Object.entries(value).map(([k, v]) => [k, utf8Text(v)])) : null
export function utf8Text(value: any): any {
  let bytes: Uint8Array
  if (typeof value === 'string' && /^0x(?:[0-9a-f]{2})*$/i.test(value)) bytes = fromHex(value)
  else if (value instanceof Uint8Array) bytes = value
  else if (Array.isArray(value) && value.every(v => Number.isInteger(Number(v)) && Number(v) >= 0 && Number(v) <= 255)) bytes = Uint8Array.from(value.map(Number))
  else return value
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes) }
  catch { return toHex(bytes) }
}
export function absentFeature(error: unknown): boolean {
  return error instanceof Error && /(?:not found|unknown (?:storage|runtime|method)|unavailable|does not exist|not supported)/i.test(error.message)
}

export const fromHex = (value: string): Uint8Array => Uint8Array.from((value.replace(/^0x/, '').match(/.{2}/g) ?? []).map(byte => parseInt(byte, 16)))
export const toHex = (bytes: Uint8Array): string => '0x' + Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
