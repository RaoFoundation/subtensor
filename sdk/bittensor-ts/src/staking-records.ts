import { bytesHex } from './portable'

/** Adapt decoded runtime maps without treating an SS58 display prefix as identity. */
export function accountRecord<T = unknown>(values: unknown, address: string, decodeAddress?: (address: string) => Uint8Array): T | undefined {
  if (values == null || typeof values !== 'object') return undefined
  const exact = values instanceof Map ? values.get(address) : Object.hasOwn(values, address) ? (values as Record<string, T>)[address] : undefined
  if (exact !== undefined) return exact as T
  if (!decodeAddress) return undefined
  const entries = values instanceof Map ? values.entries() : Object.entries(values)
  let account: string | undefined
  for (const [key, value] of entries) {
    account ??= bytesHex(decodeAddress(address))
    if (bytesHex(decodeAddress(String(key))) === account) return value as T
  }
  return undefined
}

/** Runtime decoders may preserve numeric map keys or stringify them. */
export function subnetRecord<T = unknown>(values: unknown, netuid: number): T | undefined {
  if (values == null || typeof values !== 'object') return undefined
  if (values instanceof Map) return (values.get(netuid) ?? values.get(String(netuid))) as T | undefined
  return Object.hasOwn(values, netuid) ? (values as Record<number, T>)[netuid] : undefined
}
