/** Byte helpers shared by Node and browser APIs; no Buffer or native module. */
export function bytesHex(bytes: Uint8Array): string {
  return '0x' + Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
}

export function hexBytes(value: string): Uint8Array {
  const text = value.startsWith('0x') ? value.slice(2) : value
  if (text.length % 2 || !/^[0-9a-fA-F]*$/.test(text)) throw new TypeError('invalid hexadecimal bytes')
  return Uint8Array.from(text.match(/../g) ?? [], byte => Number.parseInt(byte, 16))
}

export function concatBytes(...parts: Uint8Array[]): Uint8Array {
  const result = new Uint8Array(parts.reduce((size, part) => size + part.length, 0))
  let offset = 0
  for (const part of parts) { result.set(part, offset); offset += part.length }
  return result
}

export function unsignedInteger(value: bigint | number | string, bits: number, name: string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError(`${name} must be a safe integer or bigint`)
  if (typeof value === 'string' && !/^\d+$/.test(value)) throw new TypeError(`${name} must be an unsigned integer`)
  const result = BigInt(value)
  if (result < 0n || result >= 1n << BigInt(bits)) throw new RangeError(`${name} must fit in u${bits}`)
  return result
}

export function integerLE(value: bigint | number, size: number): Uint8Array {
  let number = unsignedInteger(value, size * 8, 'integer')
  return Uint8Array.from({ length: size }, () => { const byte = Number(number & 255n); number >>= 8n; return byte })
}

export interface PortableCrypto {
  blake2_256(data: Uint8Array): Uint8Array
  publicKeyFromSs58(address: string): Uint8Array
  ss58FromPublic(publicKey: Uint8Array, format?: number): string
  verify(message: Uint8Array, signature: Uint8Array, address: string, cryptoType?: number): boolean
}

export interface PortableSigner {
  readonly ss58Address: string
  readonly publicKey?: Uint8Array
  readonly cryptoType: number
  sign(message: Uint8Array): Uint8Array | Promise<Uint8Array>
}
