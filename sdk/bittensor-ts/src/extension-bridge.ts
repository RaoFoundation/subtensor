/** Node loopback bridge for signing with the same browser extension protocol as Python. */
import { createServer, type Server } from 'node:http'
import { randomBytes, randomUUID, timingSafeEqual } from 'node:crypto'
import { WebSocket, WebSocketServer, type RawData } from 'ws'
import { EXTENSION_BRIDGE_HTML } from './extension-page'
import { ExtensionSigner, type ExtensionAccount } from './external-signers'

export const DEFAULT_BRIDGE_HOST = '127.0.0.1'
export const DEFAULT_BRIDGE_PORT = 39295
const METHODS = new Set(['accounts.list', 'extrinsic.sign', 'bytes.sign', 'transaction.result'])
export class BridgeError extends Error { constructor(message: string) { super(message); this.name = 'BridgeError' } }
interface Pending { browser: WebSocket; resolve(value: unknown): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> }
export interface BridgeServerOptions { host?: '127.0.0.1' | '::1'; port?: number; requestTimeoutMs?: number; maxPending?: number }
function sameToken(value: unknown, expected: string): boolean {
  if (typeof value !== 'string') return false
  const a = Buffer.from(value), b = Buffer.from(expected)
  return a.length === b.length && timingSafeEqual(a, b)
}
function parseMessage(raw: RawData): Record<string, any> {
  const value = JSON.parse(raw.toString())
  if (value == null || typeof value !== 'object' || Array.isArray(value)) throw new BridgeError('invalid bridge message')
  return value
}
const send = (socket: WebSocket, value: unknown) => { if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(value)) }
export class BridgeServer {
  readonly host: '127.0.0.1' | '::1'
  readonly sessionId = randomBytes(32).toString('base64url')
  readonly clientToken = randomBytes(32).toString('base64url')
  private port: number
  private http?: Server
  private sockets?: WebSocketServer
  private browser?: WebSocket
  private readonly pending = new Map<string, Pending>()
  private readonly requestTimeoutMs: number
  private readonly maxPending: number
  constructor(options: BridgeServerOptions = {}) {
    this.host = options.host ?? DEFAULT_BRIDGE_HOST
    if (this.host !== '127.0.0.1' && this.host !== '::1') throw new TypeError('extension bridge must bind a loopback address')
    this.port = options.port ?? DEFAULT_BRIDGE_PORT
    if (!Number.isSafeInteger(this.port) || this.port < 0 || this.port > 65535) throw new RangeError('invalid bridge port')
    this.requestTimeoutMs = options.requestTimeoutMs ?? 300000
    this.maxPending = options.maxPending ?? 128
    if (!Number.isSafeInteger(this.requestTimeoutMs) || this.requestTimeoutMs < 1 || !Number.isSafeInteger(this.maxPending) || this.maxPending < 1) throw new RangeError('invalid bridge request limits')
  }
  get httpUrl(): string { return `http://${this.host === '::1' ? '[::1]' : this.host}:${this.port}/` }
  get wsUrl(): string { return this.httpUrl.replace(/^http:/, 'ws:') + 'ws' }
  get browserUrl(): string { return this.httpUrl + '?session=' + encodeURIComponent(this.sessionId) }
  get browserConnected(): boolean { return this.browser?.readyState === WebSocket.OPEN }
  async start(options: { openBrowser?: (url: string) => void | Promise<void> } = {}): Promise<void> {
    if (!this.http) {
      const http = createServer((request, response) => {
        if (request.headers.host !== new URL(this.httpUrl).host) { response.writeHead(403).end('Invalid host'); return }
        const path = new URL(request.url ?? '/', this.httpUrl).pathname
        if (request.method !== 'GET' || !['/', '/index.html'].includes(path)) { response.writeHead(404).end('Not Found'); return }
        response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer', 'X-Content-Type-Options': 'nosniff', 'X-Frame-Options': 'DENY' }).end(EXTENSION_BRIDGE_HTML)
      })
      const sockets = new WebSocketServer({ noServer: true, maxPayload: 1024 * 1024, perMessageDeflate: false })
      http.on('upgrade', (request, socket, head) => {
        if (request.headers.host !== new URL(this.httpUrl).host || new URL(request.url ?? '/', this.httpUrl).pathname !== '/ws') { socket.end('HTTP/1.1 403 Forbidden\r\n\r\n'); return }
        const origin = request.headers.origin
        if (origin != null && origin !== new URL(this.httpUrl).origin) { socket.end('HTTP/1.1 403 Forbidden\r\n\r\n'); return }
        sockets.handleUpgrade(request, socket, head, connection => this.accept(connection, origin))
      })
      try {
        await new Promise<void>((resolve, reject) => { http.once('error', reject); http.listen(this.port, this.host, () => { http.removeListener('error', reject); resolve() }) })
      } catch (error) { sockets.close(); throw error }
      const address = http.address()
      if (!address || typeof address === 'string') throw new BridgeError('bridge did not bind a TCP address')
      this.port = address.port; this.http = http; this.sockets = sockets
    }
    await options.openBrowser?.(this.browserUrl)
  }
  async stop(): Promise<void> {
    for (const [id, pending] of this.pending) { clearTimeout(pending.timer); pending.reject(new BridgeError('bridge stopped')); this.pending.delete(id) }
    this.browser = undefined
    if (this.sockets) { for (const socket of this.sockets.clients) socket.terminate(); await new Promise<void>(resolve => this.sockets!.close(() => resolve())); this.sockets = undefined }
    const http = this.http; this.http = undefined
    if (http) await new Promise<void>((resolve, reject) => http.close(error => error ? reject(error) : resolve()))
  }
  async waitUntilReady(timeoutMs = 30000): Promise<void> {
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) throw new RangeError('bridge readiness timeout must be positive')
    const deadline = Date.now() + timeoutMs
    while (!this.browserConnected) {
      if (!this.http) throw new BridgeError('bridge is not running')
      if (Date.now() >= deadline) throw new BridgeError('extension bridge page is not connected')
      await new Promise(resolve => setTimeout(resolve, Math.min(50, Math.max(1, deadline - Date.now()))))
    }
  }
  async request(method: string, params: Record<string, unknown> = {}): Promise<any> {
    if (!METHODS.has(method)) throw new BridgeError(`unsupported extension bridge method ${method}`)
    const browser = this.browser
    if (!browser || browser.readyState !== WebSocket.OPEN) throw new BridgeError('extension bridge page is not connected')
    if (this.pending.size >= this.maxPending) throw new BridgeError('extension bridge request limit reached')
    const id = randomUUID()
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new BridgeError('extension bridge request timed out')) }, this.requestTimeoutMs)
      this.pending.set(id, { browser, resolve, reject, timer })
      browser.send(JSON.stringify({ id, method, params }), error => {
        if (error) { clearTimeout(timer); this.pending.delete(id); reject(error) }
      })
    })
  }
  private accept(socket: WebSocket, origin?: string): void {
    let role: 'client' | 'bridge' | undefined
    const helloTimer = setTimeout(() => socket.close(1008, 'bridge authentication timed out'), 5000)
    socket.on('error', () => socket.close())
    socket.on('close', () => {
      clearTimeout(helloTimer)
      if (this.browser === socket) this.browser = undefined
      for (const [id, pending] of this.pending) if (pending.browser === socket) { clearTimeout(pending.timer); pending.reject(new BridgeError('bridge page disconnected')); this.pending.delete(id) }
    })
    socket.on('message', raw => {
      let message: Record<string, any>
      try { message = parseMessage(raw) } catch { socket.close(1008, 'invalid bridge JSON'); return }
      if (!role) {
        clearTimeout(helloTimer)
        if (message.role === 'bridge' && origin === new URL(this.httpUrl).origin && sameToken(message.session, this.sessionId)) {
          role = 'bridge'
          const previous = this.browser; this.browser = socket
          if (previous && previous !== socket) previous.close(1012, 'replaced by new bridge tab')
          return
        }
        if (message.role === 'client' && sameToken(message.token, this.clientToken)) { role = 'client'; send(socket, { role: 'ok' }); return }
        socket.close(1008, 'unauthorized bridge connection'); return
      }
      if (role === 'bridge') {
        const pending = typeof message.id === 'string' ? this.pending.get(message.id) : undefined
        if (!pending || pending.browser !== socket) return
        this.pending.delete(message.id); clearTimeout(pending.timer)
        if ('error' in message) pending.reject(new BridgeError(String(message.error?.message ?? message.error ?? 'extension request failed')))
        else pending.resolve(message.result)
      } else {
        if (typeof message.id !== 'string' || typeof message.method !== 'string') { socket.close(1008, 'invalid bridge request'); return }
        void this.clientRequest(message.method, message.params).then(result => send(socket, { id: message.id, result }), error => send(socket, { id: message.id, error: { message: error instanceof Error ? error.message : String(error) } }))
      }
    })
  }
  private async clientRequest(method: string, params?: Record<string, unknown>): Promise<unknown> {
    if (method !== 'bridge.status') return this.request(method, params)
    const connected = this.browserConnected
    let count = 0
    if (connected) { const result = await this.request('accounts.list'); count = Array.isArray(result?.accounts) ? result.accounts.length : 0 }
    return { browser_connected: connected, account_count: count, http_url: this.httpUrl, session_id: this.sessionId }
  }
}

