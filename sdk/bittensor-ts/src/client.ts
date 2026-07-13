import { blake2_256, generateExtrinsicProof, hexToBytes, metadataDigest } from './crypto'
import { CRYPTO_ED25519, CRYPTO_SR25519, Keypair, publicKeyFromSs58 } from './keys'
import { LedgerDevice } from './ledger'
import { Runtime, decodeOptionalOpaqueMetadata, eraBirth, type RuntimeSignerPayload } from './runtime'
import { IntentCall, NativeChainClient, Policy, isIntentCall, rawCall, type PolicyOptions, type SignerRoleLike } from './transaction'
import { WIRE_TAG, fromWire, toBigInt, toBuffer } from './wire'
import type { NativeChainInfo, NativeExternalSigningOptions, NativeExternalSigningPlanHandle, NativeTxOutcome } from './native'
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
import type { ByteLike, ChainInfo, ScaleValue, SignedExtrinsic, StorageEntry, TransactionParams } from './types'

export const SS58_FORMAT = 42
export const DEFAULT_ERA_PERIOD = 64
export const DEFAULT_HEAD_RUNTIME_TTL_MS = 12_000
export const DEFAULT_HISTORICAL_RUNTIME_CACHE_SIZE = 64
export const DEFAULT_REQUEST_TIMEOUT_MS = 30_000
export const DEFAULT_MAX_REQUEST_RETRIES = 2
export const DEFAULT_RETRY_BACKOFF_MS = 250
export const DEFAULT_MAX_RETRY_BACKOFF_MS = 5_000
export const DEFAULT_ENDPOINT_VALIDATION_TTL_MS = 60_000
export const DEFAULT_MAX_SUBSCRIPTION_QUEUE = 1024
export const DEFAULT_MAX_WS_MESSAGE_BYTES = 16 * 1024 * 1024
const V15_METADATA_VERSION_HEX = '0x0f000000'
const V15_METADATA_MISSING_NEEDLE = 'Exported method Metadata_metadata_at_version is not found'
const IDEMPOTENT_RPC_METHODS = new Set([
  'chain_getBlock',
  'chain_getBlockHash',
  'chain_getFinalizedHead',
  'chain_getHeader',
  'chain_subscribeFinalizedHeads',
  'chain_subscribeNewHeads',
  'chain_unsubscribeFinalizedHeads',
  'chain_unsubscribeNewHeads',
  'state_call',
  'state_getKeysPaged',
  'state_getMetadata',
  'state_getRuntimeVersion',
  'state_getStorage',
  'state_queryStorageAt',
  'state_subscribeRuntimeVersion',
  'state_subscribeStorage',
  'state_unsubscribeRuntimeVersion',
  'state_unsubscribeStorage',
  'system_accountNextIndex',
  'system_chain',
  'system_health',
  'system_name',
  'system_properties',
  'system_version',
])
export const NETWORKS = Object.freeze({
  finney: 'wss://entrypoint-finney.opentensor.ai:443',
  test: 'wss://test.finney.opentensor.ai:443',
  archive: 'wss://archive.chain.opentensor.ai:443',
  local: process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944',
})
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

export interface RpcRequestOptions {
  signal?: AbortSignal
  timeoutMs?: number
  maxRetries?: number
  retryForever?: boolean
  retryBackoffMs?: number
  maxRetryBackoffMs?: number
}

export interface JsonRpcTransportOptions {
  webSocket?: WebSocketConstructor
  webSocketConstructor?: WebSocketConstructor
  webSocketFactory?: WebSocketFactory
  validateEndpoint?: EndpointValidator
  requestTimeoutMs?: number
  maxRequestRetries?: number
  retryBackoffMs?: number
  maxRetryBackoffMs?: number
  endpointValidationTtlMs?: number
  maxSubscriptionQueue?: number
  maxMessageBytes?: number
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

export interface DescriptorSchemaIssue {
  kind: 'storage' | 'constant' | 'runtimeApi' | 'call'
  path: string
  descriptor: Descriptor
  message: string
}

interface RpcRequest {
  generation: number
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
  subscriptionGeneration?: number
  error?: Error
  resubscribing?: Promise<void>
}

interface EndpointAttempt {
  endpoint: string
  index: number
  generation: number
}

interface ActiveConnection extends EndpointAttempt {
  socket: WebSocketLike
}

interface ConnectingAttempt extends EndpointAttempt {
  promise: Promise<ActiveConnection>
}

interface ResolvedSigner {
  signer: Keypair | ChainSigner
  ss58Address: string
  publicKey: Buffer
  cryptoType: number
  requiresMetadataProof: boolean
}

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

function nativeRustClient(client: object): Promise<NativeChainClient | undefined> {
  return nativeClientAccess.get(client)?.() ?? Promise.resolve(undefined)
}

interface SubscriptionOptions extends RpcRequestOptions {
  resubscribe?: boolean
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
export type EndpointRequest = (method: string, params?: unknown[]) => Promise<unknown>
export type EndpointValidator = (endpoint: string, request: EndpointRequest) => Promise<void>

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

export class EndpointValidationError extends ChainError {
  constructor(message: string, details?: unknown) {
    super(message, details)
    this.name = 'EndpointValidationError'
  }
}

export class JsonRpcTransport {
  private readonly endpoints: string[]
  private readonly retryForever: boolean
  private readonly requestTimeoutMs: number
  private readonly maxRequestRetries: number
  private readonly retryBackoffMs: number
  private readonly maxRetryBackoffMs: number
  private readonly endpointValidationTtlMs: number
  private readonly maxSubscriptionQueue: number
  private readonly maxMessageBytes: number
  private readonly webSocketConstructor?: WebSocketConstructor
  private readonly webSocketFactory?: WebSocketFactory
  private readonly validateEndpoint?: EndpointValidator
  private readonly validatedEndpoints = new Map<string, number>()
  private endpointIndex = 0
  private generation = 0
  private id = 1
  private socket?: ActiveConnection
  private connecting?: ConnectingAttempt
  private pending = new Map<number, RpcRequest>()
  private subscriptions = new Set<SubscriptionState>()
  private subscriptionsById = new Map<string, { state: SubscriptionState; generation: number }>()
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
    this.endpointValidationTtlMs = nonNegativeNumber(
      options.endpointValidationTtlMs,
      DEFAULT_ENDPOINT_VALIDATION_TTL_MS,
    )
    this.maxSubscriptionQueue = nonNegativeInteger(
      options.maxSubscriptionQueue,
      DEFAULT_MAX_SUBSCRIPTION_QUEUE,
    )
    this.maxMessageBytes = nonNegativeInteger(options.maxMessageBytes, DEFAULT_MAX_WS_MESSAGE_BYTES)
    this.webSocketConstructor = options.webSocketConstructor ?? options.webSocket
    this.webSocketFactory = options.webSocketFactory
    this.validateEndpoint = options.validateEndpoint
  }

