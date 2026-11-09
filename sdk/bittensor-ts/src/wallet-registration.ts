import { Balance } from './balance'
import type { IntentDescriptor, IntentReader } from './intents'
import { encodeIntentCall } from './intents'
import { descriptorBytes, rawBytes, registrationDescriptor } from './protected-accounts'
import { quoteRegistrationDeposit, registrationDepositGuard } from './registration-deposit'
import { bytesHex as hex } from './portable'
import { isRotatingCrypto, isProtectedCrypto, isPqCrypto, u64, walletType } from './wallet-types'

export type WalletRole = 'coldkey' | 'hotkey'
export interface WalletRegistrationKey {
  readonly ss58Address: string
  readonly publicKey: Uint8Array
  readonly cryptoType: number
  readonly requiresMetadataProof?: boolean
  readonly requiresMetadataHash?: boolean
  readonly genesisHash?: string | null
  authorizationDescriptor?(): Uint8Array
  dummyAuthorization?(): Uint8Array
}
export interface WalletRegistrationQuote {
  readonly payerAccount: string
  readonly accounts: readonly string[]
  readonly roles: Readonly<Partial<Record<WalletRole, string>>>
  readonly deposit: Balance
  readonly fee: Balance
}
export interface WalletRegistrationOptions<Payer = unknown> {
  payer?: Payer
  roles?: readonly WalletRole[]
  maxRegistrationDepositRao?: bigint | number
  onRegistrationSponsorship?: (quote: WalletRegistrationQuote) => boolean | Promise<boolean>
  signal?: AbortSignal
  timeoutMs?: number
}
export interface WalletRegistrationResult {
  readonly roles: Readonly<Partial<Record<WalletRole, string>>>
  readonly accounts: readonly string[]
  readonly finalizedBlockHash: string | null
  readonly result: any | null
  readonly deposit: Balance
}
export class WalletRegistrationError extends Error {
  readonly name = 'WalletRegistrationError'
  constructor(message: string, readonly wallet: unknown, readonly result: any | null, readonly quote: WalletRegistrationQuote | null, options?: ErrorOptions) { super(message, options) }
  get receipt(): any | null { return this.result }
}
export interface WalletRegistrationAdapter {
  publicKey(wallet: any, role: WalletRole): Promise<WalletRegistrationKey>
  descriptorAccount(descriptor: Uint8Array): string
  decodeAddress(address: string): Uint8Array
  finalizedHead(): Promise<string>
  blockHash(): Promise<string>
  reader(block: string): Promise<IntentReader>
  estimateFee(call: IntentDescriptor, payer: WalletRegistrationKey, block: string): Promise<bigint>
  submit(call: IntentDescriptor, payer: any, expected: WalletRegistrationKey, options: WalletRegistrationOptions): Promise<any>
}

/** Public wallet-shaped objects are supported in browsers; no secret key is needed to select targets. */
export async function walletPublicKey(wallet: any, role: WalletRole, normalization?: {
  decodeAddress(address: string): Uint8Array
  ecdsaAccount(publicKey: Uint8Array): Uint8Array
  accountContext?(): Promise<unknown>
}): Promise<WalletRegistrationKey> {
  let key = await (wallet?.[`${role}pub`] ?? wallet?.[role] ?? (wallet?.publicKey != null || wallet?.ss58Address || wallet?.address || wallet?.getAccount ? wallet : undefined))
  if (key && normalization) {
    const account = await key.getAccount?.(await normalization.accountContext?.())
    const address = account?.ss58Address ?? account?.address ?? key.ss58Address ?? key.address
    const cryptoType = account?.cryptoType ?? key.cryptoType ?? 1
    if (typeof address !== 'string') throw new TypeError(`wallet must expose a public ${role} address`)
    const identity = normalization.decodeAddress(address)
    let publicKey = account?.publicKey ?? key.publicKey
    publicKey = publicKey == null ? identity : rawBytes(publicKey)
    if (cryptoType === 2 && publicKey.length === 33) publicKey = normalization.ecdsaAccount(publicKey)
    if (hex(publicKey) !== hex(identity)) throw new Error('signer publicKey does not match signer address')
    key = {
      ss58Address: address, publicKey, cryptoType,
      requiresMetadataProof: Boolean(account?.requiresMetadataProof ?? key.requiresMetadataProof),
      requiresMetadataHash: Boolean(account?.requiresMetadataHash ?? key.requiresMetadataHash),
      genesisHash: account?.genesisHash ?? key.genesisHash ?? null,
      ...(key.authorizationDescriptor && { authorizationDescriptor: key.authorizationDescriptor.bind(key) }),
      ...(key.dummyAuthorization && { dummyAuthorization: key.dummyAuthorization.bind(key) }),
    }
  }
  if (!key || typeof key.ss58Address !== 'string' || !(key.publicKey instanceof Uint8Array) || !Number.isInteger(key.cryptoType)) throw new TypeError(`wallet must expose a public ${role} identity`)
  return key
}