export class BridgeClient {
  private socket?: WebSocket
  private connecting?: Promise<void>
  private readonly pending = new Map<string, { socket: WebSocket; resolve(value: any): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> }>()
  constructor(readonly url: string, private readonly token: string, readonly timeoutMs = 300000) {
    const endpoint = new URL(url)
    if (endpoint.protocol !== 'ws:' || !['127.0.0.1', '[::1]', 'localhost'].includes(endpoint.hostname)) throw new TypeError('extension bridge URL must use loopback ws')
    if (!token) throw new BridgeError('extension bridge client token is required')
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) throw new RangeError('bridge request timeout must be positive')
  }
  async connect(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN && !this.connecting) return
    this.connecting ??= this.open().finally(() => { this.connecting = undefined })
    return this.connecting
  }
  private async open(): Promise<void> {
    const socket = new WebSocket(this.url, { handshakeTimeout: 5000, maxPayload: 1024 * 1024, perMessageDeflate: false })
    this.socket = socket
    await new Promise<void>((resolve, reject) => {
      let authenticated = false
      const timer = setTimeout(() => { socket.terminate(); reject(new BridgeError('extension bridge authentication timed out')) }, 5000)
      socket.on('open', () => send(socket, { role: 'client', token: this.token }))
      socket.on('error', error => { clearTimeout(timer); reject(error) })
      socket.on('close', () => {
        clearTimeout(timer)
        if (this.socket === socket) this.socket = undefined
        const error = new BridgeError(authenticated ? 'extension bridge disconnected' : 'extension bridge rejected the client token')
        if (!authenticated) reject(error)
        for (const [id, pending] of this.pending) if (pending.socket === socket) { clearTimeout(pending.timer); pending.reject(error); this.pending.delete(id) }
      })
      socket.on('message', raw => {
        let message: Record<string, any>
        try { message = parseMessage(raw) } catch (error) { socket.close(1008, 'invalid bridge JSON'); reject(error); return }
        if (!authenticated) {
          if (message.role !== 'ok') { socket.close(1008, 'unauthorized'); reject(new BridgeError('extension bridge rejected the client token')); return }
          authenticated = true; clearTimeout(timer); resolve(); return
        }
        const pending = this.pending.get(message.id)
        if (!pending) return
        this.pending.delete(message.id); clearTimeout(pending.timer)
        if ('error' in message) pending.reject(new BridgeError(String(message.error?.message ?? message.error)))
        else pending.resolve(message.result)
      })
    })
  }
  async close(): Promise<void> { const socket = this.socket; this.socket = undefined; if (socket) { socket.terminate() } }
  async request(method: string, params: Record<string, unknown> = {}): Promise<any> {
    await this.connect()
    const id = randomUUID(), socket = this.socket!
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new BridgeError('extension bridge request timed out')) }, this.timeoutMs)
      this.pending.set(id, { socket, resolve, reject, timer })
      socket.send(JSON.stringify({ id, method, params }), error => { if (error) { clearTimeout(timer); this.pending.delete(id); reject(error) } })
    })
  }
  async listAccounts(): Promise<ExtensionAccount[]> {
    const response = await this.request('accounts.list')
    return (Array.isArray(response?.accounts) ? response.accounts : []).filter((entry: any) => typeof entry?.address === 'string').map((entry: any) => ({ address: entry.address, name: entry.meta?.name ?? entry.name ?? entry.address, source: entry.meta?.source ?? entry.source ?? 'unknown', type: entry.type ?? 'sr25519', genesisHash: entry.genesisHash }))
  }
  async signExtrinsicPayload(payload: Record<string, unknown>, options: { stage?: string; moreComing?: boolean } = {}): Promise<{ signature: string }> {
    const result = await this.request('extrinsic.sign', { payload, stage: options.stage, moreComing: options.moreComing ?? false })
    if (typeof result?.signature !== 'string') throw new BridgeError('extension did not return a signature')
    return result
  }
  async signBytes(address: string, data: string): Promise<{ signature: string }> {
    const result = await this.request('bytes.sign', { address, data, type: 'bytes' })
    if (typeof result?.signature !== 'string') throw new BridgeError('extension did not return a signature')
    return result
  }
  async reportTransactionResult(success: boolean): Promise<void> { await this.request('transaction.result', { success }) }
}
export class BridgeExtensionSigner extends ExtensionSigner {
  twoStage = false
  private signIndex = 0
  constructor(account: ExtensionAccount, readonly bridge: BridgeClient) {
    super(account, { accounts: { get: () => bridge.listAccounts() }, signer: { signPayload: payload => bridge.signExtrinsicPayload(payload), signRaw: request => bridge.signBytes(request.address, request.data) } })
  }
  beginTwoStageSigning(): void { this.signIndex = 0; this.twoStage = true }
  override signPayload(payload: Record<string, unknown>): Promise<{ signature: string }> {
    if (payload.address !== this.address) throw new BridgeError('extension signer address mismatch')
    if (this.account.genesisHash && String(payload.genesisHash).toLowerCase() !== this.account.genesisHash.toLowerCase()) throw new BridgeError('extension account belongs to another chain')
    this.signIndex++
    return this.bridge.signExtrinsicPayload(payload, this.twoStage ? { stage: this.signIndex === 1 ? 'approve 1 of 2 — the shielded transaction' : 'approve 2 of 2 — the encrypted carrier', moreComing: this.signIndex === 1 } : {})
  }
  close(): Promise<void> { return this.bridge.close() }
  reportTransactionResult(success: boolean): Promise<void> { return this.bridge.reportTransactionResult(success) }
}
export async function connectBridgeSigner(url: string, token: string, options: { address?: string; source?: string; name?: string; select?: (accounts: ExtensionAccount[]) => ExtensionAccount | Promise<ExtensionAccount> } = {}): Promise<BridgeExtensionSigner> {
  const client = new BridgeClient(url, token)
  try {
    const accounts = (await client.listAccounts()).filter(a => (!options.address || a.address === options.address) && (!options.source || a.source === options.source) && (!options.name || a.name === options.name))
    const selected = options.select ? await options.select(accounts) : accounts.length === 1 ? accounts[0] : undefined
    const found = selected && accounts.find(a => a.address === selected.address && a.source === selected.source)
    if (!found) throw new BridgeError(accounts.length ? 'select an explicit extension account' : 'no matching extension account')
    return new BridgeExtensionSigner(found, client)
  } catch (error) { await client.close(); throw error }
}
export async function runBridge(options: BridgeServerOptions = {}): Promise<BridgeServer> { const server = new BridgeServer(options); await server.start(); return server }
