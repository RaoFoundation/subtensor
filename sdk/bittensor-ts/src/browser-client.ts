import { serializeUnsignedTransaction, restoreUnsignedTransaction, type UnsignedTransactionState, type UnsignedRuntime, type SerializedUnsignedTransaction } from './unsigned-transaction'
import { chainErrorFromDispatch, type ChainErrorInfo } from './chain-errors'
import { followShieldedResult } from './shielded-receipt'
import { prepareMultisigApproval, type MultisigApprovalRequest } from './multisig-workflow'
import { UpgradeClient, type UpgradeAdapter } from './upgrade'
import { bindReadNamespace, type AccountsReadNamespace, type SubnetsReadNamespace, type StakingReadNamespace, type NeuronsReadNamespace, type ChainReadNamespace, type EpochsReadNamespace, type HyperparametersReadNamespace, type DelegationReadNamespace, type LeasingReadNamespace, type CollateralReadNamespace, type LocksReadNamespace, type IdentityReadNamespace, type WeightsReadNamespace, type PricesReadNamespace } from './read-namespaces'
import {
  Runtime, initBrowser, publicKeyFromSs58, ss58FromPublic, verifySignature,
  blake2_256, storagePrefixFor, decodeOptionalOpaqueMetadata, eraBirth, metadataDigest,
  generateExtrinsicProof, generateCommitV2, multisigAccountId, sealMevShieldTransaction,
  type BrowserTransactionParams,
} from './browser'
import { Balance, type BalanceLike } from './balance'
import { JsonRpcTransport, ChainError, EndpointValidationError, withRequestSignal, type JsonRpcTransportOptions, type RpcRequestOptions } from './transport'
import { bytesHex as hex, hexBytes, concatBytes } from './portable'
import { dispatchRead, listReads, type ReadName, type ReadArgs, type ReadParams, type ReadResult, type ReadView } from './reads'
import { fetchMetagraph } from './metagraph'
import { Intent, intentFromJSON, intentSignerRole, listIntentTools, checkIntentPolicy, encodeIntentCall, PROXY_TYPES, type IntentContext, type IntentDescriptor, type IntentPolicy, type IntentReader, type PreparedIntent } from './intents'
import { iterCallLeaves } from './fee-filters'
import { resolveTransactionFeePayer } from './fee-payer'
import { createEvmAddressUtilities } from './evm'
import { normalizeHeader, normalizeStatus, nestedDispatchError, eventName, eventExtrinsicIndex, feeFromEvent, dispatchFailureMessage, type BlockHeader } from './receipt'
import type { UnsignedTransaction } from './external-signers'
import type { ScaleValue } from './types'

export const NETWORKS = Object.freeze({ finney: 'wss://entrypoint-finney.opentensor.ai:443', test: 'wss://test.finney.opentensor.ai:443', archive: 'wss://archive.chain.opentensor.ai:443', devnet: 'wss://dev.chain.opentensor.ai:443', local: 'ws://127.0.0.1:9944' })
export const FALLBACK_ENDPOINTS: Readonly<Record<string, readonly string[]>> = Object.freeze({ finney: ['wss://lite.chain.opentensor.ai:443', 'wss://lite.sub.latent.to:443'], archive: ['wss://archive.sub.latent.to:443'] })
export const ARCHIVE_ENDPOINTS: Readonly<Record<string, readonly string[]>> = Object.freeze({ finney: ['wss://archive.chain.opentensor.ai:443', 'wss://archive.sub.latent.to:443'], archive: ['wss://archive.sub.latent.to:443'] })
export const DEFAULT_ERA_PERIOD = 128
export const MEV_SHIELD_ERA_PERIOD = 8
const FINNEY_GENESIS = '0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03'
export type Descriptor = readonly [string, string]
export type BlockRef = number | string | null | undefined
export interface ClientOptions extends JsonRpcTransportOptions {
  endpoint?: string; fallbackEndpoints?: string[]; archiveEndpoints?: string[]; expectedGenesisHash?: string; retryForever?: boolean
  headRuntimeTtlMs?: number; historicalRuntimeCacheSize?: number
}
export interface BrowserSigner {
  readonly ss58Address?: string; readonly address?: string; readonly publicKey?: Uint8Array; readonly cryptoType?: number
  readonly genesisHash?: string | null; readonly requiresMetadataHash?: boolean; readonly requiresMetadataProof?: boolean
  sign?(payload: Uint8Array, context?: any): unknown | Promise<unknown>
  signBytes?(payload: Uint8Array, context?: any): unknown | Promise<unknown>
  signPayload?(payload: any, context?: any): unknown | Promise<unknown>
  signRaw?(request: { address: string; data: string; type: 'bytes' }): unknown | Promise<unknown>
}
export interface SubmitOptions {
  nonce?: number; period?: number | null; tip?: bigint; tipAssetId?: bigint | null; metadataHash?: Uint8Array
  allowRawCall?: boolean; policy?: IntentPolicy; waitForInclusion?: boolean; waitForFinalization?: boolean
  timeoutMs?: number; signal?: AbortSignal; mevShield?: boolean; expectedCallHash?: string
}
export interface WorkflowOptions extends SubmitOptions { proxyFor?: string; proxyType?: string | null; context?: IntentContext }
export interface ExtrinsicResult {
  status: 'submitted' | 'inBlock' | 'finalized' | 'failed' | 'unknown'; success?: boolean; message: string
  extrinsicHash: string; blockHash?: string; blockNumber?: number; extrinsicIndex?: number; finalized?: boolean
  events: unknown[]; fee?: Balance; error?: unknown; chainError?: ChainErrorInfo; shielded?: boolean; innerExtrinsicHash?: string; carrierExtrinsicHash?: string; carrierFee?: Balance
}
export interface PreparedTransaction extends UnsignedTransaction {
  runtime: Runtime; txParams: BrowserTransactionParams; nonce: number; blockHash: string; blockNumber: number
}
type RuntimeEntry = { runtime: Runtime; metadata: Uint8Array; version: any; properties: any }
type Call = IntentDescriptor | Uint8Array

