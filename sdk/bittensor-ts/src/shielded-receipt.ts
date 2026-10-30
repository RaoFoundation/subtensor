import { classifyChainError, type ChainErrorInfo } from './chain-errors'
import { Balance } from './balance'
import { normalizeHeader } from './receipt'
import { ChainError, withRequestSignal, type RpcRequestOptions } from './transport'

export interface ShieldedReceipt {
  status: 'submitted' | 'inBlock' | 'finalized' | 'failed' | 'unknown'
  success?: boolean; message: string; extrinsicHash: string; blockHash?: string; blockNumber?: number
  extrinsicIndex?: number; finalized?: boolean; events: unknown[]; fee?: Balance; error?: unknown; chainError?: ChainErrorInfo
  data?: Record<string, unknown>
}
export interface ShieldedReceiptAdapter {
  rpc(method: string, params: unknown[], options?: RpcRequestOptions): Promise<any>
  findExtrinsic(hash: string, blockHash: string, finalized: boolean, options?: RpcRequestOptions): Promise<ShieldedReceipt | null>
}
export interface ShieldedFollowOptions { waitForFinalization?: boolean; timeoutMs?: number; signal?: AbortSignal }
export type ShieldedResult = ShieldedReceipt & { shielded: true; innerExtrinsicHash: string; carrierExtrinsicHash: string; carrierFee?: Balance }

/** Follow an accepted carrier to the decrypted extrinsic, including nested dispatch failures. */
export async function followShieldedResult(outer: ShieldedReceipt, innerHash: string, period: number, adapter: ShieldedReceiptAdapter, options: ShieldedFollowOptions = {}): Promise<ShieldedResult> {
  if (!Number.isSafeInteger(period) || period < 1 || period > 8) throw new RangeError('shielded receipt period must be 1 to 8 blocks')
  const metadata = { shielded: true as const, innerExtrinsicHash: innerHash, carrierExtrinsicHash: outer.extrinsicHash, carrierFee: outer.fee }
  const data = { ...outer.data, shielded: true, inner_extrinsic_hash: innerHash, carrier_extrinsic_hash: outer.extrinsicHash }
  if (outer.success !== true || outer.blockNumber == null) return { ...outer, ...metadata, data: { ...data, carrier_only: true } }
  const request = withRequestSignal({ timeoutMs: options.timeoutMs ?? 120_000, signal: options.signal })
  const signal = request.signal!, deadline = outer.blockNumber + period, scanned = new Map<number, string>()
  const rpc = (method: string, params: unknown[]) => guarded(() => adapter.rpc(method, params, { signal }), signal)
  let finalized = !!options.waitForFinalization
  try {
    for (;;) {
      signal.throwIfAborted()
      // Waiting for finality scans only canonical finalized blocks, so a
      // receipt from an unfinalized fork can never become a finalized result.
      const headHash = finalized ? await rpc('chain_getFinalizedHead', []) : undefined
      const head = normalizeHeader(await rpc('chain_getHeader', headHash ? [headHash] : [])).number
      for (let block = outer.blockNumber; block <= Math.min(head, deadline); block++) {
        const hash = await rpc('chain_getBlockHash', [block])
        if (typeof hash !== 'string') throw new ChainError(`canonical block ${block} is unavailable while resolving shielded receipt`)
        if (scanned.get(block) === hash) continue
        const inner = await guarded(() => adapter.findExtrinsic(innerHash, hash, finalized, { signal }), signal)
        if (inner) return { ...inner, ...metadata, data: { ...inner.data, ...data, carrier_only: false }, fee: inner.fee && outer.fee ? Balance.fromRao(inner.fee.rao + outer.fee.rao) : inner.fee }
        scanned.set(block, hash)
      }
      if (head >= deadline) {
        // A best-head expiry can be undone by a reorg. Re-scan finalized
        // history before releasing any nonce from this envelope.
        if (!finalized) { finalized = true; scanned.clear(); continue }
        const carrierHash = await rpc('chain_getBlockHash', [outer.blockNumber])
        const carrier = await guarded(() => adapter.findExtrinsic(outer.extrinsicHash, carrierHash, true, { signal }), signal)
        if (!carrier?.success) return { ...outer, ...metadata, status: 'unknown', success: undefined, message: 'carrier inclusion changed before finalized expiry', data: { ...data, carrier_only: true, submission_uncertain: true } }
        const message = `encrypted carrier was accepted, but inner extrinsic ${innerHash} was not included before its era expired`
        return { ...outer, ...metadata, data: { ...data, carrier_only: true, inner_expiry_finalized: true }, status: 'failed', success: false, message, error: new ChainError(message), chainError: classifyChainError(message, { name: 'ExtrinsicExpired' }) }
      }
      await delay(1000, signal)
    }
  } catch (error) { if (signal.aborted) throw request.error(); throw error }
  finally { request.cleanup() }
}
function guarded<T>(run: () => Promise<T>, signal: AbortSignal): Promise<T> {
  signal.throwIfAborted()
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason)
    signal.addEventListener('abort', abort, { once: true })
    run().then(resolve, reject).finally(() => signal.removeEventListener('abort', abort))
  })
}
function delay(ms: number, signal: AbortSignal): Promise<void> {
  signal.throwIfAborted()
  return new Promise((resolve, reject) => {
    const abort = () => { clearTimeout(timer); signal.removeEventListener('abort', abort); reject(signal.reason) }
    const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve() }, ms)
    signal.addEventListener('abort', abort, { once: true })
  })
}
