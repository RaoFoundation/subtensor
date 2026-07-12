import { blake2_256, generateExtrinsicProof, metadataDigest } from './crypto'
import { CRYPTO_SR25519, Keypair, publicKeyFromSs58 } from './keys'
import { LedgerDevice } from './ledger'
import { Runtime, eraBirth } from './runtime'
import { toBuffer } from './wire'
import { Balance, type BalanceLike, balanceRao } from './balance'
import type { ByteLike, ChainInfo, ScaleValue, SignedExtrinsic, StorageEntry, TransactionParams } from './types'

export const SS58_FORMAT = 42
export const DEFAULT_ERA_PERIOD = 128
export const DEFAULT_HEAD_RUNTIME_TTL_MS = 12_000
export const DEFAULT_HISTORICAL_RUNTIME_CACHE_SIZE = 64
export const DEFAULT_REQUEST_TIMEOUT_MS = 30_000
export const DEFAULT_MAX_REQUEST_RETRIES = 2
export const DEFAULT_RETRY_BACKOFF_MS = 250
export const DEFAULT_MAX_RETRY_BACKOFF_MS = 5_000
export const DEFAULT_NONCE_RECONCILE_BLOCKS = 8
export const NETWORKS = Object.freeze({
  finney: 'wss://entrypoint-finney.opentensor.ai:443',
  test: 'wss://test.finney.opentensor.ai:443',
  archive: 'wss://archive.chain.opentensor.ai:443',
  local: process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944',
})

export type NetworkName = keyof typeof NETWORKS
export type Descriptor = readonly [string, string]
export type CallLike =
  | readonly [string, string, ScaleValue?]
  | { pallet?: string; module?: string; call?: string; function?: string; params?: ScaleValue }
  | ByteLike

export interface ClientOptions {
  endpoint?: string
  fallbackEndpoints?: string[]
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
}

export interface RpcRequestOptions {
  signal?: AbortSignal
  timeoutMs?: number
  maxRetries?: number
  retryBackoffMs?: number
  maxRetryBackoffMs?: number
}

export interface JsonRpcTransportOptions {
  webSocket?: WebSocketConstructor
  webSocketConstructor?: WebSocketConstructor
  webSocketFactory?: WebSocketFactory
  requestTimeoutMs?: number
  maxRequestRetries?: number
  retryBackoffMs?: number
  maxRetryBackoffMs?: number
}

export interface SubmitOptions {
  nonce?: number
  period?: number | null
  tip?: BalanceLike
  tipAssetId?: BalanceLike | null
  metadataHash?: ByteLike | null
  waitForInclusion?: boolean
  waitForFinalization?: boolean
  timeoutMs?: number
  signal?: AbortSignal
}

export interface SignerAccountContext {
  client: Client
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
  client: Client
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
  sign?(
    payload: ByteLike,
    context?: SignerPayloadContext,
  ): SignerSignature | Promise<SignerSignature>
  signPayload?(
    payload: ByteLike,
    context: SignerPayloadContext,
  ): SignerSignature | Promise<SignerSignature>
  signRaw?(
    request: ExtensionSignRawRequest,
  ): SignerSignature | Promise<SignerSignature>
}

export type SignerLike = Keypair | ChainSigner | LedgerDevice
export type ExtrinsicStatus = 'submitted' | 'inBlock' | 'finalized' | 'failed'

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
  timestamp?: Date
}

interface RpcRequest {
  resolve(value: unknown): void
  reject(error: Error): void
  cleanup(): void
}

interface SubscriptionWaiter {
  resolve(value: IteratorResult<unknown>): void
  reject(error: Error): void
}

interface SubscriptionState {
  queue: unknown[]
  waiters: SubscriptionWaiter[]
  closed: boolean
  resubscribe: boolean
  subscribeMethod: string
  params: unknown[]
  unsubscribeMethod: string
  requestOptions: RpcRequestOptions
  subscription?: string
  resubscribing?: Promise<void>
}

interface ResolvedSigner {
  signer: Keypair | ChainSigner
  ss58Address: string
  publicKey: Buffer
  cryptoType: number
  requiresMetadataProof: boolean
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

type NonceStatus = 'reserved' | 'submitted' | 'confirmed' | 'failed' | 'reusable'

interface NonceAccountState {
  next?: number
  reusable: number[]
  statuses: Map<number, NonceStatus>
  queue: Promise<void>
}

interface NonceReservation {
  address: string
  nonce: number
}

type SubmittedExtrinsicLocation = 'pool' | 'block' | null

interface SubscriptionOptions extends RpcRequestOptions {
  resubscribe?: boolean
}

const MANAGED_NONCE = Symbol('managedNonce')

type ManagedSignedExtrinsicResult = SignedExtrinsicResult & {
  [MANAGED_NONCE]?: NonceReservation
}

interface NormalizedSignature {
  signature: Buffer
  cryptoType: number
}

export type WebSocketLike = {
  readyState: number
  send(data: string): void
  close(): void
  addEventListener(type: string, listener: (event: { data?: unknown }) => void): void
}

export type WebSocketConstructor = new (url: string) => WebSocketLike
export type WebSocketFactory = (url: string) => WebSocketLike

export class JsonRpcError extends Error {
  readonly code?: number
  readonly data?: unknown

  constructor(message: string, code?: number, data?: unknown) {
    super(message)
    this.name = 'JsonRpcError'
    this.code = code
    this.data = data
  }
}

export class ChainError extends Error {
  readonly details?: unknown

  constructor(message: string, details?: unknown) {
    super(message)
    this.name = 'ChainError'
    this.details = details
  }
}

export class RequestTimeoutError extends ChainError {
  constructor(message: string) {
    super(message)
    this.name = 'RequestTimeoutError'
  }
}

export class RequestAbortedError extends ChainError {
  constructor(message = 'request aborted') {
    super(message)
    this.name = 'RequestAbortedError'
  }
}

export class JsonRpcTransport {
  private readonly endpoints: string[]
  private readonly retryForever: boolean
  private readonly requestTimeoutMs: number
  private readonly maxRequestRetries: number
  private readonly retryBackoffMs: number
  private readonly maxRetryBackoffMs: number
  private readonly webSocketConstructor?: WebSocketConstructor
  private readonly webSocketFactory?: WebSocketFactory
  private endpointIndex = 0
  private id = 1
  private socket?: WebSocketLike
  private connecting?: Promise<WebSocketLike>
  private pending = new Map<number, RpcRequest>()
  private subscriptions = new Set<SubscriptionState>()
  private subscriptionsById = new Map<string, SubscriptionState>()
  private closed = false

  constructor(
    endpoint: string,
    fallbackEndpoints: string[] = [],
    retryForever = false,
    options: JsonRpcTransportOptions = {},
  ) {
    this.endpoints = [endpoint, ...fallbackEndpoints.filter((item) => item !== endpoint)]
    this.retryForever = retryForever
    this.requestTimeoutMs = nonNegativeNumber(options.requestTimeoutMs, DEFAULT_REQUEST_TIMEOUT_MS)
    this.maxRequestRetries = nonNegativeInteger(options.maxRequestRetries, DEFAULT_MAX_REQUEST_RETRIES)
    this.retryBackoffMs = nonNegativeNumber(options.retryBackoffMs, DEFAULT_RETRY_BACKOFF_MS)
    this.maxRetryBackoffMs = nonNegativeNumber(options.maxRetryBackoffMs, DEFAULT_MAX_RETRY_BACKOFF_MS)
    this.webSocketConstructor = options.webSocketConstructor ?? options.webSocket
    this.webSocketFactory = options.webSocketFactory
  }

  get endpoint(): string {
    return this.endpoints[this.endpointIndex]
  }

  async request(method: string, params: unknown[] = [], options: RpcRequestOptions = {}): Promise<unknown> {
    const requestOptions = {
      ...options,
      timeoutMs: options.timeoutMs ?? this.requestTimeoutMs,
    }
    throwIfAborted(requestOptions.signal)
    const maxRetries = requestOptions.maxRetries ?? this.maxRequestRetries
    const retryBackoffMs = requestOptions.retryBackoffMs ?? this.retryBackoffMs
    const maxRetryBackoffMs = requestOptions.maxRetryBackoffMs ?? this.maxRetryBackoffMs
    let attempt = 0
    for (;;) {
      try {
        return this.isHttpEndpoint()
          ? await this.httpRequest(method, params, requestOptions)
          : await this.wsRequest(method, params, requestOptions)
      } catch (error) {
        if (error instanceof JsonRpcError || error instanceof RequestAbortedError) throw error
        if (!this.retryForever && attempt >= maxRetries) throw error
        attempt += 1
        this.rotateEndpoint()
        const capped = Math.min(retryBackoffMs * (2 ** Math.max(0, attempt - 1)), maxRetryBackoffMs)
        await delay(capped, requestOptions.signal)
      }
    }
  }

