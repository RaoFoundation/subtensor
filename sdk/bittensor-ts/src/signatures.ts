/** Normalize a signature for its declared MultiSignature scheme.
 * Raw ECDSA is 65 bytes; its first byte is signature data, never an enum tag.
 */
export function normalizeSignatureBytes(signature: Uint8Array, cryptoType: number): Uint8Array {
  const size = cryptoType === 2 ? 65 : cryptoType === 0 || cryptoType === 1 ? 64 : undefined
  if (size == null) throw new TypeError(`unsupported signer cryptoType ${cryptoType}`)
  if (signature.length === size) return new Uint8Array(signature)
  if (signature.length === size + 1) {
    if (signature[0] !== cryptoType) throw new TypeError('signature scheme differs from signer')
    return signature.slice(1)
  }
  throw new TypeError(`signature must be ${size} bytes, or ${size + 1} bytes with its scheme prefix`)
}
