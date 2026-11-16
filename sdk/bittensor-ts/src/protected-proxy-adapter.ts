import { encodeIntentCall, type Intent, type IntentReader } from './intents'
import { descriptorBytes } from './protected-accounts'
import { bytesHex, hexBytes, unsignedInteger } from './portable'
import { eventName, eventExtrinsicIndex, nestedDispatchError } from './receipt'
import type { ProtectedProxyBackend, ProtectedProxyEvent, ProtectedProxyReceipt, ProtectedProxySnapshot, ProtectedProxySubmission } from './protected-proxy-types'

export interface ProtectedProxyPublicKey {
  readonly ss58Address: string
  readonly cryptoType: number
  dummyAuthorization?(): Uint8Array
}

export interface ProtectedProxyCrypto {
  accountId(address: string): Uint8Array
  ss58Encode(account: Uint8Array): string
  hash(data: Uint8Array): Uint8Array
  fromDescriptor(data: Uint8Array): ProtectedProxyPublicKey
}

export interface ProtectedProxyClient {
  genesisHash(): Promise<string>
  finalizedHead?(): Promise<string>
  blockHash(height?: number): Promise<string>
  blockNumber(hash?: string): Promise<number>
  blockTime(hash?: string): Promise<number>
  runtimeAt(hash?: string): Promise<any>
  query(pallet: string, item: string, params: any[], block: string): Promise<any>
  queryMap(pallet: string, item: string, params: any[], block: string): Promise<Array<[any, any]>>
  rpc(method: string, params: any[]): Promise<any>
  findExtrinsic?(hash: string, block: string, finalized?: boolean): Promise<any>
}

export interface ProtectedProxyAdapterOptions {
  reader(block: string): Promise<IntentReader>
  submit(intent: Intent, options: ProtectedProxySubmission): Promise<unknown>
  crypto: ProtectedProxyCrypto
  /** Public information only. This callback must not unlock a wallet or reserve a nonce. */
  publicKeyFor?(account: string): ProtectedProxyPublicKey | undefined | Promise<ProtectedProxyPublicKey | undefined>
  /** Optional inclusion-block lookup for older uncertain submissions. */
  receiptBlockFor?(extrinsicHash: string): string | null | undefined | Promise<string | null | undefined>
  /** Bounded finalized-history search when an uncertain submission has only a hash. Default: 128. */
  receiptSearchBlocks?: number
}

function blockHash(value: unknown): string {
  if (typeof value !== 'string' || !/^0x[0-9a-f]{64}$/i.test(value)) throw new TypeError('invalid historical block hash')
  return value.toLowerCase()
}
function executionHeight(header: any, expected?: number): number {
  const raw = header?.number
  if (!(typeof raw === 'number' || typeof raw === 'string' && /^(?:0x[0-9a-f]+|[0-9]+)$/i.test(raw))) throw new TypeError('block execution metadata unavailable')
  const height = Number(raw)
  if (!Number.isSafeInteger(height) || height < 0 || expected != null && expected !== height) throw new TypeError('block execution metadata unavailable')
  if (height > 0) blockHash(header.parentHash)
  return height
}

export function protectedProxyEvent(value: any): ProtectedProxyEvent {
  const [module_id, event_id] = eventName(value).split('.')
  if (!module_id || !event_id) throw new TypeError('invalid protected-account receipt event')
  return { module_id, event_id, attributes: value.attributes ?? value.event?.attributes ?? value.event?.data ?? value.data ?? {}, extrinsic_idx: eventExtrinsicIndex(value) }
}

export function protectedProxyReceipt(value: any): ProtectedProxyReceipt {
  if (!value || typeof value !== 'object') throw new TypeError('submission did not return a receipt')
  const events = (value.events ?? []).map(protectedProxyEvent)
  const nested = nestedDispatchError(events)
  const approved = events.some((event: ProtectedProxyEvent) => event.module_id === 'Multisig' && ['NewMultisig', 'MultisigApproval'].includes(event.event_id))
  const executed = events.some((event: ProtectedProxyEvent) => event.module_id === 'Multisig' && event.event_id === 'MultisigExecuted')
  return {
    status: String(value.status ?? 'unknown'), success: nested != null ? false : typeof value.success === 'boolean' ? value.success : null,
    blockHash: value.blockHash ?? null, blockNumber: value.blockNumber ?? null, extrinsicHash: value.extrinsicHash ?? null,
    extrinsicIndex: value.extrinsicIndex ?? null, events, message: nested == null ? value.message : 'nested dispatch failed',
    awaitingApproval: Boolean(value.awaitingApproval || approved && !executed),
  }
}

