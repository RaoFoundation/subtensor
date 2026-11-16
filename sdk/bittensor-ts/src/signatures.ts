/** Normalize a signature for its declared MultiSignature scheme.
 * Raw ECDSA is 65 bytes; its first byte is signature data, never an enum tag.
 */
export function normalizeSignatureBytes(signature: Uint8Array, cryptoType: number): Uint8Array {
  return hexBytes(evaluatePolicy<{ signature: string }>('transaction.signature_normalize', { signature, crypto_type: cryptoType, strict: true }).signature)
}
import { evaluatePolicy } from './policy'
import { hexBytes } from './portable'