/** Copy identity before calling application hooks that can mutate a wallet. */
export function snapshotWalletIdentity(key: WalletRegistrationKey): WalletRegistrationKey {
  let descriptor: Uint8Array | undefined
  if (isProtectedCrypto(key.cryptoType)) {
    try { descriptor = key.authorizationDescriptor?.().slice() }
    catch (error) { if (isRotatingCrypto(key.cryptoType)) throw error }
  }
  const dummy = key.dummyAuthorization?.().slice()
  return Object.freeze({ ss58Address: key.ss58Address, publicKey: key.publicKey.slice(), cryptoType: key.cryptoType,
    requiresMetadataProof: Boolean(key.requiresMetadataProof), requiresMetadataHash: Boolean(key.requiresMetadataHash), genesisHash: key.genesisHash ?? null,
    ...(descriptor && { authorizationDescriptor: () => descriptor.slice() }),
    ...(dummy && { dummyAuthorization: () => dummy.slice() }),
  })
}

export function requireWalletIdentity(key: WalletRegistrationKey, expected: WalletRegistrationKey, decode: (address: string) => Uint8Array): void {
  if (hex(decode(key.ss58Address)) !== hex(decode(expected.ss58Address)) || hex(key.publicKey) !== hex(expected.publicKey) || key.cryptoType !== expected.cryptoType) throw new Error('unlocked signing identity does not match the public wallet')
  if (expected.authorizationDescriptor && hex(key.authorizationDescriptor!()) !== hex(expected.authorizationDescriptor())) throw new Error('unlocked signing descriptor does not match the public wallet')
  if (Boolean(key.requiresMetadataProof) !== Boolean(expected.requiresMetadataProof) || Boolean(key.requiresMetadataHash) !== Boolean(expected.requiresMetadataHash) || (key.genesisHash ?? null) !== (expected.genesisHash ?? null)) throw new Error('signer chain or metadata requirements changed after approval')
}

/** Keep remote/extension hooks attached to their original object while pinning approval. */
export function bindWalletRegistrationSigner(signer: any, expected: WalletRegistrationKey, read: () => Promise<WalletRegistrationKey>, decode: (address: string) => Uint8Array): any {
  const bound: any = { ...expected, address: expected.ss58Address }
  for (const name of ['sign', 'signBytes', 'signPayload', 'signRaw']) {
    if (typeof signer[name] !== 'function') continue
    bound[name] = async (...args: unknown[]) => {
      requireWalletIdentity(await read(), expected, decode)
      const response = await signer[name](...args)
      requireWalletIdentity(await read(), expected, decode)
      return response
    }
  }
  if (typeof signer.beginTwoStageSigning === 'function') bound.beginTwoStageSigning = () => signer.beginTwoStageSigning()
  return Object.freeze(bound)
}

/** A full inner + maximum-size carrier quote with dummy authorization and maximal nonce width. */
export async function estimateWalletRegistrationFee(client: { runtimeAt(block: string): Promise<any>; blockNumber(block: string): Promise<number>; rpc(method: string, params: unknown[]): Promise<any> }, call: IntentDescriptor, payer: WalletRegistrationKey, block: string, decode: (address: string) => Uint8Array): Promise<bigint> {
  const runtime = await client.runtimeAt(block)
  const current = Math.max(0, await client.blockNumber(block) - 1)
  const proof = payer.dummyAuthorization ? payer.dummyAuthorization() : new Uint8Array(payer.cryptoType === 2 ? 65 : 64)
  let total = 0n
  for (const descriptor of [call, { pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext: new Uint8Array(8192) } }]) {
    const data = await encodeIntentCall(descriptor, async (p, f, a) => runtime.composeCall(p, f, a))
    const encoded = runtime.encodeSignedExtrinsic(data, decode(payer.ss58Address), proof, payer.cryptoType, { era: { period: 8, current }, nonce: 0xffffffffn, tip: 0n, metadataHashEnabled: false })
    const info = await client.rpc('payment_queryInfo', [hex(encoded.bytes), block])
    const fee = info?.partialFee ?? info?.partial_fee
    if (fee == null) throw new Error('wallet registration fee unavailable')
    total += u64(typeof fee === 'string' ? BigInt(fee) : fee)
  }
  return total
}