  async subscribe(
    subscribeMethod: string,
    params: unknown[] = [],
    unsubscribeMethod: string,
    options: SubscriptionOptions = {},
  ): Promise<AsyncIterable<unknown> & { unsubscribe(): Promise<void> }> {
    if (this.isHttpEndpoint()) throw new ChainError('subscriptions require a WebSocket endpoint')
    const state: SubscriptionState = {
      queue: [],
      waiters: [],
      closed: false,
      resubscribe: options.resubscribe ?? true,
      subscribeMethod,
      params,
      unsubscribeMethod,
      requestOptions: {
        signal: options.signal,
        timeoutMs: options.timeoutMs,
        maxRetries: options.maxRetries,
        retryBackoffMs: options.retryBackoffMs,
        maxRetryBackoffMs: options.maxRetryBackoffMs,
      },
    }
    this.subscriptions.add(state)
    try {
      await this.activateSubscription(state)
    } catch (error) {
      this.subscriptions.delete(state)
      throw error
    }
    const unsubscribe = async () => {
      if (state.closed) return
      state.closed = true
      this.subscriptions.delete(state)
      if (state.subscription != null) this.subscriptionsById.delete(state.subscription)
      for (const waiter of state.waiters.splice(0)) waiter.resolve({ done: true, value: undefined })
      const subscription = state.subscription
      state.subscription = undefined
      if (subscription != null) await this.request(unsubscribeMethod, [subscription]).catch(() => undefined)
    }
    return {
      unsubscribe,
      [Symbol.asyncIterator]: () => ({
        next: () => this.subscriptionNext(state),
        return: async () => {
          await unsubscribe()
          return { done: true, value: undefined }
        },
      }),
    }
  }

  close(): void {
    this.closed = true
    this.socket?.close()
    this.socket = undefined
    this.connecting = undefined
    this.failPending(new ChainError('transport closed'))
    for (const state of [...this.subscriptions]) this.closeSubscription(state)
  }

  private isHttpEndpoint(): boolean {
    return this.endpoint.startsWith('http://') || this.endpoint.startsWith('https://')
  }

  private async httpRequest(method: string, params: unknown[], options: RpcRequestOptions): Promise<unknown> {
    const request = withRequestSignal(options)
    try {
      const response = await fetch(this.endpoint, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: this.id++, method, params }),
        signal: request.signal,
      })
      if (!response.ok) throw new JsonRpcError(`HTTP ${response.status} from ${this.endpoint}`)
      const payload = await response.json()
      if (payload.error) throw new JsonRpcError(payload.error.message, payload.error.code, payload.error.data)
      return payload.result
    } catch (error) {
      throw normalizeAbortError(error, request)
    } finally {
      request.cleanup()
    }
  }

  private async wsRequest(method: string, params: unknown[], options: RpcRequestOptions): Promise<unknown> {
    const socket = await this.connect(options)
    const id = this.id++
    const promise = new Promise<unknown>((resolve, reject) => {
      const request = withRequestSignal(options)
      const cleanup = () => request.cleanup()
      const fail = (error: Error) => {
        if (!this.pending.delete(id)) return
        cleanup()
        reject(error)
      }
      this.pending.set(id, {
        resolve(value) {
          cleanup()
          resolve(value)
        },
        reject(error) {
          cleanup()
          reject(error)
        },
        cleanup,
      })
      request.onAbort((error) => fail(error))
    })
    try {
      socket.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }))
    } catch (error) {
      const pending = this.pending.get(id)
      this.pending.delete(id)
      pending?.cleanup()
      throw error
    }
    return promise
  }

  private async connect(options: RpcRequestOptions = {}): Promise<WebSocketLike> {
    if (this.closed) throw new ChainError('transport closed')
    if (this.socket?.readyState === 1) return this.socket
    if (this.connecting != null) return this.connecting

    this.connecting = new Promise((resolve, reject) => {
      const request = withRequestSignal(options)
      let socket: WebSocketLike
      try {
        socket = this.createWebSocket(this.endpoint)
      } catch (error) {
        request.cleanup()
        this.connecting = undefined
        reject(error)
        return
      }
      let settled = false
      const cleanup = () => request.cleanup()
      const fail = (error: Error) => {
        if (settled) return
        settled = true
        cleanup()
        this.socket = undefined
        this.connecting = undefined
        try {
          socket.close()
        } catch {
          // Ignore close errors while unwinding a failed connection.
        }
        reject(error)
      }
      request.onAbort((error) => fail(error))
      socket.addEventListener('open', () => {
        if (settled) return
        settled = true
        cleanup()
        this.socket = socket
        this.connecting = undefined
        resolve(socket)
      })
      socket.addEventListener('error', () => fail(new ChainError(`could not connect to ${this.endpoint}`)))
      socket.addEventListener('close', () => this.handleSocketClose(new ChainError(`connection closed: ${this.endpoint}`)))
      socket.addEventListener('message', (event) => this.handleMessage(event.data))
    })
    return this.connecting
  }

  private createWebSocket(url: string): WebSocketLike {
    if (this.webSocketFactory != null) return this.webSocketFactory(url)
    const WebSocketImpl =
      this.webSocketConstructor ??
      (globalThis as unknown as { WebSocket?: WebSocketConstructor }).WebSocket
    if (WebSocketImpl == null) {
      throw new ChainError(
        'WebSocket is not available; pass webSocketFactory or webSocketConstructor, or use an HTTP endpoint',
      )
    }
    return new WebSocketImpl(url)
  }

  private handleMessage(data: unknown): void {
    const message = JSON.parse(String(data))
    if (typeof message.id === 'number') {
      const pending = this.pending.get(message.id)
      if (pending == null) return
      this.pending.delete(message.id)
      if (message.error) pending.reject(new JsonRpcError(message.error.message, message.error.code, message.error.data))
      else pending.resolve(message.result)
      return
    }
    const subscription = message.params?.subscription
    if (subscription == null) return
    const state = this.subscriptionsById.get(subscription)
    if (state == null || state.closed) return
    const result = message.params?.result
    const waiter = state.waiters.shift()
    if (waiter != null) waiter.resolve({ done: false, value: result })
    else state.queue.push(result)
  }

  private subscriptionNext(state: SubscriptionState): Promise<IteratorResult<unknown>> {
    if (state.queue.length > 0) return Promise.resolve({ done: false, value: state.queue.shift() })
    if (state.closed) return Promise.resolve({ done: true, value: undefined })
    return new Promise((resolve, reject) => {
      state.waiters.push({ resolve, reject })
    })
  }

  private failPending(error: Error): void {
    this.socket = undefined
    this.connecting = undefined
    for (const pending of this.pending.values()) {
      pending.cleanup()
      pending.reject(error)
    }
    this.pending.clear()
  }

  private handleSocketClose(error: Error): void {
    this.failPending(error)
    this.subscriptionsById.clear()
    if (this.closed) return
    for (const state of this.subscriptions) {
      state.subscription = undefined
      if (state.resubscribe) void this.resubscribe(state)
      else this.closeSubscription(state, error)
    }
  }

  private async activateSubscription(state: SubscriptionState): Promise<void> {
    const subscription = String(
      await this.request(state.subscribeMethod, state.params, state.requestOptions),
    )
    if (state.closed) {
      await this.request(state.unsubscribeMethod, [subscription]).catch(() => undefined)
      return
    }
    state.subscription = subscription
    this.subscriptionsById.set(subscription, state)
  }

  private resubscribe(state: SubscriptionState): Promise<void> {
    if (state.closed) return Promise.resolve()
    state.resubscribing ??= this.activateSubscription(state)
      .catch((error) => {
        this.closeSubscription(state, error instanceof Error ? error : new ChainError(String(error)))
      })
      .finally(() => {
        state.resubscribing = undefined
      })
    return state.resubscribing
  }

  private closeSubscription(state: SubscriptionState, error?: Error): void {
    if (state.closed) return
    state.closed = true
    this.subscriptions.delete(state)
    if (state.subscription != null) this.subscriptionsById.delete(state.subscription)
    state.subscription = undefined
    for (const waiter of state.waiters.splice(0)) {
      if (error == null) waiter.resolve({ done: true, value: undefined })
      else waiter.reject(error)
    }
  }

  private rotateEndpoint(): void {
    const socket = this.socket
    this.socket = undefined
    this.connecting = undefined
    try {
      socket?.close()
    } catch {
      // Ignore close errors while rotating to another endpoint.
    }
    if (this.endpoints.length > 1) this.endpointIndex = (this.endpointIndex + 1) % this.endpoints.length
  }
}

