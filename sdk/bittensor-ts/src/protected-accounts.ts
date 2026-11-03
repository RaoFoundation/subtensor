import { bytesHex as hex, hexBytes, concatBytes } from './portable'
import { isPqCrypto, isRotatingCrypto, isProtectedCrypto, pqProfile, u64 } from './wallet-types'
import type { IntentDescriptor } from './intents'

export interface ProtectedKeypair {
  dummyAuthorization(): Uint8Array
  readonly cryptoType: number
  readonly ss58Address: string
  readonly publicKey: Uint8Array
  atGeneration(generation: bigint | number): ProtectedKeypair
  publicOnly(): ProtectedKeypair
  authorizationDescriptor(): Uint8Array
  authorizationGeneration(): bigint
  authorizationCurrentCommitment(): Uint8Array
  authorizationNextCommitment(): Uint8Array
  signAuthorization(payload: Uint8Array): Uint8Array
}
export interface ProtectedAccountClient {
  query(pallet: string, item: string, params?: any[], block?: any): Promise<any>
  finalizedHead(): Promise<string>
  blockHash(number?: number): Promise<string>
  blockNumber(block?: any): Promise<number>
  accountNextIndex(address: string): Promise<number>
  runtimeAt(block?: any): Promise<any>
}
export interface SigningAuthority {
  block: string
  nonce: bigint
  generation: bigint
  descriptor: Uint8Array
  commitment: Uint8Array
  key: ProtectedKeypair
}
export function rawBytes(value: any): Uint8Array {
  if (typeof value === 'string') return hexBytes(value)
  if (value instanceof Uint8Array) return value.slice()
  if (Array.isArray(value)) return Uint8Array.from(value)
  throw new TypeError('expected bytes')
}
export function descriptorBytes(value: any): Uint8Array {
  if (value instanceof Uint8Array || Array.isArray(value) || typeof value === 'string') return rawBytes(value)
  if (!value || !Number.isInteger(value.version)) throw new TypeError('invalid account descriptor')
  const scheme = typeof value.scheme === 'string' ? value.scheme : Object.keys(value.scheme ?? {})[0]
  const tag = value.version === 2 ? value.algorithm : scheme === 'Sr25519' ? 1 : scheme === 'Ed25519' ? 3 : value.scheme
  if (![1, 3, 4, 5, 6, 7, 8, 9].includes(tag)) throw new TypeError('unsupported account descriptor')
  return concatBytes(Uint8Array.of(value.version, tag), rawBytes(value.initial_commitment))
}
export function descriptorValue(descriptor: Uint8Array): Record<string, unknown> {
  if (!descriptor.slice(2).some(byte => byte !== 0)) throw new TypeError('zero account commitment')
  if ((descriptor[0] === 1 && descriptor.length === 34 && [1, 3].includes(descriptor[1])) ||
      (descriptor[0] === 2 && descriptor.length === 66 && [4, 5, 6, 7, 8, 9].includes(descriptor[1]))) {
    return { version: descriptor[0], ...(descriptor[0] === 2 ? { algorithm: descriptor[1] } : { scheme: descriptor[1] === 3 ? 'Ed25519' : 'Sr25519' }), initial_commitment: descriptor.slice(2) }
  }
  throw new TypeError('invalid account descriptor')
}
export async function signingAuthority(client: ProtectedAccountClient, key: ProtectedKeypair, nonce?: number | bigint, signing = true): Promise<SigningAuthority> {
  if (!isProtectedCrypto(key.cryptoType)) throw new TypeError('a protected wallet is required')
  const block = await client.finalizedHead(), descriptor = key.authorizationDescriptor()
  if (await client.query('AdminUtils', 'HashedAccountsEnabled', [], block) !== true) throw new Error('protected account authorization is disabled')
  const runtime = await client.runtimeAt(block)
  if (isPqCrypto(key.cryptoType) && !runtime.supportsMldsaProfile(pqProfile(key.cryptoType))) throw new Error('runtime does not support this post-quantum profile')
  const pending = BigInt(await client.accountNextIndex(key.ss58Address))
  let generation = 0n, selectedNonce = nonce == null ? pending : u64(nonce), active = key, commitment = descriptor.slice(2)
  if (isRotatingCrypto(key.cryptoType)) {
    const record = await client.query('HashedAccounts', descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts', [key.ss58Address], block)
    if (!record) throw new Error('rotating wallet must be registered and finalized before signing')
    if (hex(descriptorBytes(record.descriptor)) !== hex(descriptor)) throw new Error('registered descriptor differs from wallet')
    generation = u64(record.generation)
    const account = await client.query('System', 'Account', [key.ss58Address], block)
    selectedNonce = u64(account.nonce)
    if (pending !== selectedNonce || nonce != null && u64(nonce) !== selectedNonce) throw new Error('rotating wallet requires its finalized nonce with no pending transaction')
    commitment = rawBytes(record.commitment)
    if (signing) {
      active = key.atGeneration(generation)
      if (hex(commitment) !== hex(active.authorizationCurrentCommitment())) throw new Error('registered commitment differs from wallet')
    }
  } else if (selectedNonce >= 0xffffffffn) throw new Error('account nonce exhausted')
  return { block, nonce: selectedNonce, generation, descriptor: descriptor.slice(), commitment, key: active }
}
export async function assertAuthorityCurrent(client: ProtectedAccountClient, authority: SigningAuthority): Promise<void> {
  if (!isRotatingCrypto(authority.key.cryptoType)) return
  if (BigInt(await client.accountNextIndex(authority.key.ss58Address)) !== authority.nonce) throw new Error('wallet nonce changed; prepare again')
  const block = await client.finalizedHead()
  const record = await client.query('HashedAccounts', authority.descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts', [authority.key.ss58Address], block)
  if (!record || u64(record.generation) !== authority.generation || hex(rawBytes(record.commitment)) !== hex(authority.commitment) || hex(descriptorBytes(record.descriptor)) !== hex(authority.descriptor)) throw new Error('wallet authority changed; prepare again')
}
export function registrationCall(key: ProtectedKeypair): IntentDescriptor {
  if (!isRotatingCrypto(key.cryptoType)) throw new Error('fixed wallets do not require registration')
  const descriptor = key.authorizationDescriptor()
  return { pallet: 'HashedAccounts', call: 'register', params: { descriptor: registrationDescriptor(descriptor) } }
}
export function registrationDescriptor(descriptor: Uint8Array): Record<string, unknown> {
  const variant = descriptor[0] === 2 ? [6, 7, 8, 9].includes(descriptor[1]) ? 'Falcon' : 'MlDsa65' : 'Classical'
  return { [variant]: descriptorValue(descriptor) }
}
export function rotationCall(key: ProtectedKeypair): IntentDescriptor {
  if (!isRotatingCrypto(key.cryptoType)) throw new Error('fixed wallets do not rotate')
  // Rotation occurs during authorization of an included transaction, including a remark.
  return { pallet: 'System', call: 'remark', params: { remark: new Uint8Array() } }
}