/** Register rotating keys through required Shield, then verify every descriptor in finalized state. */
export async function registerWallet(adapter: WalletRegistrationAdapter, wallet: unknown, options: WalletRegistrationOptions = {}): Promise<WalletRegistrationResult> {
  let result: any | null = null, quote: WalletRegistrationQuote | null = null
  try {
    options.signal?.throwIfAborted()
    const maximum = options.maxRegistrationDepositRao == null ? undefined : u64(options.maxRegistrationDepositRao)
    const roles: readonly WalletRole[] = options.roles ?? ['coldkey', 'hotkey']
    if (!Array.isArray(roles) || roles.some(role => role !== 'coldkey' && role !== 'hotkey')) throw new TypeError('roles must contain coldkey and/or hotkey')
    const selected: Partial<Record<WalletRole, string>> = {}, descriptors = new Map<string, { account: string; descriptor: Uint8Array }>()
    for (const role of new Set<WalletRole>(roles)) {
      const key = await adapter.publicKey(wallet, role)
      if (!isRotatingCrypto(key.cryptoType)) continue
      const descriptor = key.authorizationDescriptor?.().slice()
      if (!descriptor) throw new Error(`${role} requires its public registration descriptor`)
      const id = hex(adapter.decodeAddress(key.ss58Address))
      if (hex(adapter.decodeAddress(adapter.descriptorAccount(descriptor))) !== id) throw new Error(`${role} descriptor does not match its account`)
      if (descriptors.has(id) && hex(descriptors.get(id)!.descriptor) !== hex(descriptor)) throw new Error('wallet roles have conflicting descriptors')
      selected[role] = key.ss58Address
      descriptors.set(id, { account: key.ss58Address, descriptor })
    }
    const empty = () => ({ roles: Object.freeze({}), accounts: Object.freeze([]), finalizedBlockHash: null, result: null, deposit: Balance.fromRao(0n) })
    if (!descriptors.size) return empty()
    let finalized = await adapter.finalizedHead()
    if (!finalized) throw new Error('could not verify finalized wallet registration')
    const registered = async (account: string, descriptor: Uint8Array, block: string) => {
      const record = await (await adapter.reader(block)).query('HashedAccounts', descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts', [account], block) as any
      if (record == null) return false
      if (hex(descriptorBytes(record.descriptor)) !== hex(descriptor)) throw new Error('registered wallet descriptor does not match the local key')
      return true
    }
    const missing: Array<{ account: string; descriptor: Uint8Array }> = []
    for (const entry of descriptors.values()) if (!await registered(entry.account, entry.descriptor, finalized)) missing.push(entry)
    const accounts = Object.freeze([...descriptors.values()].map(entry => entry.account))
    if (!missing.length) return { roles: Object.freeze(selected), accounts, finalizedBlockHash: finalized, result: null, deposit: Balance.fromRao(0n) }
    if (options.payer == null) throw new Error('hashed wallet registration requires an existing funded payer')
    const payer = snapshotWalletIdentity(await adapter.publicKey(options.payer, 'coldkey')), payerId = hex(adapter.decodeAddress(payer.ss58Address))
    if (missing.some(entry => hex(adapter.decodeAddress(entry.account)) === payerId)) throw new Error('an unregistered wallet cannot pay for its own registration')
    if (isRotatingCrypto(payer.cryptoType) && !await registered(payer.ss58Address, payer.authorizationDescriptor!(), finalized)) throw new Error('registration payer must already be registered in finalized state')
    const head = await adapter.blockHash()
    if (!head) throw new Error('could not pin wallet registration reserve')
    const reader = await adapter.reader(head)
    if (await reader.query('AdminUtils', 'HashedAccountsEnabled', [], head) !== true) throw new Error('hashed wallet registration is disabled')
    for (const { descriptor } of missing) if (descriptor[0] === 2 && !await reader.supportsPqProfile?.(descriptor[1])) throw new Error('wallet registration scheme is unsupported')
    const nextKey = await reader.query('MevShield', 'NextKey', [], head)
    if (nextKey == null || rawBytes(nextKey).length !== 1184) throw new Error('MEV Shield is unavailable; wallet registration was not submitted')
    // The classical registration hook admits one registration per extrinsic.
    const groups = missing.some(entry => entry.descriptor[0] === 1) ? missing.map(entry => [entry]) : [missing]
    let total = 0n, fees = 0n
    const transactions: Array<{ call: IntentDescriptor; group: typeof missing }> = []
    for (const group of groups) {
      const calls: IntentDescriptor[] = []
      let ceiling: bigint | undefined
      for (const { descriptor } of group) {
        const deposit = await quoteRegistrationDeposit(reader, descriptor[0] === 2 ? 'MlDsaRegistrationDeposit' : 'RegistrationDeposit')
        total += deposit.amountRao
        if (deposit.configurable) ceiling = ceiling == null || deposit.amountRao < ceiling ? deposit.amountRao : ceiling
        calls.push({ pallet: 'HashedAccounts', call: 'register', params: { descriptor: registrationDescriptor(descriptor) } })
      }
      if (ceiling != null) calls.unshift(registrationDepositGuard({ amountRao: ceiling, configurable: true }))
      const call: IntentDescriptor = calls.length === 1 ? calls[0] : { pallet: 'Utility', call: 'batch_all', params: { calls } }
      fees += await adapter.estimateFee(call, payer, head)
      transactions.push({ call, group })
    }
    quote = Object.freeze({ payerAccount: payer.ss58Address, accounts: Object.freeze(missing.map(entry => entry.account)), roles: Object.freeze(Object.fromEntries(Object.entries(selected).filter(([, account]) => missing.some(entry => entry.account === account)))), deposit: Balance.fromRao(total), fee: Balance.fromRao(fees) })
    if (maximum != null && total > maximum) throw new Error('wallet registration reserve exceeds the approved maximum')
    if (maximum == null && !options.onRegistrationSponsorship) throw new Error('wallet registration requires maxRegistrationDepositRao or onRegistrationSponsorship approval')
    const account = await reader.query('System', 'Account', [payer.ss58Address], head) as any
    const minimum = BigInt(await reader.constant?.('Balances', 'ExistentialDeposit', head) as any ?? 0)
    const frozen = BigInt(account?.data?.frozen ?? 0)
    if (BigInt(account?.data?.free ?? 0) < total + fees + (frozen > minimum ? frozen : minimum)) throw new Error('registration payer has insufficient spendable balance for reserve and Shield fees')
    if (options.onRegistrationSponsorship && await options.onRegistrationSponsorship(quote) !== true) throw new Error('wallet registration sponsorship was declined')
    for (const { call, group } of transactions) {
      options.signal?.throwIfAborted()
      result = await adapter.submit(call, options.payer, payer, options)
      if (result?.success !== true) throw new Error(result?.message ?? 'wallet registration transaction failed')
      finalized = await adapter.finalizedHead()
      if (!finalized) throw new Error('could not verify finalized wallet registration')
      for (const { account, descriptor } of group) if (!await registered(account, descriptor, finalized)) throw new Error('wallet registration is not present in finalized state')
    }
    for (const { account, descriptor } of descriptors.values()) if (!await registered(account, descriptor, finalized)) throw new Error('wallet registration is not present in finalized state')
    return { roles: Object.freeze(selected), accounts, finalizedBlockHash: finalized, result, deposit: quote.deposit }
  } catch (error) {
    if (error instanceof WalletRegistrationError) throw error
    throw new WalletRegistrationError(`Wallet keys are preserved; registration did not complete: ${error instanceof Error ? error.message : error}`, wallet, result, quote, { cause: error })
  }
}

/** Estimate registration reserves before creating keys; standard/fixed modes make no RPC. */
export async function registrationDepositForTypes(adapter: Pick<WalletRegistrationAdapter, 'blockHash' | 'reader'>, types: readonly number[]): Promise<Balance> {
  types.forEach(walletType)
  const rotating = types.filter(isRotatingCrypto)
  if (!rotating.length) return Balance.fromRao(0n)
  const head = await adapter.blockHash()
  if (!head) throw new Error('could not pin wallet registration reserve')
  const reader = await adapter.reader(head)
  let amount = 0n
  for (const type of rotating) amount += (await quoteRegistrationDeposit(reader, isPqCrypto(type) ? 'MlDsaRegistrationDeposit' : 'RegistrationDeposit')).amountRao
  return Balance.fromRao(amount)
}
