import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
import { hexBytes } from './portable'
/** Portable wall-clock timelock workflows; cryptography is supplied by native Rust or WASM. */
export const QUICKNET_GENESIS = POLICY_CATALOG.validation.QUICKNET_GENESIS
export const QUICKNET_PERIOD = POLICY_CATALOG.validation.QUICKNET_PERIOD
export type DurationLike = number | string
export type WhenLike = Date | number | string
export type TimelockedInput = Timelocked | Uint8Array | string
export interface TimelockBackend {
  encryptAtRound(data: Uint8Array, round: bigint): Promise<{ ciphertext: Uint8Array; revealRound: bigint }> | { ciphertext: Uint8Array; revealRound: bigint }
  decrypt(data: Uint8Array): Promise<Uint8Array | null>
}
export interface TimelockEncryptOptions { revealIn?: DurationLike; revealAt?: WhenLike; revealRound?: bigint | number | string }
export interface TimelockDecryptOptions {
  wait?: boolean
  /** Maximum total wait, in seconds. */
  timeout?: number
  signal?: AbortSignal
}

export class TimelockError extends Error {
  constructor(message: string, options?: ErrorOptions) { super(message, options); this.name = 'TimelockError' }
}
export class TimelockNotReady extends TimelockError {
  readonly revealAt: Date
  constructor(readonly revealRound: bigint, readonly remaining: number) {
    const date = revealTime(revealRound)
    super(`not decryptable yet: reveals at ${date.toISOString()} (round ${revealRound}, ${formatDuration(remaining)} from now)`)
    this.name = 'TimelockNotReady'; this.revealAt = date
  }
}

export function parseDuration(value: DurationLike): number {
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new TimelockError('duration must be finite')
    return value
  }
  const text = value.trim()
  if (text && Number.isFinite(Number(text))) return Number(text)
  const units: Record<string, number> = { w: 604800, d: 86400, h: 3600, m: 60, s: 1 }
  const tokens = [...text.matchAll(/(\d+(?:\.\d+)?)\s*([wdhms])/gi)]
  if (!tokens.length || tokens.map(match => `${match[1]}${match[2]}`).join('').toLowerCase() !== text.replace(/\s+/g, '').toLowerCase()) throw new TimelockError('cannot parse duration; use seconds or units like 90s, 15m, 1h30m, 2d')
  const seconds = tokens.reduce((sum, token) => sum + Number(token[1]) * units[token[2].toLowerCase()], 0)
  if (!Number.isFinite(seconds)) throw new TimelockError('duration must be finite')
  return seconds
}

export function formatDuration(seconds: number): string {
  let remaining = Math.max(0, Math.floor(seconds))
  if (!Number.isFinite(remaining)) throw new TimelockError('duration must be finite')
  const parts: string[] = []
  for (const [unit, size] of [['d', 86400], ['h', 3600], ['m', 60], ['s', 1]] as const) {
    const count = Math.floor(remaining / size); remaining %= size
    if (count) parts.push(`${count}${unit}`)
  }
  return parts.join(' ') || '0s'
}

function timestamp(value: WhenLike): number {
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new TimelockError('timestamp must be finite')
    return value
  }
  if (value instanceof Date) return timestamp(value.getTime() / 1000)
  let iso = value.trim()
  if (!/^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?)?(?:Z|[+-]\d{2}:?\d{2})?$/i.test(iso)) throw new TimelockError('expected an ISO-8601 moment or Unix timestamp')
  if (!/[zZ]$|[+-]\d{2}:?\d{2}$/.test(iso)) iso += iso.length === 10 ? 'T00:00:00Z' : 'Z'
  const result = Date.parse(iso) / 1000
  if (!Number.isFinite(result)) throw new TimelockError('invalid reveal moment')
  return result
}

function roundValue(value: bigint | number | string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new TimelockError('round must be a safe integer or bigint')
  let round: bigint
  try { round = BigInt(value) } catch { throw new TimelockError('round must be an unsigned integer') }
  if (round < 1n || round > 0xffffffffffffffffn) throw new TimelockError('round must be in 1..u64::MAX')
  return round
}

function timelockPolicy<T>(operation: string, input: unknown): T {
  try { return evaluatePolicy<T>(operation, input) }
  catch (error) { throw new TimelockError(error instanceof Error ? error.message : String(error)) }
}
export function currentRound(at = Date.now() / 1000): bigint {
  if (!Number.isFinite(at)) throw new TimelockError('timestamp must be finite')
  return BigInt(timelockPolicy<string>('validation.timelock.round', { at, ceil: false }))
}
export function roundAt(when: WhenLike): bigint {
  return BigInt(timelockPolicy<string>('validation.timelock.round', { at: timestamp(when), ceil: true }))
}
export function revealTime(round: bigint | number | string): Date {
  const millis = BigInt(timelockPolicy<string>('validation.timelock.time', { round: roundValue(round) })) * 1000n
  if (millis > 8640000000000000n) throw new TimelockError('reveal round exceeds the Date range')
  return new Date(Number(millis))
}

