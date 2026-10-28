import { serializeUnsignedTransaction, restoreUnsignedTransaction, type SerializedUnsignedTransaction, type UnsignedTransactionState, type UnsignedRuntime } from './unsigned-transaction'
import { ProtectedWalletClient } from './protected-transactions'
import type { ProtectedKeypair } from './protected-accounts'
import { verifyAuthorization } from './keys'
import { resolveEvmRecipient } from './evm-alias'
import { sealMevShieldTransaction } from './crypto'
import { followShieldedResult } from './shielded-receipt'
import { createNodeIntentReader } from './node-workflows'
import { classifyChainError, chainErrorFromDispatch, type ChainErrorInfo } from './chain-errors'
import { nestedDispatchError } from './receipt'
import { WorkflowClient, type WorkflowOptions, type WorkflowPlan, type WorkflowSigner, type WorkflowResult } from './node-workflows'
import { runSignerQueue, signerQueueOptions, bindSignerIdentity } from './signer-queue'
import { listIntentTools, type Intent } from './intents'
import { createNodeUpgradeClient } from './node-upgrade'
import type { UpgradeClient } from './upgrade'
import { dispatchRead, listReads, type ReadName, type ReadParams, type ReadResult, type ReadArgs, type ReadView } from './reads'
import { fetchMetagraph, fetchCommitments } from './metagraph'
import { bindReadNamespace, type AccountsReadNamespace, type ChainReadNamespace, type SubnetsReadNamespace, type StakingReadNamespace, type EpochsReadNamespace, type HyperparametersReadNamespace, type DelegationReadNamespace, type NeuronsReadNamespace, type LeasingReadNamespace, type CollateralReadNamespace, type LocksReadNamespace, type IdentityReadNamespace, type WeightsReadNamespace, type PricesReadNamespace, type ReadBlock } from './read-namespaces'
import { storage as generatedStorage, constants as generatedConstants, runtimeApi as generatedRuntimeApi, calls as generatedCalls, callDescriptors } from './descriptors'
import { DEFAULT_REQUEST_TIMEOUT_MS, DEFAULT_MAX_REQUEST_RETRIES, DEFAULT_RETRY_BACKOFF_MS, DEFAULT_MAX_RETRY_BACKOFF_MS, DEFAULT_ENDPOINT_VALIDATION_TTL_MS, DEFAULT_MAX_SUBSCRIPTION_QUEUE, DEFAULT_MAX_WS_MESSAGE_BYTES, IDEMPOTENT_RPC_METHODS, RpcRequestOptions, JsonRpcTransportOptions, RpcRequest, SubscriptionWaiter, SubscriptionState, EndpointAttempt, ActiveConnection, ConnectingAttempt, SubscriptionOptions, WebSocketLike, WebSocketConstructor, WebSocketFactory, EndpointRequest, EndpointValidator, JsonRpcError, ChainError, RequestTimeoutError, RequestAbortedError, EndpointValidationError, JsonRpcTransport, nonNegativeNumber, nonNegativeInteger, hasOwn, parseJsonRpcResponse, readResponseText, jsonRpcResponseResult, jsonRpcSubscriptionNotification, RequestSignal, delay, withRequestSignal, normalizeAbortError, throwIfAborted } from './transport'
export { JsonRpcTransport, EndpointValidationError, RequestAbortedError, RequestTimeoutError, ChainError, JsonRpcError, EndpointValidator, EndpointRequest, WebSocketFactory, WebSocketConstructor, WebSocketLike, JsonRpcTransportOptions, RpcRequestOptions, DEFAULT_MAX_WS_MESSAGE_BYTES, DEFAULT_MAX_SUBSCRIPTION_QUEUE, DEFAULT_ENDPOINT_VALIDATION_TTL_MS, DEFAULT_MAX_RETRY_BACKOFF_MS, DEFAULT_RETRY_BACKOFF_MS, DEFAULT_MAX_REQUEST_RETRIES, DEFAULT_REQUEST_TIMEOUT_MS } from './transport'
import { blake2_256, generateExtrinsicProof, hexToBytes, metadataDigest } from './crypto'
import { CRYPTO_ED25519, CRYPTO_SR25519, Keypair, publicKeyFromSs58, ss58FromPublic, verify } from './keys'
import { LedgerDevice } from './ledger'
import { Runtime, decodeOptionalOpaqueMetadata, eraBirth, type RuntimeSignerPayload } from './runtime'
import { IntentCall, NativeChainClient, Policy, createNativeCancellationToken, isIntentCall, rawCall, type NativeCancellationToken, type PolicyOptions, type SignerRoleLike } from './transaction'
import { WIRE_TAG, fromWire, toBigInt, toBuffer } from './wire'
import type { NativeChainInfo, NativeExternalSigningOptions, NativeExternalSigningPlanHandle, NativeSubmitOptions, NativeTxOutcome } from './native'
import {
  Balance,
  UnitMismatchError,
  type AssetId,
  type BalanceLike,
  type TransactionAmount,
  assetIdValue,
  brandedAmountNetuid,
  brandedAmountUnit,
  taoTransactionAmountRao,
  transactionAmountRao,
} from './balance'
import type {
  ByteLike,
  ChainInfo,
  ScaleValue,
  SignedExtrinsic,
  StorageEntry,
  SubnetHyperparameters,
  TransactionParams,
} from './types'

export const SS58_FORMAT = 42
export const DEFAULT_ERA_PERIOD = 128
export const DEFAULT_HEAD_RUNTIME_TTL_MS = 12_000
export const DEFAULT_HISTORICAL_RUNTIME_CACHE_SIZE = 64
const V15_METADATA_VERSION_HEX = '0x0f000000'
const V15_METADATA_MISSING_NEEDLE = 'Exported method Metadata_metadata_at_version is not found'
export const NETWORKS = Object.freeze({
  finney: 'wss://entrypoint-finney.opentensor.ai:443',
  test: 'wss://test.finney.opentensor.ai:443',
  archive: 'wss://archive.chain.opentensor.ai:443',
  devnet: 'wss://dev.chain.opentensor.ai:443',
  local: process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944',
})
export const FALLBACK_ENDPOINTS: Readonly<Record<string, readonly string[]>> = Object.freeze({ finney: ['wss://lite.chain.opentensor.ai:443', 'wss://lite.sub.latent.to:443'], archive: ['wss://archive.sub.latent.to:443'] })
export const ARCHIVE_ENDPOINTS: Readonly<Record<string, readonly string[]>> = Object.freeze({ finney: ['wss://archive.chain.opentensor.ai:443', 'wss://archive.sub.latent.to:443'], archive: ['wss://archive.sub.latent.to:443'] })
export const NETWORK_GENESIS_HASHES = Object.freeze({
  finney: '0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03',
  archive: '0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03',
})

export type NetworkName = keyof typeof NETWORKS
export type Descriptor = readonly [string, string]
export type ServeIp = bigint | number | string
export type CallLike =
  | IntentCall
  | readonly [string, string, ScaleValue?]
  | { pallet?: string; module?: string; call?: string; function?: string; params?: ScaleValue }
  | ByteLike

export interface ClientOptions {
  endpoint?: string
  fallbackEndpoints?: string[]
  archiveEndpoints?: string[]
  expectedGenesisHash?: string
  retryForever?: boolean
  autoConnect?: boolean
  webSocket?: WebSocketConstructor
  webSocketConstructor?: WebSocketConstructor
  webSocketFactory?: WebSocketFactory
  headRuntimeTtlMs?: number
  historicalRuntimeCacheSize?: number
  requestTimeoutMs?: number
  maxRequestRetries?: number
  retryBackoffMs?: number
  maxRetryBackoffMs?: number
  endpointValidationTtlMs?: number
  maxSubscriptionQueue?: number
  maxMessageBytes?: number
  validateDescriptorSchema?: boolean
}

export interface SubmitOptions {
  nonce?: number
  period?: number | null
  tip?: TransactionAmount
  tipAssetId?: AssetId | null
  metadataHash?: ByteLike | null
  policy?: Policy | PolicyOptions | null
  allowRawCall?: boolean
  rawSignerRole?: SignerRoleLike
  waitForInclusion?: boolean
  waitForFinalization?: boolean
  timeoutMs?: number
  signal?: AbortSignal
}

export interface QueryMapOptions {
  pageSize?: number
  maxPages?: number
  maxResults?: number
  signal?: AbortSignal
}

export interface SignerAccountContext {
  client: ChainClient
  runtime: Runtime
  ss58Format: number
}

export interface SignerAccount {
  ss58Address?: string
  address?: string
  publicKey?: ByteLike
  cryptoType?: number
  requiresMetadataProof?: boolean
  requiresMetadataHash?: boolean
}

export interface SignerPayloadContext {
  client: ChainClient
  runtime: Runtime
  address: string
  publicKey: Buffer
  cryptoType: number
  callData: Buffer
  payload: Buffer
  txParams: TransactionParams
  metadataHash: Buffer | null
  metadataProof?: Buffer
  proof?: Buffer
  includedInExtrinsic?: Buffer
  includedInSignedData?: Buffer
  chainInfo?: ChainInfo
  signerPayload?: ExtensionSignPayloadRequest
}

export interface ExtensionSignRawRequest {
  address: string
  data: string
  type: 'bytes'
  payload: Buffer
  metadataHash?: string
  metadataProof?: Buffer
  proof?: Buffer
  chainInfo?: ChainInfo
}

export interface ExtensionSignPayloadRequest extends RuntimeSignerPayload {}

export type SignerSignature =
  | ByteLike
  | string
  | { signature: ByteLike | string }

export interface ChainSigner {
  readonly ss58Address?: string
  readonly address?: string
  readonly publicKey?: ByteLike
  readonly cryptoType?: number
  readonly requiresMetadataProof?: boolean
  readonly requiresMetadataHash?: boolean
  getAccount?(context: SignerAccountContext): SignerAccount | Promise<SignerAccount>
  signBytes?(
    payload: ByteLike,
    context: SignerPayloadContext,
  ): SignerSignature | Promise<SignerSignature>
  sign?(
    payload: ByteLike,
    context?: SignerPayloadContext,
  ): SignerSignature | Promise<SignerSignature>
  signRaw?(
    request: ExtensionSignRawRequest,
  ): SignerSignature | Promise<SignerSignature>
  signPayload?(
    payload: ExtensionSignPayloadRequest,
    context: SignerPayloadContext,
  ): SignerSignature | Promise<SignerSignature>
}

export type SignerLike = Keypair | ChainSigner | LedgerDevice
export type ExtrinsicStatus = 'submitted' | 'inBlock' | 'finalized' | 'failed' | 'unknown'

export interface ExtrinsicResult {
  status: ExtrinsicStatus
  success?: boolean
  message: string
  extrinsicHash: string
  blockHash?: string
  blockNumber?: number
  extrinsicIndex?: number
  extrinsicId?: string
  finalized?: boolean
  fee?: Balance
  events: unknown[]
  error?: unknown
  chainError?: ChainErrorInfo
}

export interface SignedExtrinsicResult extends SignedExtrinsic {
  hex: string
  signerAddress: string
  nonce: number
}

export interface ExtrinsicWatcher {
  readonly extrinsicHash: string
  readonly result: Promise<ExtrinsicResult>
  unsubscribe(): Promise<void>
}

export interface BlockHeader {
  number: number
  parentHash?: string
  hash?: string
  raw: unknown
}

export interface BlockInfo {
  number: number
  hash: string
  header: unknown
  extrinsics: string[]
  decodedExtrinsics?: Array<ScaleValue | null>
  timestamp?: Date
}

export interface DescriptorSchemaIssue {
  kind: 'storage' | 'constant' | 'runtimeApi' | 'call'
  path: string
  descriptor: Descriptor
  message: string
}

interface ResolvedSigner {
  signer: Keypair | ChainSigner
  ss58Address: string
  publicKey: Buffer
  cryptoType: number
  requiresMetadataProof: boolean
}

export interface PreparedCall extends SignerPayloadContext {}
const preparedCalls = new WeakMap<PreparedCall, { owner: Client; planned: NativeSigningPlan }>()
const importedCalls = new WeakMap<PreparedCall, { owner: Client; serialized: SerializedUnsignedTransaction }>()

interface NativeSigningPlan {
  native: NativeChainClient
  runtime: Runtime
  resolved: ResolvedSigner
  plan: NativeExternalSigningPlanHandle
}

interface RuntimeVersionInfo {
  specVersion: number
  transactionVersion: number
}

interface RuntimeCacheEntry extends RuntimeVersionInfo {
  runtime: Runtime
  ss58Format: number
}

interface HeadRuntimeCacheEntry extends RuntimeCacheEntry {
  expiresAtMs: number
}

interface SigningSnapshot {
  blockHash: string
  blockNumber: number
  runtime: Runtime
  genesisHash: string
}

export type ChainClient = Client | BrowserChainClient

const nativeClientAccess = new WeakMap<object, () => Promise<NativeChainClient | undefined>>()

export function nativeRustClient(client: object): Promise<NativeChainClient | undefined> {
  return nativeClientAccess.get(client)?.() ?? Promise.resolve(undefined)
}

interface NormalizedSignature {
  signature: Buffer
  cryptoType: number
}

export class BrowserChainClient {
  readonly network: string
  readonly endpoint: string
  readonly transport: JsonRpcTransport
  readonly balances: BalancesNamespace & AccountsReadNamespace
  readonly subnets: SubnetsNamespace & SubnetsReadNamespace
  readonly chain: ChainReadNamespace
  readonly epochs: EpochsReadNamespace
  readonly hyperparameters: HyperparametersReadNamespace
  readonly delegation: DelegationReadNamespace
  readonly leasing: LeasingReadNamespace
  readonly collateral: CollateralReadNamespace
  readonly locks: LocksReadNamespace
  readonly identity: IdentityReadNamespace
  readonly weights: WeightsReadNamespace
  readonly prices: PricesReadNamespace
  readonly neurons: NeuronsNamespace & NeuronsReadNamespace
  readonly staking: StakingNamespace & StakingReadNamespace
  ready?: Promise<this>

  private readonly headRuntimeTtlMs: number
  private readonly historicalRuntimeCacheSize: number
  private readonly expectedGenesisHash?: string
  private headRuntimeCache?: HeadRuntimeCacheEntry
  private runtimesBySpecVersion = new Map<number, RuntimeCacheEntry>()
  private historicalRuntimeCache = new Map<string, RuntimeCacheEntry>()
  protected genesis?: string
  private readonly validateDescriptorsOnLoad: boolean

  constructor(network: string = 'finney', options: ClientOptions = {}) {
    const [label, endpoint] = resolveEndpoint(options.endpoint ?? network)
    const fallbackEndpoints = options.fallbackEndpoints ?? []
    const expectedGenesisHash = normalizeGenesisHash(
      options.expectedGenesisHash ?? trustedGenesisHash(label),
    )
    if (fallbackEndpoints.length > 0 && expectedGenesisHash == null) {
      throw new ChainError(
        'fallbackEndpoints require expectedGenesisHash for custom or untrusted networks',
      )
    }
    this.network = label
    this.endpoint = endpoint
    this.expectedGenesisHash = expectedGenesisHash
    this.genesis = expectedGenesisHash
    this.validateDescriptorsOnLoad = options.validateDescriptorSchema ?? false
    this.transport = new JsonRpcTransport(endpoint, fallbackEndpoints, options.retryForever, {
      webSocket: options.webSocket,
      webSocketConstructor: options.webSocketConstructor,
      webSocketFactory: options.webSocketFactory,
      validateEndpoint: (activeEndpoint, request) => this.validateEndpointGenesis(activeEndpoint, request),
      requestTimeoutMs: options.requestTimeoutMs,
      maxRequestRetries: options.maxRequestRetries,
      retryBackoffMs: options.retryBackoffMs,
      maxRetryBackoffMs: options.maxRetryBackoffMs,
      endpointValidationTtlMs: options.endpointValidationTtlMs,
      maxSubscriptionQueue: options.maxSubscriptionQueue,
      maxMessageBytes: options.maxMessageBytes,
    })
    this.balances = bindReadNamespace(new BalancesNamespace(this), 'Accounts & keys', block => scopedReadView(this, block)) as BalancesNamespace & AccountsReadNamespace
    this.subnets = bindReadNamespace(new SubnetsNamespace(this), 'Subnets', block => scopedReadView(this, block)) as SubnetsNamespace & SubnetsReadNamespace
    this.neurons = bindReadNamespace(new NeuronsNamespace(this), 'Neurons & registration', block => scopedReadView(this, block)) as NeuronsNamespace & NeuronsReadNamespace
    this.staking = bindReadNamespace(new StakingNamespace(this), 'Staking', block => scopedReadView(this, block)) as StakingNamespace & StakingReadNamespace
    this.chain = bindReadNamespace({}, 'Chain', block => scopedReadView(this, block)) as ChainReadNamespace
    this.epochs = bindReadNamespace({}, 'Epochs & timing', block => scopedReadView(this, block)) as EpochsReadNamespace
    this.hyperparameters = bindReadNamespace({}, 'Hyperparameters', block => scopedReadView(this, block)) as HyperparametersReadNamespace
    this.delegation = bindReadNamespace({}, 'Delegation', block => scopedReadView(this, block)) as DelegationReadNamespace
    this.leasing = bindReadNamespace({}, 'Leases & crowdloans', block => scopedReadView(this, block)) as LeasingReadNamespace
    this.collateral = bindReadNamespace({}, 'Mining & collateral', block => scopedReadView(this, block)) as CollateralReadNamespace
    this.locks = bindReadNamespace({}, 'Locks & conviction', block => scopedReadView(this, block)) as LocksReadNamespace
    this.identity = bindReadNamespace({}, 'Identity & commitments', block => scopedReadView(this, block)) as IdentityReadNamespace
    this.weights = bindReadNamespace({}, 'Weights & bonds', block => scopedReadView(this, block)) as WeightsReadNamespace
    this.prices = bindReadNamespace({}, 'Prices & swaps', block => scopedReadView(this, block)) as PricesReadNamespace
    this.headRuntimeTtlMs = nonNegativeNumber(options.headRuntimeTtlMs, DEFAULT_HEAD_RUNTIME_TTL_MS)
    this.historicalRuntimeCacheSize = nonNegativeInteger(
      options.historicalRuntimeCacheSize,
      DEFAULT_HISTORICAL_RUNTIME_CACHE_SIZE,
    )
    if (options.autoConnect) {
      this.ready = this.connect()
      this.ready.catch(() => undefined)
    }
  }

  async connect(): Promise<this> {
    await this.runtimeAt()
    return this
  }

  async close(): Promise<void> {
    this.transport.close()
  }

  async block(): Promise<number> {
    return this.blockNumber()
  }

  getCurrentBlock(): Promise<number> {
    return this.blockNumber()
  }

  get_current_block(): Promise<number> {
    return this.getCurrentBlock()
  }

  async finalizedBlock(): Promise<number> { return this.blockNumber(await this.finalizedHead()) }
  finalized_block(): Promise<number> { return this.finalizedBlock() }
  async specVersion(): Promise<number> { return (await this.runtimeAt()).specVersion }
  spec_version(): Promise<number> { return this.specVersion() }
  async subnetHyperparametersRaw(netuid: number, block?: number | string | null): Promise<Record<string, ScaleValue> | null> { return dispatchRead(await scopedReadView(this, block), 'subnet_hyperparameters', { netuid }) }

  async blockNumber(blockHash?: string | null): Promise<number> {
    const header = await this.rpc('chain_getHeader', blockHash == null ? [] : [blockHash])
    return headerNumber(header)
  }

  async blockHash(block?: number | null): Promise<string> {
    return String(await this.rpc('chain_getBlockHash', block == null ? [] : [block]))
  }

  getBlockHash(block?: number | null): Promise<string> {
    return this.blockHash(block)
  }