  get endpoint(): string {
    return this.endpoints[this.endpointIndex]
  }

  async request(method: string, params: unknown[] = [], options: RpcRequestOptions = {}): Promise<unknown> {
    if (this.closed) throw new ChainError('transport closed')
    const requestOptions = {
      ...options,
      timeoutMs: options.timeoutMs ?? this.requestTimeoutMs,
    }
    throwIfAborted(requestOptions.signal)
    const retryByDefault = IDEMPOTENT_RPC_METHODS.has(method)
    const maxRetries = requestOptions.maxRetries ?? (retryByDefault ? this.maxRequestRetries : 0)
    const retryForever = requestOptions.retryForever ?? (retryByDefault ? this.retryForever : false)
    const retryBackoffMs = requestOptions.retryBackoffMs ?? this.retryBackoffMs
    const maxRetryBackoffMs = requestOptions.maxRetryBackoffMs ?? this.maxRetryBackoffMs
    let attempt = 0
    const attemptedEndpoints = new Set<string>()
    for (;;) {
      const endpointAttempt = this.currentAttempt()
      attemptedEndpoints.add(endpointAttempt.endpoint)
      try {
        await this.ensureEndpointValidated(endpointAttempt, requestOptions)
        return this.isHttpEndpoint(endpointAttempt.endpoint)
          ? await this.httpRequest(endpointAttempt, method, params, requestOptions)
          : await this.wsRequest(endpointAttempt, method, params, requestOptions)
      } catch (error) {
        if (error instanceof RequestAbortedError || error instanceof JsonRpcError) throw error
        if (error instanceof EndpointValidationError) {
          this.validatedEndpoints.delete(endpointAttempt.endpoint)
          if (attemptedEndpoints.size >= this.endpoints.length || !this.rotateEndpoint(endpointAttempt)) throw error
          continue
        }
        if (!retryForever && attempt >= maxRetries) throw error
        attempt += 1
        this.rotateEndpoint(endpointAttempt)
        const capped = Math.min(retryBackoffMs * (2 ** Math.max(0, attempt - 1)), maxRetryBackoffMs)
        await delay(capped, requestOptions.signal)
      }
    }
  }

  private async requestWebSocketOnly(
    method: string,
    params: unknown[] = [],
    options: RpcRequestOptions = {},
  ): Promise<unknown> {
    if (this.closed) throw new ChainError('transport closed')
    const requestOptions = {
      ...options,
      timeoutMs: options.timeoutMs ?? this.requestTimeoutMs,
    }
    throwIfAborted(requestOptions.signal)
    const maxRetries = requestOptions.maxRetries ?? this.maxRequestRetries
    const retryForever = requestOptions.retryForever ?? this.retryForever
    const retryBackoffMs = requestOptions.retryBackoffMs ?? this.retryBackoffMs
    const maxRetryBackoffMs = requestOptions.maxRetryBackoffMs ?? this.maxRetryBackoffMs
    let attempt = 0
    for (;;) {
      const endpointAttempt = this.currentWebSocketAttempt()
      try {
        await this.ensureEndpointValidated(endpointAttempt, requestOptions)
        return await this.wsRequest(endpointAttempt, method, params, requestOptions)
      } catch (error) {
        if (error instanceof RequestAbortedError || error instanceof JsonRpcError) throw error
        if (error instanceof EndpointValidationError) this.validatedEndpoints.delete(endpointAttempt.endpoint)
        if (!retryForever && attempt >= maxRetries) throw error
        attempt += 1
        const nextIndex = this.nextWebSocketEndpointIndex(endpointAttempt.index)
        if (nextIndex == null) throw error
        this.rotateEndpoint(endpointAttempt, nextIndex)
        const capped = Math.min(retryBackoffMs * (2 ** Math.max(0, attempt - 1)), maxRetryBackoffMs)
        await delay(capped, requestOptions.signal)
      }
    }
  }

  private currentAttempt(): EndpointAttempt {
    return {
      endpoint: this.endpoint,
      index: this.endpointIndex,
      generation: this.generation,
    }
  }

  private currentWebSocketAttempt(): EndpointAttempt {
    const current = this.currentAttempt()
    if (!this.isHttpEndpoint(current.endpoint)) return current
    const nextIndex = this.nextWebSocketEndpointIndex(current.index)
    if (nextIndex == null) {
      throw new ChainError('subscriptions require a WebSocket endpoint')
    }
    this.rotateEndpoint(current, nextIndex)
    return this.currentAttempt()
  }

  private nextWebSocketEndpointIndex(afterIndex: number): number | undefined {
    for (let offset = 1; offset <= this.endpoints.length; offset += 1) {
      const index = (afterIndex + offset) % this.endpoints.length
      if (!this.isHttpEndpoint(this.endpoints[index])) return index
    }
    return undefined
  }

