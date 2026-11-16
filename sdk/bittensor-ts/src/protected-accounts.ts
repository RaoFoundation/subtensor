import { bytesHex as hex, hexBytes } from './portable'
import { evaluatePolicy } from './policy'
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
  return hexBytes(evaluatePolicy<string>('transaction.descriptor_encode', { value: { ...value, initial_commitment: rawBytes(value?.initial_commitment) } }))
}
export function descriptorValue(descriptor: Uint8Array): Record<string, unknown> {
  const { value } = evaluatePolicy<{ value: Record<string, unknown> }>('transaction.descriptor', { descriptor })
  return { ...value, initial_commitment: hexBytes(value.initial_commitment as string) }
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
    const reason = evaluatePolicy<string | null>('transaction.authority_snapshot', { descriptor: descriptorBytes(record.descriptor), expected_descriptor: descriptor, generation: record.generation, commitment: rawBytes(record.commitment) })
    if (reason === 'descriptor') throw new Error('registered descriptor differs from wallet')
    if (reason === 'generation') throw new Error('wallet generation exhausted')
    if (reason === 'commitment') throw new Error('invalid registered commitment')
    generation = u64(record.generation)
    const account = await client.query('System', 'Account', [key.ss58Address], block)
    selectedNonce = u64(account.nonce)
    if (evaluatePolicy('transaction.authority_nonce', { pending, finalized: selectedNonce, requested: nonce == null ? null : u64(nonce) })) throw new Error('rotating wallet requires its finalized nonce with no pending transaction')
    commitment = rawBytes(record.commitment)
    if (signing) {
      active = key.atGeneration(generation)
      if (hex(commitment) !== hex(active.authorizationCurrentCommitment())) throw new Error('registered commitment differs from wallet')
    }
  } else evaluatePolicy('transaction.signing_bounds', { nonce: selectedNonce, period: null })
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
  const { variant } = evaluatePolicy<{ variant: string }>('transaction.descriptor', { descriptor })
  return { [variant]: descriptorValue(descriptor) }
}
export function rotationCall(key: ProtectedKeypair): IntentDescriptor {
  if (!isRotatingCrypto(key.cryptoType)) throw new Error('fixed wallets do not rotate')
  // Rotation occurs during authorization of an included transaction, including a remark.
  return { pallet: 'System', call: 'remark', params: { remark: new Uint8Array() } }
}
