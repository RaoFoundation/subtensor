import { bytesHex, hexBytes, type PortableCrypto, type PortableSigner } from './portable'

export const HTTP_AUTH_PROTOCOL = 'btauth/1'
export const HTTP_AUTH_HEADERS = Object.freeze({ version: 'X-Bittensor-Version', hotkey: 'X-Bittensor-Hotkey', receiver: 'X-Bittensor-Receiver', nonce: 'X-Bittensor-Nonce', crypto: 'X-Bittensor-Crypto', signature: 'X-Bittensor-Signature' })
export class AuthError extends Error { constructor(message: string) { super(message); this.name = new.target.name } }
export class MalformedAuth extends AuthError {}
export class WrongReceiver extends AuthError {}
export class StaleRequest extends AuthError {}
export class ReplayedRequest extends AuthError {}
export class BadSignature extends AuthError {}
export interface AuthCaller { hotkeySs58: string; nonceNs: bigint; cryptoType: number }
export interface NonceStore { retention?: number; checkAndStore(hotkeySs58: string, nonceNs: bigint): boolean | Promise<boolean> }
export class InMemoryNonceStore implements NonceStore {
  private readonly seen = new Map<string, number>()
  constructor(readonly retention = 60) { if (!Number.isFinite(retention) || retention <= 0) throw new RangeError('nonce retention must be positive') }
  checkAndStore(hotkey: string, nonce: bigint): boolean {
    const now = performance.now()
    for (const [key, expiry] of this.seen) { if (expiry > now) break; this.seen.delete(key) }
    const key = `${hotkey}:${nonce}`
    if (this.seen.has(key)) return false
    this.seen.set(key, now + this.retention * 1000)
    return true
  }
}
export interface AuthRequest { method: string; path: string; body?: Uint8Array; receiverSs58?: string; nonceNs?: bigint }
export async function buildAuthPayload(options: AuthRequest & { scheme: 'sr25519' | 'ed25519'; nonceNs: bigint; senderSs58: string }): Promise<Uint8Array> {
  const bodyHash = new Uint8Array(await globalThis.crypto.subtle.digest('SHA-256', new Uint8Array(options.body ?? []).buffer))
  return new TextEncoder().encode([HTTP_AUTH_PROTOCOL, options.scheme, options.method.toUpperCase(), options.path, bytesHex(bodyHash).slice(2), String(options.nonceNs), options.senderSs58, options.receiverSs58 || '-'].join('\n'))
}
function schemeFor(cryptoType: number): 'ed25519' | 'sr25519' { if (cryptoType === 0) return 'ed25519'; if (cryptoType === 1) return 'sr25519'; throw new MalformedAuth('unsupported signature scheme') }
function cryptoFor(scheme: string): number { if (['ed25519', '0'].includes(scheme.toLowerCase())) return 0; if (['sr25519', '1'].includes(scheme.toLowerCase())) return 1; throw new MalformedAuth('unsupported signature scheme') }
export function createHttpAuth(crypto: PortableCrypto) {
  const defaultStore = new InMemoryNonceStore()
  let lastNonce = 0n
  return {
    buildPayload: buildAuthPayload,
    async sign(signer: PortableSigner, options: AuthRequest): Promise<Record<string, string>> {
      const now = BigInt(Date.now()) * 1_000_000n
      const nonceNs = options.nonceNs ?? (now > lastNonce ? now : lastNonce + 1n)
      lastNonce = nonceNs
      const scheme = schemeFor(signer.cryptoType)
      const payload = await buildAuthPayload({ ...options, scheme, nonceNs, senderSs58: signer.ss58Address })
      const signature = await signer.sign(payload)
      if (!(signature instanceof Uint8Array)) throw new TypeError('signer must return signature bytes')
      return { [HTTP_AUTH_HEADERS.version]: '1', [HTTP_AUTH_HEADERS.hotkey]: signer.ss58Address, [HTTP_AUTH_HEADERS.nonce]: String(nonceNs), [HTTP_AUTH_HEADERS.signature]: bytesHex(signature), ...(scheme === 'sr25519' ? {} : { [HTTP_AUTH_HEADERS.crypto]: scheme }), ...(options.receiverSs58 == null ? {} : { [HTTP_AUTH_HEADERS.receiver]: options.receiverSs58 }) }
    },
    async verify(headers: Headers | Record<string, string>, body: Uint8Array, options: { method: string; path: string; selfHotkeySs58: string; maxAge?: number; allowedSkew?: number; requireReceiver?: boolean; nonceStore?: NonceStore; nowNs?: bigint }): Promise<AuthCaller> {
      const h = new Headers(headers)
      if (h.get(HTTP_AUTH_HEADERS.version) !== '1') throw new MalformedAuth('missing or unsupported authentication version')
      const sender = h.get(HTTP_AUTH_HEADERS.hotkey), nonce = h.get(HTTP_AUTH_HEADERS.nonce), signature = h.get(HTTP_AUTH_HEADERS.signature)
      if (!sender || !nonce || !/^-?\d+$/.test(nonce) || signature == null) throw new MalformedAuth('missing or malformed authentication headers')
      const nonceNs = BigInt(nonce), cryptoType = cryptoFor(h.get(HTTP_AUTH_HEADERS.crypto) ?? 'sr25519')
      let signatureBytes: Uint8Array
      try { signatureBytes = hexBytes(signature) } catch { throw new MalformedAuth('invalid signature encoding') }
      const receiverSs58 = h.get(HTTP_AUTH_HEADERS.receiver) ?? undefined
      if (receiverSs58 == null ? options.requireReceiver !== false : receiverSs58 !== options.selfHotkeySs58) throw new WrongReceiver('request is not bound to this receiver')
      const maxAge = options.maxAge ?? 10, allowedSkew = options.allowedSkew ?? 2
      if (![maxAge, allowedSkew].every(value => Number.isFinite(value) && value >= 0)) throw new RangeError('freshness limits must be finite and nonnegative')
      const now = options.nowNs ?? BigInt(Date.now()) * 1_000_000n
      if (now - nonceNs > BigInt(Math.round(maxAge * 1e9)) || nonceNs - now > BigInt(Math.round(allowedSkew * 1e9))) throw new StaleRequest('request is outside the freshness window')
      const payload = await buildAuthPayload({ method: options.method, path: options.path, body, scheme: schemeFor(cryptoType), senderSs58: sender, receiverSs58, nonceNs })
      let valid: boolean
      try { valid = crypto.verify(payload, signatureBytes, sender, cryptoType) } catch { throw new MalformedAuth('invalid hotkey address or signature encoding') }
      if (!valid) throw new BadSignature('signature does not verify against the claimed hotkey')
      const store = options.nonceStore ?? defaultStore
      if (store.retention != null && store.retention < maxAge + allowedSkew) throw new RangeError('nonce retention is shorter than the freshness window')
      if (!await store.checkAndStore(sender, nonceNs)) throw new ReplayedRequest('request nonce was already accepted')
      return { hotkeySs58: sender, nonceNs, cryptoType }
    },
  }
}
