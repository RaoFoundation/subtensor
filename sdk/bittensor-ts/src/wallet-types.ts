/** Runtime wallet identifiers. Fixed PQ keys use ordinary SS58 receiving addresses. */
export const CRYPTO_HASHED = 4
export const CRYPTO_HASHED_ED25519 = 6
export const CRYPTO_MLDSA65 = 7
export const CRYPTO_MLDSA65_FIXED = 8
export const CRYPTO_FALCON512 = 9
export const CRYPTO_FALCON512_HASHED = 10
export const CRYPTO_FALCON1024 = 11
export const CRYPTO_FALCON1024_HASHED = 12
export const ROLE_TRANSACTION = 0
export const WALLET_TYPES = Object.freeze({
  ed25519: 0, sr25519: 1, 'hashed-sr25519': 4, 'hashed-ed25519': 6,
  'hashed-mldsa65': 7, mldsa65: 8, falcon512: 9,
  'hashed-falcon512': 10, falcon1024: 11, 'hashed-falcon1024': 12,
})
export type WalletType = keyof typeof WALLET_TYPES
export type SigningScheme = 'Ed25519' | 'Sr25519' | 'MlDsa65' | 'Falcon512' | 'Falcon1024'
export function walletType(cryptoType: number): WalletType {
  const match = Object.entries(WALLET_TYPES).find(([, value]) => value === cryptoType)
  if (!match) throw new RangeError(`unsupported crypto type ${cryptoType}`)
  return match[0] as WalletType
}
export function walletCryptoType(type: WalletType): number {
  const value = WALLET_TYPES[type]
  if (value == null) throw new RangeError(`unsupported wallet type ${type}`)
  return value
}
export function walletScheme(type: number): SigningScheme {
  const name = walletType(type).replace('hashed-', '')
  const schemes = { ed25519: 'Ed25519', sr25519: 'Sr25519', mldsa65: 'MlDsa65', falcon512: 'Falcon512', falcon1024: 'Falcon1024' } as const
  return schemes[name as keyof typeof schemes]
}
export function isRotatingCrypto(type: number): boolean { return [4, 6, 7, 10, 12].includes(type) }
export function isPqCrypto(type: number): boolean { return [7, 8, 9, 10, 11, 12].includes(type) }
export function isFalconCrypto(type: number): boolean { return [9, 10, 11, 12].includes(type) }
export function isMldsaCrypto(type: number): boolean { return [7, 8].includes(type) }
export function isHashedCrypto(type: number): boolean { return [4, 6].includes(type) }
export function isFixedPqCrypto(type: number): boolean { return [8, 9, 11].includes(type) }
export function accountCryptoType(type: number, mode?: 'standard' | 'hashed'): number {
  walletType(type)
  if (mode == null) return type
  if (mode !== 'standard' && mode !== 'hashed') throw new RangeError('invalid account type')
  const name = walletType(type).replace('hashed-', '')
  return walletCryptoType((mode === 'hashed' ? `hashed-${name}` : name) as WalletType)
}
export function isProtectedCrypto(type: number): boolean { return isRotatingCrypto(type) || isPqCrypto(type) }
export function pqProfile(type: number): number | undefined { return ({ 7: 4, 8: 5, 9: 7, 10: 6, 11: 9, 12: 8 } as Record<number, number>)[type] }
export function u64(value: bigint | number): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('expected an exact integer')
  const result = BigInt(value)
  if (result < 0n || result > 0xffffffffffffffffn) throw new RangeError('expected u64')
  return result
}
