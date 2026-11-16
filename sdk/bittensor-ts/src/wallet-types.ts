import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'

export type WalletType = 'ed25519' | 'sr25519' | 'hashed-sr25519' | 'hashed-ed25519' | 'hashed-mldsa65' | 'mldsa65' | 'falcon512' | 'hashed-falcon512' | 'falcon1024' | 'hashed-falcon1024'
export type SigningScheme = 'Ed25519' | 'Sr25519' | 'MlDsa65' | 'Falcon512' | 'Falcon1024'
interface AccountProfile { crypto_type: number; name: WalletType; scheme: SigningScheme; rotating: boolean; pq: boolean; falcon: boolean; mldsa: boolean; hashed: boolean; fixed_pq: boolean; protected: boolean; profile: number | null }
const profiles = POLICY_CATALOG.transaction.profiles as readonly AccountProfile[]
const byCode = new Map(profiles.map(profile => [profile.crypto_type, profile]))
/** Generated from the portable core; safe to inspect before browser initialization. */
export const WALLET_TYPES = Object.freeze(Object.fromEntries(profiles.map(profile => [profile.name, profile.crypto_type]))) as Readonly<Record<WalletType, number>>
export const CRYPTO_HASHED = WALLET_TYPES['hashed-sr25519']
export const CRYPTO_HASHED_ED25519 = WALLET_TYPES['hashed-ed25519']
export const CRYPTO_MLDSA65 = WALLET_TYPES['hashed-mldsa65']
export const CRYPTO_MLDSA65_FIXED = WALLET_TYPES.mldsa65
export const CRYPTO_FALCON512 = WALLET_TYPES.falcon512
export const CRYPTO_FALCON512_HASHED = WALLET_TYPES['hashed-falcon512']
export const CRYPTO_FALCON1024 = WALLET_TYPES.falcon1024
export const CRYPTO_FALCON1024_HASHED = WALLET_TYPES['hashed-falcon1024']
export const ROLE_TRANSACTION = 0
export function walletType(type: number): WalletType {
  const profile = byCode.get(type)
  if (!profile) throw new RangeError(`unsupported crypto type ${type}`)
  return profile.name
}
export function walletCryptoType(type: WalletType): number {
  const value = WALLET_TYPES[type]
  if (value == null) throw new RangeError(`unsupported wallet type ${type}`)
  return value
}
export function walletScheme(type: number): SigningScheme { walletType(type); return byCode.get(type)!.scheme }
export function isRotatingCrypto(type: number): boolean { return byCode.get(type)?.rotating ?? false }
export function isPqCrypto(type: number): boolean { return byCode.get(type)?.pq ?? false }
export function isFalconCrypto(type: number): boolean { return byCode.get(type)?.falcon ?? false }
export function isMldsaCrypto(type: number): boolean { return byCode.get(type)?.mldsa ?? false }
export function isHashedCrypto(type: number): boolean { return byCode.get(type)?.hashed ?? false }
export function isFixedPqCrypto(type: number): boolean { return byCode.get(type)?.fixed_pq ?? false }
export function isProtectedCrypto(type: number): boolean { return byCode.get(type)?.protected ?? false }
export function pqProfile(type: number): number | undefined { return byCode.get(type)?.profile ?? undefined }
export function accountCryptoType(type: number, mode?: 'standard' | 'hashed'): number { return evaluatePolicy('transaction.account_type', { crypto_type: type, mode }) }
export function u64(value: bigint | number): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('expected an exact integer')
  const result = BigInt(value)
  if (result < 0n || result > 0xffffffffffffffffn) throw new RangeError('expected u64')
  return result
}
