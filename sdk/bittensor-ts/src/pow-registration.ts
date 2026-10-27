import { buildIntent, type Intent } from './intents'

export const MAX_WORK_AGE_BLOCKS = 5
export const CHALLENGE_REFRESH_SECONDS = 12
const U64_MAX = (1n << 64n) - 1n
const U256_MAX = (1n << 256n) - 1n
export interface RegistrationReader {
  query(pallet: string, item: string, params?: unknown[], block?: string): Promise<unknown>
  blockNumber(): Promise<number>
  blockHash(block?: number): Promise<string>
}
export interface RegistrationMiningBackend {
  decodeAddress(address: string): Uint8Array
  seal(prefix: Uint8Array, nonce: bigint): Uint8Array | Promise<Uint8Array>
  mine(prefix: Uint8Array, difficulty: bigint, start: bigint, attempts: number): Promise<{ nonce: bigint; work: Uint8Array } | null>
  /** Number of disjoint per-device batches produced by mine(). */
  parallelism?: number
}
export interface MineRegistrationOptions {
  workers?: number
  maxSeconds?: number
  backend?: 'auto' | 'gpu' | 'cpu'
  deviceIds?: number[]
  signal?: AbortSignal
}

function u64(value: unknown, fallback: bigint, name: string): bigint {
  if (value == null) return fallback
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError(`${name} requires a lossless integer`)
  const result = BigInt(value as bigint | number | string)
  if (result < 0n || result > U64_MAX) throw new RangeError(`${name} must fit u64`)
  return result
}
export function effectiveDifficulty(value: unknown, minimum?: unknown, maximum?: unknown): bigint {
  const min = u64(minimum, 1n, 'minimum difficulty') || 1n
  const suppliedMax = u64(maximum, U64_MAX, 'maximum difficulty'), max = suppliedMax < min ? min : suppliedMax
  const difficulty = u64(value, 1n, 'difficulty') || 1n
  return difficulty < min ? min : difficulty > max ? max : difficulty
}

function hexBytes(value: string): Uint8Array {
  const text = value.replace(/^0x/, '')
  if (!/^[0-9a-f]{64}$/i.test(text)) throw new RangeError('PoW requires a 32-byte block hash')
  return Uint8Array.from(text.match(/../g)!, part => parseInt(part, 16))
}
export function registrationPrefix(netuid: number, blockHash: string, hotkey: string, coldkey: string, decodeAddress: (address: string) => Uint8Array): Uint8Array {
  if (!Number.isInteger(netuid) || netuid <= 0 || netuid > 65535) throw new RangeError('PoW requires a non-root u16 netuid')
  const block = hexBytes(blockHash), hot = decodeAddress(hotkey), cold = decodeAddress(coldkey)
  if (hot.length !== 32 || cold.length !== 32) throw new RangeError('PoW requires AccountId32 addresses')
  const domain = new TextEncoder().encode('subtensor-pow-register-v1'), result = new Uint8Array(123)
  result.set(domain); new DataView(result.buffer).setUint16(domain.length, netuid, true)
  result.set(block, domain.length + 2); result.set(hot, domain.length + 34); result.set(cold, domain.length + 66)
  return result
}
function workValue(work: Uint8Array): bigint {
  if (work.length !== 32) throw new Error('miner returned a seal with invalid length')
  let value = 0n
  for (let i = 31; i >= 0; i--) value = (value << 8n) + BigInt(work[i])
  return value
}
function abort(signal?: AbortSignal): void { if (signal?.aborted) throw signal.reason ?? new Error('PoW mining aborted') }
async function pause(ms: number, signal?: AbortSignal): Promise<void> {
  abort(signal)
  await new Promise<void>((resolve, reject) => {
    const stop = () => { clearTimeout(timer); reject(signal?.reason ?? new Error('PoW mining aborted')) }
    const timer = setTimeout(() => { signal?.removeEventListener('abort', stop); resolve() }, ms)
    signal?.addEventListener('abort', stop, { once: true })
  })
}
function randomNonce(): bigint {
  const data = new Uint32Array(2)
  globalThis.crypto.getRandomValues(data)
  return BigInt(data[0]) | (BigInt(data[1]) << 32n)
}