  private async ensureEndpointValidated(attempt: EndpointAttempt, options: RpcRequestOptions): Promise<void> {
    if (this.validateEndpoint == null) return
    const validUntil = this.validatedEndpoints.get(attempt.endpoint)
    const now = Date.now()
    if (validUntil != null && validUntil > now) return
    await this.validateEndpoint(attempt.endpoint, (method, params = []) =>
      this.isHttpEndpoint(attempt.endpoint)
        ? this.httpRequest(attempt, method, params, options)
        : this.wsRequest(attempt, method, params, options),
    )
    if (this.endpointValidationTtlMs > 0) {
      this.validatedEndpoints.set(attempt.endpoint, Date.now() + this.endpointValidationTtlMs)
    }
  }

  async subscribe(
    subscribeMethod: string,
    params: unknown[] = [],
    unsubscribeMethod: string,
    options: SubscriptionOptions = {},
  ): Promise<AsyncIterable<unknown> & { unsubscribe(): Promise<void> }> {
    this.currentWebSocketAttempt()
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
        retryForever: options.retryForever,
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
      if (subscription != null) {
        await this.requestWebSocketOnly(unsubscribeMethod, [subscription]).catch(() => undefined)
      }
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
    this.generation += 1
    this.socket?.socket.close()
    this.socket = undefined
    this.connecting = undefined
    this.failPending(new ChainError('transport closed'))
    for (const state of [...this.subscriptions]) this.closeSubscription(state)
  }

  private isHttpEndpoint(endpoint: string): boolean {
    return endpoint.startsWith('http://') || endpoint.startsWith('https://')
  }

  private async httpRequest(
    attempt: EndpointAttempt,
    method: string,
    params: unknown[],
    options: RpcRequestOptions,
  ): Promise<unknown> {
    const request = withRequestSignal(options)
    const id = this.id++
    try {
      const response = await fetch(attempt.endpoint, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
        signal: request.signal,
      })
      if (!response.ok) throw new JsonRpcError(`HTTP ${response.status} from ${attempt.endpoint}`)
      return jsonRpcResponseResult(await parseJsonRpcResponse(response, this.maxMessageBytes), id)
    } catch (error) {
      throw normalizeAbortError(error, request)
    } finally {
      request.cleanup()
    }
  }

