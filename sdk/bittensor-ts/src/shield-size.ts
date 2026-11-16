import { evaluatePolicy } from './policy'

/** Exact encrypted length; the shared Rust policy owns the wire overhead and limit. */
export function shieldedCiphertextSize(extrinsicSize: number): number {
  return evaluatePolicy<number>('financial.shield.ciphertext_size', { size: extrinsicSize })
}
export function validateShieldedCiphertextSize(size: number): number {
  return evaluatePolicy<number>('financial.shield.validate_ciphertext', { size })
}
export function maximumShieldedCiphertextSize(): number {
  return evaluatePolicy<{ maximum: number }>('financial.shield.constants', {}).maximum
}