  get_block_hash(block?: number | null): Promise<string> {
    return this.blockHash(block)
  }

  async finalizedHead(): Promise<string> {
    return String(await this.rpc('chain_getFinalizedHead'))
  }

  async genesisHash(): Promise<string> {
    this.genesis ??= await this.blockHash(0)
    return this.genesis
  }

  private async validateEndpointGenesis(endpoint: string, request: EndpointRequest): Promise<void> {
    const genesis = String(await request('chain_getBlockHash', [0]))
    const expected = this.expectedGenesisHash ?? this.genesis
    if (expected == null) {
      this.genesis = genesis
      return
    }
    if (!sameHex(genesis, expected)) {
      throw new EndpointValidationError(
        `endpoint ${endpoint} genesis ${genesis} does not match expected genesis ${expected}`,
      )
    }
    this.genesis = expected
  }

  async runtimeAt(block?: number | string | null): Promise<Runtime> {
    const blockHash = await this.resolveBlockHash(block)
    return blockHash == null ? this.headRuntime() : this.historicalRuntimeAt(blockHash)
  }

  invalidateRuntimeCache(): void {
    this.headRuntimeCache = undefined
  }

  private async headRuntime(): Promise<Runtime> {
    const now = Date.now()
    if (this.headRuntimeCache != null && this.headRuntimeCache.expiresAtMs > now) {
      return this.headRuntimeCache.runtime
    }

    const [version, ss58Format] = await this.runtimeVersionAndSs58(null)
    if (this.headRuntimeCache != null && sameRuntimeVersion(this.headRuntimeCache, version, ss58Format)) {
      this.headRuntimeCache.expiresAtMs = Date.now() + this.headRuntimeTtlMs
      this.cacheRuntimeBySpecVersion(this.headRuntimeCache)
      return this.headRuntimeCache.runtime
    }

    this.invalidateRuntimeCache()
    const entry = await this.runtimeForVersion(version, ss58Format, null)
    this.headRuntimeCache = { ...entry, expiresAtMs: Date.now() + this.headRuntimeTtlMs }
    return entry.runtime
  }

  private async historicalRuntimeAt(blockHash: string): Promise<Runtime> {
    const cached = this.historicalRuntime(blockHash)
    if (cached != null) return cached.runtime

    const [version, ss58Format] = await this.runtimeVersionAndSs58(blockHash)
    const entry = await this.runtimeForVersion(version, ss58Format, blockHash)
    this.cacheHistoricalRuntime(blockHash, entry)
    return entry.runtime
  }

  private async runtimeVersionAndSs58(blockHash: string | null): Promise<[RuntimeVersionInfo, number]> {
    const [version, properties] = await Promise.all([
      this.rpc('state_getRuntimeVersion', blockHash == null ? [] : [blockHash]),
      this.rpc('system_properties').catch(() => ({})),
    ])
    const chainProperties = properties as Record<string, unknown>
    return [
      runtimeVersionInfo(version),
      propertyNumber(chainProperties.ss58Format ?? chainProperties.ss58Prefix, SS58_FORMAT),
    ]
  }

  private async runtimeForVersion(
    version: RuntimeVersionInfo,
    ss58Format: number,
    blockHash: string | null,
  ): Promise<RuntimeCacheEntry> {
    const cached = this.runtimesBySpecVersion.get(version.specVersion)
    if (cached != null && sameRuntimeVersion(cached, version, ss58Format)) {
      this.cacheRuntimeBySpecVersion(cached)
      return cached
    }

    const metadataBytes = await this.runtimeMetadata(blockHash)
    const runtime = new Runtime(
      metadataBytes,
      version.specVersion,
      version.transactionVersion,
      ss58Format,
    )
    if (this.validateDescriptorsOnLoad) {
      const descriptorIssues = validateDescriptorSchema(runtime)
      if (descriptorIssues.length > 0) throw descriptorSchemaDriftError(descriptorIssues)
    }
    const entry = { runtime, ss58Format, ...version }
    this.cacheRuntimeBySpecVersion(entry)
    return entry
  }

  private async runtimeMetadata(blockHash: string | null): Promise<Buffer> {
    let v15Result: unknown
    try {
      v15Result = await this.rpc('state_call', [
        'Metadata_metadata_at_version',
        V15_METADATA_VERSION_HEX,
        blockHash,
      ])
    } catch (error) {
      if (!isMetadataAtVersionUnavailable(error)) throw error
      v15Result = null
    }

    if (v15Result != null) {
      const metadata = decodeOptionalOpaqueMetadata(hexToBuffer(String(v15Result)))
      if (metadata != null) return metadata
    }

    const legacy = await this.rpc('state_getMetadata', blockHash == null ? [] : [blockHash])
    if (legacy == null) throw new ChainError(`no metadata for block ${blockHash ?? 'head'}`)
    return hexToBuffer(String(legacy))
  }

  private cacheRuntimeBySpecVersion(entry: RuntimeCacheEntry): void {
    this.runtimesBySpecVersion.delete(entry.specVersion)
    this.runtimesBySpecVersion.set(entry.specVersion, entry)
  }

  private historicalRuntime(blockHash: string): RuntimeCacheEntry | undefined {
    const entry = this.historicalRuntimeCache.get(blockHash)
    if (entry == null) return undefined
    this.historicalRuntimeCache.delete(blockHash)
    this.historicalRuntimeCache.set(blockHash, entry)
    return entry
  }

  private cacheHistoricalRuntime(blockHash: string, entry: RuntimeCacheEntry): void {
    if (this.historicalRuntimeCacheSize <= 0) return
    this.historicalRuntimeCache.delete(blockHash)
    this.historicalRuntimeCache.set(blockHash, entry)
    while (this.historicalRuntimeCache.size > this.historicalRuntimeCacheSize) {
      const oldest = this.historicalRuntimeCache.keys().next().value
      if (oldest == null) break
      this.historicalRuntimeCache.delete(oldest)
    }
  }

  async chainInfo(runtime?: Runtime, blockHash?: string | null): Promise<ChainInfo> {
    const resolvedRuntime = runtime ?? await this.runtimeAt(blockHash)
    const [version, properties] = await Promise.all([
      this.rpc('state_getRuntimeVersion', blockHash == null ? [] : [blockHash]),
      this.rpc('system_properties').catch(() => ({})),
    ])
    const runtimeVersion = version as { specName?: unknown; specVersion?: unknown }
    const chainProperties = properties as Record<string, unknown>
    return {
      specVersion: Number(runtimeVersion.specVersion ?? resolvedRuntime.specVersion),
      specName: String(runtimeVersion.specName ?? 'node-subtensor'),
      base58Prefix: propertyNumber(
        chainProperties.ss58Format ?? chainProperties.ss58Prefix,
        resolvedRuntime.ss58Format,
      ),
      decimals: propertyNumber(
        chainProperties.tokenDecimals ?? chainProperties.decimals,
        9,
      ),
      tokenSymbol: propertyString(
        chainProperties.tokenSymbol ?? chainProperties.symbol,
        'TAO',
      ),
    }
  }

  rpc(method: string, params: unknown[] = []): Promise<unknown> {
    return this.transport.request(method, params)
  }