  private async wsRequest(
    attempt: EndpointAttempt,
    method: string,
    params: unknown[],
    options: RpcRequestOptions,
  ): Promise<unknown> {
    const connection = await this.connect(attempt, options)
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
        generation: connection.generation,
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
      connection.socket.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }))
    } catch (error) {
      const pending = this.pending.get(id)
      this.pending.delete(id)
      pending?.cleanup()
      throw error
    }
    return promise
  }

  private async connect(attempt: EndpointAttempt, options: RpcRequestOptions = {}): Promise<ActiveConnection> {
    if (this.closed) throw new ChainError('transport closed')
    if (
      this.socket?.generation === attempt.generation &&
      this.socket.endpoint === attempt.endpoint &&
      this.socket.socket.readyState === 1
    ) return this.socket
    if (
      this.connecting?.generation === attempt.generation &&
      this.connecting.endpoint === attempt.endpoint
    ) return this.connecting.promise
    if (this.generation !== attempt.generation || this.endpoint !== attempt.endpoint) {
      throw new ChainError(`stale endpoint attempt for ${attempt.endpoint}`)
    }

    const connecting: ConnectingAttempt = {
      ...attempt,
      promise: undefined as unknown as Promise<ActiveConnection>,
    }
    this.connecting = connecting
    connecting.promise = new Promise((resolve, reject) => {
      const request = withRequestSignal(options)
      let socket: WebSocketLike
      try {
        socket = this.createWebSocket(attempt.endpoint)
      } catch (error) {
        request.cleanup()
        if (this.connecting === connecting) this.connecting = undefined
        reject(error)
        return
      }
      let settled = false
      const cleanup = () => request.cleanup()
      const fail = (error: Error) => {
        if (settled) return
        settled = true
        cleanup()
        if (this.generation === attempt.generation) {
          this.socket = undefined
          if (this.connecting === connecting) this.connecting = undefined
        }
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
        if (this.generation !== attempt.generation || this.endpoint !== attempt.endpoint) {
          if (this.connecting === connecting) this.connecting = undefined
          try {
            socket.close()
          } catch {
            // Ignore close errors while discarding a stale connection.
          }
          reject(new ChainError(`stale connection opened for ${attempt.endpoint}`))
          return
        }
        const connection = { ...attempt, socket }
        this.socket = connection
        if (this.connecting === connecting) this.connecting = undefined
        resolve(connection)
      })
      socket.addEventListener('error', () => fail(new ChainError(`could not connect to ${attempt.endpoint}`)))
      socket.addEventListener('close', () => {
        const error = new ChainError(`connection closed: ${attempt.endpoint}`)
        if (!settled) fail(error)
        else this.handleSocketClose({ ...attempt, socket }, error)
      })
      socket.addEventListener('message', (event) => this.handleMessage({ ...attempt, socket }, event.data))
    })
    return connecting.promise
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

  private handleMessage(connection: ActiveConnection, data: unknown): void {
    if (!this.isCurrentConnection(connection)) return
    const raw = typeof data === 'string'
      ? data
      : Buffer.isBuffer(data) || data instanceof Uint8Array
        ? Buffer.from(data).toString('utf8')
        : String(data)
    if (Buffer.byteLength(raw, 'utf8') > this.maxMessageBytes) {
      this.handleProtocolError(connection, new ChainError('JSON-RPC message exceeded size limit'))
      return
    }
    let message: unknown
    try {
      message = JSON.parse(raw)
    } catch {
      this.handleProtocolError(connection, new ChainError('invalid JSON-RPC message'))
      return
    }
    if (typeof message !== 'object' || message == null || Array.isArray(message)) {
      this.handleProtocolError(connection, new JsonRpcError('invalid JSON-RPC message envelope'))
      return
    }
    const rpcMessage = message as { id?: unknown }
    if (hasOwn(rpcMessage, 'id')) {
      if (typeof rpcMessage.id !== 'number') {
        this.handleProtocolError(connection, new JsonRpcError('invalid JSON-RPC response id'))
        return
      }
      const pending = this.pending.get(rpcMessage.id)
      if (pending == null || pending.generation !== connection.generation) return
      this.pending.delete(rpcMessage.id)
      try {
        pending.resolve(jsonRpcResponseResult(message, rpcMessage.id))
      } catch (error) {
        pending.reject(error instanceof Error ? error : new JsonRpcError(String(error)))
      }
      return
    }
    let notification: { subscription: string; result: unknown } | undefined
    try {
      notification = jsonRpcSubscriptionNotification(message)
    } catch (error) {
      this.handleProtocolError(
        connection,
        error instanceof Error ? error : new JsonRpcError(String(error)),
      )
      return
    }
    if (notification == null) return
    const entry = this.subscriptionsById.get(notification.subscription)
    if (entry == null || entry.generation !== connection.generation || entry.state.closed) return
    const state = entry.state
    const waiter = state.waiters.shift()
    if (waiter != null) waiter.resolve({ done: false, value: notification.result })
    else if (state.queue.length >= this.maxSubscriptionQueue) {
      this.closeSubscription(state, new ChainError('subscription notification queue exceeded limit'))
    } else {
      state.queue.push(notification.result)
    }
  }

  private subscriptionNext(state: SubscriptionState): Promise<IteratorResult<unknown>> {
    if (state.queue.length > 0) return Promise.resolve({ done: false, value: state.queue.shift() })
    if (state.closed) {
      return state.error == null
        ? Promise.resolve({ done: true, value: undefined })
        : Promise.reject(state.error)
    }
    return new Promise((resolve, reject) => {
      state.waiters.push({ resolve, reject })
    })
  }

  private failPending(error: Error, generation?: number): void {
    if (generation == null || this.socket?.generation === generation) this.socket = undefined
    if (generation == null || this.connecting?.generation === generation) this.connecting = undefined
    for (const [id, pending] of this.pending) {
      if (generation != null && pending.generation !== generation) continue
      this.pending.delete(id)
      pending.cleanup()
      pending.reject(error)
    }
  }

  private handleSocketClose(connection: ActiveConnection, error: Error): void {
    if (!this.isCurrentConnection(connection)) return
    this.validatedEndpoints.delete(connection.endpoint)
    this.failPending(error, connection.generation)
    try {
      connection.socket.close()
    } catch {
      // Ignore close errors while tearing down a failed connection.
    }
    this.transitionDisconnectedSubscriptions(connection.generation, error)
  }

  private handleProtocolError(connection: ActiveConnection, error: Error): void {
    if (!this.isCurrentConnection(connection)) return
    this.validatedEndpoints.delete(connection.endpoint)
    this.failPending(error, connection.generation)
    try {
      connection.socket.close()
    } catch {
      // Ignore close errors while rejecting malformed protocol state.
    }
    for (const [subscription, entry] of this.subscriptionsById) {
      if (entry.generation === connection.generation) this.subscriptionsById.delete(subscription)
    }
    for (const state of [...this.subscriptions]) {
      if (state.subscriptionGeneration === connection.generation) this.closeSubscription(state, error)
    }
  }

  private transitionDisconnectedSubscriptions(generation: number, error: Error): void {
    for (const [subscription, entry] of this.subscriptionsById) {
      if (entry.generation === generation) this.subscriptionsById.delete(subscription)
    }
    if (this.closed) return
    for (const state of this.subscriptions) {
      if (state.subscriptionGeneration !== generation) continue
      state.subscription = undefined
      state.subscriptionGeneration = undefined
      if (state.resubscribe) void this.resubscribe(state)
      else this.closeSubscription(state, error)
    }
  }

  private async activateSubscription(state: SubscriptionState): Promise<void> {
    const subscription = String(
      await this.requestWebSocketOnly(state.subscribeMethod, state.params, state.requestOptions),
    )
    if (state.closed) {
      await this.requestWebSocketOnly(state.unsubscribeMethod, [subscription]).catch(() => undefined)
      return
    }
    state.subscription = subscription
    state.subscriptionGeneration = this.generation
    this.subscriptionsById.set(subscription, { state, generation: this.generation })
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
    state.error = error
    this.subscriptions.delete(state)
    if (state.subscription != null) this.subscriptionsById.delete(state.subscription)
    state.subscription = undefined
    state.subscriptionGeneration = undefined
    for (const waiter of state.waiters.splice(0)) {
      if (error == null) waiter.resolve({ done: true, value: undefined })
      else waiter.reject(error)
    }
  }

  private rotateEndpoint(attempt: EndpointAttempt = this.currentAttempt(), nextIndex?: number): boolean {
    if (
      attempt.generation !== this.generation ||
      attempt.index !== this.endpointIndex ||
      attempt.endpoint !== this.endpoint
    ) return true
    this.validatedEndpoints.delete(attempt.endpoint)
    const socket = this.socket
    const error = new ChainError(`endpoint rotated from ${attempt.endpoint}`)
    this.failPending(error, attempt.generation)
    this.generation += 1
    if (nextIndex != null) this.endpointIndex = nextIndex
    else if (this.endpoints.length > 1) this.endpointIndex = (this.endpointIndex + 1) % this.endpoints.length
    this.socket = undefined
    this.connecting = undefined
    this.transitionDisconnectedSubscriptions(attempt.generation, error)
    try {
      socket?.socket.close()
    } catch {
      // Ignore close errors while rotating to another endpoint.
    }
    return this.endpointIndex !== attempt.index || this.endpoints.length > 1
  }

  private isCurrentConnection(connection: ActiveConnection): boolean {
    return this.socket?.socket === connection.socket &&
      this.socket.generation === connection.generation &&
      this.socket.endpoint === connection.endpoint
  }
}