/** Adapt the existing chain clients without introducing a second signing or nonce path. */
export function createProtectedProxyBackend(client: ProtectedProxyClient, options: ProtectedProxyAdapterOptions): ProtectedProxyBackend {
  const crypto = options.crypto
  const searchBlocks = options.receiptSearchBlocks ?? 128
  if (!Number.isSafeInteger(searchBlocks) || searchBlocks < 0 || searchBlocks > 4096) throw new RangeError('receiptSearchBlocks must be between 0 and 4096')
  const snapshot = async (): Promise<ProtectedProxySnapshot> => {
    const hash = await client.blockHash()
    const [genesisHash, block, blockTime, runtime, reader] = await Promise.all([
      client.genesisHash(), client.blockNumber(hash), client.blockTime(hash), client.runtimeAt(hash), options.reader(hash),
    ])
    const compose = (call: Parameters<ProtectedProxySnapshot['compose']>[0]) => encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
    const query = (pallet: string, item: string, params: unknown[] = []) => client.query(pallet, item, params, hash)
    const hasStorage = (pallet: string, item: string) => runtime.metadataIr().pallets.find((entry: any) => entry.name === pallet)?.storage.includes(item) ?? false
    return Object.freeze({
      genesisHash, hash, block, blockTime, specVersion: runtime.specVersion,
      query, queryMap: (pallet: string, item: string, params: unknown[] = []) => client.queryMap(pallet, item, params, hash),
      constant: async (pallet: string, item: string) => runtime.constant(pallet, item),
      prepare: (intent: Intent, context: Parameters<ProtectedProxySnapshot['prepare']>[1]) => intent.prepare(reader, context),
      compose, decodeCall: (data: Uint8Array) => runtime.decodeCall(data), hashCallData: crypto.hash,
      fee: async (call: Parameters<ProtectedProxySnapshot['fee']>[0], payer: string) => {
        let publicKey: ProtectedProxyPublicKey | undefined
        for (const item of ['MlDsaAccounts', 'Accounts']) {
          if (!hasStorage('HashedAccounts', item)) continue
          const record = await query('HashedAccounts', item, [payer])
          if (record != null) { publicKey = crypto.fromDescriptor(descriptorBytes(record.descriptor)); break }
        }
        publicKey ??= await options.publicKeyFor?.(payer)
        if (publicKey && bytesHex(crypto.accountId(publicKey.ss58Address)) !== bytesHex(crypto.accountId(payer))) throw new Error('fee quote descriptor does not match controller')
        const cryptoType = publicKey?.cryptoType ?? 1
        if (cryptoType > 3 && !publicKey?.dummyAuthorization) throw new Error('protected fee quote requires a public authorization descriptor')
        const signature = publicKey?.dummyAuthorization?.() ?? new Uint8Array(cryptoType === 2 ? 65 : 64)
        const account = await query('System', 'Account', [payer])
        const nonce = unsignedInteger(account?.nonce ?? 0, 32, 'controller nonce')
        const signed = runtime.encodeSignedExtrinsic(await compose(call), crypto.accountId(payer), signature, cryptoType, {
          era: { period: 64, current: block }, nonce, tip: 0n, metadataHashEnabled: true,
        })
        const info = await client.rpc('payment_queryInfo', [bytesHex(signed.bytes), hash])
        const fee = info?.partialFee ?? info?.partial_fee
        if (fee == null) throw new Error('controller fee quote unavailable')
        return unsignedInteger(fee, 128, 'controller fee')
      },
    })
  }
  return {
    snapshot, accountId: crypto.accountId, ss58Encode: crypto.ss58Encode, hash: crypto.hash,
    submit: async (intent, request) => protectedProxyReceipt(await options.submit(intent, request)),
    blockHash: height => client.blockHash(height),
    extrinsicCall: async (block, index) => {
      if (!Number.isSafeInteger(index) || index < 0) throw new TypeError('invalid extrinsic index')
      const hash = blockHash(typeof block === 'number' ? await client.blockHash(block) : block)
      const value = await client.rpc('chain_getBlock', [hash])
      const header = value?.block?.header, encoded = value?.block?.extrinsics?.[index]
      if (encoded == null) return null
      const height = executionHeight(header, typeof block === 'number' ? block : undefined)
      const runtime = await client.runtimeAt(height > 0 ? header.parentHash : hash)
      return runtime.decodeExtrinsic(hexBytes(encoded)).call ?? null
    },
    events: async block => {
      const hash = blockHash(typeof block === 'number' ? await client.blockHash(block) : block)
      const header = await client.rpc('chain_getHeader', [hash])
      const height = executionHeight(header, typeof block === 'number' ? block : undefined)
      const runtime = await client.runtimeAt(height > 0 ? header.parentHash : hash)
      const entry = runtime.storageEntry('System', 'Events')
      const raw = await client.rpc('state_getStorage', [bytesHex(runtime.storageKey('System', 'Events', [])), hash])
      const rows = runtime.decode(entry.valueType, raw == null ? entry.defaultBytes : hexBytes(raw), false)
      if (!Array.isArray(rows)) throw new TypeError('block events unavailable')
      return rows.map(protectedProxyEvent)
    },
    receipt: async (hash, block) => {
      if (!client.findExtrinsic) return null
      const transaction = blockHash(hash), hint = block ?? await options.receiptBlockFor?.(transaction)
      if (hint) {
        const receipt = await client.findExtrinsic(transaction, blockHash(hint))
        return receipt == null ? null : protectedProxyReceipt(receipt)
      }
      if (searchBlocks === 0) return null
      let cursor = blockHash(await (client.finalizedHead ? client.finalizedHead() : client.blockHash())), previousHeight: number | undefined
      for (let checked = 0; checked < searchBlocks; checked++) {
        const found = await client.rpc('chain_getBlock', [cursor])
        if (found?.block == null) return null
        const height = executionHeight(found.block.header, previousHeight == null ? undefined : previousHeight - 1)
        if (!Array.isArray(found.block.extrinsics)) throw new TypeError('historical extrinsics unavailable')
        if (found.block.extrinsics.some((bytes: string) => bytesHex(crypto.hash(hexBytes(bytes))) === transaction)) {
          const receipt = await client.findExtrinsic(transaction, cursor, client.finalizedHead != null)
          return receipt == null ? null : protectedProxyReceipt(receipt)
        }
        if (height === 0) break
        previousHeight = height; cursor = blockHash(found.block.header.parentHash)
      }
      return null
    },
  }
}