function bytes(value: Uint8Array | string): Uint8Array {
  if (typeof value !== 'string') return new Uint8Array(value)
  const text = value.trim().replace(/^0x/, '')
  if (text.length % 2 || !/^[0-9a-f]*$/i.test(text)) throw new TimelockError('expected ciphertext bytes or a hex string')
  return Uint8Array.from(text.match(/../g) ?? [], part => parseInt(part, 16))
}

// UserData is SCALE Vec<u8> followed by a u64 reveal round. Decode exactly,
// rejecting trailing bytes just as bittensor-core does.
function envelope(data: Uint8Array): { inner: Uint8Array; round: bigint } {
  const decoded = timelockPolicy<{ inner: string; round: string }>('validation.timelock.envelope', { data })
  return { inner: hexBytes(decoded.inner), round: BigInt(decoded.round) }
}

export class Timelocked {
  private readonly data: Uint8Array
  readonly revealRound: bigint
  private constructor(data: Uint8Array, private readonly backend?: TimelockBackend) {
    this.data = new Uint8Array(data); this.revealRound = envelope(this.data).round
  }
  static parse(data: TimelockedInput, backend?: TimelockBackend): Timelocked {
    if (data instanceof Timelocked && backend == null) return data
    return new Timelocked(data instanceof Timelocked ? data.ciphertext : bytes(data), backend)
  }
  get ciphertext(): Uint8Array { return new Uint8Array(this.data) }
  get encrypted(): Uint8Array { return envelope(this.data).inner }
  get revealAt(): Date { return revealTime(this.revealRound) }
  /** Seconds until the nominal reveal time. */
  get remaining(): number { return Math.max(0, (this.revealAt.getTime() - Date.now()) / 1000) }
  get revealed(): boolean { return currentRound() >= this.revealRound }
  hex(): string { return Array.from(this.data, value => value.toString(16).padStart(2, '0')).join('') }
  decrypt(options: TimelockDecryptOptions = {}): Promise<Uint8Array> {
    if (!this.backend) return Promise.reject(new TimelockError('use a timelock service to decrypt this ciphertext'))
    return createTimelock(this.backend).decrypt(this, options)
  }
}

function checkAbort(signal?: AbortSignal): void { if (signal?.aborted) throw signal.reason ?? new Error('timelock operation aborted') }
async function pause(ms: number, signal?: AbortSignal): Promise<void> {
  checkAbort(signal)
  await new Promise<void>((resolve, reject) => {
    const abort = () => { clearTimeout(timer); reject(signal?.reason ?? new Error('timelock operation aborted')) }
    const timer = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve() }, Math.min(ms, 2147483647))
    signal?.addEventListener('abort', abort, { once: true })
  })
}

export function createTimelock(backend: TimelockBackend) {
  return {
    parse: (data: TimelockedInput): Timelocked => Timelocked.parse(data, backend),
    async encrypt(data: string | Uint8Array, options: TimelockEncryptOptions): Promise<Timelocked> {
      if ([options.revealIn, options.revealAt, options.revealRound].filter(value => value != null).length !== 1) throw new TimelockError('give exactly one of revealIn, revealAt, or revealRound')
      let round: bigint
      if (options.revealIn != null) {
        const seconds = parseDuration(options.revealIn)
        if (seconds <= 0) throw new TimelockError('revealIn must be positive')
        round = roundAt(Date.now() / 1000 + seconds)
      } else round = options.revealAt != null ? roundAt(options.revealAt) : roundValue(options.revealRound!)
      if (round <= currentRound()) throw new TimelockError('reveal moment must be in the future')
      const encrypted = await backend.encryptAtRound(typeof data === 'string' ? new TextEncoder().encode(data) : new Uint8Array(data), round)
      const result = Timelocked.parse(encrypted.ciphertext, backend)
      if (result.revealRound !== round || encrypted.revealRound !== round) throw new TimelockError('encryption returned an inconsistent reveal round')
      return result
    },
    async decrypt(data: TimelockedInput, options: TimelockDecryptOptions = {}): Promise<Uint8Array> {
      if (options.timeout != null && (!Number.isFinite(options.timeout) || options.timeout < 0)) throw new TimelockError('timeout must be non-negative seconds')
      const sealed = Timelocked.parse(data, backend), deadline = options.timeout == null ? Infinity : performance.now() + options.timeout * 1000
      checkAbort(options.signal)
      while (sealed.remaining > 0) {
        if (!options.wait || performance.now() + sealed.remaining * 1000 > deadline) throw new TimelockNotReady(sealed.revealRound, sealed.remaining)
        await pause(sealed.remaining * 1000, options.signal)
      }
      const grace = Math.min(performance.now() + 30000, deadline)
      let lastError: unknown
      while (true) {
        checkAbort(options.signal)
        try {
          const result = await backend.decrypt(sealed.ciphertext)
          if (result != null) return new Uint8Array(result)
        } catch (error) { lastError = error }
        checkAbort(options.signal)
        if (performance.now() >= grace) break
        await pause(Math.min(1000, grace - performance.now()), options.signal)
      }
      throw new TimelockError(`failed to decrypt round ${sealed.revealRound} (beacon unreachable or ciphertext corrupt)`, { cause: lastError })
    },
  }
}