export class BrowserChainClient {
  readonly network: string
  readonly endpoint: string
  readonly transport: JsonRpcTransport
  readonly balances: BalancesNamespace
  readonly subnets: SubnetsNamespace
  readonly neurons: NeuronsNamespace
  readonly staking: StakingNamespace
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
    this.validateDescriptorsOnLoad = options.validateDescriptorSchema ?? expectedGenesisHash != null
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
    this.balances = new BalancesNamespace(this)
    this.subnets = new SubnetsNamespace(this)
    this.neurons = new NeuronsNamespace(this)
    this.staking = new StakingNamespace(this)
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
    const supportsMetadataHash = runtimeSupportsMetadataHash(runtime)
    const defaultMetadataHash = supportsMetadataHash || resolved.requiresMetadataProof
    if (metadataHashOptionProvided && options.metadataHash == null && defaultMetadataHash) {
      throw new ChainError(
        supportsMetadataHash
          ? 'metadataHash cannot be disabled when the runtime declares CheckMetadataHash'
          : 'metadataHash cannot be disabled for a signer that requires metadata proof',
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
    options: Pick<SubmitOptions, 'waitForFinalization' | 'timeoutMs' | 'signal'> & { signerAddress?: string } = {},
  ): Promise<ExtrinsicWatcher> {
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

  async estimateFee(
    call: CallLike,
    signer: SignerLike,
    options: Pick<SubmitOptions, 'allowRawCall' | 'policy' | 'rawSignerRole'> = {},
  ): Promise<Balance> {
    const snapshot = await this.signingSnapshot()
    const { runtime } = snapshot
    const account = await this.resolveSigner(signer, runtime)
    const signed = await this.signExtrinsicWithSnapshot(call, signer, {
      ...options,
      nonce: await this.peekNextIndex(account.ss58Address),
      period: null,
    }, snapshot)
    const queryInfo = runtime.runtimeApis().TransactionPaymentApi?.query_info
    if (queryInfo == null) {
      throw new ChainError('TransactionPaymentApi.query_info metadata is unavailable')
    }
    const encodedInput = runtime.encodeRuntimeApiInput('TransactionPaymentApi', 'query_info', [signed.bytes, signed.bytes.length])
    const raw = await this.rpc('state_call', [
      'TransactionPaymentApi_query_info',
      hex(encodedInput),
      snapshot.blockHash,
    ])
    const info = runtime.decodeTypeId<Record<string, ScaleValue>>(
      queryInfo.outputTypeId,
      hexToBuffer(String(raw)),
      false,
    )
    return Balance.fromRao(String(info.partial_fee ?? info.partialFee ?? 0))
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

  getSubnetHyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    return this.subnets.hyperparameters<T>(netuid, block)
  }

  get_subnet_hyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
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
    const blockHash = await this.finalizedHead()
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
    const finalized = snapshot?.blockHash ?? await this.finalizedHead()
    const current = snapshot?.blockNumber ?? await this.blockNumber(finalized)
    const birth = Number(eraBirth(period, current))
    const eraBlockHash = birth === current && snapshot != null
      ? snapshot.blockHash
      : await this.blockHash(birth)
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
    const failureMessage = failed == null
      ? undefined
      : dispatchFailureMessage(await this.runtimeAt(blockHash), failed)
    if (!success && failed == null) {
      return {
        status: 'unknown',
        success: undefined,
        message: 'Included, but no dispatch outcome event was found',
        extrinsicHash,
        blockHash,
        blockNumber,
        extrinsicIndex,
        extrinsicId: `${blockNumber}-${String(extrinsicIndex).padStart(4, '0')}`,
        finalized,
        fee: feeEvent == null ? undefined : feeFromEvent(feeEvent),
        events: triggered,
      }
    }
    const dispatchSuccess = success && failed == null
    return {
      status: dispatchSuccess ? finalized ? 'finalized' : 'inBlock' : 'failed',
      success: dispatchSuccess,
      message: failed == null ? 'Success' : failureMessage ?? 'Extrinsic failed',
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

export class Client extends BrowserChainClient {
  private readonly nodeExpectedGenesisHash?: string
  private nodeNativeClient?: NativeChainClient
  private nodeNativeClientPromise?: Promise<NativeChainClient>

  constructor(network: string = 'finney', options: ClientOptions = {}) {
    assertNodeClientOptions(options)
    super(network, { ...options, autoConnect: false })
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
    // NativeChainClient owns its connection and has no explicit close hook.
  }

  private async requireNativeClient(): Promise<NativeChainClient> {
    if (this.nodeNativeClient != null) return this.nodeNativeClient
    this.nodeNativeClientPromise ??= NativeChainClient.connect(this.endpoint)
      .then((client) => {
        const genesis = hex(client.genesisHash)
        if (this.nodeExpectedGenesisHash != null && !sameHex(genesis, this.nodeExpectedGenesisHash)) {
          throw new EndpointValidationError(
            `endpoint ${this.endpoint} genesis ${genesis} does not match expected genesis ${this.nodeExpectedGenesisHash}`,
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
      throw new ChainError(
        'historical runtime snapshots are only available on BrowserChainClient; Node Client uses the Rust NativeChainClient runtime cache',
      )
    }
    return (await this.requireNativeClient()).runtime()
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
    return await (await this.requireNativeClient()).query(moduleName, itemName, itemParams, blockHash) as T
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
    return await (await this.requireNativeClient()).queryBatch(moduleName, itemName, sets, blockHash) as Array<T | undefined>
  }

  override invalidateRuntimeCache(): void {
    void this.nodeNativeClient?.refreshRuntime().catch(() => undefined)
  }

  override async chainInfo(_runtime?: Runtime, _blockHash?: string | null): Promise<ChainInfo> {
    return nativeChainInfoToChainInfo(await (await this.requireNativeClient()).chainInfo())
  }

  override rpc(method: string, _params: unknown[] = []): Promise<unknown> {
    return Promise.reject(new ChainError(
      `raw JSON-RPC method ${method} is not available on the Node Client; use BrowserChainClient for custom transport RPC`,
    ))
  }

  override async runtimeCall<T extends ScaleValue = ScaleValue>(
    api: string | Descriptor,
    method?: string | ScaleValue[],
    paramsOrBlock: ScaleValue[] | number | string | null = [],
    block?: number | string | null,
  ): Promise<T> {
    const [apiName, methodName, callParams, blockRef] = normalizeRuntimeArgs(api, method, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    return await (await this.requireNativeClient()).runtimeCall(apiName, methodName, callParams, blockHash) as T
  }

  override async queryMap<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    storageFunction?: string | ScaleValue[],
    paramsOrBlock?: ScaleValue[] | number | string | null,
    block?: number | string | null,
    pageSizeOrOptions: number | QueryMapOptions = 512,
  ): Promise<Array<[K, V]>> {
    if (pageSizeOrOptions !== 512) {
      throw new ChainError(
        'queryMap pagination controls are only available on BrowserChainClient; Node Client delegates map pagination to Rust',
      )
    }
    const [moduleName, itemName, itemParams, blockRef] =
      normalizeStorageArgs(pallet, storageFunction, paramsOrBlock, block)
    const blockHash = await this.resolveReadBlockHash(blockRef)
    return await (await this.requireNativeClient()).queryMap(moduleName, itemName, itemParams, blockHash) as Array<[K, V]>
  }

  override async stateCall<T = string>(_method: string, _data: ByteLike | string, _block?: number | string | null): Promise<T> {
    throw new ChainError('raw state_call is only available on BrowserChainClient; use runtimeCall() for Rust-backed runtime APIs')
  }

  override async constant<T extends ScaleValue = ScaleValue>(
    pallet: string | Descriptor,
    name?: string,
    block?: number | string | null,
  ): Promise<T | undefined> {
    if (block != null) {
      throw new ChainError('historical constants are only available on BrowserChainClient')
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
      throw new ChainError('historical SCALE decoding is only available on BrowserChainClient')
    }
    return (await this.requireNativeClient()).decodeScale(
      typeString,
      typeof data === 'string' ? hexToBuffer(data) : toBuffer(data, 'data'),
    ) as T
  }

  override async composeCall(pallet: string, fn: string, params: ScaleValue = {}, block?: number | string | null): Promise<Buffer> {
    if (block != null) {
      throw new ChainError('historical call composition is only available on BrowserChainClient')
    }
    return (await this.requireNativeClient()).composeCall(pallet, fn, params)
  }

  override async signExtrinsic(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<SignedExtrinsicResult> {
    const native = await this.requireNativeClient()
    const planned = await this.nativeSigningPlan(call, signer, options, native)
    const signature = await this.signRustPlan(planned)
    const signed = await native.assembleExternal(planned.plan, signature.signature, signature.cryptoType)
    return {
      bytes: Buffer.from(signed.bytes),
      hash: hexToBuffer(signed.hash),
      hex: hex(signed.bytes),
      signerAddress: planned.resolved.ss58Address,
      nonce: parseNonce(planned.plan.nonce, 'native signing plan nonce'),
    }
  }

  override async submit(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<ExtrinsicResult> {
    assertNativeSubmitOptions(options)
    const native = await this.requireNativeClient()
    const planned = await this.nativeSigningPlan(call, signer, options, native)
    const signature = await this.signRustPlan(planned)
    return nativeOutcomeToExtrinsicResult(
      await native.submitExternal(
        planned.plan,
        signature.signature,
        options.waitForFinalization === true,
        signature.cryptoType,
      ),
      options.waitForFinalization === true,
    )
  }

  override async *blocks(_options: { finalized?: boolean } = {}): AsyncIterable<BlockHeader> {
    throw new ChainError('block subscriptions are only available on BrowserChainClient')
  }

  override waitForBlock(_block?: number | null, _options: { timeoutMs?: number } = {}): Promise<BlockHeader> {
    return Promise.reject(new ChainError('block subscriptions are only available on BrowserChainClient'))
  }

  override blockInfo(_block?: number | null): Promise<BlockInfo | null> {
    return Promise.reject(new ChainError('block inspection is only available on BrowserChainClient'))
  }

  override async submitSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    signerAddress?: string,
    options: SubmitOptions = {},
  ): Promise<ExtrinsicResult> {
    void signerAddress
    assertNativeSubmitOptions(options)
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    return nativeOutcomeToExtrinsicResult(
      await (await this.requireNativeClient()).submitEncoded(
        bytes,
        extrinsicHash,
        options.waitForFinalization === true,
      ),
      options.waitForFinalization === true,
    )
  }

  override watchSigned(
    extrinsic: SignedExtrinsicResult | SignedExtrinsic | ByteLike,
    options: Pick<SubmitOptions, 'waitForFinalization' | 'timeoutMs' | 'signal'> & { signerAddress?: string } = {},
  ): Promise<ExtrinsicWatcher> {
    assertNativeWatchOptions(options)
    const bytes = Buffer.isBuffer(extrinsic) || extrinsic instanceof Uint8Array
      ? toBuffer(extrinsic, 'extrinsic')
      : toBuffer(extrinsic.bytes, 'extrinsic.bytes')
    const extrinsicHash = hex(blake2_256(bytes))
    const result = this.requireNativeClient()
      .then((native) => native.submitEncoded(
        bytes,
        extrinsicHash,
        options.waitForFinalization === true,
      ))
      .then((outcome) => nativeOutcomeToExtrinsicResult(
        outcome,
        options.waitForFinalization === true,
      ))
    return Promise.resolve({
      extrinsicHash,
      result,
      unsubscribe: async () => {
        // Native receipt tracking is a bounded blocking task rather than a
        // JSON-RPC subscription, so there is no remote subscription to cancel.
      },
    })
  }

  override async estimateFee(
    call: CallLike,
    signer: SignerLike,
    options: Pick<SubmitOptions, 'allowRawCall' | 'policy' | 'rawSignerRole'> = {},
  ): Promise<Balance> {
    const native = await this.requireNativeClient()
    const planned = await this.nativeSigningPlan(call, signer, options as SubmitOptions, native)
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

  override async validateDescriptorSchema(block?: number | string | null): Promise<DescriptorSchemaIssue[]> {
    if (block != null) {
      throw new ChainError('historical descriptor validation is only available on BrowserChainClient')
    }
    return validateDescriptorSchema((await this.requireNativeClient()).runtime())
  }

  private async nativeSigningPlan(
    call: CallLike,
    signer: SignerLike,
    options: SubmitOptions,
    native: NativeChainClient,
  ): Promise<NativeSigningPlan> {
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
    return { native, runtime, resolved, plan }
  }

  private async signRustPlan(planned: NativeSigningPlan): Promise<NormalizedSignature> {
    const context = nativePlanSignerContext(this, planned.runtime, planned.resolved, planned.plan)
    return this.signWithSigner(planned.resolved.signer, context.payload, context)
  }
}

export class Snapshot {
  readonly balances: SnapshotBalancesNamespace
  readonly subnets: SnapshotSubnetsNamespace
  readonly staking: SnapshotStakingNamespace
  readonly neurons: SnapshotNeuronsNamespace

  constructor(readonly client: ChainClient, readonly block: number, readonly blockHash: string) {
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
}

export class SubnetsNamespace {
  constructor(private readonly client: ChainClient) {}

  async subnet(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    const blockHash = await this.client.resolveReadBlockHash(block)
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const subnet = (await native.subnets(blockHash)).find((item) => item.netuid === netuid)
      if (subnet != null) {
        return {
          netuid: subnet.netuid,
          tempo: subnet.tempo,
          burn: Balance.fromRao(subnet.burnRao),
          neuronCount: subnet.neuronCount,
        }
      }
    }
    const [tempo, burn, count] = await Promise.all([
      this.client.query(storage.SubtensorModule.Tempo, [netuid], blockHash),
      this.client.query(storage.SubtensorModule.Burn, [netuid], blockHash),
      this.client.query(storage.SubtensorModule.SubnetworkN, [netuid], blockHash),
    ])
    return { netuid, tempo: Number(tempo ?? 0), burn: Balance.fromRao(String(burn ?? 0)), neuronCount: Number(count ?? 0) }
  }

  info(netuid: number, block?: number | string | null): Promise<SubnetInfo> {
    return this.subnet(netuid, block)
  }

  async all(block?: number | string | null): Promise<SubnetInfo[]> {
    const blockHash = await this.client.resolveReadBlockHash(block)
    const native = await nativeRustClient(this.client)
    if (native != null) {
      return (await native.subnets(blockHash)).map((subnet) => ({
        netuid: subnet.netuid,
        tempo: subnet.tempo,
        burn: Balance.fromRao(subnet.burnRao),
        neuronCount: subnet.neuronCount,
      }))
    }
    const [added, tempos, burns, counts] = await Promise.all([
      this.client.queryMap<number, boolean>(storage.SubtensorModule.NetworksAdded, [], blockHash),
      this.client.queryMap<number, number>(storage.SubtensorModule.Tempo, [], blockHash),
      this.client.queryMap<number, bigint>(storage.SubtensorModule.Burn, [], blockHash),
      this.client.queryMap<number, number>(storage.SubtensorModule.SubnetworkN, [], blockHash),
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

  async metagraph<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return await native.metagraph(netuid, blockHash) as T
    }
    return this.client.runtime<T>(runtimeApi.SubnetInfoRuntimeApi.get_metagraph, [netuid], block)
  }

  async hyperparameters<T extends ScaleValue = ScaleValue>(netuid: number, block?: number | string | null): Promise<T> {
    const native = await nativeRustClient(this.client)
    if (native != null) {
      const blockHash = await this.client.resolveReadBlockHash(block)
      return await native.subnetHyperparameters(netuid, blockHash) as T
    }
    return this.client.runtime<T>(runtimeApi.SubnetInfoRuntimeApi.get_subnet_hyperparams_v3, [netuid], block)
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
    get_subnet_hyperparams_v3: descriptor('SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3'),
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
    transfer_keep_alive(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_keep_alive', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
    transfer_allow_death(dest: string, value: TransactionAmount) {
      return call('Balances', 'transfer_allow_death', { dest, value: taoTransactionAmountRao(value, 'transfer amount') })
    },
  }),
  SubtensorModule: Object.freeze({
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
  argTypes: readonly string[]
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
          if (actualArgTypes[index] !== entry.argTypes[index]) {
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

async function read(client: ChainClient, name: string, params: Record<string, ScaleValue>, block?: number | string | null): Promise<unknown> {
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

function normalizeQueryMapOptions(value: number | QueryMapOptions): Required<Pick<QueryMapOptions, 'pageSize'>> & Omit<QueryMapOptions, 'pageSize'> {
  if (typeof value === 'number') return { pageSize: positiveInteger(value, 'queryMap pageSize') }
  return {
    ...value,
    pageSize: positiveInteger(value.pageSize ?? 512, 'queryMap pageSize'),
    maxPages: value.maxPages == null ? undefined : positiveInteger(value.maxPages, 'queryMap maxPages'),
    maxResults: value.maxResults == null ? undefined : positiveInteger(value.maxResults, 'queryMap maxResults'),
  }
}

function assertNodeClientOptions(options: ClientOptions): void {
  const unsupported = unsupportedNodeClientOptions(options)
  if (unsupported.length === 0) return
  throw new ChainError(
    `Node Client is backed exclusively by Rust NativeChainClient; use BrowserChainClient for TypeScript transport options: ${unsupported.join(', ')}`,
  )
}

function unsupportedNodeClientOptions(options: ClientOptions): string[] {
  const unsupported: string[] = []
  if ((options.fallbackEndpoints?.length ?? 0) > 0) unsupported.push('fallbackEndpoints')
  for (const key of [
    'retryForever',
    'webSocket',
    'webSocketConstructor',
    'webSocketFactory',
    'headRuntimeTtlMs',
    'historicalRuntimeCacheSize',
    'requestTimeoutMs',
    'maxRequestRetries',
    'retryBackoffMs',
    'maxRetryBackoffMs',
    'endpointValidationTtlMs',
    'maxSubscriptionQueue',
    'maxMessageBytes',
    'validateDescriptorSchema',
  ] satisfies Array<keyof ClientOptions>) {
    if (hasOwn(options, key)) unsupported.push(key)
  }
  return unsupported
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

function runtimeSupportsMetadataHash(runtime: Runtime): boolean {
  const identifiers = runtime.signedExtensionIdentifiers?.() ?? []
  return identifiers.includes('CheckMetadataHash')
}

function isMetadataAtVersionUnavailable(error: unknown): boolean {
  const message = String(error instanceof Error ? error.message : error)
  const data = String(error instanceof JsonRpcError && error.data != null ? error.data : '')
  const text = `${message} ${data}`
  return text.includes(V15_METADATA_MISSING_NEEDLE) ||
    (text.includes('Metadata_metadata_at_version') && /not found|unknown function/i.test(text))
}

function hasOwn(value: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key)
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function signerPublicKey(value: ByteLike | undefined, ss58Address: string): Buffer {
  return value == null
    ? publicKeyFromSs58(ss58Address)
    : toBuffer(value, 'signer.publicKey')
}

async function parseJsonRpcResponse(response: Response, maxBytes: number): Promise<unknown> {
  const responseLike = response as Response & { json?: () => Promise<unknown> }
  if (response.body != null || typeof response.text === 'function') {
    const raw = await readResponseText(response, maxBytes)
    try {
      return JSON.parse(raw) as unknown
    } catch {
      throw new JsonRpcError('invalid JSON-RPC response')
    }
  }
  if (typeof responseLike.json === 'function') return responseLike.json()
  throw new JsonRpcError('HTTP JSON-RPC response body is not readable')
}

async function readResponseText(response: Response, maxBytes: number): Promise<string> {
  const body = response.body
  if (body == null || typeof body.getReader !== 'function') {
    const text = await response.text()
    if (Buffer.byteLength(text, 'utf8') > maxBytes) {
      throw new JsonRpcError('JSON-RPC response exceeded size limit')
    }
    return text
  }

  const reader = body.getReader()
  const chunks: Buffer[] = []
  let total = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      if (value == null) continue
      const chunk = Buffer.from(value)
      total += chunk.byteLength
      if (total > maxBytes) {
        await reader.cancel().catch(() => undefined)
        throw new JsonRpcError('JSON-RPC response exceeded size limit')
      }
      chunks.push(chunk)
    }
  } finally {
    reader.releaseLock()
  }
  return Buffer.concat(chunks, total).toString('utf8')
}

function jsonRpcResponseResult(payload: unknown, expectedId: number): unknown {
  if (payload == null || typeof payload !== 'object' || Array.isArray(payload)) {
    throw new JsonRpcError('invalid JSON-RPC response envelope')
  }
  const envelope = payload as {
    jsonrpc?: unknown
    id?: unknown
    result?: unknown
    error?: unknown
  }
  if (envelope.jsonrpc !== '2.0') {
    throw new JsonRpcError('invalid JSON-RPC response version')
  }
  if (envelope.id !== expectedId) {
    throw new JsonRpcError('JSON-RPC response id did not match request id')
  }
  const hasResult = hasOwn(envelope, 'result')
  const hasError = hasOwn(envelope, 'error')
  if (hasResult === hasError) {
    throw new JsonRpcError('JSON-RPC response must contain exactly one of result or error')
  }
  if (hasError) {
    const error = envelope.error
    if (error == null || typeof error !== 'object') {
      throw new JsonRpcError('invalid JSON-RPC error response')
    }
    const rpcError = error as { message?: unknown; code?: unknown; data?: unknown }
    if (typeof rpcError.message !== 'string' || typeof rpcError.code !== 'number') {
      throw new JsonRpcError('invalid JSON-RPC error response')
    }
    throw new JsonRpcError(
      rpcError.message,
      rpcError.code,
      rpcError.data,
    )
  }
  return envelope.result
}

function jsonRpcSubscriptionNotification(
  payload: unknown,
): { subscription: string; result: unknown } | undefined {
  if (payload == null || typeof payload !== 'object' || Array.isArray(payload)) {
    throw new JsonRpcError('invalid JSON-RPC notification envelope')
  }
  const envelope = payload as {
    jsonrpc?: unknown
    id?: unknown
    method?: unknown
    result?: unknown
    error?: unknown
    params?: unknown
  }
  if (envelope.jsonrpc !== '2.0') {
    throw new JsonRpcError('invalid JSON-RPC notification version')
  }
  if (hasOwn(envelope, 'id')) {
    throw new JsonRpcError('JSON-RPC subscription notification must not contain id')
  }
  if (hasOwn(envelope, 'result') || hasOwn(envelope, 'error')) {
    throw new JsonRpcError('JSON-RPC subscription notification result belongs in params.result')
  }
  if (typeof envelope.method !== 'string') return undefined
  const params = envelope.params
  if (params == null || typeof params !== 'object' || Array.isArray(params)) return undefined
  const paramsObject = params as { subscription?: unknown; result?: unknown }
  if (!hasOwn(paramsObject, 'subscription')) return undefined
  const subscription = paramsObject.subscription
  if (subscription == null || (typeof subscription !== 'string' && typeof subscription !== 'number')) {
    throw new JsonRpcError('invalid JSON-RPC subscription id')
  }
  if (!hasOwn(paramsObject, 'result')) {
    throw new JsonRpcError('JSON-RPC subscription notification is missing result')
  }
  return { subscription: String(subscription), result: paramsObject.result }
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

function assertNativeSubmitOptions(options: SubmitOptions): void {
  if (options.signal != null || options.timeoutMs != null) {
    throw new ChainError(
      'native Rust transaction execution does not support signal or timeoutMs; use submitSigned()/watchSigned() for custom transport cancellation',
    )
  }
}

function assertNativeWatchOptions(options: Pick<SubmitOptions, 'signal' | 'timeoutMs'>): void {
  if (options.signal != null || options.timeoutMs != null) {
    throw new ChainError(
      'native Rust submit-and-watch does not support signal or timeoutMs; use BrowserChainClient for custom transport cancellation',
    )
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