export class Client {
  readonly network: string
  readonly endpoint: string
  readonly transport: JsonRpcTransport
  readonly balances: BalancesNamespace
  readonly subnets: SubnetsNamespace
  readonly neurons: NeuronsNamespace
  readonly staking: StakingNamespace

  private readonly headRuntimeTtlMs: number
  private readonly historicalRuntimeCacheSize: number
  private headRuntimeCache?: HeadRuntimeCacheEntry
  private runtimesBySpecVersion = new Map<number, RuntimeCacheEntry>()
  private historicalRuntimeCache = new Map<string, RuntimeCacheEntry>()
  private genesis?: string
  private nonceAccounts = new Map<string, NonceAccountState>()

  constructor(network: string = 'finney', options: ClientOptions = {}) {
    const [label, endpoint] = resolveEndpoint(options.endpoint ?? network)
    this.network = label
    this.endpoint = endpoint
    this.transport = new JsonRpcTransport(endpoint, options.fallbackEndpoints, options.retryForever, {
      webSocket: options.webSocket,
      webSocketConstructor: options.webSocketConstructor,
      webSocketFactory: options.webSocketFactory,
      requestTimeoutMs: options.requestTimeoutMs,
      maxRequestRetries: options.maxRequestRetries,
      retryBackoffMs: options.retryBackoffMs,
      maxRetryBackoffMs: options.maxRetryBackoffMs,
    })
    this.balances = new BalancesNamespace(this)
    this.subnets = new SubnetsNamespace(this)
    this.neurons = new NeuronsNamespace(this)
    this.staking = new StakingNamespace(this)
    this.headRuntimeTtlMs = nonNegativeNumber(options.headRuntimeTtlMs, DEFAULT_HEAD_RUNTIME_TTL_MS)
    this.historicalRuntimeCacheSize = nonNegativeInteger(
      options.historicalRuntimeCacheSize,
      DEFAULT_HISTORICAL_RUNTIME_CACHE_SIZE,
    )
    if (options.autoConnect) void this.connect()
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

    const metadataHex = await this.rpc('state_getMetadata', blockHash == null ? [] : [blockHash])
    const runtime = new Runtime(
      hexToBuffer(String(metadataHex)),
      version.specVersion,
      version.transactionVersion,
      ss58Format,
    )
    const entry = { runtime, ss58Format, ...version }
    this.cacheRuntimeBySpecVersion(entry)
    return entry
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

  async chainInfo(runtime?: Runtime): Promise<ChainInfo> {
    const resolvedRuntime = runtime ?? await this.runtimeAt()
    const [version, properties] = await Promise.all([
      this.rpc('state_getRuntimeVersion'),
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
    const blockHash = await this.resolveBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const key = runtime.storageKey(moduleName, itemName, itemParams)
    const raw = await this.rpc('state_getStorage', [hex(key), ...(blockHash == null ? [] : [blockHash])])
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
    const blockHash = await this.resolveBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const keys = runtime.storageKeyBatch(moduleName, itemName, sets)
    const raw = await this.rpc('state_queryStorageAt', [keys.map(hex), ...(blockHash == null ? [] : [blockHash])])
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
    pageSize = 512,
  ): Promise<Array<[K, V]>> {
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const prefix = runtime.storageKey(moduleName, itemName, itemParams)
    const entry = runtime.storageEntry(moduleName, itemName)
    const out: Array<[K, V]> = []
    let startKey: string | null = null
    for (;;) {
      const keys = (await this.rpc('state_getKeysPaged', [
        hex(prefix),
        pageSize,
        startKey,
        ...(blockHash == null ? [] : [blockHash]),
      ])) as string[]
      if (keys.length === 0) break
      const raw = await this.rpc('state_queryStorageAt', [keys, ...(blockHash == null ? [] : [blockHash])])
      const changes = ((raw as Array<{ changes?: Array<[string, string | null]> }>)[0]?.changes ?? [])
      const valueByKey = new Map(changes.map(([key, value]) => [key.toLowerCase(), value]))
      for (const key of keys) {
        const value = valueByKey.get(key.toLowerCase())
        if (value == null) continue
        const decodedKey = runtime.decodeStorageKeyParams<K>(moduleName, itemName, hexToBuffer(key), itemParams.length)
        const normalizedKey = (decodedKey.length === 1 ? decodedKey[0] : decodedKey) as K
        out.push([normalizedKey, runtime.decode<V>(entry.valueType, hexToBuffer(value), false)])
      }
      startKey = keys[keys.length - 1]
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
    const blockHash = await this.resolveBlockHash(blockRef)
    const runtime = await this.runtimeAt(blockHash)
    const info = runtime.runtimeApis()[apiName]?.[methodName]
    if (info == null) throw new ChainError(`runtime API ${apiName}.${methodName} not found`)
    const inputDetails = info.inputDetails ?? []
    if (inputDetails.length !== callParams.length) {
      throw new ChainError(`${apiName}.${methodName} expects ${inputDetails.length} params`)
    }
    const encoded = Buffer.concat(callParams.map((value, index) => runtime.encodeId(inputDetails[index].typeId, value)))
    const raw = await this.rpc('state_call', [`${apiName}_${methodName}`, hex(encoded), blockHash ?? null])
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

  decodeScale<T extends ScaleValue = ScaleValue>(
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

  composeCall(pallet: string, fn: string, params: ScaleValue = {}, block?: number | string | null): Promise<Buffer> {
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

  async waitForBlock(block?: number | null, options: { timeoutMs?: number } = {}): Promise<BlockHeader> {
    const target = block ?? (await this.blockNumber()) + 1
    const wait = async () => {
      for await (const header of this.blocks()) {
        if (header.number >= target) return header
      }
      throw new ChainError('block subscription ended before the target block')
    }
    return options.timeoutMs == null ? wait() : withTimeout(wait(), options.timeoutMs)
  }

  wait_for_block(block?: number | null, options: { timeoutMs?: number } = {}): Promise<BlockHeader> {
    return this.waitForBlock(block, options)
  }

  async timestamp(block?: number | string | null): Promise<Date> {
    const ms = await this.query<bigint | number>(storage.Timestamp.Now, [], block)
    return new Date(Number(ms ?? 0))
  }

  async blockInfo(block?: number | null): Promise<BlockInfo | null> {
    const raw = await this.rpc('chain_getBlock', block == null ? [] : [await this.blockHash(block)])
    const value = raw as { block?: { header?: { number?: string; hash?: string }; extrinsics?: string[] } }
    if (value.block?.header == null) return null
    const number = headerNumber(value.block.header)
    return {
      number,
      hash: await this.blockHash(number),
      header: value.block.header,
      extrinsics: value.block.extrinsics ?? [],
      timestamp: await this.timestamp(number).catch(() => undefined),
    }
  }

  block_info(block?: number | null): Promise<BlockInfo | null> {
    return this.blockInfo(block)
  }

  async at(block?: number | null): Promise<Snapshot> {
    const number = block ?? await this.blockNumber()
    return new Snapshot(this, number, await this.blockHash(number))
  }

  balance(rao: BalanceLike, netuid = 0, symbol?: string | null): Balance {
    return Balance.fromRao(rao, netuid, symbol)
  }

  read(name: string, params: Record<string, ScaleValue> = {}): Promise<unknown> {
    return read(this, name, params)
  }

  reads(): Array<{ name: string; category: string; params: string[] }> {
    return READS.slice()
  }

  async signExtrinsic(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<SignedExtrinsicResult> {
    return this.signExtrinsicWithNonce(call, signer, options, false)
  }

  private async signExtrinsicWithNonce(
    call: CallLike,
    signer: SignerLike,
    options: SubmitOptions,
    manageNonce: boolean,
  ): Promise<ManagedSignedExtrinsicResult> {
    let resolved: ResolvedSigner | undefined
    let reservation: NonceReservation | undefined
    try {
      const runtime = await this.runtimeAt()
      const callData = await this.callData(call)
      resolved = await this.resolveSigner(signer, runtime)
      reservation = manageNonce && options.nonce == null
        ? await this.reserveNonce(resolved.ss58Address)
        : undefined
      const nonce = options.nonce ?? reservation?.nonce ?? await this.peekNextIndex(resolved.ss58Address)
      const period = options.period === undefined ? DEFAULT_ERA_PERIOD : options.period
      const { era, eraBlockHash } = await this.normalizeEra(period)
      const tip = balanceRao(options.tip ?? 0)
      const tipAssetId = options.tipAssetId == null ? null : balanceRao(options.tipAssetId)
      const chainInfo = resolved.requiresMetadataProof ? await this.chainInfo(runtime) : undefined
      const metadataHash =
        options.metadataHash == null
          ? resolved.requiresMetadataProof
            ? metadataDigest(runtime.metadataBytes, chainInfo!)
            : null
          : toBuffer(options.metadataHash, 'metadataHash')
      const txParams = {
        era,
        nonce,
        tip,
        tipAssetId,
        genesisHash: hexToBuffer(await this.genesisHash()),
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
      const result: ManagedSignedExtrinsicResult = {
        ...signed,
        hex: hex(signed.bytes),
        signerAddress: resolved.ss58Address,
        nonce,
      }
      if (reservation != null) result[MANAGED_NONCE] = reservation
      return result
    } catch (error) {
      if (reservation != null) await this.failNonce(reservation, true)
      throw error
    }
  }

  async submit(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    const signed = await this.signExtrinsicWithNonce(call, signer, options, true)
    return this.submitSigned(signed, signed.signerAddress, options)
  }

  async submitSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    signerAddress?: string,
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    const reservation = managedNonceReservation(extrinsic)
    if (options.waitForInclusion || options.waitForFinalization) {
      const watcher = await this.watchSigned(extrinsic, {
        waitForFinalization: options.waitForFinalization ?? false,
        timeoutMs: options.timeoutMs,
        signal: options.signal,
      })
      return await watcher.result
    }
    try {
      const hash = String(await this.transport.request('author_submitExtrinsic', [hex(bytes)], {
        timeoutMs: options.timeoutMs,
        signal: options.signal,
      }))
      if (reservation != null) await this.submitNonce(reservation)
      return { status: 'submitted', message: 'Submitted', extrinsicHash: hash, events: [] }
    } catch (error) {
      if (reservation != null) await this.reconcileNonceReservation(reservation, extrinsicHash)
      throw error
    }
  }

  async watchSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    options: Pick<SubmitOptions, 'waitForFinalization' | 'timeoutMs' | 'signal'> = {},
  ): Promise<ExtrinsicWatcher> {
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    const reservation = managedNonceReservation(extrinsic)
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
        },
      )
      if (reservation != null) await this.submitNonce(reservation)
    } catch (error) {
      if (reservation != null) await this.reconcileNonceReservation(reservation, extrinsicHash)
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
    ).then(
      async (value) => {
        if (reservation != null) await this.confirmNonce(reservation)
        return value
      },
      async (error) => {
        if (reservation != null) await this.reconcileNonceReservation(reservation, extrinsicHash)
        throw error
      },
    )
    return {
      extrinsicHash,
      result,
      unsubscribe: () => subscription.unsubscribe(),
    }
  }

  async peekNextIndex(address: string): Promise<number> {
    const nonce = Number(await this.rpc('system_accountNextIndex', [address]))
    return nonce
  }

  async accountNextIndex(address: string, _useCache = false): Promise<number> {
    return this.peekNextIndex(address)
  }

  clearNonce(address: string): void {
    this.nonceAccounts.delete(address)
  }

  private async reserveNonce(address: string): Promise<NonceReservation> {
    return this.withNonceAccount(address, async (state) => {
      if (state.next == null) state.next = await this.peekNextIndex(address)
      state.reusable.sort((left, right) => left - right)
      let nonce = state.next
      if (state.reusable.length > 0 && state.reusable[0] <= state.next) {
        nonce = state.reusable.shift() as number
      }
      while (state.statuses.has(nonce) && state.statuses.get(nonce) !== 'reusable') nonce += 1
      if (nonce >= state.next) state.next = nonce + 1
      state.statuses.set(nonce, 'reserved')
      return { address, nonce }
    })
  }

  private async submitNonce(reservation: NonceReservation): Promise<void> {
    await this.withNonceAccount(reservation.address, (state) => {
      state.statuses.set(reservation.nonce, 'submitted')
    })
  }

  private async confirmNonce(reservation: NonceReservation): Promise<void> {
    await this.withNonceAccount(reservation.address, (state) => {
      state.statuses.set(reservation.nonce, 'confirmed')
      pruneNonceStatuses(state)
    })
  }

  private async failNonce(reservation: NonceReservation, reusable: boolean): Promise<void> {
    await this.withNonceAccount(reservation.address, (state) => {
      const current = state.statuses.get(reservation.nonce)
      if (current === 'confirmed') return
      state.statuses.set(reservation.nonce, reusable ? 'reusable' : 'failed')
      if (reusable && !state.reusable.includes(reservation.nonce)) {
        state.reusable.push(reservation.nonce)
        state.reusable.sort((left, right) => left - right)
      } else if (!reusable) {
        state.reusable = state.reusable.filter((nonce) => nonce !== reservation.nonce)
      }
      pruneNonceStatuses(state)
    })
  }

  private async reconcileNonceReservation(
    reservation: NonceReservation,
    extrinsicHash?: string,
  ): Promise<void> {
    const [locationResult, chainNextResult] = await Promise.all([
      extrinsicHash == null
        ? Promise.resolve<{ ok: true; location: SubmittedExtrinsicLocation }>({
            ok: true,
            location: null,
          })
        : this.submittedExtrinsicLocation(extrinsicHash).then(
            (location) => ({ ok: true as const, location }),
            (error) => ({ ok: false as const, error }),
          ),
      this.peekNextIndex(reservation.address).then(
        (nonce) => ({ ok: true as const, nonce }),
        (error) => ({ ok: false as const, error }),
      ),
    ])
    await this.withNonceAccount(reservation.address, (state) => {
      const location = locationResult.ok ? locationResult.location : undefined
      const chainNext = chainNextResult.ok ? chainNextResult.nonce : undefined
      const submitted = location != null || (chainNext != null && chainNext > reservation.nonce)
      const definitelyAbsent =
        locationResult.ok &&
        location == null &&
        chainNext != null &&
        chainNext <= reservation.nonce
      if (!submitted && !definitelyAbsent) {
        invalidateNonceState(state)
        return
      }

      if (chainNext != null) state.next = chainNext
      else if (state.next == null || state.next <= reservation.nonce) state.next = reservation.nonce + 1

      const minimumReusableNonce = submitted
        ? Math.max(state.next, reservation.nonce + 1)
        : state.next
      state.reusable = state.reusable.filter(
        (nonce) => nonce >= minimumReusableNonce && nonce !== reservation.nonce,
      )
      for (const [nonce, status] of state.statuses) {
        if (status === 'reusable' || (submitted && nonce <= reservation.nonce)) {
          state.statuses.delete(nonce)
        }
      }

      if (submitted) {
        if (state.next <= reservation.nonce) state.next = reservation.nonce + 1
        state.statuses.set(reservation.nonce, location === 'block' ? 'confirmed' : 'submitted')
      } else {
        state.statuses.set(reservation.nonce, 'reusable')
        if (!state.reusable.includes(reservation.nonce)) {
          state.reusable.push(reservation.nonce)
          state.reusable.sort((left, right) => left - right)
        }
      }
      while (state.statuses.has(state.next) && state.statuses.get(state.next) !== 'reusable') {
        state.next += 1
      }
      pruneNonceStatuses(state)
    })
  }

  private async submittedExtrinsicLocation(extrinsicHash: string): Promise<SubmittedExtrinsicLocation> {
    const normalized = extrinsicHash.toLowerCase()
    if (await this.pendingExtrinsicsContain(normalized)) return 'pool'
    return await this.recentBlocksContainExtrinsic(normalized) ? 'block' : null
  }

  private async pendingExtrinsicsContain(extrinsicHash: string): Promise<boolean> {
    const pending = await this.rpc('author_pendingExtrinsics')
    if (!Array.isArray(pending)) return false
    return pending.some((extrinsic) => hashExtrinsicHex(extrinsic) === extrinsicHash)
  }

  private async recentBlocksContainExtrinsic(extrinsicHash: string): Promise<boolean> {
    const current = await this.blockNumber()
    const earliest = Math.max(0, current - DEFAULT_NONCE_RECONCILE_BLOCKS + 1)
    for (let number = current; number >= earliest; number -= 1) {
      const blockHash = await this.blockHash(number)
      const raw = await this.rpc('chain_getBlock', [blockHash])
      const extrinsics = (raw as { block?: { extrinsics?: unknown[] } })?.block?.extrinsics ?? []
      if (extrinsics.some((extrinsic) => hashExtrinsicHex(extrinsic) === extrinsicHash)) return true
    }
    return false
  }

  private withNonceAccount<T>(
    address: string,
    operation: (state: NonceAccountState) => T | Promise<T>,
  ): Promise<T> {
    const state = this.nonceAccount(address)
    const run = state.queue.then(() => operation(state), () => operation(state))
    state.queue = run.then(() => undefined, () => undefined)
    return run
  }

  private nonceAccount(address: string): NonceAccountState {
    let state = this.nonceAccounts.get(address)
    if (state == null) {
      state = {
        reusable: [],
        statuses: new Map(),
        queue: Promise.resolve(),
      }
      this.nonceAccounts.set(address, state)
    }
    return state
  }

  async estimateFee(call: CallLike, signer: SignerLike): Promise<Balance> {
    const runtime = await this.runtimeAt()
    const account = await this.resolveSigner(signer, runtime)
    const signed = await this.signExtrinsic(call, signer, {
      nonce: await this.peekNextIndex(account.ss58Address),
      period: null,
    })
    const length = Buffer.alloc(4)
    length.writeUInt32LE(signed.bytes.length, 0)
    const raw = await this.rpc('state_call', ['TransactionPaymentApi_query_info', hex(Buffer.concat([signed.bytes, length])), null])
    const info = runtime.decodeTypeId<Record<string, ScaleValue>>(
      runtime.runtimeApis().TransactionPaymentApi.query_info.outputTypeId,
      hexToBuffer(String(raw)),
      false,
    )
    return Balance.fromRao(String(info.partial_fee ?? info.partialFee ?? 0))
  }

  submitCall(pallet: string, fn: string, params: ScaleValue, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit([pallet, fn, params], signer, options)
  }

  submit_call(pallet: string, fn: string, params: ScaleValue, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submitCall(pallet, fn, params, signer, options)
  }

  transfer(signer: SignerLike, dest: string, amount: BalanceLike, options: SubmitOptions & { keepAlive?: boolean } = {}): Promise<ExtrinsicResult> {
    return this.submit(calls.balances[options.keepAlive === false ? 'transferAllowDeath' : 'transferKeepAlive'](dest, amount), signer, {
      waitForInclusion: true,
      ...options,
    })
  }

  transfer_keep_alive(signer: SignerLike, dest: string, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.transfer(signer, dest, amount, { keepAlive: true, ...options })
  }

  transfer_allow_death(signer: SignerLike, dest: string, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.transfer(signer, dest, amount, { keepAlive: false, ...options })
  }

  setWeights(signer: SignerLike, netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(calls.subtensor.setWeights(netuid, dests, weights, versionKey), signer, { waitForInclusion: true, ...options })
  }

  set_weights(signer: SignerLike, netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.setWeights(signer, netuid, dests, weights, versionKey, options)
  }

  burnedRegister(signer: SignerLike, netuid: number, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(calls.subtensor.burnedRegister(netuid, hotkey), signer, { waitForInclusion: true, ...options })
  }

  burned_register(signer: SignerLike, netuid: number, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.burnedRegister(signer, netuid, hotkey, options)
  }

  rootRegister(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(calls.subtensor.rootRegister(hotkey), signer, { waitForInclusion: true, ...options })
  }

  root_register(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.rootRegister(signer, hotkey, options)
  }

  registerNetwork(signer: SignerLike, hotkey: string, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.submit(calls.subtensor.registerNetwork(hotkey), signer, { waitForInclusion: true, ...options })
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
    args: { netuid: number; ip: number; port: number; version?: number; ipType?: number; protocol?: number },
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    return this.submit(calls.subtensor.serveAxon(args.netuid, args.ip, args.port, args.version, args.ipType, args.protocol), signer, {
      waitForInclusion: true,
      ...options,
    })
  }

  serve_axon(
    signer: SignerLike,
    args: { netuid: number; ip: number; port: number; version?: number; ipType?: number; protocol?: number },
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

  getSubnetHyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.subnets.hyperparameters<T>(netuid, block)
  }

  get_subnet_hyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.getSubnetHyperparameters<T>(netuid, block)
  }

  async callData(call: CallLike, block?: number | string | null): Promise<Buffer> {
    if (Buffer.isBuffer(call) || call instanceof Uint8Array) return toBuffer(call, 'call')
    const [pallet, fn, params] = normalizeCall(call)
    return this.composeCall(pallet, fn, params, block)
  }

  async resolveBlockHash(block?: number | string | null): Promise<string | null> {
    if (block == null) return null
    if (typeof block === 'string') return block
    return this.blockHash(block)
  }

  private async resolveSigner(signer: SignerLike, runtime: Runtime): Promise<ResolvedSigner> {
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
    const cryptoType = account?.cryptoType ?? signerShape.cryptoType ?? CRYPTO_SR25519
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

  private async signWithSigner(
    signer: Keypair | ChainSigner,
    payload: Buffer,
    context: SignerPayloadContext,
  ): Promise<NormalizedSignature> {
    const signerShape = signer as ChainSigner
    if (signerShape.signPayload != null) {
      return normalizeSignature(
        await signerShape.signPayload(payload, context),
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
    throw new ChainError('signer must implement sign(), signPayload(), or signRaw()')
  }

  private async normalizeEra(period: number | null): Promise<{ era: ScaleValue; eraBlockHash: string }> {
    if (period == null) return { era: '00', eraBlockHash: await this.genesisHash() }
    const finalized = await this.finalizedHead()
    const current = await this.blockNumber(finalized)
    const birth = Number(eraBirth(period, current))
    return { era: { period, current }, eraBlockHash: await this.blockHash(birth) }
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
        const fatal = ['usurped', 'retracted', 'finalitytimeout', 'dropped', 'invalid'].find((name) => normalized[name] != null)
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

  private async resolveInclusion(extrinsicHash: string, blockHash: string, finalized: boolean): Promise<ExtrinsicResult> {
    const block = (await this.rpc('chain_getBlock', [blockHash])) as { block: { header: unknown; extrinsics: string[] } }
    const blockNumber = headerNumber(block.block.header)
    const extrinsicIndex = block.block.extrinsics.findIndex((item) => hex(blake2_256(hexToBuffer(item))) === extrinsicHash)
    if (extrinsicIndex < 0) throw new ChainError(`extrinsic ${extrinsicHash} was not found in block ${blockHash}`)
    const events = ((await this.query<ScaleValue[]>(storage.System.Events, [], blockHash)) ?? []) as unknown[]
    if (events.some((event) => eventName(event) === 'System.CodeUpdated')) this.invalidateRuntimeCache()
    const triggered = events.filter((event) => eventExtrinsicIndex(event) === extrinsicIndex)
    const failed = triggered.find((event) => eventName(event) === 'System.ExtrinsicFailed')
    const success = triggered.some((event) => eventName(event) === 'System.ExtrinsicSuccess')
    const feeEvent = triggered.find((event) => eventName(event) === 'TransactionPayment.TransactionFeePaid')
    const dispatchSuccess = success && failed == null
    return {
      status: dispatchSuccess ? finalized ? 'finalized' : 'inBlock' : 'failed',
      success: dispatchSuccess,
      message: failed == null ? 'Success' : 'Extrinsic failed',
      extrinsicHash,
      blockHash,
      blockNumber,
      extrinsicIndex,
      extrinsicId: `${blockNumber}-${String(extrinsicIndex).padStart(4, '0')}`,
      finalized,
      fee: feeEvent == null ? undefined : feeFromEvent(feeEvent),
      events: triggered,
      error: failed,
    }
  }
}

export class Snapshot {
  readonly balances: SnapshotBalancesNamespace
  readonly subnets: SnapshotSubnetsNamespace
  readonly staking: SnapshotStakingNamespace
  readonly neurons: SnapshotNeuronsNamespace

  constructor(readonly client: Client, readonly block: number, readonly blockHash: string) {
    this.balances = new SnapshotBalancesNamespace(this)
    this.subnets = new SnapshotSubnetsNamespace(this)
    this.staking = new SnapshotStakingNamespace(this)
    this.neurons = new SnapshotNeuronsNamespace(this)
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

  read(name: string, params: Record<string, ScaleValue> = {}): Promise<unknown> {
    return read(this.client, name, params, this.blockHash)
  }
}

export class BalancesNamespace {
  constructor(private readonly client: Client) {}

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
}

export class SubnetsNamespace {
  constructor(private readonly client: Client) {}

  async subnet(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    const [tempo, burn, count] = await Promise.all([
      this.client.query(storage.SubtensorModule.Tempo, [netuid], block),
      this.client.query(storage.SubtensorModule.Burn, [netuid], block),
      this.client.query(storage.SubtensorModule.SubnetworkN, [netuid], block),
    ])
    return { netuid, tempo: Number(tempo ?? 0), burn: Balance.fromRao(String(burn ?? 0)), neuronCount: Number(count ?? 0) }
  }

  info(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    return this.subnet(netuid, block)
  }

  async all(block?: number | string | null): Promise<SubnetInfo[]> {
    const [added, tempos, burns, counts] = await Promise.all([
      this.client.queryMap<number, boolean>(storage.SubtensorModule.NetworksAdded, [], block),
      this.client.queryMap<number, number>(storage.SubtensorModule.Tempo, [], block),
      this.client.queryMap<number, bigint>(storage.SubtensorModule.Burn, [], block),
      this.client.queryMap<number, number>(storage.SubtensorModule.SubnetworkN, [], block),
    ])
    const tempoByNetuid = new Map(tempos.map(([key, value]) => [Number(key), Number(value)]))
    const burnByNetuid = new Map(burns.map(([key, value]) => [Number(key), String(value)]))
    const countByNetuid = new Map(counts.map(([key, value]) => [Number(key), Number(value)]))
    return added
      .filter(([, value]) => value)
      .map(([netuid]) => Number(netuid))
      .sort((a, b) => a - b)
      .map((netuid) => ({
        netuid,
        tempo: tempoByNetuid.get(netuid) ?? 0,
        burn: Balance.fromRao(burnByNetuid.get(netuid) ?? 0),
        neuronCount: countByNetuid.get(netuid) ?? 0,
      }))
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

  metagraph<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.client.runtime<T>(runtimeApi.SubnetInfoRuntimeApi.get_metagraph, [netuid], block)
  }

  hyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.client.runtime<T>(runtimeApi.SubnetInfoRuntimeApi.get_subnet_hyperparams, [netuid], block)
  }

  subnetHyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.hyperparameters<T>(netuid, block)
  }

  subnet_hyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
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
  constructor(private readonly client: Client) {}

  all<T extends ScaleValue = ScaleValue>(netuid: number, lite = true, block?: number | string | null): Promise<T> {
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
  raw: Record<string, ScaleValue>
}

export class StakingNamespace {
  constructor(private readonly client: Client) {}

  async get(coldkey: string, hotkey: string, netuid: number, block?: number | string | null): Promise<Balance> {
    const info = await this.client.runtime<Record<string, ScaleValue> | null>(
      runtimeApi.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
      [hotkey, coldkey, netuid],
      block,
    )
    return Balance.fromRao(String(info?.stake ?? 0), netuid)
  }

  async positions(coldkey: string, block?: number | string | null): Promise<StakePosition[]> {
    const records = await this.client.runtime<Array<Record<string, ScaleValue>>>(
      runtimeApi.StakeInfoRuntimeApi.get_stake_info_for_coldkey,
      [coldkey],
      block,
    )
    return (records ?? []).map((record) => ({
      hotkey: String(record.hotkey),
      coldkey: String(record.coldkey),
      netuid: Number(record.netuid ?? 0),
      stake: Balance.fromRao(String(record.stake ?? 0), Number(record.netuid ?? 0)),
      isRegistered: Boolean(record.is_registered ?? record.isRegistered ?? false),
      raw: record,
    }))
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

  addStake(signer: SignerLike, hotkey: string, netuid: number, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.client.submit(calls.subtensor.addStake(hotkey, netuid, amount), signer, { waitForInclusion: true, ...options })
  }

  add_stake(signer: SignerLike, hotkey: string, netuid: number, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.addStake(signer, hotkey, netuid, amount, options)
  }

  removeStake(signer: SignerLike, hotkey: string, netuid: number, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    return this.client.submit(calls.subtensor.removeStake(hotkey, netuid, amount), signer, { waitForInclusion: true, ...options })
  }

  remove_stake(signer: SignerLike, hotkey: string, netuid: number, amount: BalanceLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
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

export const storage = Object.freeze({
  Balances: Object.freeze({
    Account: descriptor('Balances', 'Account'),
    TotalIssuance: descriptor('Balances', 'TotalIssuance'),
    Locks: descriptor('Balances', 'Locks'),
    Holds: descriptor('Balances', 'Holds'),
  }),
  Multisig: Object.freeze({ Multisigs: descriptor('Multisig', 'Multisigs') }),
  Proxy: Object.freeze({ Proxies: descriptor('Proxy', 'Proxies') }),
  SubtensorModule: Object.freeze({
    NetworksAdded: descriptor('SubtensorModule', 'NetworksAdded'),
    Tempo: descriptor('SubtensorModule', 'Tempo'),
    Burn: descriptor('SubtensorModule', 'Burn'),
    SubnetworkN: descriptor('SubtensorModule', 'SubnetworkN'),
    CommitRevealWeightsEnabled: descriptor('SubtensorModule', 'CommitRevealWeightsEnabled'),
    TokenSymbol: descriptor('SubtensorModule', 'TokenSymbol'),
    SubnetIdentitiesV3: descriptor('SubtensorModule', 'SubnetIdentitiesV3'),
    StakingHotkeys: descriptor('SubtensorModule', 'StakingHotkeys'),
    OwnedHotkeys: descriptor('SubtensorModule', 'OwnedHotkeys'),
    AutoStakeDestination: descriptor('SubtensorModule', 'AutoStakeDestination'),
    AutoStakeDestinationColdkeys: descriptor('SubtensorModule', 'AutoStakeDestinationColdkeys'),
    TotalStake: descriptor('SubtensorModule', 'TotalStake'),
    StakeThreshold: descriptor('SubtensorModule', 'StakeThreshold'),
    LastEpochBlock: descriptor('SubtensorModule', 'LastEpochBlock'),
  }),
  System: Object.freeze({
    Account: descriptor('System', 'Account'),
    Events: descriptor('System', 'Events'),
  }),
  Timestamp: Object.freeze({ Now: descriptor('Timestamp', 'Now') }),
})

export const constants = Object.freeze({
  Balances: Object.freeze({ ExistentialDeposit: descriptor('Balances', 'ExistentialDeposit') }),
  SubtensorModule: Object.freeze({
    InitialMinStake: descriptor('SubtensorModule', 'InitialMinStake'),
    InitialStartCallDelay: descriptor('SubtensorModule', 'InitialStartCallDelay'),
    InitialWeightsVersionKey: descriptor('SubtensorModule', 'InitialWeightsVersionKey'),
  }),
})

export const runtimeApi = Object.freeze({
  DelegateInfoRuntimeApi: Object.freeze({
    get_delegates: descriptor('DelegateInfoRuntimeApi', 'get_delegates'),
    get_delegate: descriptor('DelegateInfoRuntimeApi', 'get_delegate'),
    get_delegated: descriptor('DelegateInfoRuntimeApi', 'get_delegated'),
  }),
  NeuronInfoRuntimeApi: Object.freeze({
    get_neurons: descriptor('NeuronInfoRuntimeApi', 'get_neurons'),
    get_neuron: descriptor('NeuronInfoRuntimeApi', 'get_neuron'),
    get_neurons_lite: descriptor('NeuronInfoRuntimeApi', 'get_neurons_lite'),
    get_neuron_lite: descriptor('NeuronInfoRuntimeApi', 'get_neuron_lite'),
  }),
  StakeInfoRuntimeApi: Object.freeze({
    get_stake_info_for_coldkey: descriptor('StakeInfoRuntimeApi', 'get_stake_info_for_coldkey'),
    get_stake_info_for_coldkeys: descriptor('StakeInfoRuntimeApi', 'get_stake_info_for_coldkeys'),
    get_stake_info_for_hotkey_coldkey_netuid: descriptor('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid'),
    get_stake_availability_for_coldkeys: descriptor('StakeInfoRuntimeApi', 'get_stake_availability_for_coldkeys'),
    get_stake_fee: descriptor('StakeInfoRuntimeApi', 'get_stake_fee'),
  }),
  SubnetInfoRuntimeApi: Object.freeze({
    get_subnet_info: descriptor('SubnetInfoRuntimeApi', 'get_subnet_info'),
    get_subnets_info: descriptor('SubnetInfoRuntimeApi', 'get_subnets_info'),
    get_subnet_hyperparams: descriptor('SubnetInfoRuntimeApi', 'get_subnet_hyperparams'),
    get_all_dynamic_info: descriptor('SubnetInfoRuntimeApi', 'get_all_dynamic_info'),
    get_all_metagraphs: descriptor('SubnetInfoRuntimeApi', 'get_all_metagraphs'),
    get_metagraph: descriptor('SubnetInfoRuntimeApi', 'get_metagraph'),
    get_dynamic_info: descriptor('SubnetInfoRuntimeApi', 'get_dynamic_info'),
    get_subnet_state: descriptor('SubnetInfoRuntimeApi', 'get_subnet_state'),
    get_selective_metagraph: descriptor('SubnetInfoRuntimeApi', 'get_selective_metagraph'),
    get_next_epoch_start_block: descriptor('SubnetInfoRuntimeApi', 'get_next_epoch_start_block'),
  }),
  SubnetRegistrationRuntimeApi: Object.freeze({
    get_network_registration_cost: descriptor('SubnetRegistrationRuntimeApi', 'get_network_registration_cost'),
  }),
  TransactionPaymentApi: Object.freeze({
    query_info: descriptor('TransactionPaymentApi', 'query_info'),
    query_fee_details: descriptor('TransactionPaymentApi', 'query_fee_details'),
  }),
})

export const runtimeApis = runtimeApi

export const calls = Object.freeze({
  balances: Object.freeze({
    transferKeepAlive(dest: string, value: BalanceLike) {
      return call('Balances', 'transfer_keep_alive', { dest, value: balanceRao(value) })
    },
    transferAllowDeath(dest: string, value: BalanceLike) {
      return call('Balances', 'transfer_allow_death', { dest, value: balanceRao(value) })
    },
  }),
  subtensor: Object.freeze({
    addStake(hotkey: string, netuid: number, amount: BalanceLike) {
      return call('SubtensorModule', 'add_stake', { hotkey, netuid, amount_staked: balanceRao(amount) })
    },
    burnedRegister(netuid: number, hotkey: string) {
      return call('SubtensorModule', 'burned_register', { netuid, hotkey })
    },
    commitWeights(netuid: number, commitHash: ByteLike | string) {
      return call('SubtensorModule', 'commit_weights', { netuid, commit_hash: commitHash })
    },
    moveStake(originHotkey: string, destinationHotkey: string, originNetuid: number, destinationNetuid: number, amount: BalanceLike) {
      return call('SubtensorModule', 'move_stake', {
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: balanceRao(amount),
      })
    },
    register(netuid: number, blockNumber: bigint | number | string, nonce: bigint | number | string, work: ByteLike, hotkey: string, coldkey: string) {
      return call('SubtensorModule', 'register', { netuid, block_number: BigInt(blockNumber), nonce: BigInt(nonce), work, hotkey, coldkey })
    },
    registerNetwork(hotkey: string) {
      return call('SubtensorModule', 'register_network', { hotkey })
    },
    removeStake(hotkey: string, netuid: number, amount: BalanceLike) {
      return call('SubtensorModule', 'remove_stake', { hotkey, netuid, amount_unstaked: balanceRao(amount) })
    },
    revealWeights(netuid: number, uids: number[], values: number[], salt: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'reveal_weights', { netuid, uids, values, salt, version_key: BigInt(versionKey) })
    },
    rootRegister(hotkey: string) {
      return call('SubtensorModule', 'root_register', { hotkey })
    },
    serveAxon(netuid: number, ip: number, port: number, version = 0, ipType = 4, protocol = 4) {
      return call('SubtensorModule', 'serve_axon', { netuid, version, ip, port, ip_type: ipType, protocol, placeholder1: 0, placeholder2: 0 })
    },
    servePrometheus(netuid: number, ip: number, port: number, version = 0, ipType = 4) {
      return call('SubtensorModule', 'serve_prometheus', { netuid, version, ip, port, ip_type: ipType })
    },
    setChildren(hotkey: string, netuid: number, children: ScaleValue) {
      return call('SubtensorModule', 'set_children', { hotkey, netuid, children })
    },
    setWeights(netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'set_weights', { netuid, dests, weights, version_key: BigInt(versionKey) })
    },
    startCall(netuid: number) {
      return call('SubtensorModule', 'start_call', { netuid })
    },
    transferStake(destinationColdkey: string, hotkey: string, originNetuid: number, destinationNetuid: number, amount: BalanceLike) {
      return call('SubtensorModule', 'transfer_stake', {
        destination_coldkey: destinationColdkey,
        hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: balanceRao(amount),
      })
    },
    unstakeAll(hotkey: string) {
      return call('SubtensorModule', 'unstake_all', { hotkey })
    },
  }),
  Balances: Object.freeze({
    transfer_keep_alive(dest: string, value: BalanceLike) {
      return call('Balances', 'transfer_keep_alive', { dest, value: balanceRao(value) })
    },
    transfer_allow_death(dest: string, value: BalanceLike) {
      return call('Balances', 'transfer_allow_death', { dest, value: balanceRao(value) })
    },
  }),
  SubtensorModule: Object.freeze({
    add_stake(hotkey: string, netuid: number, amountStaked: BalanceLike) {
      return call('SubtensorModule', 'add_stake', { hotkey, netuid, amount_staked: balanceRao(amountStaked) })
    },
    burned_register(netuid: number, hotkey: string) {
      return call('SubtensorModule', 'burned_register', { netuid, hotkey })
    },
    commit_weights(netuid: number, commitHash: ByteLike | string) {
      return call('SubtensorModule', 'commit_weights', { netuid, commit_hash: commitHash })
    },
    move_stake(originHotkey: string, destinationHotkey: string, originNetuid: number, destinationNetuid: number, alphaAmount: BalanceLike) {
      return call('SubtensorModule', 'move_stake', {
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: balanceRao(alphaAmount),
      })
    },
    register(netuid: number, blockNumber: bigint | number | string, nonce: bigint | number | string, work: ByteLike, hotkey: string, coldkey: string) {
      return call('SubtensorModule', 'register', { netuid, block_number: BigInt(blockNumber), nonce: BigInt(nonce), work, hotkey, coldkey })
    },
    register_network(hotkey: string) {
      return call('SubtensorModule', 'register_network', { hotkey })
    },
    remove_stake(hotkey: string, netuid: number, amountUnstaked: BalanceLike) {
      return call('SubtensorModule', 'remove_stake', { hotkey, netuid, amount_unstaked: balanceRao(amountUnstaked) })
    },
    reveal_weights(netuid: number, uids: number[], values: number[], salt: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'reveal_weights', { netuid, uids, values, salt, version_key: BigInt(versionKey) })
    },
    root_register(hotkey: string) {
      return call('SubtensorModule', 'root_register', { hotkey })
    },
    serve_axon(netuid: number, ip: number, port: number, version = 0, ipType = 4, protocol = 4) {
      return call('SubtensorModule', 'serve_axon', { netuid, version, ip, port, ip_type: ipType, protocol, placeholder1: 0, placeholder2: 0 })
    },
    serve_prometheus(netuid: number, ip: number, port: number, version = 0, ipType = 4) {
      return call('SubtensorModule', 'serve_prometheus', { netuid, version, ip, port, ip_type: ipType })
    },
    set_children(hotkey: string, netuid: number, children: ScaleValue) {
      return call('SubtensorModule', 'set_children', { hotkey, netuid, children })
    },
    set_weights(netuid: number, dests: number[], weights: number[], versionKey: bigint | number | string) {
      return call('SubtensorModule', 'set_weights', { netuid, dests, weights, version_key: BigInt(versionKey) })
    },
    start_call(netuid: number) {
      return call('SubtensorModule', 'start_call', { netuid })
    },
    transfer_stake(destinationColdkey: string, hotkey: string, originNetuid: number, destinationNetuid: number, alphaAmount: BalanceLike) {
      return call('SubtensorModule', 'transfer_stake', {
        destination_coldkey: destinationColdkey,
        hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: balanceRao(alphaAmount),
      })
    },
    unstake_all(hotkey: string) {
      return call('SubtensorModule', 'unstake_all', { hotkey })
    },
  }),
})

const READS = [
  { name: 'balance', category: 'Accounts & keys', params: ['coldkey_ss58'] },
  { name: 'balances', category: 'Accounts & keys', params: ['coldkey_ss58s'] },
  { name: 'subnet', category: 'Subnets', params: ['netuid'] },
  { name: 'subnets', category: 'Subnets', params: [] },
  { name: 'burn', category: 'Subnets', params: ['netuid'] },
  { name: 'commit_reveal_enabled', category: 'Subnets', params: ['netuid'] },
  { name: 'subnet_hyperparameters', category: 'Subnets', params: ['netuid'] },
  { name: 'metagraph', category: 'Subnets', params: ['netuid'] },
  { name: 'stake', category: 'Staking', params: ['coldkey_ss58', 'hotkey_ss58', 'netuid'] },
  { name: 'stake_for_coldkey', category: 'Staking', params: ['coldkey_ss58'] },
]

async function read(client: Client, name: string, params: Record<string, ScaleValue>, block?: number | string | null): Promise<unknown> {
  switch (name) {
    case 'balance':
      return client.balances.get(String(params.coldkey_ss58), block)
    case 'balances':
      return client.balances.getMany(Array.isArray(params.coldkey_ss58s) ? params.coldkey_ss58s.map(String) : [], block)
    case 'subnet':
      return client.subnets.info(Number(params.netuid), block)
    case 'subnets':
      return client.subnets.all(block)
    case 'burn':
      return client.subnets.burn(Number(params.netuid), block)
    case 'commit_reveal_enabled':
      return client.subnets.commitRevealEnabled(Number(params.netuid), block)
    case 'subnet_hyperparameters':
      return client.subnets.hyperparameters(Number(params.netuid), block)
    case 'metagraph':
      return client.subnets.metagraph(Number(params.netuid), block)
    case 'stake':
      return client.staking.get(String(params.coldkey_ss58), String(params.hotkey_ss58), Number(params.netuid), block)
    case 'stake_for_coldkey':
      return client.staking.positions(String(params.coldkey_ss58), block)
    default:
      throw new ChainError(`unknown read ${name}`)
  }
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

function nonNegativeNumber(value: unknown, fallback: number): number {
  if (value == null) return fallback
  const parsed = Number(value)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback
}

function nonNegativeInteger(value: unknown, fallback: number): number {
  return Math.floor(nonNegativeNumber(value, fallback))
}

function accountAddress(account?: SignerAccount | null): string | undefined {
  return stringValue(account?.ss58Address) ?? stringValue(account?.address)
}

function staticSignerAddress(signer: unknown): string | undefined {
  const value = signer as { ss58Address?: unknown; address?: unknown }
  return stringValue(value.ss58Address) ?? stringValue(value.address)
}

function managedNonceReservation(extrinsic: unknown): NonceReservation | undefined {
  if (extrinsic == null || typeof extrinsic !== 'object') return undefined
  if (Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array) return undefined
  return (extrinsic as ManagedSignedExtrinsicResult)[MANAGED_NONCE]
}

function hashExtrinsicHex(extrinsic: unknown): string | undefined {
  if (typeof extrinsic !== 'string') return undefined
  try {
    return hex(blake2_256(hexToBuffer(extrinsic))).toLowerCase()
  } catch {
    return undefined
  }
}

function pruneNonceStatuses(state: NonceAccountState): void {
  if (state.statuses.size <= 512) return
  for (const [nonce, status] of state.statuses) {
    if (state.statuses.size <= 256) break
    if (status === 'confirmed' || status === 'failed') state.statuses.delete(nonce)
  }
}

function invalidateNonceState(state: NonceAccountState): void {
  state.next = undefined
  state.reusable = []
  state.statuses.clear()
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function signerPublicKey(value: ByteLike | undefined, ss58Address: string): Buffer {
  return value == null
    ? publicKeyFromSs58(ss58Address)
    : toBuffer(value, 'signer.publicKey')
}

function normalizeSignature(
  result: SignerSignature,
  fallbackCryptoType: number,
): NormalizedSignature {
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

function normalizeCall(callLike: Exclude<CallLike, ByteLike>): [string, string, ScaleValue] {
  if (isCallTuple(callLike)) return [callLike[0], callLike[1], callLike[2] ?? {}]
  return [callLike.pallet ?? callLike.module ?? '', callLike.call ?? callLike.function ?? '', callLike.params ?? {}]
}

function isCallTuple(callLike: Exclude<CallLike, ByteLike>): callLike is readonly [string, string, ScaleValue?] {
  return Array.isArray(callLike)
}

function hex(bytes: ByteLike): string {
  return `0x${toBuffer(bytes, 'bytes').toString('hex')}`
}

function hexToBuffer(value: string): Buffer {
  const text = value.startsWith('0x') ? value.slice(2) : value
  return Buffer.from(text, 'hex')
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
  if (value.extrinsic_idx != null) return Number(value.extrinsic_idx)
  const phase = value.phase as Record<string, unknown> | undefined
  const apply = phase?.ApplyExtrinsic ?? phase?.applyExtrinsic
  return apply == null ? null : Number(apply)
}

function feeFromEvent(event: unknown): Balance | undefined {
  const attrs = (event as { attributes?: unknown }).attributes
  if (attrs == null || typeof attrs !== 'object') return undefined
  const amount = (attrs as Record<string, unknown>).actual_fee ?? (attrs as Record<string, unknown>).actualFee ?? (attrs as Record<string, unknown>).fee
  return amount == null ? undefined : Balance.fromRao(String(amount))
}

interface RequestSignal {
  signal?: AbortSignal
  cleanup(): void
  onAbort(handler: (error: Error) => void): void
  error(): Error
}

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  throwIfAborted(signal)
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      cleanup()
      resolve()
    }, ms)
    const abort = () => {
      cleanup()
      reject(new RequestAbortedError())
    }
    const cleanup = () => {
      clearTimeout(timer)
      signal?.removeEventListener('abort', abort)
    }
    signal?.addEventListener('abort', abort, { once: true })
  })
}

function withRequestSignal(options: RpcRequestOptions): RequestSignal {
  const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS
  const controller = new AbortController()
  let timedOut = false
  let abortError: Error | undefined
  let cleaned = false
  const listeners = new Set<(error: Error) => void>()
  const notify = (error: Error) => {
    abortError = error
    for (const listener of [...listeners]) listener(error)
  }
  const timeout = timeoutMs === 0
    ? undefined
    : setTimeout(() => {
        timedOut = true
        controller.abort()
        notify(new RequestTimeoutError(`request timed out after ${timeoutMs}ms`))
      }, timeoutMs)
  const abort = () => {
    controller.abort()
    notify(new RequestAbortedError())
  }
  if (options.signal?.aborted) abort()
  else options.signal?.addEventListener('abort', abort, { once: true })

  return {
    signal: controller.signal,
    cleanup() {
      if (cleaned) return
      cleaned = true
      if (timeout != null) clearTimeout(timeout)
      options.signal?.removeEventListener('abort', abort)
      listeners.clear()
    },
    onAbort(handler: (error: Error) => void) {
      if (abortError != null) handler(abortError)
      else listeners.add(handler)
    },
    error() {
      return abortError ?? (timedOut
        ? new RequestTimeoutError(`request timed out after ${timeoutMs}ms`)
        : new RequestAbortedError())
    },
  }
}

function normalizeAbortError(error: unknown, request: RequestSignal): Error {
  if (request.signal?.aborted) return request.error()
  return error instanceof Error ? error : new Error(String(error))
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw new RequestAbortedError()
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