/** Mine public challenges and return a ready-to-prepare intent, refreshing stale/reorged proofs. */
export async function mineRegistration(
  reader: RegistrationReader,
  netuid: number,
  hotkey: string,
  coldkey: string,
  miner: RegistrationMiningBackend,
  options: MineRegistrationOptions = {},
): Promise<Intent> {
  const workers = options.workers ?? 4, maxSeconds = options.maxSeconds ?? 300
  if (!Number.isInteger(workers) || workers < 1 || workers > 32 || !Number.isFinite(maxSeconds) || maxSeconds <= 0 || maxSeconds > 3600) throw new RangeError('workers must be 1..32 and maxSeconds must be in (0,3600]')
  if (!Number.isInteger(netuid) || netuid < 1 || netuid > 65535) throw new RangeError('PoW requires a non-root u16 netuid')
  const backend = options.backend ?? 'auto'
  if (!['auto', 'gpu', 'cpu'].includes(backend)) throw new TypeError('backend must be auto, gpu, or cpu')
  if (options.deviceIds != null && (backend === 'cpu' || options.deviceIds.length === 0 || options.deviceIds.length > 32 || new Set(options.deviceIds).size !== options.deviceIds.length || options.deviceIds.some(id => !Number.isSafeInteger(id) || id < 0))) throw new RangeError('select 1..32 unique nonnegative GPU ids with auto or gpu')
  const deadline = performance.now() + maxSeconds * 1000
  const timeout = () => new Error('No fresh registration proof found within the mining time limit')
  while (performance.now() < deadline) {
    abort(options.signal)
    const head = await reader.blockNumber()
    if (!Number.isSafeInteger(head) || head < 0) throw new RangeError('invalid chain block number')
    if (head < 1) { await pause(Math.min(1000, Math.max(0, deadline - performance.now())), options.signal); continue }
    const anchor = await reader.blockHash(head), refresh = performance.now() + CHALLENGE_REFRESH_SECONDS * 1000
    const query = (item: string, params: unknown[] = [netuid]) => reader.query('SubtensorModule', item, params, anchor)
    const [allowed, raw, minimum, maximum, uid, last, registrations, limit] = await Promise.all([
      query('NetworkPowRegistrationAllowed'), query('Difficulty'), query('MinDifficulty'), query('MaxDifficulty'),
      query('Uids', [netuid, hotkey]), query('LastPowRegistrationBlock', [hotkey]), query('RegistrationsThisBlock'), query('MaxRegistrationsPerBlock'),
    ])
    if (!allowed) throw new Error('the subnet owner must enable PoW registration')
    if (uid != null) throw new Error('the hotkey is already registered on this subnet')
    const maxRegistrations = u64(limit, 0n, 'registration limit')
    if (!maxRegistrations) throw new Error('per-block registration limit must be positive')
    if (u64(registrations, 0n, 'registrations') >= maxRegistrations || (last != null && BigInt(head) <= u64(last, 0n, 'last registration'))) {
      await pause(Math.min(1000, Math.max(0, deadline - performance.now())), options.signal); continue
    }
    const difficulty = effectiveDifficulty(raw, minimum, maximum)
    const prefix = registrationPrefix(netuid, anchor, hotkey, coldkey, miner.decodeAddress)
    while (performance.now() < Math.min(deadline, refresh)) {
      abort(options.signal)
      const start = randomNonce(), attempts = 100000, batches = miner.parallelism == null ? workers : 1
      const results = await Promise.all(Array.from({ length: batches }, (_, index) => miner.mine(prefix, difficulty, (start + BigInt(index * attempts)) & U64_MAX, attempts)))
      abort(options.signal)
      if (performance.now() >= deadline) throw timeout()
      if (!results.some(result => result != null)) { await pause(0, options.signal); continue }
      const current = await reader.blockNumber()
      if (current < head || current - head >= MAX_WORK_AGE_BLOCKS || await reader.blockHash(head) !== anchor) break
      for (const result of results) {
        if (result == null) continue
        const nonce = u64(result.nonce, 0n, 'miner nonce'), expected = await miner.seal(prefix, nonce)
        if (result.work.length !== expected.length || result.work.some((byte, i) => byte !== expected[i])) throw new Error('miner returned an invalid registration proof')
        const latest = await reader.blockHash(current)
        const bounds = await Promise.all(['Difficulty', 'MinDifficulty', 'MaxDifficulty'].map(item => reader.query('SubtensorModule', item, [netuid], latest)))
        if (workValue(result.work) > U256_MAX / effectiveDifficulty(...bounds as [unknown, unknown, unknown])) break
        if (performance.now() >= deadline) throw timeout()
        return buildIntent('pow_register', {
          netuid, work_block: head, nonce, work_hex: Array.from(result.work, byte => byte.toString(16).padStart(2, '0')).join(''), hotkey_ss58: hotkey,
          mining_workers: workers, mining_timeout: maxSeconds, mining_backend: backend, mining_device_ids: options.deviceIds ?? null,
        })
      }
      await pause(0, options.signal)
    }
  }
  throw timeout()
}
