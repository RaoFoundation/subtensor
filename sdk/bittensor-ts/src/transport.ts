export const DEFAULT_REQUEST_TIMEOUT_MS = 30_000

export const DEFAULT_MAX_REQUEST_RETRIES = 2

export const DEFAULT_RETRY_BACKOFF_MS = 250

export const DEFAULT_MAX_RETRY_BACKOFF_MS = 5_000

export const DEFAULT_ENDPOINT_VALIDATION_TTL_MS = 60_000

export const DEFAULT_MAX_SUBSCRIPTION_QUEUE = 1024

export const DEFAULT_MAX_WS_MESSAGE_BYTES = 16 * 1024 * 1024

export const IDEMPOTENT_RPC_METHODS = new Set([
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

export interface RpcRequest {
  generation: number
  resolve(value: unknown): void
  reject(error: Error): void
  cleanup(): void
}

export interface SubscriptionWaiter {
  resolve(value: IteratorResult<unknown>): void
  reject(error: Error): void
}

export interface SubscriptionState {
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

export interface EndpointAttempt {
  endpoint: string
  index: number
  generation: number
}

export interface ActiveConnection extends EndpointAttempt {
  socket: WebSocketLike
}

export interface ConnectingAttempt extends EndpointAttempt {
  promise: Promise<ActiveConnection>
}

export interface SubscriptionOptions extends RpcRequestOptions {
  resubscribe?: boolean
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
  readonly chainError: ChainErrorInfo

  constructor(message: string, code?: number, data?: unknown) {
    super(message)
    this.name = 'JsonRpcError'
    this.code = code
    this.data = data
    this.chainError = classifyChainError(message, { payload: { data } })
  }
}

export class ChainError extends Error {
  readonly details?: unknown
  readonly code: ErrorCode
  readonly chainError: ChainErrorInfo

  constructor(message: string, details?: unknown) {
    super(message)
    this.name = 'ChainError'
    this.details = details
    this.chainError = classifyChainError(message, { payload: details })
    this.code = this.chainError.code
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
      : data instanceof Uint8Array
        ? new TextDecoder().decode(data)
        : String(data)
    if (new TextEncoder().encode(raw).length > this.maxMessageBytes) {
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

export function nonNegativeNumber(value: unknown, fallback: number): number {
  if (value == null) return fallback
  const parsed = Number(value)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback
}

export function nonNegativeInteger(value: unknown, fallback: number): number {
  return Math.floor(nonNegativeNumber(value, fallback))
}

export function hasOwn(value: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key)
}

export async function parseJsonRpcResponse(response: Response, maxBytes: number): Promise<unknown> {
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

export async function readResponseText(response: Response, maxBytes: number): Promise<string> {
  const body = response.body
  if (body == null || typeof body.getReader !== 'function') {
    const text = await response.text()
    if (new TextEncoder().encode(text).length > maxBytes) {
      throw new JsonRpcError('JSON-RPC response exceeded size limit')
    }
    return text
  }

  const reader = body.getReader()
  const chunks: Uint8Array[] = []
  let total = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      if (value == null) continue
      const chunk = new Uint8Array(value)
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
  const joined = new Uint8Array(total)
  let offset = 0
  for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.length }
  return new TextDecoder().decode(joined)
}

export function jsonRpcResponseResult(payload: unknown, expectedId: number): unknown {
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

export function jsonRpcSubscriptionNotification(
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

export interface RequestSignal {
  signal?: AbortSignal
  cleanup(): void
  onAbort(handler: (error: Error) => void): void
  error(): Error
}

export function delay(ms: number, signal?: AbortSignal): Promise<void> {
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

export function withRequestSignal(options: RpcRequestOptions): RequestSignal {
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

export function normalizeAbortError(error: unknown, request: RequestSignal): Error {
  if (request.signal?.aborted) return request.error()
  return error instanceof Error ? error : new Error(String(error))
}

export function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw new RequestAbortedError()
}
import { classifyChainError, type ChainErrorInfo, type ErrorCode } from './chain-errors'