/** Browser chain I/O backed by the same Rust SCALE and crypto implementation as Node. */
export class Client {
  readonly balances: AccountsReadNamespace = bindReadNamespace({}, 'Accounts & keys', block => this.at(block)) as AccountsReadNamespace
  readonly subnets: SubnetsReadNamespace = bindReadNamespace({}, 'Subnets', block => this.at(block)) as SubnetsReadNamespace
  readonly staking: StakingReadNamespace = bindReadNamespace({}, 'Staking', block => this.at(block)) as StakingReadNamespace
  readonly neurons: NeuronsReadNamespace = bindReadNamespace({}, 'Neurons & registration', block => this.at(block)) as NeuronsReadNamespace
  readonly chain: ChainReadNamespace = bindReadNamespace({}, 'Chain', block => this.at(block)) as ChainReadNamespace
  readonly epochs: EpochsReadNamespace = bindReadNamespace({}, 'Epochs & timing', block => this.at(block)) as EpochsReadNamespace
  readonly hyperparameters: HyperparametersReadNamespace = bindReadNamespace({}, 'Hyperparameters', block => this.at(block)) as HyperparametersReadNamespace
  readonly delegation: DelegationReadNamespace = bindReadNamespace({}, 'Delegation', block => this.at(block)) as DelegationReadNamespace
  readonly leasing: LeasingReadNamespace = bindReadNamespace({}, 'Leases & crowdloans', block => this.at(block)) as LeasingReadNamespace
  readonly collateral: CollateralReadNamespace = bindReadNamespace({}, 'Mining & collateral', block => this.at(block)) as CollateralReadNamespace
  readonly locks: LocksReadNamespace = bindReadNamespace({}, 'Locks & conviction', block => this.at(block)) as LocksReadNamespace
  readonly identity: IdentityReadNamespace = bindReadNamespace({}, 'Identity & commitments', block => this.at(block)) as IdentityReadNamespace
  readonly weights: WeightsReadNamespace = bindReadNamespace({}, 'Weights & bonds', block => this.at(block)) as WeightsReadNamespace
  readonly prices: PricesReadNamespace = bindReadNamespace({}, 'Prices & swaps', block => this.at(block)) as PricesReadNamespace
  readonly transport: JsonRpcTransport
  readonly network: string
  private genesis?: string
  private archiveTransport?: JsonRpcTransport
  private readonly archiveEndpoints: string[]
  private runtimeCache = new Map<string, RuntimeEntry>()
  private transactionQueues = new Map<string, Promise<unknown>>()
  private nonceFloors = new Map<string, number>()
  private importedTransactions = new WeakMap<PreparedTransaction, SerializedUnsignedTransaction>()
  constructor(network = 'finney', readonly options: ClientOptions = {}) {
    const target = options.endpoint ?? network
    this.network = Object.entries(NETWORKS).find(([, endpoint]) => endpoint === target)?.[0] ?? target
    const label = this.network
    this.archiveEndpoints = options.archiveEndpoints ?? [...ARCHIVE_ENDPOINTS[label ?? ''] ?? []]
    const endpoint = NETWORKS[label as keyof typeof NETWORKS] ?? target
    if (!/^wss?:\/\//.test(endpoint) && !/^https?:\/\//.test(endpoint)) throw new TypeError(`unknown network ${network}`)
    const expected = options.expectedGenesisHash ?? (['finney', 'archive'].includes(label ?? '') ? FINNEY_GENESIS : undefined)
    if (expected != null && !/^0x[0-9a-fA-F]{64}$/.test(expected)) throw new TypeError('expectedGenesisHash must be a 32-byte hex string')
    const fallbacks = options.fallbackEndpoints ?? [...FALLBACK_ENDPOINTS[label] ?? []]
    if (fallbacks.length && expected == null) throw new ChainError('fallbackEndpoints require expectedGenesisHash for custom or untrusted networks')
    this.transport = new JsonRpcTransport(endpoint, fallbacks, options.retryForever, {
      ...options,
      validateEndpoint: async (url, request) => {
        const actual = String(await request('chain_getBlockHash', [0])).toLowerCase()
        if (!/^0x[0-9a-f]{64}$/.test(actual)) throw new EndpointValidationError('invalid genesis hash')
        if ((expected ?? this.genesis)?.toLowerCase() !== actual && (expected ?? this.genesis) != null) throw new EndpointValidationError(`endpoint ${url} belongs to another chain`)
        this.genesis = actual
        await options.validateEndpoint?.(url, request)
      },
    })
  }
  get endpoint(): string { return this.transport.endpoint }
  async connect(): Promise<this> { await initBrowser(); await this.runtimeAt(); return this }
  async close(): Promise<void> { this.transport.close(); this.archiveTransport?.close() }
  async rpc<T = any>(method: string, params: unknown[] = [], options?: RpcRequestOptions): Promise<T> {
    try { return await this.transport.request(method, params, options) as T }
    catch (error) {
      // Only replay historical reads, never a submission, against the archive pool.
      if (!this.archiveEndpoints.length || !/^(?:state_(?:call|getStorage|queryStorageAt|getKeysPaged|getMetadata|getRuntimeVersion)|chain_get(?:Block|Header))$/.test(method) || !/state.*(?:discarded|pruned)|pruned.*state|unknown block|state unavailable/i.test(String(error))) throw error
      const expected = await this.genesisHash()
      this.archiveTransport ??= new JsonRpcTransport(this.archiveEndpoints[0], this.archiveEndpoints.slice(1), this.options.retryForever, {
        ...this.options, validateEndpoint: async (url, request) => {
          if (String(await request('chain_getBlockHash', [0])).toLowerCase() !== expected.toLowerCase()) throw new EndpointValidationError(`archive endpoint ${url} belongs to another chain`)
          await this.options.validateEndpoint?.(url, request)
        },
      })
      return await this.archiveTransport.request(method, params, options) as T
    }
  }
  async blockHash(block?: number | null): Promise<string> { return this.rpc('chain_getBlockHash', block == null ? [] : [block]) }
  async blockNumber(hash?: string): Promise<number> { return normalizeHeader(await this.rpc('chain_getHeader', hash ? [hash] : [])).number }
  block(): Promise<number> { return this.blockNumber() }
  finalizedHead(): Promise<string> { return this.rpc('chain_getFinalizedHead') }
  async genesisHash(): Promise<string> { return this.genesis ??= await this.blockHash(0) }
  async resolveBlockHash(block?: BlockRef): Promise<string> { return typeof block === 'string' ? block : block == null ? this.finalizedHead() : this.blockHash(block) }
  invalidateRuntimeCache(): void { this.runtimeCache.clear() }
  private async runtimeEntry(block?: BlockRef): Promise<RuntimeEntry> {
    await initBrowser()
    const hash = await this.resolveBlockHash(block)
    const cached = this.runtimeCache.get(hash)
    if (cached) return cached
    const [version, properties] = await Promise.all([this.rpc('state_getRuntimeVersion', [hash]), this.rpc('system_properties')])
    let metadata: Uint8Array | null = null
    try { metadata = decodeOptionalOpaqueMetadata(hexBytes(await this.rpc('state_call', ['Metadata_metadata_at_version', '0x0f000000', hash]))) }
    catch (error) { if (!/Metadata_metadata_at_version.*(?:not found|not available|unknown function)|method not found/i.test(`${error} ${(error as { data?: unknown })?.data ?? ''}`)) throw error }
    metadata ??= hexBytes(await this.rpc('state_getMetadata', [hash]))
    const ss58 = Number(first(properties.ss58Format ?? properties.ss58Prefix) ?? 42)
    const entry = { runtime: new Runtime(metadata, version.specVersion, version.transactionVersion, ss58), metadata, version, properties }
    this.runtimeCache.set(hash, entry)
    const limit = this.options.historicalRuntimeCacheSize ?? 64
    while (this.runtimeCache.size > Math.max(0, limit)) this.runtimeCache.delete(this.runtimeCache.keys().next().value!)
    return entry
  }
  async currentRuntime(): Promise<Runtime> { return this.runtimeAt(await this.blockHash()) }
  async runtimeAt(block?: BlockRef): Promise<Runtime> { return (await this.runtimeEntry(block)).runtime }
  async query(item: Descriptor, params?: ScaleValue[], block?: BlockRef): Promise<any>
  async query(pallet: string, item: string, params?: ScaleValue[], block?: BlockRef): Promise<any>
  async query(pallet: string | Descriptor, item: string | ScaleValue[] = [], params?: ScaleValue[] | BlockRef, block?: BlockRef): Promise<any> {
    const [module, name, args, at] = storageArgs(pallet, item, params, block)
    const hash = await this.resolveBlockHash(at), runtime = await this.runtimeAt(hash)
    if (module === 'System' && name === 'Events') return (await this.eventsAt(hash)).events
    return decodeStorage(runtime, module, name, await this.rpc('state_getStorage', [hex(runtime.storageKey(module, name, args)), hash]))
  }
  async queryBatch(item: Descriptor, sets: ScaleValue[][], block?: BlockRef): Promise<any[]>
  async queryBatch(pallet: string, item: string, sets: ScaleValue[][], block?: BlockRef): Promise<any[]>
  async queryBatch(pallet: string | Descriptor, item: string | ScaleValue[][], sets?: ScaleValue[][] | BlockRef, block?: BlockRef): Promise<any[]> {
    const [module, name] = typeof pallet === 'string' ? [pallet, item as string] : pallet
    const params = (typeof pallet === 'string' ? sets : item) as ScaleValue[][]
    if (!params.length) return []
    const hash = await this.resolveBlockHash(typeof pallet === 'string' ? block : sets as BlockRef), runtime = await this.runtimeAt(hash)
    if (module === 'System' && name === 'Events') {
      const { events } = await this.eventsAt(hash)
      return params.map(() => events)
    }
    const keys = runtime.storageKeyBatch(module, name, params).map(hex)
    const changes = new Map<string, string | null>((await this.rpc('state_queryStorageAt', [keys, hash]))[0]?.changes ?? [])
    return keys.map(key => decodeStorage(runtime, module, name, changes.get(key)))
  }
  async queryMap(item: Descriptor, params?: ScaleValue[], block?: BlockRef): Promise<Array<[any, any]>>
  async queryMap(pallet: string, item: string, params?: ScaleValue[], block?: BlockRef, options?: { pageSize?: number; maxPages?: number; maxResults?: number; signal?: AbortSignal }): Promise<Array<[any, any]>>
  async queryMap(pallet: string | Descriptor, item: string | ScaleValue[] = [], params?: ScaleValue[] | BlockRef, block?: BlockRef, options: { pageSize?: number; maxPages?: number; maxResults?: number; signal?: AbortSignal } = {}): Promise<Array<[any, any]>> {
    const [module, name, args, at] = storageArgs(pallet, item, params, block)
    const hash = await this.resolveBlockHash(at), runtime = await this.runtimeAt(hash)
    const prefix = hex(runtime.storageKey(module, name, args)), pageSize = options.pageSize ?? 512
    if (!Number.isSafeInteger(pageSize) || pageSize <= 0) throw new TypeError('pageSize must be positive')
    const output: Array<[any, any]> = []; let start: string | null = null, pages = 0
    for (;;) {
      options.signal?.throwIfAborted()
      if (options.maxPages != null && pages >= options.maxPages) throw new ChainError('queryMap exceeded maxPages')
      const keys: string[] = await this.rpc('state_getKeysPaged', [prefix, pageSize, start, hash], options)
      if (!keys.length) return output
      if (keys.some((key, i) => (i > 0 && key <= keys[i - 1]) || (start != null && key <= start))) throw new ChainError('queryMap pagination did not advance')
      const changes: Array<[string, string | null]> = (await this.rpc('state_queryStorageAt', [keys, hash], options))[0]?.changes ?? []
      for (const pair of runtime.decodeMapChanges(module, name, changes.map(([key, value]) => ({ key, value })), args.length)) {
        output.push([pair.key, pair.value])
        if (options.maxResults != null && output.length >= options.maxResults) return output
      }
      start = keys[keys.length - 1]; pages++
    }
  }
  async constant(item: Descriptor, block?: BlockRef): Promise<any>
  async constant(pallet: string, name: string, block?: BlockRef): Promise<any>
  async constant(pallet: string | Descriptor, name?: string | BlockRef, block?: BlockRef): Promise<any> {
    const [module, item] = typeof pallet === 'string' ? [pallet, name as string] : pallet
    return (await this.runtimeAt(typeof pallet === 'string' ? block : name)).constant(module, item)
  }
  async runtimeCall(api: string, method: string, params: ScaleValue[] = [], block?: BlockRef): Promise<any> {
    const hash = await this.resolveBlockHash(block), runtime = await this.runtimeAt(hash), info = runtime.runtimeApis()[api]?.[method]
    if (!info) throw new ChainError(`runtime API ${api}.${method} not found`)
    if (params.length !== info.inputs.length) throw new TypeError(`${api}.${method} expects ${info.inputs.length} arguments`)
    const input = concatBytes(...info.inputs.map(([, type], index) => runtime.encode(type, params[index])))
    return runtime.decode(info.output, hexBytes(await this.rpc('state_call', [`${api}_${method}`, hex(input), hash])), false)
  }
  runtime(item: Descriptor, params: ScaleValue[] = [], block?: BlockRef): Promise<any> { return this.runtimeCall(item[0], item[1], params, block) }
  async composeCall(pallet: string, call: string, params: ScaleValue = {}, block?: BlockRef): Promise<Uint8Array> { return (await this.runtimeAt(block)).composeCall(pallet, call, params) }
  async compose(call: Call, block?: BlockRef): Promise<Uint8Array> { const runtime = await this.runtimeAt(block); return call instanceof Uint8Array ? call.slice() : encodeIntentCall(call, async (p, f, a) => runtime.composeCall(p, f, a as ScaleValue)) }
  async decodeCall(bytes: Uint8Array, block?: BlockRef): Promise<ScaleValue> { return (await this.runtimeAt(block)).decodeCall(bytes) }
  async at(block?: BlockRef): Promise<Snapshot> { const hash = await this.resolveBlockHash(block); return new Snapshot(this, await this.blockNumber(hash), hash) }
  balance(rao: BalanceLike, netuid = 0, symbol?: string | null): Balance { return Balance.fromRao(rao, netuid, symbol) }
  async timestamp(block?: BlockRef): Promise<Date> { return new Date(Number(await this.query(['Timestamp', 'Now'], [], block))) }
  async blockTime(block?: BlockRef): Promise<number> {
    const hash = await this.resolveBlockHash(block), runtime = await this.runtimeAt(hash)
    let value = runtime.constant('Aura', 'SlotDuration')
    if (value == null && runtime.runtimeApis().AuraApi?.slot_duration) value = await this.runtimeCall('AuraApi', 'slot_duration', [], hash)
    if (value == null) value = Number(runtime.constant('Timestamp', 'MinimumPeriod')) * 2
    const seconds = Number(value) / 1000
    if (!(seconds > 0) || !Number.isFinite(seconds)) throw new ChainError('chain block time unavailable')
    return seconds
  }
  async isFastBlocks(): Promise<boolean> { return await this.blockTime() === 0.25 }
  read<N extends ReadName>(name: N, ...args: ReadArgs<N>): Promise<ReadResult<N>> { return dispatchRead(this, name, (args[0] ?? {}) as ReadParams[N]) }
  reads() { return listReads() }
  async metagraph(netuid: number, options: { block?: BlockRef; commitments?: boolean } = {}) { return fetchMetagraph(await this.at(options.block), netuid, options) }
  async blockInfo(block?: BlockRef) {
    const hash = await this.resolveBlockHash(block), raw = await this.rpc('chain_getBlock', [hash])
    if (!raw?.block) return null
    const number = normalizeHeader(raw.block.header).number
    const runtime = await this.runtimeAt(number > 0 ? raw.block.header.parentHash : hash)
    return { number, hash, header: raw.block.header, extrinsics: raw.block.extrinsics, decodedExtrinsics: raw.block.extrinsics.map((value: string) => runtime.decodeExtrinsic(hexBytes(value))), timestamp: await this.timestamp(hash) }
  }
  async *blocks(options: { finalized?: boolean; signal?: AbortSignal } = {}): AsyncIterable<BlockHeader> {
    const sub = await this.transport.subscribe(options.finalized ? 'chain_subscribeFinalizedHeads' : 'chain_subscribeNewHeads', [], options.finalized ? 'chain_unsubscribeFinalizedHeads' : 'chain_unsubscribeNewHeads', options)
    const abort = () => { void sub.unsubscribe() }; options.signal?.addEventListener('abort', abort, { once: true })
    try { for await (const header of sub) { options.signal?.throwIfAborted(); yield normalizeHeader(header) } options.signal?.throwIfAborted() }
    finally { options.signal?.removeEventListener('abort', abort); await sub.unsubscribe() }
  }
  async waitForBlock(block?: number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> {
    const request = withRequestSignal(options), target = block ?? await this.blockNumber() + 1
    try {
      const current = normalizeHeader(await this.rpc('chain_getHeader', [], { signal: request.signal }))
      if (current.number >= target) return current
      for await (const header of this.blocks({ signal: request.signal })) if (header.number >= target) return header
      throw new ChainError('block subscription ended')
    }
    finally { request.cleanup() }
  }
  async waitTimestamp(target: Date, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<Date> {
    const request = withRequestSignal(options)
    try { for (;;) { request.signal?.throwIfAborted(); const now = await this.timestamp(); if (now >= target) return now; await this.waitForBlock(undefined, { signal: request.signal }) } }
    finally { request.cleanup() }
  }
  waitForTimestamp(target: Date, options: { timeoutMs?: number; signal?: AbortSignal } = {}) { return this.waitTimestamp(target, options) }
  waitForEpoch(netuid: number, options: { timeoutMs?: number; signal?: AbortSignal } = {}) { return this.waitEpoch(netuid, options) }
  async waitEpoch(netuid: number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<number> {
    const snapshot = await this.at(), tempo = Number(await snapshot.query(['SubtensorModule', 'Tempo'], [netuid]))
    if (!(tempo > 0)) throw new ChainError('subnet has no active epoch schedule')
    const original = Number(await snapshot.query(['SubtensorModule', 'LastEpochBlock'], [netuid])), request = withRequestSignal(options)
    try { for (;;) { await this.waitForBlock(undefined, { signal: request.signal }); const current = Number(await this.query(['SubtensorModule', 'LastEpochBlock'], [netuid])); if (current > original) return current } }
    finally { request.cleanup() }
  }
  async prepareCall(call: Call, signer: BrowserSigner, options: SubmitOptions = {}): Promise<PreparedTransaction> {
    if (!options.allowRawCall) throw new ChainError('prepareCall requires allowRawCall: true; use prepareIntent for semantic policies')
    rejectDetachedPolicy(options)
    return this.prepareRaw(call, pinSigner(signer), options)
  }
  private async prepareRaw(call: Call, signer: BrowserSigner, options: SubmitOptions, at?: string): Promise<PreparedTransaction> {
    const hash = at ?? await this.blockHash(), [{ runtime, metadata, version, properties }, number, genesisHash] = await Promise.all([this.runtimeEntry(hash), this.blockNumber(hash), this.genesisHash()])
    const address = signer.ss58Address ?? signer.address
    if (!address) throw new TypeError('signer must expose an address')
    if (signer.genesisHash && signer.genesisHash.toLowerCase() !== genesisHash.toLowerCase()) throw new ChainError('signer belongs to another chain')
    const publicKey = publicKeyFromSs58(address), cryptoType = signer.cryptoType ?? 1
    if (signer.publicKey && hex(signer.publicKey) !== hex(publicKey)) throw new TypeError('signer public key does not match address')
    if (![0, 1].includes(cryptoType)) throw new TypeError('unsupported signer scheme')
    const nonce = options.nonce ?? await this.peekNonce(address)
    if (!Number.isSafeInteger(nonce) || nonce < 0) throw new TypeError('invalid nonce')
    const period = options.period === undefined ? DEFAULT_ERA_PERIOD : options.period
    if (period != null && (!Number.isSafeInteger(period) || period < 4 || period > 65536)) throw new RangeError('era period must be between 4 and 65536 blocks')
    if (options.tip != null && (typeof options.tip !== 'bigint' || options.tip < 0n)) throw new TypeError('tip must be a nonnegative bigint')
    // Parent-state runtime APIs cannot read System.BlockHash[best] until the next
    // block initializes. Keep fresh metadata while anchoring mortality one block earlier.
    const eraAnchor = Math.max(0, number - 1)
    const era = period == null ? '00' : { period, current: eraAnchor }
    const eraBlockHash = period == null ? genesisHash : await this.blockHash(eraBirth(period, eraAnchor))
    const callData = await this.compose(call, hash)
    const chain = [Number(first(properties.ss58Format ?? properties.ss58Prefix) ?? 42), Number(first(properties.tokenDecimals) ?? 9), String(first(properties.tokenSymbol) ?? 'TAO')] as const
    const metadataHash = options.metadataHash ?? ((signer.requiresMetadataHash || signer.requiresMetadataProof) ? metadataDigest(metadata, runtime.specVersion, version.specName, ...chain) : undefined)
    const txParams = { era, nonce, tip: options.tip ?? 0n, tipAssetId: options.tipAssetId, genesisHash: hexBytes(genesisHash), eraBlockHash: hexBytes(eraBlockHash), metadataHash }
    const parts = runtime.signaturePayloadParts(txParams), payload = runtime.signaturePayload(callData, txParams)
    const proof = signer.requiresMetadataProof ? generateExtrinsicProof(callData, parts.includedInExtrinsic, parts.includedInSignedData, metadata, runtime.specVersion, version.specName, ...chain) : undefined
    return { address, publicKey, cryptoType, callData, genesisHash, payload, ...parts, runtime, txParams, nonce, blockHash: hash, blockNumber: number, metadataProof: proof, signerPayload: runtime.signerPayload(address, callData, txParams) }
  }
  private signed(unsigned: PreparedTransaction, response: unknown) {
    const expectedPayload = unsigned.runtime.signaturePayload(unsigned.callData, unsigned.txParams)
    if (hex(expectedPayload) !== hex(unsigned.payload) || hex(publicKeyFromSs58(unsigned.address)) !== hex(unsigned.publicKey)) throw new ChainError('prepared transaction was modified after signing preparation')
    let signature = signatureBytes(response)
    if (signature.length === 65) { if (signature[0] !== unsigned.cryptoType) throw new ChainError('signature scheme differs from signer'); signature = signature.slice(1) }
    if (signature.length !== 64 || !verifySignature(unsigned.payload, signature, unsigned.address, unsigned.cryptoType)) throw new ChainError('invalid transaction signature')
    const signed = unsigned.runtime.encodeSignedExtrinsic(unsigned.callData, unsigned.publicKey, signature, unsigned.cryptoType, { ...unsigned.txParams, metadataHashEnabled: unsigned.txParams.metadataHash != null })
    return { ...signed, hex: hex(signed.bytes), nonce: unsigned.nonce, signerAddress: unsigned.address }
  }
  private async signPrepared(unsigned: PreparedTransaction, signer: BrowserSigner) {
    const address = signer.ss58Address ?? signer.address
    if (!address || hex(publicKeyFromSs58(address)) !== hex(unsigned.publicKey) || (signer.cryptoType ?? 1) !== unsigned.cryptoType) throw new ChainError('signer identity changed after preparation')
    const { runtime: _runtime, ...fields } = unsigned
    const context = { ...structuredClone(fields), client: this, metadataHash: structuredClone(unsigned.txParams.metadataHash), proof: unsigned.metadataProof?.slice() }
    const response = signer.signBytes ? await signer.signBytes(unsigned.payload, context) : signer.signPayload ? await signer.signPayload(unsigned.signerPayload, context) : signer.signRaw ? await signer.signRaw({ address: unsigned.address, data: hex(unsigned.payload), type: 'bytes' }) : signer.sign ? await signer.sign(unsigned.payload, context) : undefined
    return this.signed(unsigned, response)
  }
  async signExtrinsic(call: Call, signer: BrowserSigner, options: SubmitOptions = {}) {
    if (options.nonce == null) throw new ChainError('detached signing requires an explicit nonce')
    signer = pinSigner(signer)
    return this.signPrepared(await this.prepareCall(call, signer, options), signer)
  }
  async exportPreparedCall(unsigned: PreparedTransaction): Promise<SerializedUnsignedTransaction> {
    const entry = await this.runtimeEntry(unsigned.blockHash)
    return serializeUnsignedTransaction(unsigned as unknown as UnsignedTransactionState, { blake2_256, publicKeyFromSs58 }, entry.metadata)
  }
  async importPreparedCall(json: unknown, options: SubmitOptions = {}): Promise<PreparedTransaction> {
    if (!options.allowRawCall) throw new ChainError('importPreparedCall requires allowRawCall: true; serialized bytes carry no semantic policy approval')
    if (options.policy && Object.values(options.policy).some(value => value != null)) throw new ChainError('imported calls cannot establish a semantic policy')
    const blockHash = await this.blockHash(), entry = await this.runtimeEntry(blockHash)
    const restored = restoreUnsignedTransaction(json, entry.runtime as unknown as UnsignedRuntime, { blake2_256, publicKeyFromSs58 }, entry.metadata, await this.genesisHash())
    const unsigned = { ...restored, runtime: entry.runtime, txParams: restored.txParams as BrowserTransactionParams, signerPayload: restored.signerPayload as Record<string, unknown>, blockHash, blockNumber: await this.blockNumber(blockHash) }
    this.importedTransactions.set(unsigned, serializeUnsignedTransaction(restored, { blake2_256, publicKeyFromSs58 }, entry.metadata))
    return unsigned
  }
  async submitSignature(unsigned: PreparedTransaction, signature: unknown, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    rejectDetachedPolicy(options)
    const serialized = this.importedTransactions.get(unsigned)
    const { runtime, ...fields } = unsigned
    unsigned = { ...structuredClone(fields), runtime }
    signature = signatureBytes(signature).slice()
    return this.serial({ address: serialized?.address ?? unsigned.address }, async () => {
      if (serialized) {
        const hash = await this.blockHash(), entry = await this.runtimeEntry(hash)
        const restored = restoreUnsignedTransaction(serialized, entry.runtime as unknown as UnsignedRuntime, { blake2_256, publicKeyFromSs58 }, entry.metadata, await this.genesisHash())
        // Rebuild from the admitted immutable JSON, not caller-mutated fields.
        unsigned = { ...unsigned, ...restored, runtime: entry.runtime, txParams: restored.txParams as BrowserTransactionParams, signerPayload: restored.signerPayload as Record<string, unknown> }
      }
      return this.broadcastExtrinsic(this.signed(unsigned, signature).bytes, options)
    })
  }
  async submit(call: Call, signer: BrowserSigner, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    if (!options.allowRawCall) throw new ChainError('raw calls require allowRawCall: true; use execute for semantic intents')
    if (options.policy && Object.values(options.policy).some(v => v != null)) throw new ChainError('raw calls cannot establish semantic spending or subnet policy')
    signer = pinSigner(signer)
    return this.serial(signer, () => this.submitPreparedCall(call, signer, options))
  }
  async submitExtrinsic(bytes: Uint8Array, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    rejectDetachedPolicy(options)
    const submitted = bytes.slice(), decoded = (await this.currentRuntime()).decodeExtrinsic(submitted) as Record<string, any>
    if (decoded.address == null) return this.broadcastExtrinsic(submitted, options)
    const address = decoded.address.Id ?? decoded.address
    const signer = { address: typeof address === 'string' ? address : ss58FromPublic(Uint8Array.from(address)) }
    return this.serial(signer, () => this.broadcastExtrinsic(submitted, options))
  }
  private async broadcastExtrinsic(bytes: Uint8Array, options: SubmitOptions): Promise<ExtrinsicResult> {
    const hash = hex(blake2_256(bytes))
    if (options.waitForInclusion === false && !options.waitForFinalization) { const accepted = await this.rpc('author_submitExtrinsic', [hex(bytes)], options); if (String(accepted).toLowerCase() !== hash) throw new ChainError('node returned a different submitted extrinsic hash'); return { status: 'submitted', message: 'Submitted', extrinsicHash: hash, events: [] } }
    const request = withRequestSignal({ timeoutMs: options.timeoutMs ?? 120_000, signal: options.signal })
    let sub: Awaited<ReturnType<JsonRpcTransport['subscribe']>> | undefined
    let error: Error | undefined
    try {
      sub = await this.transport.subscribe('author_submitAndWatchExtrinsic', [hex(bytes)], 'author_unwatchExtrinsic', { signal: request.signal, resubscribe: false })
      request.onAbort(e => { error = e; void sub?.unsubscribe() })
      for await (const raw of sub) {
        if (error) throw error
        const status = normalizeStatus(raw), fatal = ['usurped', 'finalitytimeout', 'dropped', 'invalid'].find(key => key in status)
        if (fatal) throw new ChainError(`extrinsic ${fatal}`, raw)
        const included = options.waitForFinalization ? status.finalized : status.inblock ?? status.finalized
        if (included != null) return this.resolveInclusion(hash, String(included), status.finalized != null)
      }
      throw error ?? new ChainError('extrinsic watch ended before inclusion')
    } finally { request.cleanup(); await sub?.unsubscribe() }
  }
  private async eventsAt(blockHash: string, header?: any): Promise<{ runtime: Runtime; events: unknown[] }> {
    header ??= await this.rpc('chain_getHeader', [blockHash])
    // Upgrade-block events use the runtime that executed the block, before its post-state metadata.
    const runtime = await this.runtimeAt(normalizeHeader(header).number > 0 ? header.parentHash : blockHash)
    const raw = await this.rpc('state_getStorage', [hex(runtime.storageKey('System', 'Events', [])), blockHash])
    const events = (decodeStorage(runtime, 'System', 'Events', raw) ?? []) as unknown[]
    if (events.some((event: unknown) => eventName(event) === 'System.CodeUpdated')) this.invalidateRuntimeCache()
    return { runtime, events }
  }
  async findExtrinsic(hash: string, block: BlockRef, finalized = false): Promise<ExtrinsicResult | null> {
    const blockHash = await this.resolveBlockHash(block), result = await this.rpc('chain_getBlock', [blockHash])
    if (!result?.block?.extrinsics.some((item: string) => hex(blake2_256(hexBytes(item))) === hash.toLowerCase())) return null
    return this.resolveInclusion(hash.toLowerCase(), blockHash, finalized, result.block)
  }
  private async resolveInclusion(hash: string, blockHash: string, finalized: boolean, foundBlock?: any): Promise<ExtrinsicResult> {
    const block = foundBlock ?? (await this.rpc('chain_getBlock', [blockHash])).block
    const index = block.extrinsics.findIndex((item: string) => hex(blake2_256(hexBytes(item))) === hash)
    if (index < 0) throw new ChainError('included extrinsic was not found in block')
    const decoded = await this.eventsAt(blockHash, block.header)
    const events = decoded.events.filter((event: unknown) => eventExtrinsicIndex(event) === index)
    const outerFailure = events.find(event => eventName(event) === 'System.ExtrinsicFailed')
    const nested = nestedDispatchError(events)
    const failed = outerFailure ?? (nested == null ? undefined : { attributes: { dispatch_error: nested } })
    const success = !failed && events.some(event => eventName(event) === 'System.ExtrinsicSuccess')
    const fee = events.find(event => eventName(event) === 'TransactionPayment.TransactionFeePaid')
    return { status: failed ? 'failed' : success ? finalized ? 'finalized' : 'inBlock' : 'unknown', success: failed ? false : success || undefined, message: failed ? dispatchFailureMessage(decoded.runtime, failed) : success ? 'Success' : 'Included without a dispatch outcome', extrinsicHash: hash, blockHash, blockNumber: normalizeHeader(block.header).number, extrinsicIndex: index, finalized, events, error: failed, chainError: failed ? chainErrorFromDispatch(failed, (pallet, error) => decoded.runtime.moduleError(pallet, error)) : undefined, fee: fee ? feeFromEvent(fee) : undefined }
  }
  private async resolveShieldedInner(outer: ExtrinsicResult, innerHash: string, period: number, options: SubmitOptions): Promise<ExtrinsicResult> {
    return followShieldedResult(outer, innerHash, period, this, options)
  }
  private async serial<T>(signer: BrowserSigner, action: () => Promise<T>): Promise<T> {
    const address = signer.ss58Address ?? signer.address
    if (!address) throw new TypeError('signer must expose an address')
    const key = hex(publicKeyFromSs58(address)), prior = this.transactionQueues.get(key) ?? Promise.resolve(), next = prior.catch(() => {}).then(action)
    this.transactionQueues.set(key, next)
    try { return await next } finally { if (this.transactionQueues.get(key) === next) this.transactionQueues.delete(key) }
  }
  private async peekNonce(address: string): Promise<number> {
    const nonce = Number(await this.rpc('system_accountNextIndex', [address]))
    if (!Number.isSafeInteger(nonce) || nonce < 0) throw new ChainError('invalid account nonce')
    return Math.max(nonce, this.nonceFloors.get(hex(publicKeyFromSs58(address))) ?? 0)
  }
  private reserveNonceRange(address: string, nonce: number, count: number): number {
    if (!Number.isSafeInteger(count) || count < 1 || !Number.isSafeInteger(nonce) || nonce < 0 || !Number.isSafeInteger(nonce + count)) throw new RangeError('invalid nonce reservation')
    const key = hex(publicKeyFromSs58(address)), start = Math.max(nonce, this.nonceFloors.get(key) ?? 0)
    if (!Number.isSafeInteger(start + count)) throw new RangeError('nonce reservation exceeds safe integer range')
    this.nonceFloors.set(key, start + count)
    return start
  }
  /** Reserve consecutive nonces for manual signing. Reservations survive uncertain delivery. */
  reserveNonces(address: string, count = 1): Promise<number> {
    return this.serial({ address }, async () => this.reserveNonceRange(address, await this.peekNonce(address), count))
  }
  /** Clear a cached reservation only after reconciling inclusion, expiry, or an abandoned
   * signing request. Clearing while a shielded inner transaction is pending can reuse its nonce. */
  invalidateNonce(address: string): void { this.nonceFloors.delete(hex(publicKeyFromSs58(address))) }
  async estimateFee(call: Call, address: string, options: SubmitOptions = {}, block?: BlockRef): Promise<Balance> {
    const unsigned = await this.prepareRaw(call, { address }, options, block == null ? undefined : await this.resolveBlockHash(block))
    const bytes = unsigned.runtime.encodeSignedExtrinsic(unsigned.callData, unsigned.publicKey, new Uint8Array(64), 1, { ...unsigned.txParams, metadataHashEnabled: unsigned.txParams.metadataHash != null }).bytes
    const info = await this.rpc('payment_queryInfo', [hex(bytes), unsigned.blockHash])
    const payer = await resolveTransactionFeePayer(await this.intentReader(unsigned.blockHash), unsigned.runtime.decodeCall(unsigned.callData), address)
    // Keep the original tip encoded for length fees; sponsored calls discard its charge.
    return Balance.fromRao(BigInt(info.partialFee) + (payer.dropsTip ? 0n : options.tip ?? 0n))
  }
  async estimateWeight(call: Call, address: string, block?: BlockRef): Promise<{ ref_time: bigint; proof_size: bigint }> {
    const unsigned = await this.prepareRaw(call, { address }, {}, block == null ? undefined : await this.resolveBlockHash(block)), bytes = unsigned.runtime.encodeSignedExtrinsic(unsigned.callData, unsigned.publicKey, new Uint8Array(64), 1, unsigned.txParams).bytes
    const { weight } = await this.rpc('payment_queryInfo', [hex(bytes), unsigned.blockHash])
    return { ref_time: BigInt(weight.refTime ?? weight.ref_time ?? weight), proof_size: BigInt(weight.proofSize ?? weight.proof_size ?? 0) }
  }
  async intentReader(block?: BlockRef): Promise<IntentReader> {
    const snapshot = await this.at(block), crypto = { blake2_256, publicKeyFromSs58, ss58FromPublic, verify: verifySignature }
    const reader: IntentReader = {
      queryMap: (p, f, a = [], limit) => this.queryMap(p, f, a as ScaleValue[], snapshot.hash, { maxResults: limit }),
      countStorageKeys: async (pallet, item, params, limit) => {
        if (!Number.isSafeInteger(limit) || limit <= 0) throw new RangeError('storage key limit must be positive')
        const runtime = await this.runtimeAt(snapshot.hash)
        let prefix = runtime.storageKey(pallet, pallet === 'SubtensorModule' && item === 'Alpha' ? 'AlphaV2' : item, params as ScaleValue[])
        if (pallet === 'SubtensorModule' && item === 'Alpha') prefix = concatBytes(storagePrefixFor(pallet, item), prefix.subarray(32))
        const keys = await this.rpc('state_getKeysPaged', [hex(prefix), limit, null, snapshot.hash])
        if (!Array.isArray(keys)) throw new ChainError('storage key admission count unavailable')
        return keys.length
      },
      query: (p, f, a = []) => snapshot.query([p, f], a as ScaleValue[]), runtimeCall: (p, f, a = []) => snapshot.runtime([p, f], a as ScaleValue[]), constant: (p, f) => snapshot.constant([p, f]),
      blockNumber: async () => snapshot.block, blockHash: n => n == null ? Promise.resolve(snapshot.hash) : this.blockHash(n), blockTime: () => snapshot.blockTime(),
      decodeAddress: publicKeyFromSs58, encodeAddress: ss58FromPublic,
      multisigAccount: (signers, threshold) => { const { accountId } = multisigAccountId(signers.map(publicKeyFromSs58), threshold); return { address: ss58FromPublic(accountId), publicKey: accountId } },
      hashCall: async call => blake2_256(await this.compose(call, snapshot.hash)), estimateFee: async (call, signer) => (await this.estimateFee(call, signer, {}, snapshot.hash)).rao, estimateWeight: (call, signer) => this.estimateWeight(call, signer, snapshot.hash),
      evmMirror: createEvmAddressUtilities(crypto).h160ToSs58,
      encryptWeightsV2: async o => { const [commit, revealRound] = generateCommitV2(o.uids, o.values, BigInt(o.versionKey), { ...o.state, lastEpochBlock: BigInt(o.state.lastEpochBlock), pendingEpochAt: BigInt(o.state.pendingEpochAt), subnetEpochIndex: BigInt(o.state.subnetEpochIndex), blocksSinceLastStep: BigInt(o.state.blocksSinceLastStep), currentBlock: BigInt(o.state.currentBlock) }, BigInt(o.subnetRevealPeriodEpochs), o.blockTime, o.hotkey); return { commit, revealRound: BigInt(revealRound) } },
    }
    reader.resolveFeePayer = async (call, address) => (await resolveTransactionFeePayer(reader, call, address)).address
    return reader
  }
  multisig(signatories: readonly string[], threshold: number): BrowserMultisig { return new BrowserMultisig(this, signatories, threshold) }
  private async submitPreparedCall(call: Call, signer: BrowserSigner, options: SubmitOptions, at?: string): Promise<ExtrinsicResult> {
    const blockHash = at ?? await this.blockHash()
    const address = signer.ss58Address ?? signer.address!
    const nonce = options.nonce ?? this.reserveNonceRange(address, await this.peekNonce(address), options.mevShield ? 2 : 1)
    options = { ...options, nonce }
    const { policy: _policy, ...submissionOptions } = options
    if (!options.mevShield) return this.broadcastExtrinsic((await this.signPrepared(await this.prepareRaw(call, signer, options, blockHash), signer)).bytes, submissionOptions)
    const signingOptions = shieldOptions(options)
    if (!Number.isSafeInteger(nonce) || nonce < 0 || nonce >= Number.MAX_SAFE_INTEGER) throw new TypeError('invalid shielded nonce')
    const rawKey = await this.query(['MevShield', 'NextKey'], [], blockHash)
    const key = typeof rawKey === 'string' ? hexBytes(rawKey) : rawKey instanceof Uint8Array ? rawKey : Array.isArray(rawKey) ? Uint8Array.from(rawKey) : new Uint8Array()
    if (key.length !== 1184) throw new ChainError('MevShield.NextKey is not an ML-KEM-768 key')
    const inner = await this.signPrepared(await this.prepareRaw(call, signer, { ...signingOptions, nonce: nonce + 1 }, blockHash), signer)
    const ciphertext = sealMevShieldTransaction(key, inner.bytes)
    if (ciphertext.length > 8192) throw new ChainError('shielded ciphertext exceeds 8192 bytes')
    const carrier = { pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext } }
    const outer = await this.signPrepared(await this.prepareRaw(carrier, signer, { ...signingOptions, nonce }, blockHash), signer)
    const result = await this.broadcastExtrinsic(outer.bytes, submissionOptions)
    return this.resolveShieldedInner(result, hex(inner.hash), Number(signingOptions.period), options)
  }
  async prepareMultisig(request: MultisigApprovalRequest, block?: BlockRef) { return prepareMultisigApproval(await this.intentReader(block), request) }
  upgrades(): UpgradeClient {
    const adapter = (hash?: string): UpgradeAdapter => ({
      query: (item, params = []) => this.query(item, params as ScaleValue[], hash),
      queryMap: (item, params = []) => this.queryMap(item, params as ScaleValue[], hash),
      compose: call => this.compose(call, hash),
      crypto: { blake2_256, publicKeyFromSs58, ss58FromPublic },
      multisig: (signatories, threshold) => ({ address: ss58FromPublic(multisigAccountId(signatories.map(publicKeyFromSs58), threshold).accountId) }),
      estimateWeight: (call, address) => this.estimateWeight(call, address, hash),
      at: async () => adapter(hash ?? await this.finalizedHead()),
    })
    return new UpgradeClient(adapter())
  }
  async prepareIntent(intent: Intent, context: IntentContext): Promise<PreparedIntent> { return intent.prepare(await this.intentReader(), context) }
  tools() { return listIntentTools() }
  async preflight(intent: Intent, context: IntentContext, policy: IntentPolicy = {}, options: WorkflowOptions = {}) { return this.plan(intent, context, policy, options) }
  async plan(intent: Intent, context: IntentContext, policy: IntentPolicy = {}, options: WorkflowOptions = {}) {
    return this.planForSigner(intent, context, policy, options)
  }
  private async planForSigner(intent: Intent, context: IntentContext, policy: IntentPolicy = {}, options: WorkflowOptions = {}, signer?: BrowserSigner) {
    if (options.proxyType != null && !PROXY_TYPES.includes(options.proxyType)) throw new TypeError('unknown proxy type')
    const role = intentSignerRole(intent)
    const signerAddress = context.feePayer ?? (role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey)
    if (options.proxyFor) {
      publicKeyFromSs58(options.proxyFor)
      context = { ...context, coldkey: options.proxyFor, feePayer: signerAddress, ...(role === 'hotkey' ? { hotkey: options.proxyFor, hotkeyPublicKey: publicKeyFromSs58(options.proxyFor) } : {}) }
    }
    const snapshot = await this.at(await this.blockHash()), baseReader = await this.intentReader(snapshot.hash)
    const wrap = (call: IntentDescriptor): IntentDescriptor => options.proxyFor ? { pallet: 'Proxy', call: 'proxy', params: { real: options.proxyFor, force_proxy_type: options.proxyType ?? null, call } } : call
    const reader: IntentReader = { ...baseReader, resolveFeePayer: async (call, address) => (await resolveTransactionFeePayer(baseReader, wrap(call), address)).address, estimateFee: (call, address) => baseReader.estimateFee!(wrap(call), address) }
    const prepared = await intent.prepare(reader, context), call = wrap(prepared.call)
    const payer = await resolveTransactionFeePayer(baseReader, call, prepared.feePayer)
    const shielded = options.mevShield ?? prepared.mevShieldDefault
    const signingOptions = shielded ? shieldOptions(options) : { ...options }
    const nonce = options.nonce ?? await this.peekNonce(prepared.feePayer)
    if (!Number.isSafeInteger(nonce) || nonce < 0 || nonce >= Number.MAX_SAFE_INTEGER) throw new TypeError('invalid nonce')
    if (signingOptions.metadataHash == null && (signer?.requiresMetadataHash || signer?.requiresMetadataProof)) {
      const unsigned = await this.prepareRaw(call, signer, { ...signingOptions, nonce }, snapshot.hash)
      signingOptions.metadataHash = unsigned.txParams.metadataHash as Uint8Array
    }
    const innerFee = await this.estimateFee(call, prepared.feePayer, { ...signingOptions, nonce: nonce + (shielded ? 1 : 0) }, snapshot.hash)
    const carrierFee = shielded ? await this.estimateFee({ pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext: new Uint8Array(8192) } }, prepared.feePayer, { ...signingOptions, nonce }, snapshot.hash) : Balance.fromRao(0)
    const fee = Balance.fromRao(innerFee.rao + carrierFee.rao), violations = checkIntentPolicy(prepared, policy, fee.rao)
    if (prepared.mevShieldRequired && !shielded) violations.push('this intent requires MEV shielding')
    if (options.expectedCallHash != null && String(prepared.extras.multisig_call_hash).toLowerCase() !== options.expectedCallHash.toLowerCase()) violations.push('multisig call changed from expectedCallHash')
    const owner = payer.address
    const accountKey = (address: string) => hex(publicKeyFromSs58(address))
    const spendByAccount = new Map<string, { address: string; amountRao: bigint }>()
    for (const spend of prepared.spendByAccount) {
      const key = accountKey(spend.address), previous = spendByAccount.get(key)?.amountRao ?? 0n
      spendByAccount.set(key, { address: spend.address, amountRao: previous + spend.amountRao })
    }
    const ownerKey = accountKey(owner), signerKey = accountKey(prepared.feePayer), ownerIsSigner = ownerKey === signerKey
    const ownerSpend = spendByAccount.get(ownerKey)?.amountRao ?? 0n, signerSpend = spendByAccount.get(signerKey)?.amountRao ?? 0n
    const free = BigInt((await snapshot.query(['System', 'Account'], [owner]))?.data?.free ?? 0)
    const required = innerFee.rao + ownerSpend + (ownerIsSigner ? carrierFee.rao : 0n)
    if (shielded && !ownerIsSigner && BigInt((await snapshot.query(['System', 'Account'], [prepared.feePayer]))?.data?.free ?? 0) < carrierFee.rao + signerSpend) violations.push('signing account cannot cover the MEV carrier fee')
    if (free < required) violations.push(`fee payer needs ${required} rao; available ${free}`)
    for (const [key, spend] of spendByAccount) {
      if (key === ownerKey || shielded && key === signerKey) continue
      if (spend.amountRao > BigInt((await snapshot.query(['System', 'Account'], [spend.address]))?.data?.free ?? 0)) violations.push(`dispatch account ${spend.address} cannot cover outgoing spend`)
    }
    for (const reserve of prepared.requiredFreeByAccount) {
      const reserveKey = accountKey(reserve.address)
      if (reserve.availableRao < reserve.requiredRao + (spendByAccount.get(reserveKey)?.amountRao ?? 0n) + (reserveKey === ownerKey && !payer.dropsTip ? options.tip ?? 0n : 0n) + (reserveKey === signerKey ? carrierFee.rao : 0n)) violations.push(`account ${reserve.address} cannot cover its reserves, outgoing spend, tip and MEV carrier fee`)
    }
    return { prepared, call, fee, carrierFee, shielded, violations, ok: violations.length === 0, blockHash: snapshot.hash, nonce, signingOptions }
  }
  submitShielded(intent: Intent, signer: BrowserSigner, options: WorkflowOptions = {}) { return this.execute(intent, signer, { ...options, mevShield: true }) }
  executeTool(spec: Record<string, unknown>, signer: BrowserSigner, options: WorkflowOptions = {}) { return this.execute(intentFromJSON(spec), signer, options) }
  async execute(intent: Intent, signer: BrowserSigner, options: WorkflowOptions = {}): Promise<ExtrinsicResult> {
    signer = pinSigner(signer)
    return this.serial(signer, async () => {
      const address = signer.ss58Address ?? signer.address!, context = options.context ?? { coldkey: address, ...(intentSignerRole(intent) === 'hotkey' ? { hotkey: address, hotkeyPublicKey: publicKeyFromSs58(address) } : {}) }
      const { prepared, call, violations, blockHash, nonce, signingOptions } = await this.planForSigner(intent, context, options.policy, options, signer)
      if (violations.length) throw new ChainError(`transaction rejected: ${violations.join('; ')}`)
      if (hex(publicKeyFromSs58(prepared.feePayer)) !== hex(publicKeyFromSs58(address))) throw new ChainError('signer differs from intent fee payer')
      const shield = options.mevShield ?? prepared.mevShieldDefault
      if (prepared.mevShieldRequired && !shield) throw new ChainError('this intent requires MEV shielding')
      const submissionNonce = options.nonce == null ? this.reserveNonceRange(address, nonce, shield ? 2 : 1) : nonce
      return this.submitPreparedCall(call, signer, { ...signingOptions, nonce: submissionNonce, mevShield: shield }, blockHash)

    })
  }
}

/** Deterministic multisig approvals with live pending-state and deposit checks. */
export class BrowserMultisig {
  readonly address: string
  readonly signatories: readonly string[]
  constructor(readonly client: Client, signatories: readonly string[], readonly threshold: number) {
    const ids = signatories.map(publicKeyFromSs58), distinct = new Set(ids.map(hex))
    if (distinct.size !== ids.length) throw new RangeError('duplicate multisig signatory')
    if (!Number.isInteger(threshold) || threshold < 1 || threshold > ids.length || threshold > 65535) throw new RangeError('invalid multisig threshold')
    this.signatories = Object.freeze(signatories.slice().sort((a, b) => hex(publicKeyFromSs58(a)).localeCompare(hex(publicKeyFromSs58(b)))))
    this.address = ss58FromPublic(multisigAccountId(ids, threshold).accountId)
  }
  async prepare(call: IntentDescriptor, signerAddress: string, expectedCallHash?: string) {
    return this.client.prepareMultisig({ call, signatories: this.signatories, threshold: this.threshold, signerAddress, expectedCallHash })
  }
  async approve(call: Intent | IntentDescriptor, signer: BrowserSigner, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    signer = pinSigner(signer)
    const address = signer.ss58Address ?? signer.address
    if (!address) throw new TypeError('signer must expose an address')
    const identity = hex(publicKeyFromSs58(address))
    if (!this.signatories.some(member => hex(publicKeyFromSs58(member)) === identity)) throw new ChainError('signer is not a member of this multisig')
    if (call instanceof Intent) {
      const reader = await this.client.intentReader()
      const hotkey = intentSignerRole(call) === 'hotkey'
      const inner = await call.prepare(reader, { coldkey: this.address, feePayer: address, ...(hotkey ? { hotkey: this.address, hotkeyPublicKey: publicKeyFromSs58(this.address) } : {}) })
      const approval = await prepareMultisigApproval(reader, { call: inner.call, signatories: this.signatories, threshold: this.threshold, signerAddress: address, expectedCallHash: options.expectedCallHash })
      const wrapped = new Intent(this.threshold === 1 ? 'multisig_threshold_1' : 'multisig_execute', {
        other_signatories: this.signatories.filter(member => hex(publicKeyFromSs58(member)) !== identity), call: call.toJSON(),
        ...(this.threshold === 1 ? {} : { threshold: this.threshold, timepoint: approval.timepoint }),
      })
      return this.client.execute(wrapped, signer, { ...options, expectedCallHash: approval.callHash })
    }
    if (!options.allowRawCall) throw new ChainError('descriptor multisig approval requires allowRawCall: true; pass an Intent for semantic policies')
    if (options.policy && Object.values(options.policy).some(value => value != null)) throw new ChainError('raw multisig calls cannot establish a semantic policy')
    const requiresShield = [...iterCallLeaves(call)].some(([pallet, fn]) => pallet === 'SubtensorModule' && fn === 'add_collateral')
    if (requiresShield && !options.mevShield) throw new ChainError('add_collateral requires MEV shielding')
    const approval = await this.prepare(call, address, options.expectedCallHash)
    const signing = options.mevShield ? shieldOptions(options) : options
    const fee = await this.client.estimateFee(approval.call, address, signing)
    const carrierFee = options.mevShield ? await this.client.estimateFee({ pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext: new Uint8Array(8192) } }, address, signing) : Balance.fromRao(0)
    if (approval.violations.length || approval.availableFreeRao < approval.depositRao + fee.rao + carrierFee.rao) throw new ChainError('multisig signer cannot cover deposit and fees')
    return this.client.submit(approval.call, signer, options)
  }
}

export class Snapshot implements ReadView {
  readonly balances: AccountsReadNamespace = bindReadNamespace({}, 'Accounts & keys', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as AccountsReadNamespace
  readonly subnets: SubnetsReadNamespace = bindReadNamespace({}, 'Subnets', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as SubnetsReadNamespace
  readonly staking: StakingReadNamespace = bindReadNamespace({}, 'Staking', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as StakingReadNamespace
  readonly neurons: NeuronsReadNamespace = bindReadNamespace({}, 'Neurons & registration', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as NeuronsReadNamespace
  readonly chain: ChainReadNamespace = bindReadNamespace({}, 'Chain', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as ChainReadNamespace
  readonly epochs: EpochsReadNamespace = bindReadNamespace({}, 'Epochs & timing', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as EpochsReadNamespace
  readonly hyperparameters: HyperparametersReadNamespace = bindReadNamespace({}, 'Hyperparameters', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as HyperparametersReadNamespace
  readonly delegation: DelegationReadNamespace = bindReadNamespace({}, 'Delegation', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as DelegationReadNamespace
  readonly leasing: LeasingReadNamespace = bindReadNamespace({}, 'Leases & crowdloans', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as LeasingReadNamespace
  readonly collateral: CollateralReadNamespace = bindReadNamespace({}, 'Mining & collateral', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as CollateralReadNamespace
  readonly locks: LocksReadNamespace = bindReadNamespace({}, 'Locks & conviction', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as LocksReadNamespace
  readonly identity: IdentityReadNamespace = bindReadNamespace({}, 'Identity & commitments', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as IdentityReadNamespace
  readonly weights: WeightsReadNamespace = bindReadNamespace({}, 'Weights & bonds', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as WeightsReadNamespace
  readonly prices: PricesReadNamespace = bindReadNamespace({}, 'Prices & swaps', block => block == null || block === this.block ? Promise.resolve(this) : this.client.at(block)) as PricesReadNamespace
  constructor(readonly client: Client, readonly block: number, readonly hash: string) {}
  query(item: Descriptor, params: ScaleValue[] = []) { return this.client.query(item, params, this.hash) }
  queryBatch(item: Descriptor, sets: ScaleValue[][]) { return this.client.queryBatch(item, sets, this.hash) }
  queryMap(item: Descriptor, params: ScaleValue[] = []) { return this.client.queryMap(item, params, this.hash) }
  runtime(item: Descriptor, params: ScaleValue[] = []) { return this.client.runtime(item, params, this.hash) }
  constant(item: Descriptor) { return this.client.constant(item, this.hash) }
  async at(block?: number | null): Promise<Snapshot> { return block == null || block === this.block ? this : this.client.at(block) }
  balance(rao: BalanceLike, netuid = 0, symbol?: string | null) { return this.client.balance(rao, netuid, symbol) }
  timestamp() { return this.client.timestamp(this.hash) }
  blockTime() { return this.client.blockTime(this.hash) }
  async isFastBlocks() { return await this.blockTime() === 0.25 }
  blockInfo(block?: number | null) { return this.client.blockInfo(block == null ? this.hash : block) }
  read<N extends ReadName>(name: N, ...args: ReadArgs<N>): Promise<ReadResult<N>> { return dispatchRead(this, name, (args[0] ?? {}) as ReadParams[N]) }
  metagraph(netuid: number, options: { commitments?: boolean } = {}) { return fetchMetagraph(this, netuid, options) }
}
export { Client as BrowserChainClient, Client as SubtensorClient }
function first(value: unknown): unknown { return Array.isArray(value) ? value[0] : value }
function storageArgs(pallet: string | Descriptor, item: string | ScaleValue[], params: ScaleValue[] | BlockRef, block: BlockRef): [string, string, ScaleValue[], BlockRef] { return typeof pallet === 'string' ? [pallet, item as string, params as ScaleValue[] ?? [], block] : [pallet[0], pallet[1], item as ScaleValue[], params as BlockRef] }
function decodeStorage(runtime: Runtime, pallet: string, item: string, raw?: string | null) { const entry = runtime.storageEntry(pallet, item); return raw == null ? entry.modifier === 'Optional' ? null : runtime.decode(entry.valueType, entry.defaultBytes, false) : runtime.decode(entry.valueType, hexBytes(raw), false) }
function signatureBytes(response: any): Uint8Array { const raw = response?.signature ?? response; if (typeof raw === 'string') return hexBytes(raw); if (raw instanceof Uint8Array) return raw; throw new ChainError('signer did not return a signature') }

function pinSigner(signer: BrowserSigner): BrowserSigner {
  const address = signer.ss58Address ?? signer.address
  if (!address) throw new TypeError('signer must expose an address')
  const publicKey = publicKeyFromSs58(address), cryptoType = signer.cryptoType ?? 1
  const genesisHash = signer.genesisHash?.toLowerCase() ?? null
  const requiresMetadataHash = Boolean(signer.requiresMetadataHash), requiresMetadataProof = Boolean(signer.requiresMetadataProof)
  const assertIdentity = () => {
    const current = signer.ss58Address ?? signer.address
    if (!current || hex(publicKeyFromSs58(current)) !== hex(publicKey) || (signer.cryptoType ?? 1) !== cryptoType ||
      signer.publicKey != null && hex(signer.publicKey) !== hex(publicKey) ||
      (signer.genesisHash?.toLowerCase() ?? null) !== genesisHash || Boolean(signer.requiresMetadataHash) !== requiresMetadataHash || Boolean(signer.requiresMetadataProof) !== requiresMetadataProof) {
      throw new ChainError('signer identity changed while the transaction was pending')
    }
  }
  assertIdentity()
  const pinned: BrowserSigner = { address, ss58Address: address, publicKey, cryptoType, genesisHash, requiresMetadataHash, requiresMetadataProof }
  for (const name of ['sign', 'signBytes', 'signPayload', 'signRaw'] as const) {
    const method = signer[name]
    if (method) pinned[name] = (async (...args: any[]) => {
      assertIdentity()
      const response = await (method as (...args: any[]) => unknown).apply(signer, args)
      assertIdentity()
      return response
    }) as any
  }
  return Object.freeze(pinned)
}

function shieldOptions(options: SubmitOptions): SubmitOptions {
  if (options.period === null || (options.period != null && (!Number.isSafeInteger(options.period) || options.period < 4 || options.period > MEV_SHIELD_ERA_PERIOD))) throw new RangeError('shielded extrinsics require a mortal era of 4 to 8 blocks')
  return { ...options, period: options.period ?? MEV_SHIELD_ERA_PERIOD }
}

function rejectDetachedPolicy(options: SubmitOptions): void {
  if (options.policy && Object.values(options.policy).some(value => value != null)) throw new ChainError('detached bytes cannot establish a semantic policy; use execute with an Intent')
}