  async query<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction?: string | ScaleValue[],
    paramsOrBlock?: ScaleValue[] | number | string | null,
    block?: number | string | null,
  ): Promise<T | undefined> {
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const key = runtime.storageKey(moduleName, itemName, itemParams)
    const raw = await this.rpc('state_getStorage', [hex(key), blockHash])
    const entry = runtime.storageEntry(moduleName, itemName)
    return decodeStorageValue<T>(runtime, entry, raw)
  }

  async queryBatch<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction: string | ScaleValue[][],
    paramSetsOrBlock?: ScaleValue[][] | number | string | null,
    block?: number | string | null,
  ): Promise<Array<T | undefined>> {
    const [moduleName, itemName, sets, blockRef] =
      normalizeBatchArgs(pallet, storageFunction, paramSetsOrBlock, block)
    if (sets.length === 0) return []
    const blockHash = await this.resolveReadBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const keys = runtime.storageKeyBatch(moduleName, itemName, sets)
    const raw = await this.rpc('state_queryStorageAt', [keys.map(hex), blockHash])
    const changes = ((raw as Array<{ changes?: Array<[string, string | null]> }>)[0]?.changes ?? [])
    const valueByKey = new Map(changes.map(([key, value]) => [key.toLowerCase(), value]))
    const entry = runtime.storageEntry(moduleName, itemName)
    return keys.map((key) => {
      const value = valueByKey.get(hex(key).toLowerCase())
      return decodeStorageValue<T>(runtime, entry, value)
    })
  }

  async queryMap<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction?: string | ScaleValue[],
    paramsOrBlock?: ScaleValue[] | number | string | null,
    block?: number | string | null,
    pageSizeOrOptions: number | QueryMapOptions = 512,
  ): Promise<Array<[K, V]>> {
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const prefix = runtime.storageKey(moduleName, itemName, itemParams)
    const entry = runtime.storageEntry(moduleName, itemName)
    const options = normalizeQueryMapOptions(pageSizeOrOptions)
    const out: Array<[K, V]> = []
    let startKey: string | null = null
    let pages = 0
    const seenPageStarts = new Set<string>()
    for (;;) {
      throwIfAborted(options.signal)
      if (options.maxPages != null && pages >= options.maxPages) {
        throw new ChainError(`queryMap exceeded maxPages ${options.maxPages}`)
      }
      const pageStart = startKey ?? ''
      if (seenPageStarts.has(pageStart)) throw new ChainError('queryMap pagination did not advance')
      seenPageStarts.add(pageStart)
      const keys = (await this.transport.request('state_getKeysPaged', [
        hex(prefix),
        options.pageSize,
        startKey,
        ...(blockHash == null ? [] : [blockHash]),
      ], { signal: options.signal })) as string[]
      if (keys.length === 0) break
      pages += 1
      const lastKey = keys[keys.length - 1]
      if (lastKey == null || (startKey != null && lastKey.toLowerCase() === startKey.toLowerCase())) {
        throw new ChainError('queryMap pagination did not advance')
      }
      const raw = await this.transport.request('state_queryStorageAt', [keys, ...(blockHash == null ? [] : [blockHash])], { signal: options.signal })
      const changes = ((raw as Array<{ changes?: Array<[string, string | null]> }>)[0]?.changes ?? [])
      const valueByKey = new Map(changes.map(([key, value]) => [key.toLowerCase(), value]))
      for (const key of keys) {
        const value = valueByKey.get(key.toLowerCase())
        if (value == null) continue
        const decodedKey = runtime.decodeStorageKeyParams<K>(moduleName, itemName, hexToBuffer(key), itemParams.length)
        const normalizedKey = (decodedKey.length === 1 ? decodedKey[0] : decodedKey) as K
        out.push([normalizedKey, runtime.decode<V>(entry.valueType, hexToBuffer(value), false)])
        if (options.maxResults != null && out.length >= options.maxResults) return out
      }
      startKey = lastKey
    }
    return out
  }

  queryModule<T extends ScaleValue = ScaleValue>(
    moduleName: string,
    name: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T | undefined> {
    return this.query<T>(moduleName, name, params, block)
  }

  query_module<T extends ScaleValue = ScaleValue>(
    moduleName: string,
    name: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T | undefined> {
    return this.queryModule<T>(moduleName, name, params, block)
  }

  querySubtensor<T extends ScaleValue = ScaleValue>(
    name: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T | undefined> {
    return this.query<T>('SubtensorModule', name, params, block)
  }

  query_subtensor<T extends ScaleValue = ScaleValue>(
    name: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T | undefined> {
    return this.querySubtensor<T>(name, params, block)
  }

  async runtimeCall<T extends ScaleValue = ScaleValue>(
    api: string | Descriptor,
    method?: string | ScaleValue[],
    paramsOrBlock: ScaleValue[] | number | string | null = [],
    block?: number | string | null,
  ): Promise<T> {
    const [apiName, methodName, callParams, blockRef] = normalizeRuntimeArgs(api, method, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const info = runtime.runtimeApis()[apiName]?.[methodName]
    if (info == null) throw new ChainError(`runtime API ${apiName}.${methodName} not found`)
    const encoded = runtime.encodeRuntimeApiInput(apiName, methodName, callParams)
    const raw = await this.rpc('state_call', [`${apiName}_${methodName}`, hex(encoded), blockHash])
    return runtime.decodeTypeId<T>(info.outputTypeId, hexToBuffer(String(raw)), false)
  }

  runtime<T extends ScaleValue = ScaleValue>(
    method: Descriptor,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T> {
    return this.runtimeCall<T>(method, params, block)
  }

  queryRuntimeApi<T extends ScaleValue = ScaleValue>(
    api: string,
    method: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T> {
    return this.runtimeCall<T>(api, method, params, block)
  }

  query_runtime_api<T extends ScaleValue = ScaleValue>(
    api: string,
    method: string,
    params: ScaleValue[] = [],
    block?: number | string | null,
  ): Promise<T> {
    return this.queryRuntimeApi<T>(api, method, params, block)
  }

  async stateCall<T = string>(method: string, data: ByteLike | string, block?: number | string | null): Promise<T> {
    const blockHash = await this.resolveBlockHash(block)
    return (await this.rpc('state_call', [method, typeof data === 'string' ? data : hex(data), blockHash ?? null])) as T
  }

  state_call<T = string>(method: string, data: ByteLike | string, block?: number | string | null): Promise<T> {
    return this.stateCall<T>(method, data, block)
  }

  async constant<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    name?: string,
    block?: number | string | null,
  ): Promise<T | undefined> {
    const [moduleName, constantName] = typeof pallet === 'string' ? [pallet, name as string] : pallet
    return (await this.runtimeAt(block)).constant<T>(moduleName, constantName)
  }

  async decodeScale<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteLike | string,
    block?: number | string | null,
  ): Promise<T> {
    return this.runtimeAt(block).then((runtime) =>
      runtime.decode<T>(typeString, typeof data === 'string' ? hexToBuffer(data) : data, false),
    )
  }

  decode_scale<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteLike | string,
    block?: number | string | null,
  ): Promise<T> {
    return this.decodeScale<T>(typeString, data, block)
  }

  async composeCall(pallet: string, fn: string, params: ScaleValue = {}, block?: number | string | null): Promise<Buffer> {
    return this.runtimeAt(block).then((runtime) => runtime.composeCall(pallet, fn, params))
  }

  compose(call: CallLike, block?: number | string | null): Promise<Buffer> {
    return this.callData(call, block)
  }

  async *blocks(options: { finalized?: boolean } = {}): AsyncIterable<BlockHeader> {
    const subscription = await this.transport.subscribe(
      options.finalized ? 'chain_subscribeFinalizedHeads' : 'chain_subscribeNewHeads',
      [],
      options.finalized ? 'chain_unsubscribeFinalizedHeads' : 'chain_unsubscribeNewHeads',
    )
    try {
      for await (const raw of subscription) yield normalizeHeader(raw)
    } finally {
      await subscription.unsubscribe()
    }
  }

  private async waitForObservedHeader<T>(predicate: (header: BlockHeader) => Promise<T | undefined>, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<T> {
    const request = withRequestSignal({ timeoutMs: options.timeoutMs ?? 0, signal: options.signal })
    let subscription: (AsyncIterable<unknown> & { unsubscribe(): Promise<void> }) | undefined
    try {
      subscription = await this.transport.subscribe('chain_subscribeNewHeads', [], 'chain_unsubscribeNewHeads', { signal: request.signal })
      const aborted = new Promise<never>((_, reject) => request.onAbort(reject))
      const observe = async () => {
        for await (const raw of subscription!) {
          throwIfAborted(request.signal)
          const result = await predicate(normalizeHeader(raw))
          if (result !== undefined) return result
        }
        throw new ChainError('block subscription ended before the target condition')
      }
      return await Promise.race([observe(), aborted])
    } catch (error) { throw normalizeAbortError(error, request) }
    finally { request.cleanup(); await subscription?.unsubscribe() }
  }

  async waitForBlock(block?: number | null, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> {
    const target = block ?? (await this.blockNumber()) + 1
    nonNegativeSafeInteger(target, 'block')
    return this.waitForObservedHeader(async header => header.number >= target ? header : undefined, options)
  }

  wait_for_block(block?: number | null, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> { return this.waitForBlock(block, options) }

  async waitForTimestamp(when: Date | string | number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> {
    const value = typeof when === 'string' && !/(?:Z|[+-]\d{2}:?\d{2})$/i.test(when) ? when + 'Z' : when
    const target = typeof value === 'number' ? value * 1000 : value instanceof Date ? value.getTime() : Date.parse(value)
    if (!Number.isFinite(target)) throw new TypeError('when must be a Date, ISO-8601 string or unix timestamp in seconds')
    return this.waitForObservedHeader(async header => (await this.timestamp(header.hash ?? header.number)).getTime() >= target ? header : undefined, options)
  }

  wait_for_timestamp(when: Date | string | number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> { return this.waitForTimestamp(when, options) }

  async waitForEpoch(netuid: number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<{ netuid: number; block: number; previous: number }> {
    nonNegativeSafeInteger(netuid, 'netuid')
    const snapshot = await this.at()
    const tempo = Number(await snapshot.query(storage.SubtensorModule.Tempo, [netuid]) ?? 0)
    if (tempo === 0) throw new ChainError(`subnet ${netuid} has tempo 0 and never runs epochs`)
    const previous = Number(await snapshot.query(storage.SubtensorModule.LastEpochBlock, [netuid]) ?? 0)
    return this.waitForObservedHeader(async header => {
      const block = Number(await this.query(storage.SubtensorModule.LastEpochBlock, [netuid], header.hash ?? header.number) ?? 0)
      return block > previous ? { netuid, block, previous } : undefined
    }, options)
  }

  wait_for_epoch(netuid: number, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<{ netuid: number; block: number; previous: number }> { return this.waitForEpoch(netuid, options) }

  async timestamp(block?: number | string | null): Promise<Date> {
    const ms = await this.query<bigint | number>(storage.Timestamp.Now, [], block)
    return new Date(Number(ms ?? 0))
  }

  async blockInfo(block?: number | null): Promise<BlockInfo | null> {
    const hash = block == null ? await this.finalizedHead() : await this.blockHash(block)
    const raw = await this.rpc('chain_getBlock', [hash])
    const value = raw as { block?: { header?: { number?: string; parentHash?: string }; extrinsics?: string[] } }
    if (value.block?.header == null) return null
    const number = headerNumber(value.block.header)
    const runtime = await this.runtimeAt(number === 0 ? hash : value.block.header.parentHash ?? hash)
    const extrinsics = value.block.extrinsics ?? []
    const decodedExtrinsics = extrinsics.map(encoded => {
      try { return runtime.decodeExtrinsic(hexToBuffer(encoded)) }
      catch { return null }
    })
    return { number, hash, header: value.block.header, extrinsics, decodedExtrinsics, timestamp: await this.timestamp(hash) }
  }

  block_info(block?: number | null): Promise<BlockInfo | null> {
    return this.blockInfo(block)
  }

  async at(block?: number | null): Promise<Snapshot> {
    const hash = block == null ? await this.finalizedHead() : await this.blockHash(block)
    const number = block ?? await this.blockNumber(hash)
    return new Snapshot(this, number, hash)
  }

  balance(rao: BalanceLike, netuid = 0, symbol?: string | null): Balance {
    return Balance.fromRao(rao, netuid, symbol)
  }

  read<Name extends ReadName>(name: Name, ...args: ReadArgs<Name>): Promise<ReadResult<Name>>
  read(name: string, params?: Record<string, unknown>): Promise<unknown>
  read(name: string, params: Record<string, unknown> = {}): Promise<unknown> {
    return dispatchRead(this, name, params)
  }

  reads() { return listReads() }

  async blockTime(block?: ReadBlock): Promise<number> {
    const hash = await this.resolveReadBlockHash(block)
    const runtime = await this.runtimeAt(hash)
    const slot = runtime.constant('Aura', 'SlotDuration')
    if (slot != null) return Number(slot) / 1000
    if (runtime.runtimeApis().AuraApi?.slot_duration != null) return Number(await this.runtime(['AuraApi', 'slot_duration'], [], hash)) / 1000
    const minimum = runtime.constant('Timestamp', 'MinimumPeriod')
    if (minimum != null && Number(minimum) > 0) return Number(minimum) * 2 / 1000
    throw new ChainError('block duration is unavailable in runtime metadata')
  }
  block_time(block?: ReadBlock): Promise<number> { return this.blockTime(block) }
  async isFastBlocks(block?: ReadBlock): Promise<boolean> { return await this.blockTime(block) === 0.25 }
  is_fast_blocks(block?: ReadBlock): Promise<boolean> { return this.isFastBlocks(block) }


  async validateDescriptorSchema(block?: number | string | null): Promise<DescriptorSchemaIssue[]> {
    const blockHash = await this.resolveBlockHash(block)
    return validateDescriptorSchema(await this.runtimeAt(blockHash))
  }

  validate_descriptor_schema(block?: number | string | null): Promise<DescriptorSchemaIssue[]> {
    return this.validateDescriptorSchema(block)
  }

  async assertDescriptorSchema(block?: number | string | null): Promise<void> {
    const issues = await this.validateDescriptorSchema(block)
    if (issues.length === 0) return
    throw descriptorSchemaDriftError(issues)
  }

  assert_descriptor_schema(block?: number | string | null): Promise<void> {
    return this.assertDescriptorSchema(block)
  }

  async signExtrinsic(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<SignedExtrinsicResult> {
    this.assertExplicitNonce(options, 'signExtrinsic')
    return this.signExtrinsicWithSnapshot(call, signer, options, await this.signingSnapshot())
  }

  private async signExtrinsicWithSnapshot(
    call: CallLike,
    signer: SignerLike,
    options: SubmitOptions,
    snapshot: SigningSnapshot,
  ): Promise<SignedExtrinsicResult> {
    const { runtime } = snapshot
    const prepared = this.prepareCallForSigning(call, runtime, options)
    const callData = prepared.callData
    const resolved = await this.resolveSigner(signer, runtime)
    const nonce = this.explicitNonce(options, 'signExtrinsic')
    const period = options.period === undefined ? DEFAULT_ERA_PERIOD : options.period
    const { era, eraBlockHash } = await this.normalizeEra(period, snapshot)
    const tip = taoTransactionAmountRao(options.tip ?? 0n, 'tip')
    const tipAssetId = options.tipAssetId == null ? null : assetIdValue(options.tipAssetId, 'tipAssetId')
    const metadataHashOptionProvided = hasOwn(options, 'metadataHash')
    const defaultMetadataHash = resolved.requiresMetadataProof
    if (metadataHashOptionProvided && options.metadataHash == null && defaultMetadataHash) {
      throw new ChainError(
        'metadataHash cannot be disabled for a signer that requires metadata proof',
      )
    }
    const chainInfo = resolved.requiresMetadataProof || (!metadataHashOptionProvided && defaultMetadataHash)
      ? await this.chainInfo(runtime, snapshot.blockHash)
      : undefined
    const metadataHash =
      metadataHashOptionProvided
        ? options.metadataHash == null
          ? null
          : toBuffer(options.metadataHash, 'metadataHash')
        : defaultMetadataHash
          ? metadataDigest(runtime.metadataBytes, chainInfo!)
          : null
    const txParams = {
      era,
      nonce,
      tip,
      tipAssetId,
      genesisHash: hexToBuffer(snapshot.genesisHash),
      eraBlockHash: hexToBuffer(eraBlockHash),
      metadataHash,
    }
    const proofParts = resolved.requiresMetadataProof
      ? runtime.signaturePayloadParts(txParams)
      : undefined
    const metadataProof = proofParts == null
      ? undefined
      : generateExtrinsicProof(
          callData,
          proofParts.includedInExtrinsic,
          proofParts.includedInSignedData,
          runtime.metadataBytes,
          chainInfo!,
        )
    const payload = runtime.signaturePayload(callData, txParams)
    const context: SignerPayloadContext = {
      client: this,
      runtime,
      address: resolved.ss58Address,
      publicKey: resolved.publicKey,
      cryptoType: resolved.cryptoType,
      callData,
      payload,
      txParams,
      metadataHash,
      metadataProof,
      proof: metadataProof,
      includedInExtrinsic: proofParts?.includedInExtrinsic,
      includedInSignedData: proofParts?.includedInSignedData,
      chainInfo,
    }
    const signedBySigner = await this.signWithSigner(resolved.signer, payload, context)
    const signed = runtime.encodeSignedExtrinsic(callData, resolved.publicKey, signedBySigner.signature, signedBySigner.cryptoType, {
      era,
      nonce,
      tip,
      tipAssetId,
      metadataHashEnabled: metadataHash != null,
    })
    return {
      ...signed,
      hex: hex(signed.bytes),
      signerAddress: resolved.ss58Address,
      nonce,
    }
  }

  async submit(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    this.assertExplicitNonce(options, 'submit')
    const signed = await this.signExtrinsic(call, signer, options)
    return this.submitSigned(signed, signed.signerAddress, options)
  }

  async submitSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    signerAddress?: string,
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    if (options.policy != null) assertRawBytesAllowed(options)
    void signerAddress
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    if (options.waitForInclusion || options.waitForFinalization) {
      const watcher = await this.watchSigned(extrinsic, {
        signerAddress,
        waitForFinalization: options.waitForFinalization ?? false,
        timeoutMs: options.timeoutMs,
        signal: options.signal,
      })
      return await watcher.result
    }
    let hash: string
    try {
      hash = String(await this.transport.request('author_submitExtrinsic', [hex(bytes)], {
        timeoutMs: options.timeoutMs,
        signal: options.signal,
        maxRetries: 0,
        retryForever: false,
      }))
    } catch (error) {
      throw error
    }
    let returnedHash: string
    try {
      returnedHash = normalizeHash32(hash, 'author_submitExtrinsic hash')
    } catch (error) {
      throw error
    }
    if (!sameHex(returnedHash, extrinsicHash)) {
      throw new ChainError(
        `author_submitExtrinsic returned hash ${returnedHash}, expected ${extrinsicHash}`,
      )
    }
    return { status: 'submitted', message: 'Submitted', extrinsicHash, events: [] }
  }

  async watchSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    options: Pick<SubmitOptions, 'waitForFinalization' | 'timeoutMs' | 'signal' | 'policy' | 'allowRawCall'> & { signerAddress?: string } = {},
  ): Promise<ExtrinsicWatcher> {
    if (options.policy != null) assertRawBytesAllowed(options)
    void options.signerAddress
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    let subscription: AsyncIterable<unknown> & { unsubscribe(): Promise<void> }
    try {
      subscription = await this.transport.subscribe(
        'author_submitAndWatchExtrinsic',
        [hex(bytes)],
        'author_unwatchExtrinsic',
        {
          resubscribe: false,
          timeoutMs: options.timeoutMs,
          signal: options.signal,
          maxRetries: 0,
          retryForever: false,
        },
      )
    } catch (error) {
      throw error
    }
    const result = this.resolveWatchedExtrinsic(
      subscription,
      extrinsicHash,
      options.waitForFinalization ?? false,
      {
        timeoutMs: options.timeoutMs,
        signal: options.signal,
      },
    )
    return {
      extrinsicHash,
      result,
      unsubscribe: () => subscription.unsubscribe(),
    }
  }

  async peekNextIndex(address: string): Promise<number> {
    return parseNonce(await this.rpc('system_accountNextIndex', [address]), 'system_accountNextIndex')
  }

  async accountNextIndex(address: string, _useCache = false): Promise<number> {
    return this.peekNextIndex(address)
  }

  private assertExplicitNonce(options: SubmitOptions, operation: string): void {
    this.explicitNonce(options, operation)
  }

  private explicitNonce(options: SubmitOptions, operation: string): number {
    if (options.nonce == null) {
      throw new ChainError(
        `${operation} requires options.nonce; use submit() with a native Keypair for Rust-managed nonce submission or pass an explicit nonce for low-level signing`,
      )
    }
    return parseNonce(options.nonce, 'nonce')
  }

  /** Declared dispatch weight, with a dummy signature and no private-key access or nonce reservation. */
  async estimateWeight(call: CallLike, address: string, block?: number | string | null): Promise<{ ref_time: bigint; proof_size: bigint }> {
    const hash = typeof block === 'string' ? block : block == null ? await this.finalizedHead() : await this.blockHash(block)
    const runtime = await this.runtimeAt(hash)
    const encoded = runtime.encodeSignedExtrinsic(await this.callData(call, hash), publicKeyFromSs58(address), Buffer.alloc(64), CRYPTO_SR25519, { era: '00', nonce: 0, tip: 0n })
    const info = recordValue(await this.rpc('payment_queryInfo', [hex(encoded.bytes), hash]))
    const weight = info?.weight
    if (weight == null) throw new ChainError('payment_queryInfo response is missing declared weight')
    const fields = recordValue(weight)
    return fields ? { ref_time: toBigInt((fields.ref_time ?? fields.refTime) as bigint | number | string, 'ref_time'), proof_size: toBigInt((fields.proof_size ?? fields.proofSize ?? 0) as bigint | number | string, 'proof_size') } : { ref_time: toBigInt(weight as bigint | number | string, 'weight'), proof_size: 0n }
  }
  estimate_weight(call: CallLike, address: string, block?: number | string | null): Promise<{ ref_time: bigint; proof_size: bigint }> { return this.estimateWeight(call, address, block) }

  /** Quote with invalid authorization; never request a signature or reserve a nonce. */
  async estimateFee(
    call: CallLike,
    signer: SignerLike,
    options: Pick<SubmitOptions, 'allowRawCall' | 'policy' | 'rawSignerRole'> = {},
  ): Promise<Balance> {
    const snapshot = await this.signingSnapshot()
    const { runtime } = snapshot
    const { callData } = this.prepareCallForSigning(call, runtime, options)
    const account = await this.resolveSigner(signer, runtime)
    // Both supported schemes use 64-byte signatures. A zero-filled signature
    // preserves the encoded length without granting the RPC signing authority.
    const quoted = runtime.encodeSignedExtrinsic(callData, account.publicKey, Buffer.alloc(64), account.cryptoType, {
      era: '00',
      nonce: await this.peekNextIndex(account.ss58Address),
      tip: 0n,
      metadataHashEnabled: account.requiresMetadataProof,
    })
    const info = await this.rpc('payment_queryInfo', [hex(quoted.bytes), snapshot.blockHash])
    const fields = recordValue(info)
    const partialFee = decimalAmount(fields?.partialFee ?? fields?.partial_fee)
    if (partialFee == null) {
      throw new ChainError('payment_queryInfo response is missing partialFee', info)
    }
    return Balance.fromRao(partialFee)
  }

  submitCall(pallet: string, fn: string, params: ScaleValue, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(rawCall(pallet, fn, params, { signerRole: options.rawSignerRole ?? 'coldkey' }), signer, options)
  }

  submit_call(pallet: string, fn: string, params: ScaleValue, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submitCall(pallet, fn, params, signer, options)
  }

  transfer(signer: SignerLike, dest: string, amount: TransactionAmount, options: SubmitOptions & { keepAlive?: boolean } = {}): Promise<ExtrinsicResult> {
    const amountRao = taoTransactionAmountRao(amount, 'transfer amount')
    const intent = options.keepAlive === false
      ? IntentCall.transferAllowDeath(dest, amountRao)
      : IntentCall.transfer(dest, amountRao)
    return this.submit(intent, signer, {
      waitForInclusion: true,
      ...options,
    })
  }

  transfer_keep_alive(signer: SignerLike, dest: string, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.transfer(signer, dest, amount, { keepAlive: true, ...options })
  }

  transfer_allow_death(signer: SignerLike, dest: string, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.transfer(signer, dest, amount, { keepAlive: false, ...options })
  }

  setWeights(signer: SignerLike, netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(IntentCall.setWeights(netuid, dests, weights, toBigInt(versionKey, 'versionKey')), signer, { waitForInclusion: true, ...options })
  }

  set_weights(signer: SignerLike, netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.setWeights(signer, netuid, dests, weights, versionKey, options)
  }

  burnedRegister(signer: SignerLike, netuid: number, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(IntentCall.burnedRegister(netuid, hotkey), signer, { waitForInclusion: true, ...options })
  }

  burned_register(signer: SignerLike, netuid: number, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.burnedRegister(signer, netuid, hotkey, options)
  }

  rootRegister(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(IntentCall.rootRegister(hotkey), signer, { waitForInclusion: true, ...options })
  }

  root_register(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.rootRegister(signer, hotkey, options)
  }

  registerNetwork(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(IntentCall.registerSubnet(hotkey), signer, { waitForInclusion: true, ...options })
  }

  register_network(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.registerNetwork(signer, hotkey, options)
  }

  registerSubnet(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.registerNetwork(signer, hotkey, options)
  }

  register_subnet(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.registerSubnet(signer, hotkey, options)
  }

  serveAxon(
    signer: SignerLike,
    args: { netuid: number; ip: ServeIp; port: number; version?: number; ipType?: number; protocol?: number },
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    const normalizedIp = normalizeServeIp(args.ip, args.ipType)
    return this.submit(IntentCall.serveAxon(
      args.netuid,
      normalizedIp.value,
      args.port,
      args.version ?? 0,
      normalizedIp.type,
      args.protocol ?? 4,
    ), signer, {
      waitForInclusion: true,
      ...options,
    })
  }

  serve_axon(
    signer: SignerLike,
    args: { netuid: number; ip: ServeIp; port: number; version?: number; ipType?: number; protocol?: number },
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    return this.serveAxon(signer, args, options)
  }

  getBalance(address: string | Keypair, block?: number | string | null): Promise<Balance> {
    return this.balances.get(typeof address === 'string' ? address : address.ss58Address, block)
  }

  get_balance(address: string | Keypair, block?: number | string | null): Promise<Balance> {
    return this.getBalance(address, block)
  }

  getBalances(addresses: Array<string | Keypair>, block?: number | string | null): Promise<Record<string, Balance>> {
    return this.balances.getMany(addresses.map((address) => typeof address === 'string' ? address : address.ss58Address), block)
  }

  get_balances(addresses: Array<string | Keypair>, block?: number | string | null): Promise<Record<string, Balance>> {
    return this.getBalances(addresses, block)
  }

  getStake(coldkey: string | Keypair, hotkey: string | Keypair, netuid: number, block?: number | string | null): Promise<Balance> {
    return this.staking.get(
      typeof coldkey === 'string' ? coldkey : coldkey.ss58Address,
      typeof hotkey === 'string' ? hotkey : hotkey.ss58Address,
      netuid,
      block,
    )
  }

  get_stake(coldkey: string | Keypair, hotkey: string | Keypair, netuid: number, block?: number | string | null): Promise<Balance> {
    return this.getStake(coldkey, hotkey, netuid, block)
  }

  metagraph<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.subnets.metagraph<T>(netuid, block)
  }

  subnet(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    return this.subnets.subnet(netuid, block)
  }

  allSubnets(block?: number | string | null): Promise<SubnetInfo[]> {
    return this.subnets.all(block)
  }

  all_subnets(block?: number | string | null): Promise<SubnetInfo[]> {
    return this.allSubnets(block)
  }

  subnetExists(netuid: number, block?: number | string | null): Promise<boolean> {
    return this.subnets.exists(netuid, block)
  }

  subnet_exists(netuid: number, block?: number | string | null): Promise<boolean> {
    return this.subnetExists(netuid, block)
  }

  getSubnetHyperparameters<T = SubnetHyperparameters | null>(netuid: number, block?: number | string | null): Promise<T> {
    return this.subnets.hyperparameters<T>(netuid, block)
  }

  get_subnet_hyperparameters<T = SubnetHyperparameters | null>(netuid: number, block?: number | string | null): Promise<T> {
    return this.getSubnetHyperparameters<T>(netuid, block)
  }

  async callData(call: CallLike, block?: number | string | null): Promise<Buffer> {
    if (Buffer.isBuffer(call) || call instanceof Uint8Array) return toBuffer(call, 'call')
    if (isIntentCall(call)) {
      const [pallet, fn, params] = call.asCallTuple()
      return this.composeCall(pallet, fn, params, block)
    }
    const [pallet, fn, params] = normalizeCall(call)
    return this.composeCall(pallet, fn, params, block)
  }

  private callDataWithRuntime(call: CallLike, runtime: Runtime): Buffer {
    if (isIntentCall(call)) {
      const [pallet, fn, params] = call.asCallTuple()
      return runtime.composeCall(pallet, fn, params)
    }
    if (Buffer.isBuffer(call) || call instanceof Uint8Array) return toBuffer(call, 'call')
    const [pallet, fn, params] = normalizeCall(call)
    return runtime.composeCall(pallet, fn, params)
  }

  private prepareCallForSigning(
    call: CallLike,
    runtime: Runtime,
    options: SubmitOptions,
  ): { callData: Buffer; intent?: IntentCall } {
    if (isIntentCall(call)) {
      this.enforceIntentPolicy(call, options)
      return { callData: this.callDataWithRuntime(call, runtime), intent: call }
    }
    if (Buffer.isBuffer(call) || call instanceof Uint8Array) {
      assertRawBytesAllowed(options)
      return { callData: toBuffer(call, 'call') }
    }
    if (!rawCallsAllowed(options)) {
      throw new ChainError(
        'raw metadata calls require explicit raw-call permission; use an IntentCall trusted constructor or pass allowRawCall: true',
      )
    }
    const [pallet, fn, params] = normalizeCall(call)
    const intent = rawCall(pallet, fn, params, {
      op: `${pallet}.${fn}`,
      signerRole: options.rawSignerRole ?? 'coldkey',
    })
    this.enforceIntentPolicy(intent, options)
    return { callData: this.callDataWithRuntime(intent, runtime), intent }
  }

  private enforceIntentPolicy(intent: IntentCall, options: SubmitOptions): void {
    const policy = policyForSubmitOptions(options)
    const violations = policy.check(intent)
    if (violations.length > 0) {
      throw new ChainError(`transaction rejected by policy: ${violations.join('; ')}`)
    }
  }

  private async signingSnapshot(): Promise<SigningSnapshot> {
    const blockHash = await this.blockHash()
    const [blockNumber, runtime, genesisHash] = await Promise.all([
      this.blockNumber(blockHash),
      this.runtimeAt(blockHash),
      this.genesisHash(),
    ])
    return {
      blockHash,
      blockNumber,
      runtime,
      genesisHash,
    }
  }

  async resolveBlockHash(block?: number | string | null): Promise<string | null> {
    if (block == null) return null
    if (typeof block === 'string') return block
    return this.blockHash(block)
  }

  async resolveReadBlockHash(block?: number | string | null): Promise<string> {
    const blockHash = await this.resolveBlockHash(block)
    return blockHash ?? await this.finalizedHead()
  }

  protected async resolveSigner(signer: SignerLike, runtime: Runtime): Promise<ResolvedSigner> {
    const normalizedSigner: Keypair | ChainSigner =
      signer instanceof LedgerDevice ? signer.signer() : signer
    const signerShape = normalizedSigner as ChainSigner
    const account = await signerShape.getAccount?.({
      client: this,
      runtime,
      ss58Format: runtime.ss58Format,
    })
    const ss58Address =
      accountAddress(account) ?? staticSignerAddress(normalizedSigner)
    if (ss58Address == null) {
      throw new ChainError('signer must expose an address, ss58Address, or getAccount()')
    }
    const publicKey = signerPublicKey(account?.publicKey ?? signerShape.publicKey, ss58Address)
    const addressPublicKey = publicKeyFromSs58(ss58Address)
    if (!publicKey.equals(addressPublicKey)) {
      throw new ChainError('signer publicKey does not match signer address')
    }
    const cryptoType = account?.cryptoType ?? signerShape.cryptoType ?? CRYPTO_SR25519
    if (cryptoType !== CRYPTO_ED25519 && cryptoType !== CRYPTO_SR25519) {
      throw new ChainError(`unsupported signer cryptoType ${cryptoType}`)
    }
    return {
      signer: normalizedSigner,
      ss58Address,
      publicKey,
      cryptoType,
      requiresMetadataProof: Boolean(
        account?.requiresMetadataProof ??
          account?.requiresMetadataHash ??
          signerShape.requiresMetadataProof ??
          signerShape.requiresMetadataHash,
      ),
    }
  }

  protected async signWithSigner(
    signer: Keypair | ChainSigner,
    payload: Buffer,
    context: SignerPayloadContext,
  ): Promise<NormalizedSignature> {
    const signerShape = signer as ChainSigner
    if (signerShape.signBytes != null) {
      return normalizeSignature(
        await signerShape.signBytes(payload, context),
        context.cryptoType,
      )
    }
    if (signerShape.signPayload != null) {
      return normalizeSignature(
        await signerShape.signPayload(extensionSignPayload(context), context),
        context.cryptoType,
      )
    }
    if (signerShape.signRaw != null) {
      return normalizeSignature(
        await signerShape.signRaw({
          address: context.address,
          data: hex(payload),
          type: 'bytes',
          payload,
          metadataHash: context.metadataHash == null ? undefined : hex(context.metadataHash),
          metadataProof: context.metadataProof,
          proof: context.proof,
          chainInfo: context.chainInfo,
        }),
        context.cryptoType,
      )
    }
    if (signerShape.sign != null) {
      return normalizeSignature(await signerShape.sign(payload, context), context.cryptoType)
    }
    throw new ChainError('signer must implement signBytes(), signRaw(), sign(), or extension-style signPayload()')
  }

  private async normalizeEra(
    period: number | null,
    snapshot?: SigningSnapshot,
  ): Promise<{ era: ScaleValue; eraBlockHash: string }> {
    if (period == null) {
      return { era: '00', eraBlockHash: snapshot?.genesisHash ?? await this.genesisHash() }
    }
    const head = snapshot?.blockHash ?? await this.blockHash()
    const height = snapshot?.blockNumber ?? await this.blockNumber(head)
    // CheckMortality reads the parent's stored BlockHash map during validation;
    // the current head's own hash is not inserted there until the next block.
    const current = Math.max(0, height - 1)
    const birth = Number(eraBirth(period, current))
    const eraBlockHash = await this.blockHash(birth)
    return { era: { period, current }, eraBlockHash }
  }

  private async resolveWatchedExtrinsic(
    subscription: AsyncIterable<unknown> & { unsubscribe(): Promise<void> },
    extrinsicHash: string,
    waitForFinalization: boolean,
    options: Pick<SubmitOptions, 'timeoutMs' | 'signal'> = {},
  ): Promise<ExtrinsicResult> {
    const request = withRequestSignal({
      timeoutMs: options.timeoutMs ?? 0,
      signal: options.signal,
    })
    let abortError: Error | undefined
    request.onAbort((error) => {
      abortError = error
      void subscription.unsubscribe()
    })
    try {
      for await (const status of subscription) {
        if (abortError != null) throw abortError
        const normalized = normalizeStatus(status)
        const fatal = ['usurped', 'finalitytimeout', 'dropped', 'invalid'].find((name) =>
          hasOwn(normalized, name),
        )
        if (fatal != null) throw new ChainError(`Extrinsic ${fatal}`, status)
        if (waitForFinalization && normalized.finalized != null) {
          return this.resolveInclusion(extrinsicHash, String(normalized.finalized), true)
        }
        if (!waitForFinalization && normalized.inblock != null) {
          return this.resolveInclusion(extrinsicHash, String(normalized.inblock), false)
        }
      }
      if (abortError != null) throw abortError
      throw new ChainError('extrinsic watch ended before inclusion')
    } finally {
      request.cleanup()
      await subscription.unsubscribe()
    }
  }

  async findExtrinsic(extrinsicHash: string, blockHash: string, finalized = false, options: RpcRequestOptions = {}): Promise<ExtrinsicResult | null> {
    const result = await this.rpc('chain_getBlock', [blockHash]) as { block?: { extrinsics: string[] } }
    if (!result?.block?.extrinsics.some(item => hex(blake2_256(hexToBuffer(item))) === extrinsicHash.toLowerCase())) return null
    return this.resolveInclusion(extrinsicHash.toLowerCase(), blockHash, finalized, options)
  }
  private async resolveInclusion(extrinsicHash: string, blockHash: string, finalized: boolean, options: RpcRequestOptions = {}): Promise<ExtrinsicResult> {
    const block = (await this.rpc('chain_getBlock', [blockHash])) as { block: { header: { parentHash: string; number: string | number }; extrinsics: string[] } }
    const blockNumber = headerNumber(block.block.header)
    const extrinsicIndex = block.block.extrinsics.findIndex((item) => hex(blake2_256(hexToBuffer(item))) === extrinsicHash)
    if (extrinsicIndex < 0) throw new ChainError(`extrinsic ${extrinsicHash} was not found in block ${blockHash}`)
    // Upgrade-block events were emitted by the parent runtime, before the new code took effect.
    const runtime = await this.runtimeAt(blockNumber > 0 ? block.block.header.parentHash : blockHash)
    const entry = runtime.storageEntry('System', 'Events')
    const raw = await this.rpc('state_getStorage', [hex(runtime.storageKey('System', 'Events', [])), blockHash])
    const events = runtime.decode(entry.valueType, raw == null ? entry.defaultBytes : hexToBuffer(String(raw)), false) as unknown[]
    if (events.some((event) => eventName(event) === 'System.CodeUpdated')) this.invalidateRuntimeCache()
    const triggered = events.filter((event) => eventExtrinsicIndex(event) === extrinsicIndex)
    const nested = nestedDispatchError(triggered)
    const failed = triggered.find((event) => eventName(event) === 'System.ExtrinsicFailed') ?? (nested == null ? undefined : { attributes: { dispatch_error: nested } })
    const success = triggered.some((event) => eventName(event) === 'System.ExtrinsicSuccess')
    const feeEvent = triggered.find((event) => eventName(event) === 'TransactionPayment.TransactionFeePaid')
    const dispatchSuccess = success && failed == null
    return {
      status: failed ? 'failed' : dispatchSuccess ? finalized ? 'finalized' : 'inBlock' : 'unknown',
      success: failed ? false : dispatchSuccess || undefined,
      message: failed ? dispatchFailureMessage(runtime, failed) : dispatchSuccess ? 'Success' : 'Included, but no dispatch outcome event was found',
      extrinsicHash, blockHash, blockNumber, extrinsicIndex,
      extrinsicId: `${blockNumber}-${String(extrinsicIndex).padStart(4, '0')}`,
      finalized, fee: feeEvent == null ? undefined : feeFromEvent(feeEvent), events: triggered, error: failed,
      chainError: failed ? chainErrorFromDispatch(failed, (pallet, error) => runtime.moduleError(pallet, error)) : undefined,
    }
  }

}

export class Client extends BrowserChainClient {
  async resolveEvmRecipient(address: string) { return resolveEvmRecipient(await createNodeIntentReader(this,await this.requireNativeClient(),await this.finalizedHead()),address) }
  protectedWallet(keypair: ProtectedKeypair): ProtectedWalletClient {
    return new ProtectedWalletClient({
      decodeAddress: publicKeyFromSs58, hash: blake2_256,
      query: (p, s, args, block) => this.query(p, s, args, block),
      finalizedHead: () => this.finalizedHead(), blockHash: n => this.blockHash(n),
      blockNumber: block => this.blockNumber(block), runtimeAt: block => this.runtimeAt(block),
      accountNextIndex: address => this.peekNextIndex(address),
      rpc: (method, params) => this.rpc(method, params),
      eraBirth: (period, current) => Number(eraBirth(period, current)), seal: sealMevShieldTransaction, verify: verifyAuthorization,
      intentReader: async block => createNodeIntentReader(this, await this.requireNativeClient(), block),
      submitBytes: (bytes, address, options) => this.submitSigned(bytes, address, { ...options, waitForInclusion: options.waitForInclusion ?? true }),
      followShielded: (carrier, hash, period, options) => followShieldedResult(carrier, hash, period, this, options),
    }, keypair, this)
  }
  /** Checked integer TAO-to-rao conversion, matching Rust Client::tao. */
  static tao(value: bigint | number | string): bigint {
    const amount = toBigInt(value, 'TAO amount')
    const result = amount * 1_000_000_000n
    if (amount < 0n || result >= 1n << 128n) throw new RangeError('TAO amount overflows u128')
    return result
  }
  async estimateShieldedCarrierFee(address: string): Promise<Balance> {
    const call = await this.composeCall('MevShield', 'submit_encrypted', { ciphertext: new Uint8Array(8192) })
    return this.estimateFee(call, { ss58Address: address, cryptoType: CRYPTO_SR25519 }, { allowRawCall: true })
  }
  estimate_shielded_carrier_fee(address: string): Promise<Balance> { return this.estimateShieldedCarrierFee(address) }

  workflows(): WorkflowClient { return new WorkflowClient(this) }
  upgrades(): UpgradeClient { return createNodeUpgradeClient(this) }
  tools() { return listIntentTools() }
  plan(intent: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowPlan> { return this.workflows().plan(intent, signer, options) }
  preflight(intent: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowPlan> { return this.plan(intent, signer, options) }
  executeTool(spec: Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowResult> { return this.execute(spec, signer, options) }
  submitShielded(intent: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowResult> { return this.workflows().submitShielded(intent, signer, options) }
  submitRawShielded(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<WorkflowResult> { return this.workflows().submitRawShielded(call, signer, options) }
  multisig(signatories: readonly string[], threshold: number, accountMode: 'legacy' | 'safe' = 'legacy') { return this.workflows().multisig(signatories, threshold, accountMode) }
  planIntent(intent: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowPlan> { return this.workflows().plan(intent, signer, options) }
  execute(intent: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowResult> { return this.workflows().execute(intent, signer, options) }

  private readonly nodeExpectedGenesisHash?: string
  private readonly nodeEndpoints: string[]
  private readonly nodeArchiveEndpoints: string[]
  private readonly nodeOptions: ClientOptions
  private nodeArchiveTransport?: JsonRpcTransport
  private nodeNativeClient?: NativeChainClient
  private nodeNativeClientPromise?: Promise<NativeChainClient>

  constructor(network: string = 'finney', options: ClientOptions = {}) {
    const [label] = resolveEndpoint(options.endpoint ?? network)
    const fallbackEndpoints = options.fallbackEndpoints ?? [...FALLBACK_ENDPOINTS[label] ?? []]
    if (fallbackEndpoints.length && options.expectedGenesisHash == null && trustedGenesisHash(label) == null) throw new ChainError('fallbackEndpoints require expectedGenesisHash for custom or untrusted networks')
    super(network, { ...options, fallbackEndpoints: [], autoConnect: false })
    this.nodeOptions = options
    this.nodeArchiveEndpoints = options.archiveEndpoints ?? [...ARCHIVE_ENDPOINTS[label] ?? []]
    this.nodeEndpoints = [this.endpoint, ...fallbackEndpoints.filter((endpoint) => endpoint !== this.endpoint)]
    this.nodeExpectedGenesisHash = normalizeGenesisHash(
      options.expectedGenesisHash ?? trustedGenesisHash(this.network),
    )
    nativeClientAccess.set(this, () => this.requireNativeClient())
    if (options.autoConnect) {
      this.ready = this.connect()
      this.ready.catch(() => undefined)
    }
  }

  async connect(): Promise<this> {
    await this.requireNativeClient()
    return this
  }

  async close(): Promise<void> {
    this.transport.close()
    this.nodeArchiveTransport?.close()
  }

  private async requireNativeClient(): Promise<NativeChainClient> {
    if (this.nodeNativeClient != null) return this.nodeNativeClient
    this.nodeNativeClientPromise ??= NativeChainClient.connectEndpoints(this.nodeEndpoints)
      .then((client) => {
        const genesis = hex(client.genesisHash)
        if (this.nodeExpectedGenesisHash != null && !sameHex(genesis, this.nodeExpectedGenesisHash)) {
          throw new EndpointValidationError(
            `endpoint ${client.endpoint} genesis ${genesis} does not match expected genesis ${this.nodeExpectedGenesisHash}`,
          )
        }
        this.nodeNativeClient = client
        this.genesis = genesis
        return client
      })
      .catch((error) => {
        this.nodeNativeClientPromise = undefined
        throw error
      })
    return this.nodeNativeClientPromise
  }

  override async blockNumber(blockHash?: string | null): Promise<number> {
    return (await this.requireNativeClient()).blockNumber(blockHash)
  }

  override async blockHash(block?: number | null): Promise<string> {
    return (await this.requireNativeClient()).blockHash(block == null ? undefined : block)
  }

  override async finalizedHead(): Promise<string> {
    return (await this.requireNativeClient()).finalizedHead()
  }

  override async genesisHash(): Promise<string> {
    const genesis = hex((await this.requireNativeClient()).genesisHash)
    this.genesis = genesis
    return genesis
  }

  override async runtimeAt(block?: number | string | null): Promise<Runtime> {
    if (block != null) {
      return super.runtimeAt(block)
    }
    const native = await this.requireNativeClient()
    await native.refreshRuntime()
    return native.runtime()
  }

  override async query<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction?: string | ScaleValue[],
    paramsOrBlock?: ScaleValue[] | number | string | null,
    block?: number | string | null,
  ): Promise<T | undefined> {
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    try { return await (await this.requireNativeClient()).query(moduleName, itemName, itemParams, blockHash) as T }
    catch (error) { if (!this.nodeArchiveEndpoints.length || blockHash == null || !isPrunedState(error)) throw error; return super.query<T>([moduleName, itemName], itemParams, blockHash) }
  }

  override async queryBatch<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction: string | ScaleValue[][],
    paramSetsOrBlock?: ScaleValue[][] | number | string | null,
    block?: number | string | null,
  ): Promise<Array<T | undefined>> {
    const [moduleName, itemName, sets, blockRef] =
      normalizeBatchArgs(pallet, storageFunction, paramSetsOrBlock, block)
    if (sets.length === 0) return []
    const blockHash = await this.resolveReadBlockHash(blockRef)
    try { return await (await this.requireNativeClient()).queryBatch(moduleName, itemName, sets, blockHash) as Array<T | undefined> }
    catch (error) { if (!this.nodeArchiveEndpoints.length || blockHash == null || !isPrunedState(error)) throw error; return super.queryBatch<T>([moduleName, itemName], sets, blockHash) }
  }

  override invalidateRuntimeCache(): void {
    super.invalidateRuntimeCache()
    void this.nodeNativeClient?.refreshRuntime().catch(() => undefined)
  }

  async refreshRuntime(): Promise<boolean> {
    super.invalidateRuntimeCache()
    return (await this.requireNativeClient()).refreshRuntime()
  }

  override async chainInfo(_runtime?: Runtime, _blockHash?: string | null): Promise<ChainInfo> {
    return nativeChainInfoToChainInfo(await (await this.requireNativeClient()).chainInfo())
  }

  override async rpc(method: string, params: unknown[] = []): Promise<unknown> {
    try { return await (await this.requireNativeClient()).rpcValue(method, params) }
    catch (error) {
      if (!this.nodeArchiveEndpoints.length || !isPrunedState(error) || !/^(?:state_(?:call|getStorage|queryStorageAt|getKeysPaged|getMetadata|getRuntimeVersion)|chain_get(?:Block|Header))$/.test(method)) throw error
      const expected = await this.genesisHash()
      this.nodeArchiveTransport ??= new JsonRpcTransport(this.nodeArchiveEndpoints[0], this.nodeArchiveEndpoints.slice(1), this.nodeOptions.retryForever, {
        ...this.nodeOptions,
        validateEndpoint: async (endpoint, request) => { if (!sameHex(String(await request('chain_getBlockHash', [0])), expected)) throw new EndpointValidationError(`archive endpoint ${endpoint} belongs to another chain`) },
      })
      return this.nodeArchiveTransport.request(method, params)
    }
  }

  override async runtimeCall<T extends ScaleValue = ScaleValue>(
    api: string | Descriptor,
    method?: string | ScaleValue[],
    paramsOrBlock: ScaleValue[] | number | string | null = [],
    block?: number | string | null,
  ): Promise<T> {
    const [apiName, methodName, callParams, blockRef] = normalizeRuntimeArgs(api, method, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    try { return await (await this.requireNativeClient()).runtimeCall(apiName, methodName, callParams, blockHash) as T }
    catch (error) { if (!this.nodeArchiveEndpoints.length || blockHash == null || !isPrunedState(error)) throw error; return super.runtimeCall<T>([apiName, methodName], callParams, blockHash) }
  }

  override async queryMap<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction?: string | ScaleValue[],
    paramsOrBlock?: ScaleValue[] | number | string | null,
    block?: number | string | null,
    pageSizeOrOptions: number | QueryMapOptions = 512,
  ): Promise<Array<[K, V]>> {
    if (pageSizeOrOptions !== 512) {
      return super.queryMap<K, V>(
        pallet,
        storageFunction,
        paramsOrBlock,
        block,
        pageSizeOrOptions,
      )
    }
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    try { return await (await this.requireNativeClient()).queryMap(moduleName, itemName, itemParams, blockHash) as Array<[K, V]> }
    catch (error) { if (!this.nodeArchiveEndpoints.length || blockHash == null || !isPrunedState(error)) throw error; return super.queryMap<K, V>([moduleName, itemName], itemParams, blockHash) }
  }

  override async stateCall<T = string>(method: string, data: ByteLike | string, block?: number | string | null): Promise<T> {
    return super.stateCall<T>(method, data, block)
  }

  override async constant<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    name?: string,
    block?: number | string | null,
  ): Promise<T | undefined> {
    if (block != null) {
      return super.constant<T>(pallet, name, block)
    }
    const [moduleName, constantName] = typeof pallet === 'string' ? [pallet, name as string] : pallet
    return (await this.requireNativeClient()).constant(moduleName, constantName) as T
  }

  override async decodeScale<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteLike | string,
    block?: number | string | null,
  ): Promise<T> {
    if (block != null) {
      return super.decodeScale<T>(typeString, data, block)
    }
    return (await this.requireNativeClient()).decodeScale(
      typeString,
      typeof data === 'string' ? hexToBuffer(data) : toBuffer(data, 'data'),
    ) as T
  }

  override async composeCall(pallet: string, fn: string, params: ScaleValue = {}, block?: number | string | null): Promise<Buffer> {
    if (block != null) {
      return super.composeCall(pallet, fn, params, block)
    }
    return (await this.requireNativeClient()).composeCall(pallet, fn, params)
  }

  /** Prepare the exact Rust signing payload without asking the signer to sign. */
  async prepareCall(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<PreparedCall> {
    const resolved = await this.resolveSigner(signer, await this.runtimeAt())
    const bound = bindSignerIdentity(resolved.signer, { ...resolved, address: resolved.ss58Address })
    return runSignerQueue(this, resolved.ss58Address, async () => {
      options.signal?.throwIfAborted()
      const planned = await this.nativeSigningPlan(call, bound, options, await this.requireNativeClient())
      options.signal?.throwIfAborted()
      const context = Object.freeze(nativePlanSignerContext(this, planned.runtime, planned.resolved, planned.plan))
      preparedCalls.set(context, { owner: this, planned })
      return context
    }, options)
  }

  exportPreparedCall(prepared: PreparedCall): SerializedUnsignedTransaction {
    const record = preparedCalls.get(prepared) ?? importedCalls.get(prepared)
    if (!record || record.owner !== this) throw new TypeError('expected a signing plan prepared or imported by this client')
    return serializeUnsignedTransaction(prepared as unknown as UnsignedTransactionState, { blake2_256, publicKeyFromSs58 }, prepared.runtime.metadataBytes)
  }

  /** Imported bytes carry no policy approval; raw-call admission is required explicitly. */
  async importPreparedCall(value: unknown, options: Pick<SubmitOptions, 'allowRawCall' | 'policy'> = {}): Promise<PreparedCall> {
    assertRawBytesAllowed(options)
    const runtime = await this.runtimeAt(), genesis = await this.genesisHash()
    const restored = restoreUnsignedTransaction(value, runtime as unknown as UnsignedRuntime, { blake2_256, publicKeyFromSs58 }, runtime.metadataBytes, genesis)
    const context: PreparedCall = Object.freeze({
      ...restored, client: this, runtime, publicKey: Buffer.from(restored.publicKey), callData: Buffer.from(restored.callData), payload: Buffer.from(restored.payload),
      includedInExtrinsic: Buffer.from(restored.includedInExtrinsic), includedInSignedData: Buffer.from(restored.includedInSignedData),
      txParams: restored.txParams, metadataHash: restored.txParams.metadataHash == null ? null : Buffer.from(restored.txParams.metadataHash),
      metadataProof: restored.metadataProof == null ? undefined : Buffer.from(restored.metadataProof), signerPayload: restored.signerPayload as ExtensionSignPayloadRequest,
    })
    importedCalls.set(context, { owner: this, serialized: serializeUnsignedTransaction(restored, { blake2_256, publicKeyFromSs58 }, runtime.metadataBytes) })
    return context
  }

  /** Submit a detached signature only for a plan prepared by this client. */
  async submitSignature(prepared: PreparedCall, signature: SignerSignature, options: Pick<SubmitOptions, 'waitForInclusion' | 'waitForFinalization' | 'timeoutMs' | 'signal' | 'policy' | 'allowRawCall'> = {}): Promise<ExtrinsicResult> {
    if (options.policy != null) assertRawBytesAllowed(options)
    const record = preparedCalls.get(prepared)
    if (!record || record.owner !== this) {
      const imported = importedCalls.get(prepared)
      if (!imported || imported.owner !== this) throw new TypeError('expected a signing plan prepared by this client')
      return runSignerQueue(this, imported.serialized.address, async permit => {
        options.signal?.throwIfAborted()
        const runtime = await this.runtimeAt()
        const restored = restoreUnsignedTransaction(imported.serialized, runtime as unknown as UnsignedRuntime, { blake2_256, publicKeyFromSs58 }, runtime.metadataBytes, await this.genesisHash())
        const normalized = normalizeSignature(signature, restored.cryptoType)
        if (normalized.cryptoType !== restored.cryptoType || !verify(restored.payload, normalized.signature, restored.address, restored.cryptoType)) throw new ChainError('external signature does not verify against the imported signing context')
        const signed = runtime.encodeSignedExtrinsic(restored.callData, restored.publicKey, normalized.signature, normalized.cryptoType, { ...restored.txParams, metadataHashEnabled: restored.txParams.metadataHash != null })
        return this.submitSigned(signed, restored.address, signerQueueOptions(options, permit))
      }, options)
    }
    return runSignerQueue(this, record.planned.resolved.ss58Address, async () => {
      options.signal?.throwIfAborted()
      const normalized = normalizeSignature(signature, record.planned.resolved.cryptoType)
      const cancellation = nativeCancellationForSignal(options.signal)
      const outcome = await abortableNative(record.planned.native.submitExternal(record.planned.plan, normalized.signature, nativeSubmitOptions(options), normalized.cryptoType, cancellation), options.signal, cancellation)
      return nativeOutcomeToExtrinsicResult(outcome, options.waitForFinalization === true)
    }, options)
  }

  override async signExtrinsic(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<SignedExtrinsicResult> {
    const resolved = await this.resolveSigner(signer, await this.runtimeAt())
    const bound = bindSignerIdentity(resolved.signer, { ...resolved, address: resolved.ss58Address })
    return runSignerQueue(this, resolved.ss58Address, async () => {
      options.signal?.throwIfAborted()
      const native = await this.requireNativeClient()
      const planned = await this.nativeSigningPlan(call, bound, options, native)
      options.signal?.throwIfAborted()
      const signature = await this.signRustPlan(planned)
      options.signal?.throwIfAborted()
      const signed = await native.assembleExternal(planned.plan, signature.signature, signature.cryptoType)
      return {
        bytes: Buffer.from(signed.bytes),
        hash: hexToBuffer(signed.hash),
        hex: hex(signed.bytes),
        signerAddress: planned.resolved.ss58Address,
        nonce: parseNonce(planned.plan.nonce, 'native signing plan nonce'),
      }
    }, options)
  }

  override async submit(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    const resolved = await this.resolveSigner(signer, await this.runtimeAt())
    const bound = bindSignerIdentity(resolved.signer, { ...resolved, address: resolved.ss58Address })
    return runSignerQueue(this, resolved.ss58Address, async () => {
    options.signal?.throwIfAborted()
    const native = await this.requireNativeClient()
    const planned = await this.nativeSigningPlan(call, bound, options, native)
    options.signal?.throwIfAborted()
    const signature = await this.signRustPlan(planned)
    options.signal?.throwIfAborted()
    const cancellation = nativeCancellationForSignal(options.signal)
    const outcome = await abortableNative(
      native.submitExternal(
        planned.plan,
        signature.signature,
        nativeSubmitOptions(options),
        signature.cryptoType,
        cancellation,
      ),
      options.signal,
      cancellation,
    )
    return nativeOutcomeToExtrinsicResult(
      outcome,
      options.waitForFinalization === true,
    )
    }, options)
  }

  override async *blocks(options: { finalized?: boolean } = {}): AsyncIterable<BlockHeader> {
    yield* super.blocks(options)
  }

  override waitForBlock(block?: number | null, options: { timeoutMs?: number; signal?: AbortSignal } = {}): Promise<BlockHeader> {
    return super.waitForBlock(block, options)
  }

  override blockInfo(block?: number | null): Promise<BlockInfo | null> {
    return super.blockInfo(block)
  }

  override async submitSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    signerAddress?: string,
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    if (options.policy != null) assertRawBytesAllowed(options)
    const bytes = Buffer.from(Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes'))
    const address = await this.signedExtrinsicAccount(bytes, signerAddress)
    const broadcast = async () => {
    options.signal?.throwIfAborted()
    const native = await this.requireNativeClient()
    options.signal?.throwIfAborted()
    const extrinsicHash = hex(blake2_256(bytes))
    const cancellation = nativeCancellationForSignal(options.signal)
    const outcome = await abortableNative(
      native.submitEncoded(
        bytes,
        extrinsicHash,
        nativeSubmitOptions(options),
        cancellation,
      ),
      options.signal,
      cancellation,
    )
    return nativeOutcomeToExtrinsicResult(
      outcome,
      options.waitForFinalization === true,
    )
    }
    return address == null ? broadcast() : runSignerQueue(this, address, broadcast, options)
  }

  override async watchSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    options: Pick<SubmitOptions, 'waitForFinalization' | 'timeoutMs' | 'signal' | 'policy' | 'allowRawCall'> & { signerAddress?: string } = {},
  ): Promise<ExtrinsicWatcher> {
    if (options.policy != null) assertRawBytesAllowed(options)
    const bytes = Buffer.from(Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes'))
    const address = await this.signedExtrinsicAccount(bytes, options.signerAddress)
    const extrinsicHash = hex(blake2_256(bytes))
    const unsubscribeController = new AbortController()
    const signal = joinedAbortSignal(options.signal, unsubscribeController.signal)
    const cancellation = nativeCancellationForSignal(signal)
    const broadcast = () => {
      signal?.throwIfAborted()
      return abortableNative(this.requireNativeClient()
      .then((native) => native.submitEncoded(
        bytes,
        extrinsicHash,
        nativeSubmitOptions({
          ...options,
          waitForInclusion: options.waitForFinalization === true ? undefined : true,
        }),
        cancellation,
      ))
      .then((outcome) => nativeOutcomeToExtrinsicResult(
        outcome,
        options.waitForFinalization === true,
      )), signal, cancellation)
    }
    const result = address == null ? broadcast() : runSignerQueue(this, address, broadcast, options)
    return Promise.resolve({
      extrinsicHash,
      result,
      unsubscribe: async () => {
        unsubscribeController.abort()
      },
    })
  }

  /** Use the encoded signer for queueing; an arbitrary address hint cannot bypass its nonce lock. */
  private async signedExtrinsicAccount(bytes: Buffer, expectedAddress?: string): Promise<string | undefined> {
    const runtime = await this.runtimeAt()
    const decoded = runtime.decodeExtrinsic(bytes) as Record<string, any>
    if (decoded.address == null) return undefined
    const raw = decoded.address.Id ?? decoded.address
    const publicKey = typeof raw === 'string' ? raw.startsWith('0x') ? hexToBuffer(raw) : publicKeyFromSs58(raw) : toBuffer(raw, 'extrinsic signer')
    if (expectedAddress != null && !publicKeyFromSs58(expectedAddress).equals(publicKey)) throw new ChainError('submitted extrinsic signer differs from signerAddress')
    return ss58FromPublic(publicKey, runtime.ss58Format)
  }

  override async estimateFee(
    call: CallLike,
    signer: SignerLike,
    options: Pick<SubmitOptions, 'allowRawCall' | 'policy' | 'rawSignerRole'> = {},
  ): Promise<Balance> {
    const native = await this.requireNativeClient()
    const resolved = await this.resolveSigner(signer, await this.runtimeAt())
    // Previewing a fee must not reserve or advance the Rust client's nonce cache.
    const nonce = await native.accountNextIndex(resolved.ss58Address)
    const planned = await this.nativeSigningPlan(call, signer, { ...options, nonce }, native)
    const feeRao = planned.plan.feeRao == null
      ? await native.estimateFeeExternal(planned.plan)
      : BigInt(planned.plan.feeRao)
    return Balance.fromRao(feeRao)
  }

  override async peekNextIndex(address: string): Promise<number> {
    return (await this.requireNativeClient()).accountNextIndex(address)
  }

  override async accountNextIndex(address: string, _useCache = false): Promise<number> {
    return this.peekNextIndex(address)
  }

  /** Reserve a range for detached or multi-envelope signing; reconcile uncertain delivery before recovery. */
  reserveNonces(address: string, count: number): Promise<number> {
    return runSignerQueue(this, address, async () => (await this.requireNativeClient()).reserveNonces(address, count))
  }

  /** Reset the cached nonce from RPC after all detached and shielded transactions have been reconciled. A timeout or abort alone is insufficient. */
  invalidateNonce(address: string): Promise<void> {
    return runSignerQueue(this, address, async () => (await this.requireNativeClient()).invalidateNonce(address))
  }

  override async validateDescriptorSchema(block?: number | string | null): Promise<DescriptorSchemaIssue[]> {
    if (block != null) {
      return super.validateDescriptorSchema(block)
    }
    return validateDescriptorSchema(await this.runtimeAt())
  }

  private async nativeSigningPlan(
    call: CallLike,
    signer: SignerLike,
    options: SubmitOptions,
    native: NativeChainClient,
  ): Promise<NativeSigningPlan> {
    options.signal?.throwIfAborted()
    const runtime = await this.runtimeAt()
    const resolved = await this.resolveSigner(signer, runtime)
    const planOptions = nativeExternalSigningOptions(options)
    let plan: NativeExternalSigningPlanHandle
    if (isIntentCall(call)) {
      plan = await native.externalSigningPlanForIntent(
        call,
        resolved.ss58Address,
        resolved.publicKey,
        resolved.cryptoType,
        resolved.requiresMetadataProof,
        policyForSubmitOptions(options),
        planOptions,
      )
    } else if (Buffer.isBuffer(call) || call instanceof Uint8Array) {
      assertRawBytesAllowed(options)
      plan = await native.externalSigningPlan(
        toBuffer(call, 'call'),
        resolved.ss58Address,
        resolved.publicKey,
        resolved.cryptoType,
        resolved.requiresMetadataProof,
        planOptions,
      )
    } else {
      if (!rawCallsAllowed(options)) {
        throw new ChainError(
          'raw metadata calls require explicit raw-call permission; use an IntentCall trusted constructor or pass allowRawCall: true',
        )
      }
      const [pallet, fn, params] = normalizeCall(call)
      const intent = rawCall(pallet, fn, params, {
        op: `${pallet}.${fn}`,
        signerRole: options.rawSignerRole ?? 'coldkey',
      })
      plan = await native.externalSigningPlanForIntent(
        intent,
        resolved.ss58Address,
        resolved.publicKey,
        resolved.cryptoType,
        resolved.requiresMetadataProof,
        policyForSubmitOptions(options),
        planOptions,
      )
    }
    return { native, runtime: Runtime.fromNativeHandle(plan.runtime), resolved, plan }
  }

  private async signRustPlan(planned: NativeSigningPlan): Promise<NormalizedSignature> {
    const context = nativePlanSignerContext(this, planned.runtime, planned.resolved, planned.plan)
    return this.signWithSigner(planned.resolved.signer, context.payload, context)
  }
}

export class Snapshot {
  readonly balances: SnapshotBalancesNamespace & AccountsReadNamespace
  readonly subnets: SnapshotSubnetsNamespace & SubnetsReadNamespace
  readonly staking: SnapshotStakingNamespace & StakingReadNamespace
  readonly chain: ChainReadNamespace
  readonly epochs: EpochsReadNamespace
  readonly hyperparameters: HyperparametersReadNamespace
  readonly delegation: DelegationReadNamespace
  readonly leasing: LeasingReadNamespace
  readonly collateral: CollateralReadNamespace
  readonly locks: LocksReadNamespace
  readonly identity: IdentityReadNamespace
  readonly weights: WeightsReadNamespace
  readonly prices: PricesReadNamespace
  readonly neurons: SnapshotNeuronsNamespace & NeuronsReadNamespace

  constructor(readonly client: ChainClient, readonly block: number, readonly blockHash: string) {
    this.chain = bindReadNamespace({}, 'Chain', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as ChainReadNamespace
    this.epochs = bindReadNamespace({}, 'Epochs & timing', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as EpochsReadNamespace
    this.hyperparameters = bindReadNamespace({}, 'Hyperparameters', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as HyperparametersReadNamespace
    this.delegation = bindReadNamespace({}, 'Delegation', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as DelegationReadNamespace
    this.leasing = bindReadNamespace({}, 'Leases & crowdloans', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as LeasingReadNamespace
    this.collateral = bindReadNamespace({}, 'Mining & collateral', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as CollateralReadNamespace
    this.locks = bindReadNamespace({}, 'Locks & conviction', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as LocksReadNamespace
    this.identity = bindReadNamespace({}, 'Identity & commitments', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as IdentityReadNamespace
    this.weights = bindReadNamespace({}, 'Weights & bonds', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as WeightsReadNamespace
    this.prices = bindReadNamespace({}, 'Prices & swaps', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as PricesReadNamespace
    this.balances = bindReadNamespace(new SnapshotBalancesNamespace(this), 'Accounts & keys', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as SnapshotBalancesNamespace & AccountsReadNamespace
    this.subnets = bindReadNamespace(new SnapshotSubnetsNamespace(this), 'Subnets', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as SnapshotSubnetsNamespace & SubnetsReadNamespace
    this.staking = bindReadNamespace(new SnapshotStakingNamespace(this), 'Staking', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as SnapshotStakingNamespace & StakingReadNamespace
    this.neurons = bindReadNamespace(new SnapshotNeuronsNamespace(this), 'Neurons & registration', block => block == null ? Promise.resolve(this) : scopedReadView(this.client, block)) as SnapshotNeuronsNamespace & NeuronsReadNamespace
  }

  query<T extends ScaleValue = ScaleValue>(item: string | Descriptor, nameOrParams?: string | ScaleValue[], params?: ScaleValue[]): Promise<T | undefined> {
    return typeof item === 'string'
      ? this.client.query<T>(item, nameOrParams as string, params ?? [], this.blockHash)
      : this.client.query<T>(item, (nameOrParams as ScaleValue[] | undefined) ?? [], this.blockHash)
  }

  queryMap<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(item: Descriptor, params: ScaleValue[] = []): Promise<Array<[K, V]>> {
    return this.client.queryMap<K, V>(item, params, this.blockHash)
  }

  queryBatch<T extends ScaleValue = ScaleValue>(item: Descriptor, paramSets: ScaleValue[][]): Promise<Array<T | undefined>> {
    return this.client.queryBatch<T>(item, paramSets, this.blockHash)
  }

  runtime<T extends ScaleValue = ScaleValue>(method: Descriptor, params: ScaleValue[] = []): Promise<T> {
    return this.client.runtime<T>(method, params, this.blockHash)
  }

  constant<T extends ScaleValue = ScaleValue>(item: Descriptor): Promise<T | undefined> {
    return this.client.constant<T>(item, undefined, this.blockHash)
  }

  at(block?: number | null): Promise<Snapshot> { return block == null || block === this.block ? Promise.resolve(this) : this.client.at(block) }
  balance(rao: BalanceLike, netuid = 0, symbol?: string | null): Balance { return this.client.balance(rao, netuid, symbol) }
  timestamp(): Promise<Date> { return this.client.timestamp(this.blockHash) }
  blockTime(): Promise<number> { return this.client.blockTime(this.blockHash) }
  block_time(): Promise<number> { return this.blockTime() }
  isFastBlocks(): Promise<boolean> { return this.client.isFastBlocks(this.blockHash) }
  is_fast_blocks(): Promise<boolean> { return this.isFastBlocks() }
  blockInfo(block?: number | null): Promise<BlockInfo | null> { return this.client.blockInfo(block ?? this.block) }
  block_info(block?: number | null): Promise<BlockInfo | null> { return this.blockInfo(block) }
  reads() { return listReads() }
  read<Name extends ReadName>(name: Name, ...args: ReadArgs<Name>): Promise<ReadResult<Name>>
  read(name: string, params?: Record<string, unknown>): Promise<unknown>
  read(name: string, params: Record<string, unknown> = {}): Promise<unknown> { return dispatchRead(this, name, params) }

}

export class BalancesNamespace {
  constructor(private readonly client: ChainClient) {}

  async free(address: string, block?: number | string | null): Promise<Balance> {
    const account = await this.client.query<Record<string, ScaleValue>>(storage.System.Account, [address], block)
    const data = account?.data as Record<string, ScaleValue> | undefined
    return Balance.fromRao(String(data?.free ?? 0))
  }

  get(address: string, block?: number | string | null): Promise<Balance> {
    return this.free(address, block)
  }

  async getMany(addresses: string[], block?: number | string | null): Promise<Record<string, Balance>> {
    const accounts = await this.client.queryBatch<Record<string, ScaleValue>>(storage.System.Account, addresses.map((address) => [address]), block)
    const out: Record<string, Balance> = {}
    addresses.forEach((address, index) => {
      const data = accounts[index]?.data as Record<string, ScaleValue> | undefined
      out[address] = Balance.fromRao(String(data?.free ?? 0))
    })
    return out
  }

  get_many(addresses: string[], block?: number | string | null): Promise<Record<string, Balance>> {
    return this.getMany(addresses, block)
  }

  async existentialDeposit(block?: number | string | null): Promise<Balance> {
    return Balance.fromRao(String((await this.client.constant(constants.Balances.ExistentialDeposit, undefined, block)) ?? 0))
  }

  existential_deposit(block?: number | string | null): Promise<Balance> {
    return this.existentialDeposit(block)
  }
}

export interface SubnetInfo {
  netuid: number
  tempo: number
  burn: Balance
  neuronCount: number
  neuron_count: number
  state: import('./reads').SubnetLifecycleState | null
  pool_emission_enabled: boolean
}

export class SubnetsNamespace {
  constructor(private readonly client: ChainClient) {}

  async subnet(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    const info = await dispatchRead(await scopedReadView(this.client, block), 'subnet', { netuid })
    return { ...info, neuronCount: info.neuron_count }
  }

  info(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    return this.subnet(netuid, block)
  }

  async all(block?: number | string | null): Promise<SubnetInfo[]> {
    const infos = await dispatchRead(await scopedReadView(this.client, block), 'subnets')
    return infos.map(info => ({ ...info, neuronCount: info.neuron_count }))
  }

  async graph(netuid: number, options: { block?: ReadBlock; commitments?: boolean } = {}) {
    return fetchMetagraph(await scopedReadView(this.client, options.block), netuid, options)
  }

  async commitments(netuid: number, block?: ReadBlock) {
    return fetchCommitments(await scopedReadView(this.client, block), netuid)
  }

  async state(netuid: number, block?: ReadBlock) {
    return dispatchRead(await scopedReadView(this.client, block), 'subnet_state', { netuid })
  }

  async states(block?: ReadBlock) {
    return dispatchRead(await scopedReadView(this.client, block), 'subnet_states')
  }

  async exists(netuid: number, block?: number | string | null): Promise<boolean> {
    return Boolean(await this.client.query(storage.SubtensorModule.NetworksAdded, [netuid], block))
  }

  subnetExists(netuid: number, block?: number | string | null): Promise<boolean> {
    return this.exists(netuid, block)
  }

  subnet_exists(netuid: number, block?: number | string | null): Promise<boolean> {
    return this.exists(netuid, block)
  }

  async metagraph<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return await native.metagraph(netuid, blockHash) as T
    }
    return this.client.runtime<T>(runtimeApi.SubnetInfoRuntimeApi.get_metagraph, [netuid], block)
  }

  async hyperparameters<T = SubnetHyperparameters | null>(netuid: number, block?: number | string | null): Promise<T> {
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return await native.subnetHyperparameters(netuid, blockHash) as T
    }
    const value = await this.client.runtime<ScaleValue>(
      runtimeApi.SubnetInfoRuntimeApi.get_subnet_hyperparams_v3,
      [netuid],
      block,
    )
    return subnetHyperparametersFromRuntime(value) as T
  }

  subnetHyperparameters<T = SubnetHyperparameters | null>(netuid: number, block?: number | string | null): Promise<T> {
    return this.hyperparameters<T>(netuid, block)
  }

  subnet_hyperparameters<T = SubnetHyperparameters | null>(netuid: number, block?: number | string | null): Promise<T> {
    return this.hyperparameters<T>(netuid, block)
  }

  async commitRevealEnabled(netuid: number, block?: number | string | null): Promise<boolean> {
    return Boolean(await this.client.query(storage.SubtensorModule.CommitRevealWeightsEnabled, [netuid], block))
  }

  commit_reveal_enabled(netuid: number, block?: number | string | null): Promise<boolean> {
    return this.commitRevealEnabled(netuid, block)
  }

  burn(netuid: number, block?: number | string | null): Promise<Balance> {
    return this.subnet(netuid, block).then((info) => info.burn)
  }
}

export class NeuronsNamespace {
  constructor(private readonly client: ChainClient) {}

  async all<T extends ScaleValue = ScaleValue>(netuid: number, lite = true, block?: number | string | null): Promise<T> {
    const native = await nativeRustClient(this.client)
    if (native != null && lite) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return await native.neurons(netuid, blockHash) as T
    }
    return this.client.runtime<T>(
      lite ? runtimeApi.NeuronInfoRuntimeApi.get_neurons_lite : runtimeApi.NeuronInfoRuntimeApi.get_neurons,
      [netuid],
      block,
    )
  }

  get<T extends ScaleValue = ScaleValue>(netuid: number, uid: number, lite = true, block?: number | string | null): Promise<T> {
    return this.client.runtime<T>(
      lite ? runtimeApi.NeuronInfoRuntimeApi.get_neuron_lite : runtimeApi.NeuronInfoRuntimeApi.get_neuron,
      [netuid, uid],
      block,
    )
  }
}

export interface StakePosition {
  hotkey: string
  coldkey: string
  netuid: number
  stake: Balance
  isRegistered: boolean
  is_registered: boolean
  subnet_state: import('./reads').SubnetLifecycleState | null
  raw: Record<string, ScaleValue>
}

export class StakingNamespace {
  constructor(private readonly client: ChainClient) {}

  async get(coldkey: string, hotkey: string, netuid: number, block?: number | string | null): Promise<Balance> {
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return Balance.fromRao(await native.stakeRao(coldkey, hotkey, netuid, blockHash), netuid)
    }
    const info = await this.client.runtime<Record<string, ScaleValue> | null>(
      runtimeApi.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
      [hotkey, coldkey, netuid],
      block,
    )
    return Balance.fromRao(String(info?.stake ?? 0), netuid)
  }

  async positions(coldkey: string, block?: number | string | null): Promise<StakePosition[]> {
    const positions = await dispatchRead(await scopedReadView(this.client, block), 'stake_for_coldkey', { coldkey_ss58: coldkey })
    return positions.map(position => ({ ...position, isRegistered: position.is_registered }))
  }

  stake(coldkey: string, hotkey: string, netuid: number, block?: number | string | null): Promise<Balance> {
    return this.get(coldkey, hotkey, netuid, block)
  }

  stakeForColdkey(coldkey: string, block?: number | string | null): Promise<StakePosition[]> {
    return this.positions(coldkey, block)
  }

  stake_for_coldkey(coldkey: string, block?: number | string | null): Promise<StakePosition[]> {
    return this.positions(coldkey, block)
  }

  addStake(signer: SignerLike, hotkey: string, netuid: number, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.client.submit(
      IntentCall.addStake(hotkey, netuid, taoTransactionAmountRao(amount, 'stake amount')),
      signer,
      { waitForInclusion: true, ...options },
    )
  }

  add_stake(signer: SignerLike, hotkey: string, netuid: number, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.addStake(signer, hotkey, netuid, amount, options)
  }

  removeStake(signer: SignerLike, hotkey: string, netuid: number, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.client.submit(
      IntentCall.removeStake(hotkey, netuid, alphaTransactionAmountForNetuid(amount, netuid, 'unstake amount')),
      signer,
      { waitForInclusion: true, ...options },
    )
  }

  remove_stake(signer: SignerLike, hotkey: string, netuid: number, amount: TransactionAmount, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.removeStake(signer, hotkey, netuid, amount, options)
  }
}

export class SnapshotBalancesNamespace {
  constructor(private readonly snapshot: Snapshot) {}
  get(address: string): Promise<Balance> { return this.snapshot.client.balances.get(address, this.snapshot.blockHash) }
  getMany(addresses: string[]): Promise<Record<string, Balance>> { return this.snapshot.client.balances.getMany(addresses, this.snapshot.blockHash) }
}

export class SnapshotSubnetsNamespace {
  constructor(private readonly snapshot: Snapshot) {}
  graph(netuid: number, options: { commitments?: boolean } = {}) { return fetchMetagraph(this.snapshot, netuid, options) }
  commitments(netuid: number) { return fetchCommitments(this.snapshot, netuid) }
  state(netuid: number) { return dispatchRead(this.snapshot, 'subnet_state', { netuid }) }
  states() { return dispatchRead(this.snapshot, 'subnet_states') }

  info(netuid: number): Promise<SubnetInfo> { return this.snapshot.client.subnets.info(netuid, this.snapshot.blockHash) }
  all(): Promise<SubnetInfo[]> { return this.snapshot.client.subnets.all(this.snapshot.blockHash) }
  metagraph<T extends ScaleValue = ScaleValue>(netuid: number): Promise<T> { return this.snapshot.client.subnets.metagraph<T>(netuid, this.snapshot.blockHash) }
}

export class SnapshotStakingNamespace {
  constructor(private readonly snapshot: Snapshot) {}
  get(coldkey: string, hotkey: string, netuid: number): Promise<Balance> { return this.snapshot.client.staking.get(coldkey, hotkey, netuid, this.snapshot.blockHash) }
  positions(coldkey: string): Promise<StakePosition[]> { return this.snapshot.client.staking.positions(coldkey, this.snapshot.blockHash) }
}

export class SnapshotNeuronsNamespace {
  constructor(private readonly snapshot: Snapshot) {}
  all<T extends ScaleValue = ScaleValue>(netuid: number, lite = true): Promise<T> { return this.snapshot.client.neurons.all<T>(netuid, lite, this.snapshot.blockHash) }
}

export class SubtensorClient extends Client {}
export const Subtensor = SubtensorClient

export function subtensor(network: string = 'finney', options: ClientOptions = {}): Client {
  return new Client(network, options)
}

export function call(pallet: string, fn: string, params: ScaleValue = {}): [string, string, ScaleValue] {
  return [pallet, fn, params]
}

export function descriptor(pallet: string, item: string): Descriptor {
  return [pallet, item]
}

export const storage = generatedStorage
export const constants = generatedConstants
export const runtimeApi = generatedRuntimeApi
export const runtimeApis = runtimeApi

export const calls = Object.freeze({
  ...generatedCalls,
  balances: Object.freeze({
    transferKeepAlive(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_keep_alive', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
    transferAllowDeath(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_allow_death', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
  }),
  subtensor: Object.freeze({
    addStake(hotkey: string, netuid: number, amount: TransactionAmount) {
      return call('SubtensorModule', 'add_stake', { hotkey, netuid, amount_staked: taoTransactionAmountRao(amount, 'stake amount') })
    },
    burnedRegister(netuid: number, hotkey: string) {
      return call('SubtensorModule', 'burned_register', { netuid, hotkey })
    },
    commitWeights(netuid: number, commitHash: ByteLike | string) {
      return call('SubtensorModule', 'commit_weights', { netuid, commit_hash: commitHash })
    },
    moveStake(originHotkey: string, destinationHotkey: string, originNetuid: number, destinationNetuid: number, amount: TransactionAmount) {
      return call('SubtensorModule', 'move_stake', {
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: alphaTransactionAmountForNetuid(amount, originNetuid, 'alpha amount'),
      })
    },
    register(netuid: number, blockNumber: bigint | number | string, nonce: bigint | number | string, work: ByteLike, hotkey: string, coldkey: string) {
      return call('SubtensorModule', 'register', { netuid, block_number: toBigInt(blockNumber, 'blockNumber'), nonce: toBigInt(nonce, 'nonce'), work, hotkey, coldkey })
    },
    registerNetwork(hotkey: string) {
      return call('SubtensorModule', 'register_network', { hotkey })
    },
    removeStake(hotkey: string, netuid: number, amount: TransactionAmount) {
      return call('SubtensorModule', 'remove_stake', { hotkey, netuid, amount_unstaked: alphaTransactionAmountForNetuid(amount, netuid, 'unstake amount') })
    },
    revealWeights(netuid: number, uids: number[], values: number[], salt: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'reveal_weights', { netuid, uids, values, salt, version_key: toBigInt(versionKey, 'versionKey') })
    },
    rootRegister(hotkey: string) {
      return call('SubtensorModule', 'root_register', { hotkey })
    },
    serveAxon(netuid: number, ip: ServeIp, port: number, version = 0, ipType?: number, protocol = 4) {
      const normalizedIp = normalizeServeIp(ip, ipType)
      return call('SubtensorModule', 'serve_axon', { netuid, version, ip: normalizedIp.value, port, ip_type: normalizedIp.type, protocol, placeholder1: 0, placeholder2: 0 })
    },
    servePrometheus(netuid: number, ip: ServeIp, port: number, version = 0, ipType?: number) {
      const normalizedIp = normalizeServeIp(ip, ipType)
      return call('SubtensorModule', 'serve_prometheus', { netuid, version, ip: normalizedIp.value, port, ip_type: normalizedIp.type })
    },
    setChildren(hotkey: string, netuid: number, children: ScaleValue) {
      return call('SubtensorModule', 'set_children', { hotkey, netuid, children })
    },
    setWeights(netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'set_weights', { netuid, dests, weights, version_key: toBigInt(versionKey, 'versionKey') })
    },
    startCall(netuid: number) {
      return call('SubtensorModule', 'start_call', { netuid })
    },
    transferStake(destinationColdkey: string, hotkey: string, originNetuid: number, destinationNetuid: number, amount: TransactionAmount) {
      return call('SubtensorModule', 'transfer_stake', {
        destination_coldkey: destinationColdkey,
        hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: alphaTransactionAmountForNetuid(amount, originNetuid, 'alpha amount'),
      })
    },
    unstakeAll(hotkey: string) {
      return call('SubtensorModule', 'unstake_all', { hotkey })
    },
  }),
  Balances: Object.freeze({
    ...generatedCalls.Balances,
    transfer_keep_alive(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_keep_alive', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
    transfer_allow_death(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_allow_death', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
  }),
  SubtensorModule: Object.freeze({
    ...generatedCalls.SubtensorModule,
    add_stake(hotkey: string, netuid: number, amountStaked: TransactionAmount) {
      return call('SubtensorModule', 'add_stake', { hotkey, netuid, amount_staked: taoTransactionAmountRao(amountStaked, 'stake amount') })
    },
    burned_register(netuid: number, hotkey: string) {
      return call('SubtensorModule', 'burned_register', { netuid, hotkey })
    },
    commit_weights(netuid: number, commitHash: ByteLike | string) {
      return call('SubtensorModule', 'commit_weights', { netuid, commit_hash: commitHash })
    },
    move_stake(originHotkey: string, destinationHotkey: string, originNetuid: number, destinationNetuid: number, alphaAmount: TransactionAmount) {
      return call('SubtensorModule', 'move_stake', {
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: alphaTransactionAmountForNetuid(alphaAmount, originNetuid, 'alpha amount'),
      })
    },
    register(netuid: number, blockNumber: bigint | number | string, nonce: bigint | number | string, work: ByteLike, hotkey: string, coldkey: string) {
      return call('SubtensorModule', 'register', { netuid, block_number: toBigInt(blockNumber, 'blockNumber'), nonce: toBigInt(nonce, 'nonce'), work, hotkey, coldkey })
    },
    register_network(hotkey: string) {
      return call('SubtensorModule', 'register_network', { hotkey })
    },
    remove_stake(hotkey: string, netuid: number, amountUnstaked: TransactionAmount) {
      return call('SubtensorModule', 'remove_stake', { hotkey, netuid, amount_unstaked: alphaTransactionAmountForNetuid(amountUnstaked, netuid, 'unstake amount') })
    },
    reveal_weights(netuid: number, uids: number[], values: number[], salt: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'reveal_weights', { netuid, uids, values, salt, version_key: toBigInt(versionKey, 'versionKey') })
    },
    root_register(hotkey: string) {
      return call('SubtensorModule', 'root_register', { hotkey })
    },
    serve_axon(netuid: number, ip: ServeIp, port: number, version = 0, ipType?: number, protocol = 4) {
      const normalizedIp = normalizeServeIp(ip, ipType)
      return call('SubtensorModule', 'serve_axon', { netuid, version, ip: normalizedIp.value, port, ip_type: normalizedIp.type, protocol, placeholder1: 0, placeholder2: 0 })
    },
    serve_prometheus(netuid: number, ip: ServeIp, port: number, version = 0, ipType?: number) {
      const normalizedIp = normalizeServeIp(ip, ipType)
      return call('SubtensorModule', 'serve_prometheus', { netuid, version, ip: normalizedIp.value, port, ip_type: normalizedIp.type })
    },
    set_children(hotkey: string, netuid: number, children: ScaleValue) {
      return call('SubtensorModule', 'set_children', { hotkey, netuid, children })
    },
    set_weights(netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'set_weights', { netuid, dests, weights, version_key: toBigInt(versionKey, 'versionKey') })
    },
    start_call(netuid: number) {
      return call('SubtensorModule', 'start_call', { netuid })
    },
    transfer_stake(destinationColdkey: string, hotkey: string, originNetuid: number, destinationNetuid: number, alphaAmount: TransactionAmount) {
      return call('SubtensorModule', 'transfer_stake', {
        destination_coldkey: destinationColdkey,
        hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: alphaTransactionAmountForNetuid(alphaAmount, originNetuid, 'alpha amount'),
      })
    },
    unstake_all(hotkey: string) {
      return call('SubtensorModule', 'unstake_all', { hotkey })
    },
  }),
})

interface CallDescriptorEntry {
  path: string
  descriptor: Descriptor
  args: readonly string[]
  argTypes?: readonly string[]
}

const CALL_DESCRIPTOR_ENTRIES: CallDescriptorEntry[] = [
  { path: 'calls.balances.transferKeepAlive', descriptor: descriptor('Balances', 'transfer_keep_alive'), args: ['dest', 'value'], argTypes: ['MultiAddress<AccountId32, ()>', 'Compact<TaoBalance>'] },
  { path: 'calls.balances.transferAllowDeath', descriptor: descriptor('Balances', 'transfer_allow_death'), args: ['dest', 'value'], argTypes: ['MultiAddress<AccountId32, ()>', 'Compact<TaoBalance>'] },
  { path: 'calls.subtensor.addStake', descriptor: descriptor('SubtensorModule', 'add_stake'), args: ['hotkey', 'netuid', 'amount_staked'], argTypes: ['AccountId32', 'NetUid', 'TaoBalance'] },
  { path: 'calls.subtensor.burnedRegister', descriptor: descriptor('SubtensorModule', 'burned_register'), args: ['netuid', 'hotkey'], argTypes: ['NetUid', 'AccountId32'] },
  { path: 'calls.subtensor.commitWeights', descriptor: descriptor('SubtensorModule', 'commit_weights'), args: ['netuid', 'commit_hash'], argTypes: ['NetUid', 'H256'] },
  { path: 'calls.subtensor.moveStake', descriptor: descriptor('SubtensorModule', 'move_stake'), args: ['origin_hotkey', 'destination_hotkey', 'origin_netuid', 'destination_netuid', 'alpha_amount'], argTypes: ['AccountId32', 'AccountId32', 'NetUid', 'NetUid', 'AlphaBalance'] },
  { path: 'calls.subtensor.register', descriptor: descriptor('SubtensorModule', 'register'), args: ['netuid', 'block_number', 'nonce', 'work', 'hotkey', 'coldkey'], argTypes: ['NetUid', 'u64', 'u64', 'Vec<u8>', 'AccountId32', 'AccountId32'] },
  { path: 'calls.subtensor.registerNetwork', descriptor: descriptor('SubtensorModule', 'register_network'), args: ['hotkey'], argTypes: ['AccountId32'] },
  { path: 'calls.subtensor.removeStake', descriptor: descriptor('SubtensorModule', 'remove_stake'), args: ['hotkey', 'netuid', 'amount_unstaked'], argTypes: ['AccountId32', 'NetUid', 'AlphaBalance'] },
  { path: 'calls.subtensor.revealWeights', descriptor: descriptor('SubtensorModule', 'reveal_weights'), args: ['netuid', 'uids', 'values', 'salt', 'version_key'], argTypes: ['NetUid', 'Vec<u16>', 'Vec<u16>', 'Vec<u16>', 'u64'] },
  { path: 'calls.subtensor.rootRegister', descriptor: descriptor('SubtensorModule', 'root_register'), args: ['hotkey'], argTypes: ['AccountId32'] },
  { path: 'calls.subtensor.serveAxon', descriptor: descriptor('SubtensorModule', 'serve_axon'), args: ['netuid', 'version', 'ip', 'port', 'ip_type', 'protocol', 'placeholder1', 'placeholder2'], argTypes: ['NetUid', 'u32', 'u128', 'u16', 'u8', 'u8', 'u8', 'u8'] },
  { path: 'calls.subtensor.servePrometheus', descriptor: descriptor('SubtensorModule', 'serve_prometheus'), args: ['netuid', 'version', 'ip', 'port', 'ip_type'], argTypes: ['NetUid', 'u32', 'u128', 'u16', 'u8'] },
  { path: 'calls.subtensor.setChildren', descriptor: descriptor('SubtensorModule', 'set_children'), args: ['hotkey', 'netuid', 'children'], argTypes: ['AccountId32', 'NetUid', 'Vec<(u64, AccountId32)>'] },
  { path: 'calls.subtensor.setWeights', descriptor: descriptor('SubtensorModule', 'set_weights'), args: ['netuid', 'dests', 'weights', 'version_key'], argTypes: ['NetUid', 'Vec<u16>', 'Vec<u16>', 'u64'] },
  { path: 'calls.subtensor.startCall', descriptor: descriptor('SubtensorModule', 'start_call'), args: ['netuid'], argTypes: ['NetUid'] },
  { path: 'calls.subtensor.transferStake', descriptor: descriptor('SubtensorModule', 'transfer_stake'), args: ['destination_coldkey', 'hotkey', 'origin_netuid', 'destination_netuid', 'alpha_amount'], argTypes: ['AccountId32', 'AccountId32', 'NetUid', 'NetUid', 'AlphaBalance'] },
  { path: 'calls.subtensor.unstakeAll', descriptor: descriptor('SubtensorModule', 'unstake_all'), args: ['hotkey'], argTypes: ['AccountId32'] },
]

for (const entry of callDescriptors) {
  if (CALL_DESCRIPTOR_ENTRIES.some(existing => existing.descriptor[0] === entry.pallet && existing.descriptor[1] === entry.function)) continue
  CALL_DESCRIPTOR_ENTRIES.push({ path: `calls.${entry.pallet}.${entry.function}`, descriptor: [entry.pallet, entry.function], args: entry.args })
}

export function validateDescriptorSchema(runtime: Runtime): DescriptorSchemaIssue[] {
  const issues: DescriptorSchemaIssue[] = []
  for (const entry of descriptorEntries(storage, 'storage')) {
    const [pallet, item] = entry.descriptor
    const palletInfo = runtime.pallet(pallet)
    if (palletInfo == null) {
      issues.push({ ...entry, kind: 'storage', message: `pallet ${pallet} not found` })
    } else if (!palletInfo.storage.some((storageItem) => storageItem.name === item)) {
      issues.push({ ...entry, kind: 'storage', message: `storage item ${pallet}.${item} not found` })
    }
  }
  for (const entry of descriptorEntries(constants, 'constants')) {
    const [pallet, item] = entry.descriptor
    const palletInfo = runtime.pallet(pallet)
    if (palletInfo == null) {
      issues.push({ ...entry, kind: 'constant', message: `pallet ${pallet} not found` })
    } else if (runtime.constantInfo(pallet, item) == null) {
      issues.push({ ...entry, kind: 'constant', message: `constant ${pallet}.${item} not found` })
    }
  }
  const apiMap = runtime.runtimeApis()
  for (const entry of descriptorEntries(runtimeApi, 'runtimeApi')) {
    const [api, method] = entry.descriptor
    if (apiMap[api] == null) {
      issues.push({ ...entry, kind: 'runtimeApi', message: `runtime API ${api} not found` })
    } else if (apiMap[api][method] == null) {
      issues.push({ ...entry, kind: 'runtimeApi', message: `runtime API method ${api}.${method} not found` })
    } else {
      const methodInfo = apiMap[api][method]
      const inputs = methodInfo.inputs ?? []
      const inputDetails = methodInfo.inputDetails ?? []
      if (inputDetails.length > 0 && inputs.length !== inputDetails.length) {
        issues.push({
          ...entry,
          kind: 'runtimeApi',
          message: `runtime API method ${api}.${method} input metadata disagrees: inputs=${inputs.length}, inputDetails=${inputDetails.length}`,
        })
      }
      for (let index = 0; index < Math.min(inputs.length, inputDetails.length); index += 1) {
        const [inputName, inputType] = inputs[index]
        const detail = inputDetails[index]
        if (inputName !== detail.name || inputType !== detail.type) {
          issues.push({
            ...entry,
            kind: 'runtimeApi',
            message: `runtime API method ${api}.${method} input ${index} drifted: expected ${inputName}: ${inputType}, got ${detail.name}: ${detail.type}`,
          })
        }
      }
    }
  }
  const metadata = runtime.metadataIr()
  for (const entry of CALL_DESCRIPTOR_ENTRIES) {
    const [pallet, item] = entry.descriptor
    const palletInfo = metadata.pallets.find((candidate) => candidate.name === pallet)
    if (palletInfo == null) {
      issues.push({ ...entry, kind: 'call', message: `pallet ${pallet} not found` })
    } else {
      const callInfo = palletInfo.calls.find((candidate) => candidate.name === item)
      if (callInfo == null) {
        issues.push({ ...entry, kind: 'call', message: `call ${pallet}.${item} not found` })
        continue
      }
      const actualArgs = callInfo.args ?? []
      if (actualArgs.length !== entry.args.length) {
        issues.push({
          ...entry,
          kind: 'call',
          message: `call ${pallet}.${item} argument count drifted: expected ${entry.args.length}, got ${actualArgs.length}`,
        })
        continue
      }
      for (let index = 0; index < entry.args.length; index += 1) {
        if (actualArgs[index] !== entry.args[index]) {
          issues.push({
            ...entry,
            kind: 'call',
            message: `call ${pallet}.${item} argument ${index} drifted: expected ${entry.args[index]}, got ${actualArgs[index]}`,
          })
        }
      }
      if (entry.argTypes == null) continue
      const actualArgTypeIds = callArgumentTypeIds(callInfo)
      if (actualArgTypeIds == null) {
        issues.push({
          ...entry,
          kind: 'call',
          message: `call ${pallet}.${item} argument type ID metadata is unavailable`,
        })
      } else if (actualArgTypeIds.length !== entry.argTypes.length) {
        issues.push({
          ...entry,
          kind: 'call',
          message: `call ${pallet}.${item} argument type count drifted: expected ${entry.argTypes.length}, got ${actualArgTypeIds.length}`,
        })
      } else {
        const actualArgTypes = actualArgTypeIds.map((typeId) => runtimeTypeName(runtime, typeId))
        for (let index = 0; index < entry.argTypes.length; index += 1) {
          if (!runtimeTypeMatches(entry.argTypes[index], actualArgTypes[index])) {
            issues.push({
              ...entry,
              kind: 'call',
              message: `call ${pallet}.${item} argument ${index} type drifted: expected ${entry.argTypes[index]}, got ${actualArgTypes[index]} (type ID ${actualArgTypeIds[index]})`,
            })
          }
        }
      }
    }
  }
  return issues
}

function descriptorSchemaDriftError(issues: DescriptorSchemaIssue[]): ChainError {
  const sample = issues.slice(0, 8).map((issue) => `${issue.path}: ${issue.message}`)
  return new ChainError(
    `descriptor schema drift detected (${issues.length} issue${issues.length === 1 ? '' : 's'}): ${sample.join('; ')}`,
    issues,
  )
}

function callArgumentTypeIds(callInfo: { argTypeIds?: unknown; argTypes?: unknown }): number[] | null {
  if (Array.isArray(callInfo.argTypeIds)) {
    const ids = callInfo.argTypeIds.map((value) => Number(value))
    return ids.every((value) => Number.isSafeInteger(value) && value >= 0) ? ids : null
  }
  if (Array.isArray(callInfo.argTypes)) {
    const ids = callInfo.argTypes.map((value) => {
      const match = /^scale_info::(\d+)$/.exec(String(value))
      return match == null ? NaN : Number(match[1])
    })
    return ids.every((value) => Number.isSafeInteger(value) && value >= 0) ? ids : null
  }
  return null
}

const RUNTIME_TYPE_ALIASES = new Map<string, string>([
  ['AlphaBalance', 'u64'],
  ['Compact<TaoBalance>', 'Compact<u64>'],
  ['NetUid', 'u16'],
  ['TaoBalance', 'u64'],
])

function runtimeTypeMatches(expected: string, actual: string): boolean {
  return actual === expected || actual === RUNTIME_TYPE_ALIASES.get(expected)
}

function runtimeTypeName(runtime: Runtime, typeId: number): string {
  try {
    return runtime.typeNameOf(typeId) ?? `scale_info::${typeId}`
  } catch {
    return `scale_info::${typeId}`
  }
}

function descriptorEntries(value: unknown, prefix: string): Array<{ path: string; descriptor: Descriptor }> {
  if (isDescriptor(value)) return [{ path: prefix, descriptor: value }]
  if (typeof value !== 'object' || value == null) return []
  return Object.entries(value).flatMap(([key, child]) =>
    descriptorEntries(child, `${prefix}.${key}`),
  )
}

function isDescriptor(value: unknown): value is Descriptor {
  return (
    Array.isArray(value) &&
    value.length === 2 &&
    typeof value[0] === 'string' &&
    typeof value[1] === 'string'
  )
}

export async function scopedReadView(client: ChainClient, block?: ReadBlock): Promise<Snapshot> {
  if (typeof block === 'string') return new Snapshot(client, await client.blockNumber(block), block)
  return client.at(block)
}

function decodeStorageValue<T extends ScaleValue>(
  runtime: Runtime,
  entry: StorageEntry,
  value: unknown,
): T | undefined {
  const bytes = value == null ? entry.defaultBytes : hexToBuffer(String(value))
  if (bytes == null || bytes.length === 0) return undefined
  return runtime.decode<T>(entry.valueType, bytes, false)
}

function firstProperty(value: unknown): unknown {
  return Array.isArray(value) ? value[0] : value
}

function propertyNumber(value: unknown, fallback: number): number {
  const item = firstProperty(value)
  if (item == null) return fallback
  const parsed = Number(item)
  return Number.isFinite(parsed) ? parsed : fallback
}

function propertyString(value: unknown, fallback: string): string {
  const item = firstProperty(value)
  return item == null ? fallback : String(item)
}

function runtimeVersionInfo(value: unknown): RuntimeVersionInfo {
  const version = value as { specVersion?: unknown; transactionVersion?: unknown }
  const specVersion = Number(version.specVersion)
  const transactionVersion = Number(version.transactionVersion)
  if (!Number.isFinite(specVersion) || !Number.isFinite(transactionVersion)) {
    throw new ChainError('runtime version response is missing specVersion or transactionVersion', value)
  }
  return { specVersion, transactionVersion }
}

function sameRuntimeVersion(
  entry: RuntimeVersionInfo & { ss58Format: number },
  version: RuntimeVersionInfo,
  ss58Format: number,
): boolean {
  return (
    entry.specVersion === version.specVersion &&
    entry.transactionVersion === version.transactionVersion &&
    entry.ss58Format === ss58Format
  )
}

function normalizeQueryMapOptions(value: number | QueryMapOptions): Required<Pick<QueryMapOptions, 'pageSize'>> & Omit<QueryMapOptions, 'pageSize'> {
  if (typeof value === 'number') return { pageSize: positiveInteger(value, 'queryMap pageSize') }
  return {
    ...value,
    pageSize: positiveInteger(value.pageSize ?? 512, 'queryMap pageSize'),
    maxPages: value.maxPages == null ? undefined : positiveInteger(value.maxPages, 'queryMap maxPages'),
    maxResults: value.maxResults == null ? undefined : positiveInteger(value.maxResults, 'queryMap maxResults'),
  }
}

function positiveInteger(value: unknown, name: string): number {
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new RangeError(`${name} must be a positive safe integer`)
  return parsed
}

function alphaTransactionAmountForNetuid(value: TransactionAmount, netuid: number, name: string): bigint {
  if (value instanceof Balance && value.netuid !== netuid) {
    throw new UnitMismatchError(`${name} must be subnet-${netuid} alpha, not ${value.netuid === 0 ? 'TAO' : `subnet-${value.netuid} alpha`}`)
  }
  const unit = brandedAmountUnit(value)
  if (unit === 'tao') {
    throw new UnitMismatchError(`${name} must be subnet-${netuid} alpha, not TAO`)
  }
  if (unit === 'alpha') {
    const amountNetuid = brandedAmountNetuid(value)
    if (amountNetuid !== netuid) {
      throw new UnitMismatchError(`${name} must be subnet-${netuid} alpha, not subnet-${amountNetuid ?? 'unknown'} alpha`)
    }
  }
  return transactionAmountRao(value, { name })
}

const IPV4_MAX = 0xffff_ffffn
const IPV6_MAX_EXCLUSIVE = 1n << 128n

function normalizeServeIp(value: ServeIp, ipType?: number): { value: bigint | number; type: number } {
  if (ipType != null && ipType !== 4 && ipType !== 6) {
    throw new RangeError('ipType must be 4 or 6')
  }
  let parsed: bigint
  let syntaxType: number | undefined
  if (typeof value === 'bigint') {
    parsed = value
  } else if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new RangeError('ip must be a safe integer, bigint, or string')
    parsed = BigInt(value)
  } else {
    const text = value.trim()
    if (/^\d+$/.test(text)) parsed = BigInt(text)
    else if (/^0x[0-9a-fA-F]+$/.test(text)) parsed = BigInt(text)
    else if (text.includes(':')) {
      parsed = parseIpv6(text)
      syntaxType = 6
    } else if (text.includes('.')) {
      parsed = parseIpv4(text)
      syntaxType = 4
    } else {
      throw new RangeError('ip string must be decimal, hex, IPv4, or IPv6')
    }
  }
  if (parsed < 0n || parsed >= IPV6_MAX_EXCLUSIVE) throw new RangeError('ip must fit in u128')
  if (syntaxType != null && ipType != null && syntaxType !== ipType) {
    throw new RangeError('ipType does not match IP address family')
  }
  const inferredType = syntaxType ?? ipType ?? (parsed <= IPV4_MAX ? 4 : 6)
  if (inferredType === 4 && parsed > IPV4_MAX) {
    throw new RangeError('IPv4 address must fit in u32')
  }
  return {
    value: parsed <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(parsed) : parsed,
    type: inferredType,
  }
}

function parseIpv4(value: string): bigint {
  const parts = value.split('.')
  if (parts.length !== 4) throw new RangeError('invalid IPv4 address')
  let out = 0n
  for (const part of parts) {
    if (!/^\d+$/.test(part)) throw new RangeError('invalid IPv4 address')
    const byte = Number(part)
    if (!Number.isInteger(byte) || byte < 0 || byte > 255) throw new RangeError('invalid IPv4 address')
    out = (out << 8n) + BigInt(byte)
  }
  return out
}

function parseIpv6(value: string): bigint {
  const zoneFree = value.split('%', 1)[0]
  const doubleColon = zoneFree.indexOf('::')
  if (doubleColon !== zoneFree.lastIndexOf('::')) throw new RangeError('invalid IPv6 address')
  const parseSide = (side: string): number[] => side === ''
    ? []
    : side.split(':').flatMap((part) => {
        if (part.includes('.')) {
          const ipv4 = parseIpv4(part)
          return [Number((ipv4 >> 16n) & 0xffffn), Number(ipv4 & 0xffffn)]
        }
        if (!/^[0-9a-fA-F]{1,4}$/.test(part)) throw new RangeError('invalid IPv6 address')
        return [Number.parseInt(part, 16)]
      })
  const left = parseSide(doubleColon >= 0 ? zoneFree.slice(0, doubleColon) : zoneFree)
  const right = doubleColon >= 0 ? parseSide(zoneFree.slice(doubleColon + 2)) : []
  const missing = doubleColon >= 0 ? 8 - left.length - right.length : 0
  if (doubleColon >= 0 && missing <= 0) throw new RangeError('invalid IPv6 address')
  const groups = doubleColon >= 0 ? [...left, ...Array(missing).fill(0), ...right] : left
  if (missing < 0 || groups.length !== 8) throw new RangeError('invalid IPv6 address')
  return groups.reduce((out, group) => (out << 16n) + BigInt(group), 0n)
}

function accountAddress(account?: SignerAccount | null): string | undefined {
  return stringValue(account?.ss58Address) ?? stringValue(account?.address)
}

function staticSignerAddress(signer: unknown): string | undefined {
  const value = signer as { ss58Address?: unknown; address?: unknown }
  return stringValue(value.ss58Address) ?? stringValue(value.address)
}

function sameHex(left: string, right: string): boolean {
  return left.toLowerCase() === right.toLowerCase()
}

function normalizeHash32(value: string, name: string): string {
  const bytes = hexToBuffer(value)
  if (bytes.length !== 32) throw new ChainError(`${name} must be a 32-byte hex string`)
  return hex(bytes)
}

function parseNonce(value: unknown, name: string): number {
  const nonce = typeof value === 'bigint' ? Number(value) : Number(value)
  if (!Number.isSafeInteger(nonce) || nonce < 0) {
    throw new ChainError(`${name} returned invalid nonce ${String(value)}`)
  }
  return nonce
}

function isMetadataAtVersionUnavailable(error: unknown): boolean {
  const message = String(error instanceof Error ? error.message : error)
  const data = String(error instanceof JsonRpcError && error.data != null ? error.data : '')
  const text = `${message} ${data}`
  return text.includes(V15_METADATA_MISSING_NEEDLE) ||
    (text.includes('Metadata_metadata_at_version') && /not found|unknown function/i.test(text))
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function signerPublicKey(value: ByteLike | undefined, ss58Address: string): Buffer {
  return value == null
    ? publicKeyFromSs58(ss58Address)
    : toBuffer(value, 'signer.publicKey')
}

function nativeExternalSigningOptions(options: SubmitOptions): NativeExternalSigningOptions {
  const out: NativeExternalSigningOptions = {}
  if (options.nonce != null) out.nonce = toBigInt(options.nonce, 'nonce')
  if (options.period === null) {
    out.immortal = true
  } else if (options.period !== undefined) {
    out.period = toBigInt(options.period, 'period')
  }
  if (options.tip != null) {
    out.tip = taoTransactionAmountRao(options.tip, 'tip')
  }
  if (options.tipAssetId != null) {
    out.tipAssetId = assetIdValue(options.tipAssetId, 'tipAssetId')
  }
  if (hasOwn(options, 'metadataHash')) {
    if (options.metadataHash == null) {
      out.metadataHashMode = 'disabled'
    } else {
      out.metadataHashMode = 'explicit'
      out.metadataHash = toBuffer(options.metadataHash, 'metadataHash')
    }
  }
  return out
}

const SUBNET_HYPERPARAMETER_VALUE_TYPES = new Set<string>([
  'Bool',
  'U16',
  'U32',
  'U64',
  'U128',
  'TaoBalance',
  'I32F32',
  'U64F64',
])

function subnetHyperparametersFromRuntime(value: ScaleValue): SubnetHyperparameters | null {
  if (value == null) return null
  if (!Array.isArray(value)) throw new ChainError('get_subnet_hyperparams_v3 returned a non-list value')
  return value.map((entry) => {
    const record = scaleRecord(entry, 'hyperparameter entry')
    const name = hyperparameterName(record.name)
    const variant = scaleRecord(record.value, `hyperparameter ${name} value`)
    const keys = Object.keys(variant)
    if (keys.length !== 1) {
      throw new ChainError(`hyperparameter ${name} value is not a single V3 enum variant`)
    }
    const valueType = keys[0]
    if (!SUBNET_HYPERPARAMETER_VALUE_TYPES.has(valueType)) {
      throw new ChainError(`unknown subnet hyperparameter V3 value type ${valueType}`)
    }
    const raw = variant[valueType]
    const value = valueType === 'I32F32' || valueType === 'U64F64'
      ? scaleRecord(raw, `hyperparameter ${name} ${valueType} value`).bits
      : raw
    return { name, valueType, value } as SubnetHyperparameters[number]
  })
}

function scaleRecord(value: ScaleValue, name: string): Record<string, ScaleValue> {
  if (
    value == null ||
    typeof value !== 'object' ||
    Buffer.isBuffer(value) ||
    value instanceof Uint8Array ||
    Array.isArray(value) ||
    value instanceof Map
  ) {
    throw new ChainError(`${name} must be an object`)
  }
  return value as Record<string, ScaleValue>
}

function hyperparameterName(value: ScaleValue): string {
  if (typeof value === 'string') return value
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return toBuffer(value, 'hyperparameter name').toString('utf8')
  if (Array.isArray(value)) return Buffer.from(value.map((byte) => Number(byte))).toString('utf8')
  throw new ChainError('hyperparameter entry name must be bytes or string')
}

function nativeSubmitOptions(
  options: Pick<SubmitOptions, 'waitForInclusion' | 'waitForFinalization' | 'timeoutMs'>,
): NativeSubmitOptions {
  const out: NativeSubmitOptions = {}
  if (options.waitForFinalization === true) out.waitForFinalization = true
  else if (options.waitForInclusion === true) out.waitForInclusion = true
  if (options.timeoutMs != null) out.timeoutMs = BigInt(nonNegativeSafeInteger(options.timeoutMs, 'timeoutMs'))
  return out
}

function nonNegativeSafeInteger(value: unknown, name: string): number {
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new RangeError(`${name} must be a non-negative safe integer`)
  }
  return parsed
}

function nativeCancellationForSignal(signal?: AbortSignal): NativeCancellationToken | undefined {
  return signal == null ? undefined : createNativeCancellationToken()
}

function abortableNative<T>(
  promise: Promise<T>,
  signal?: AbortSignal,
  cancellation?: NativeCancellationToken,
): Promise<T> {
  if (signal?.aborted) {
    cancellation?.cancel()
    throw new RequestAbortedError()
  }
  if (signal == null) return promise
  return new Promise((resolve, reject) => {
    const abort = () => {
      cleanup()
      cancellation?.cancel()
      reject(new RequestAbortedError())
    }
    const cleanup = () => signal.removeEventListener('abort', abort)
    signal.addEventListener('abort', abort, { once: true })
    promise.then(
      (value) => {
        cleanup()
        resolve(value)
      },
      (error) => {
        cleanup()
        reject(error)
      },
    )
  })
}

function joinedAbortSignal(left?: AbortSignal, right?: AbortSignal): AbortSignal | undefined {
  if (left == null) return right
  if (right == null) return left
  const controller = new AbortController()
  const abort = () => controller.abort()
  if (left.aborted || right.aborted) abort()
  else {
    left.addEventListener('abort', abort, { once: true })
    right.addEventListener('abort', abort, { once: true })
  }
  return controller.signal
}

function nativePlanSignerContext(
  client: ChainClient,
  runtime: Runtime,
  resolved: ResolvedSigner,
  plan: NativeExternalSigningPlanHandle,
): SignerPayloadContext {
  const metadataHash = plan.metadataHash == null ? null : Buffer.from(plan.metadataHash)
  const metadataProof = plan.metadataProof == null ? undefined : Buffer.from(plan.metadataProof)
  return {
    client,
    runtime,
    address: resolved.ss58Address,
    publicKey: Buffer.from(plan.publicKey),
    cryptoType: plan.cryptoType,
    callData: Buffer.from(plan.callData),
    payload: Buffer.from(plan.payload),
    txParams: nativeTxParamsToTransactionParams(plan.txParams),
    metadataHash,
    metadataProof,
    proof: metadataProof,
    includedInExtrinsic: Buffer.from(plan.includedInExtrinsic),
    includedInSignedData: Buffer.from(plan.includedInSignedData),
    chainInfo: plan.chainInfo == null ? undefined : nativeChainInfoToChainInfo(plan.chainInfo),
    signerPayload: plan.signerPayload,
  }
}

function nativeTxParamsToTransactionParams(
  params: NativeExternalSigningPlanHandle['txParams'],
): TransactionParams {
  return {
    era: fromWire(params.era),
    nonce: params.nonce,
    tip: params.tip,
    tipAssetId: params.tipAssetId ?? null,
    genesisHash: Buffer.from(params.genesisHash),
    eraBlockHash: Buffer.from(params.eraBlockHash),
    metadataHash: params.metadataHash == null ? null : Buffer.from(params.metadataHash),
  }
}

function nativeChainInfoToChainInfo(info: NativeChainInfo): ChainInfo {
  return {
    specVersion: info.specVersion,
    specName: info.specName,
    base58Prefix: info.base58Prefix,
    decimals: info.decimals,
    tokenSymbol: info.tokenSymbol,
  }
}

function extensionSignPayload(context: SignerPayloadContext): ExtensionSignPayloadRequest {
  return context.signerPayload ?? context.runtime.signerPayload(context.address, context.callData, context.txParams)
}

function normalizeSignature(
  result: SignerSignature,
  fallbackCryptoType: number,
): NormalizedSignature {
  if (
    result != null &&
    typeof result === 'object' &&
    !(Buffer.isBuffer(result) || result instanceof Uint8Array) &&
    (result as { signedTransaction?: unknown }).signedTransaction != null
  ) {
    throw new ChainError('signer returned signedTransaction, which is not supported by this signing path')
  }
  const raw = signatureBytes(result)
  if (raw.length === 65 && raw[0] <= CRYPTO_SR25519) {
    return { signature: Buffer.from(raw.subarray(1)), cryptoType: raw[0] }
  }
  return { signature: raw, cryptoType: fallbackCryptoType }
}

function signatureBytes(result: SignerSignature): Buffer {
  if (typeof result === 'string') return hexToBuffer(result)
  if (Buffer.isBuffer(result) || result instanceof Uint8Array) {
    return toBuffer(result, 'signature')
  }
  return signatureBytes(result.signature)
}

function resolveEndpoint(network: string): [string, string] {
  if (network.startsWith('ws://') || network.startsWith('wss://') || network.startsWith('http')) return [network, network]
  if (Object.prototype.hasOwnProperty.call(NETWORKS, network)) return [network, NETWORKS[network as NetworkName]]
  throw new Error(`Unknown network ${network}`)
}

function trustedGenesisHash(network: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(NETWORK_GENESIS_HASHES, network)
    ? NETWORK_GENESIS_HASHES[network as keyof typeof NETWORK_GENESIS_HASHES]
    : undefined
}

function normalizeGenesisHash(value: string | null | undefined): string | undefined {
  if (value == null) return undefined
  const normalized = value.startsWith('0x') ? value : `0x${value}`
  if (!/^0x[0-9a-fA-F]{64}$/.test(normalized)) {
    throw new ChainError('expectedGenesisHash must be a 32-byte hex string')
  }
  return normalized.toLowerCase()
}

function normalizeStorageArgs(
  pallet: string | Descriptor,
  storageFunction?: string | ScaleValue[],
  paramsOrBlock?: ScaleValue[] | number | string | null,
  block?: number | string | null,
): [string, string, ScaleValue[], number | string | null | undefined] {
  if (typeof pallet !== 'string') {
    const itemParams = Array.isArray(storageFunction) ? storageFunction : []
    const blockRef = Array.isArray(storageFunction)
      ? blockFrom(paramsOrBlock)
      : blockFrom(storageFunction)
    return [pallet[0], pallet[1], itemParams, blockRef]
  }
  return [pallet, storageFunction as string, Array.isArray(paramsOrBlock) ? paramsOrBlock : [], block ?? (Array.isArray(paramsOrBlock) ? undefined : paramsOrBlock)]
}

function normalizeBatchArgs(
  pallet: string | Descriptor,
  storageFunction: string | ScaleValue[][],
  paramSetsOrBlock?: ScaleValue[][] | number | string | null,
  block?: number | string | null,
): [string, string, ScaleValue[][], number | string | null | undefined] {
  if (typeof pallet !== 'string') {
    return [pallet[0], pallet[1], Array.isArray(storageFunction) ? storageFunction : [], blockFrom(paramSetsOrBlock)]
  }
  return [pallet, storageFunction as string, Array.isArray(paramSetsOrBlock) ? paramSetsOrBlock : [], block ?? (Array.isArray(paramSetsOrBlock) ? undefined : paramSetsOrBlock)]
}

function normalizeRuntimeArgs(
  api: string | Descriptor,
  method?: string | ScaleValue[],
  paramsOrBlock: ScaleValue[] | number | string | null = [],
  block?: number | string | null,
): [string, string, ScaleValue[], number | string | null | undefined] {
  if (typeof api !== 'string') {
    const callParams = Array.isArray(method) ? method : []
    const blockRef = Array.isArray(method) ? blockFrom(paramsOrBlock) : blockFrom(method)
    return [api[0], api[1], callParams, blockRef]
  }
  return [api, method as string, Array.isArray(paramsOrBlock) ? paramsOrBlock : [], block ?? (Array.isArray(paramsOrBlock) ? undefined : paramsOrBlock)]
}

function blockFrom(value: unknown): number | string | null | undefined {
  return typeof value === 'number' || typeof value === 'string' || value == null ? value : undefined
}

function normalizeCall(callLike: Exclude<CallLike, ByteLike | IntentCall>): [string, string, ScaleValue] {
  if (isCallTuple(callLike)) return [callLike[0], callLike[1], callLike[2] ?? {}]
  const pallet = callLike.pallet ?? callLike.module
  const fn = callLike.call ?? callLike.function
  if (typeof pallet !== 'string' || typeof fn !== 'string') {
    throw new ChainError('call must include pallet/module and call/function')
  }
  return [pallet, fn, callLike.params ?? {}]
}

function policyForSubmitOptions(options: SubmitOptions): Policy {
  if (options.policy instanceof Policy) {
    return options.allowRawCall === true ? options.policy.withRawCalls() : options.policy
  }
  const policy = options.policy ?? {}
  return new Policy({
    ...policy,
    allowRawCalls: policy.allowRawCalls === true || options.allowRawCall === true,
  })
}

function nativeOutcomeToExtrinsicResult(outcome: NativeTxOutcome, finalized: boolean): ExtrinsicResult {
  const blockNumber = outcome.blockNumber == null ? undefined : Number(outcome.blockNumber)
  const extrinsicIndex = outcome.extrinsicIndex ?? undefined
  const included = outcome.blockHash != null && blockNumber != null && extrinsicIndex != null
  return {
    status: outcome.success
      ? finalized ? 'finalized' : included ? 'inBlock' : 'submitted'
      : included ? 'failed' : 'failed',
    success: outcome.success,
    message: outcome.message,
    extrinsicHash: outcome.extrinsicHash,
    blockHash: outcome.blockHash ?? undefined,
    blockNumber,
    extrinsicIndex,
    extrinsicId: included ? `${blockNumber}-${String(extrinsicIndex).padStart(4, '0')}` : undefined,
    finalized: included ? finalized : undefined,
    fee: outcome.feeRao == null ? undefined : Balance.fromRao(outcome.feeRao),
    events: outcome.events,
    error: outcome.error ?? undefined,
    chainError: outcome.error != null || outcome.success === false ? classifyChainError(outcome.message, { payload: outcome.error }) : undefined,
  }
}

function rawCallsAllowed(options: SubmitOptions): boolean {
  if (options.allowRawCall === true) return true
  if (options.policy instanceof Policy) return options.policy.allowRawCalls
  return options.policy?.allowRawCalls === true
}

function assertRawBytesAllowed(options: SubmitOptions): void {
  if (!rawCallsAllowed(options)) {
    throw new ChainError(
      'opaque call bytes require explicit raw-call permission; pass allowRawCall: true only after constructing the call through a trusted path',
    )
  }
  const policy = options.policy instanceof Policy
    ? options.policy
    : options.policy == null
      ? undefined
      : new Policy(options.policy)
  if (policy?.hasOpaqueByteRestrictions()) {
    throw new ChainError(
      'opaque call bytes cannot prove fee, spend, or subnet policy; use an IntentCall or raw pallet/function call instead',
    )
  }
}

function isCallTuple(callLike: Exclude<CallLike, ByteLike | IntentCall>): callLike is readonly [string, string, ScaleValue?] {
  return Array.isArray(callLike) && typeof callLike[0] === 'string' && typeof callLike[1] === 'string'
}

function hex(bytes: ByteLike): string {
  return `0x${toBuffer(bytes, 'bytes').toString('hex')}`
}

function hexToBuffer(value: string): Buffer {
  return hexToBytes(value)
}

function hexNumber(value: string): number {
  return Number.parseInt(value, 16)
}

function headerNumber(header: unknown): number {
  const value = (header as { number?: string | number }).number
  return typeof value === 'number' ? value : hexNumber(String(value ?? '0x0'))
}

function normalizeHeader(raw: unknown): BlockHeader {
  const value = raw as { number?: string | number; parentHash?: string; hash?: string }
  return { number: headerNumber(raw), parentHash: value.parentHash, hash: value.hash, raw }
}

function normalizeStatus(status: unknown): Record<string, unknown> {
  if (typeof status === 'string') return { [status.toLowerCase()]: null }
  const out: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(status as Record<string, unknown>)) out[key.toLowerCase()] = value
  return out
}

function eventName(event: unknown): string {
  const value = event as { module_id?: string; event_id?: string; event?: { module_id?: string; event_id?: string; section?: string; method?: string } }
  const nested = value.event
  return `${value.module_id ?? nested?.module_id ?? nested?.section ?? ''}.${value.event_id ?? nested?.event_id ?? nested?.method ?? ''}`
}

function eventExtrinsicIndex(event: unknown): number | null {
  const value = event as { extrinsic_idx?: unknown; phase?: unknown }
  const index = eventIndexNumber(value.extrinsic_idx)
  if (index != null) return index
  const phase = value.phase as Record<string, unknown> | undefined
  const apply = phase?.ApplyExtrinsic ?? phase?.applyExtrinsic
  return eventIndexNumber(apply)
}

function eventIndexNumber(value: unknown): number | null {
  if (value == null) return null
  if (typeof value === 'number') return Number.isSafeInteger(value) && value >= 0 ? value : null
  if (typeof value === 'bigint') {
    const number = Number(value)
    return Number.isSafeInteger(number) && number >= 0 ? number : null
  }
  if (typeof value === 'string') {
    if (!/^(?:0x[0-9a-fA-F]+|\d+)$/.test(value)) return null
    const number = Number(value)
    return Number.isSafeInteger(number) && number >= 0 ? number : null
  }
  if (typeof value !== 'object' || Buffer.isBuffer(value) || value instanceof Uint8Array) return null

  const object = value as Record<string, unknown>
  if (object[WIRE_TAG] === 'bigint' && typeof object.value === 'string') {
    return eventIndexNumber(object.value)
  }
  if (Array.isArray(value) && value.length === 1) return eventIndexNumber(value[0])

  const entries = Object.entries(object).filter(([key]) => key !== WIRE_TAG)
  if (entries.length !== 1) return null
  return eventIndexNumber(entries[0][1])
}

function feeFromEvent(event: unknown): Balance | undefined {
  const attrs = (event as { attributes?: unknown }).attributes
  if (attrs == null || typeof attrs !== 'object') return undefined
  const amount = (attrs as Record<string, unknown>).actual_fee ?? (attrs as Record<string, unknown>).actualFee ?? (attrs as Record<string, unknown>).fee
  const rao = decimalAmount(amount)
  return rao == null ? undefined : Balance.fromRao(rao)
}

function decimalAmount(value: unknown): string | undefined {
  if (value == null) return undefined
  if (typeof value === 'bigint') return value >= 0n ? value.toString(10) : undefined
  if (typeof value === 'number') {
    return Number.isSafeInteger(value) && value >= 0 ? value.toString(10) : undefined
  }
  if (typeof value === 'string') {
    if (/^\d+$/.test(value)) return value
    if (/^0x[0-9a-fA-F]+$/.test(value)) return BigInt(value).toString(10)
    return undefined
  }
  if (typeof value !== 'object' || Buffer.isBuffer(value) || value instanceof Uint8Array) return undefined

  const object = value as Record<string, unknown>
  if (object[WIRE_TAG] === 'bigint' && typeof object.value === 'string') {
    return decimalAmount(object.value)
  }
  if (Array.isArray(value) && value.length === 1) return decimalAmount(value[0])

  const entries = Object.entries(object).filter(([key]) => key !== WIRE_TAG)
  if (entries.length !== 1) return undefined
  return decimalAmount(entries[0][1])
}

function dispatchFailureMessage(runtime: Runtime, event: unknown): string {
  const message = formatDispatchError(runtime, dispatchErrorFromEvent(event))
  return message == null ? 'Extrinsic failed' : `Extrinsic failed: ${message}`
}

function dispatchErrorFromEvent(event: unknown): unknown {
  const outer = recordValue(event)
  if (outer == null) return undefined
  const attrs = recordValue(outer.attributes)
  if (attrs != null) {
    return attrs.dispatch_error ?? attrs.dispatchError ?? attrs.error
  }
  const nested = recordValue(outer.ExtrinsicFailed)
  return nested?.dispatch_error ?? nested?.dispatchError ?? nested
}

function formatDispatchError(runtime: Runtime, error: unknown): string | undefined {
  if (typeof error === 'string') return error
  const value = recordValue(error)
  if (value == null) return undefined
  const moduleError = recordValue(value.Module ?? value.module)
  if (moduleError != null) return formatModuleDispatchError(runtime, moduleError)
  const err = value.Err ?? value.err
  if (err != null) return formatDispatchError(runtime, err)
  const entries = Object.entries(value)
  if (entries.length !== 1) return undefined
  const [variant, payload] = entries[0]
  const nested = formatDispatchError(runtime, payload)
  return nested == null ? variant : `${variant}.${nested}`
}

function formatModuleDispatchError(runtime: Runtime, value: Record<string, unknown>): string | undefined {
  const moduleIndex = unsignedByteValue(value.index)
  const errorIndex = dispatchModuleErrorIndex(value.error)
  if (moduleIndex == null || errorIndex == null) return undefined
  try {
    const resolved = runtime.moduleError(moduleIndex, errorIndex)
    const name = Array.isArray(resolved) ? resolved[0] : resolved.name
    const rawDocs: string[] = Array.isArray(resolved) ? resolved[1] : resolved.docs
    const docs = rawDocs.map((doc: string) => doc.trim()).filter(Boolean).join(' ')
    return docs.length === 0 ? name : `${name}: ${docs}`
  } catch {
    return `Module(${moduleIndex}, ${errorIndex})`
  }
}

function dispatchModuleErrorIndex(value: unknown): number | undefined {
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return value[0]
  if (typeof value === 'string' && value.startsWith('0x')) {
    try {
      return hexToBuffer(value)[0]
    } catch {
      return undefined
    }
  }
  if (Array.isArray(value)) return unsignedByteValue(value[0])
  return unsignedByteValue(value)
}

function unsignedByteValue(value: unknown): number | undefined {
  let numeric: number
  if (typeof value === 'number') numeric = value
  else if (typeof value === 'bigint') {
    if (value < 0n || value > 255n) return undefined
    numeric = Number(value)
  } else if (typeof value === 'string') {
    numeric = value.startsWith('0x') ? Number.parseInt(value.slice(2), 16) : Number(value)
  } else {
    return undefined
  }
  return Number.isInteger(numeric) && numeric >= 0 && numeric <= 255 ? numeric : undefined
}

function recordValue(value: unknown): Record<string, unknown> | undefined {
  return value != null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new ChainError(`timed out after ${timeoutMs}ms`)), timeoutMs)
    promise.then(
      (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      (error) => {
        clearTimeout(timer)
        reject(error)
      },
    )
  })
}

function isPrunedState(error: unknown): boolean { return /state.*(?:discarded|pruned)|pruned.*state|unknown block|state unavailable/i.test(String(error)) }
