import { hexBytes } from './portable'

function encodedBytes(value: unknown): Uint8Array {
  if (value instanceof Uint8Array) return value.slice()
  if (typeof value === 'string' && /^0x(?:[0-9a-f]{2})*$/i.test(value)) return hexBytes(value)
  if (Array.isArray(value) && value.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)) return Uint8Array.from(value)
  throw new TypeError('extrinsic signer must contain canonical bytes')
}

/** Recover the permanent account used for submission queueing, never an address alias.
 * Protected MultiAddress::Raw envelopes put AccountId32 first; their fixed layouts
 * are defined in primitives/hashed/{lib,mldsa,falcon}.rs. Authorization remains the
 * runtime's responsibility, but malformed or unknown shapes cannot choose a queue.
 */
export function extrinsicSignerAccountId(address: unknown, decodeSs58: (value: string) => Uint8Array): Uint8Array | undefined {
  if (address == null) return undefined
  if (typeof address === 'object' && !Array.isArray(address) && !(address instanceof Uint8Array)) {
    const fields = Object.keys(address)
    if (fields.length !== 1) throw new TypeError('extrinsic signer must have exactly one address variant')
    const variant = fields[0], value = (address as Record<string, unknown>)[variant]
    if (variant === 'Raw') {
      const raw = encodedBytes(value), version = raw[32], scheme = raw[33]
      const valid = version === 1 && [1, 3].includes(scheme) && raw.length === 106
        || version === 2 && [4, 5].includes(scheme) && raw.length === 5367
        || version === 2 && [6, 7].includes(scheme) && raw.length === 1669
        || version === 2 && [8, 9].includes(scheme) && raw.length === 3179
      if (!valid) throw new TypeError('unsupported protected extrinsic signer envelope')
      return raw.slice(0, 32)
    }
    if (variant !== 'Id' && variant !== 'Address32') throw new TypeError('unsupported extrinsic signer address variant')
    address = value
  }
  const account = typeof address === 'string' && !address.startsWith('0x') ? decodeSs58(address) : encodedBytes(address)
  if (account.length !== 32) throw new TypeError('extrinsic signer account must contain 32 bytes')
  return account.slice()
}
