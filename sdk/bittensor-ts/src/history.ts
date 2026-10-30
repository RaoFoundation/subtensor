import { Balance } from './balance'
export const TRANSFERS_API = 'https://api.taomarketcap.com/internal/v1/transactions/transfers/'
export interface TransferRecord {
  blockNumber: number; extrinsicIndex: number; timestamp: string; amount: Balance; netuid: number
  from: string; to: string; direction: 'self' | 'out' | 'in'; success: boolean; taoPriceUsd: number | string | null
}
/** Indexer-backed transfer history; chain RPC alone cannot enumerate an account's past transfers. */
export async function transferHistory(owner: string, options: { limit?: number; endpoint?: string; fetch?: typeof fetch; signal?: AbortSignal; timeoutMs?: number } = {}): Promise<TransferRecord[]> {
  const limit = options.limit ?? 100
  if (!Number.isSafeInteger(limit) || limit < 0 || limit > 100_000) throw new RangeError('limit must be in 0..100000')
  if (typeof owner !== 'string' || !owner) throw new TypeError('transfer history requires an owner')
  if (options.timeoutMs != null && (!Number.isSafeInteger(options.timeoutMs) || options.timeoutMs <= 0)) throw new RangeError('timeoutMs must be a positive integer')
  const endpoint = new URL(options.endpoint ?? TRANSFERS_API), result: TransferRecord[] = [], seen = new Set<string>()
  if (!['http:', 'https:'].includes(endpoint.protocol)) throw new TypeError('transfer endpoint must use HTTP(S)')
  endpoint.searchParams.set('coldkey', owner); endpoint.searchParams.set('limit', String(Math.min(limit, 200)))
  let next: URL | null = endpoint
  while (next && result.length < limit) {
    if (seen.has(next.href)) throw new Error('transfer pagination did not advance')
    seen.add(next.href)
    const response = await (options.fetch ?? fetch)(next, { headers: { Accept: 'application/json' }, signal: options.signal ? AbortSignal.any([options.signal, AbortSignal.timeout(options.timeoutMs ?? 30_000)]) : AbortSignal.timeout(options.timeoutMs ?? 30_000) })
    if (!response.ok) throw new Error(`transfer history HTTP ${response.status}`)
    const body = await response.json() as { results: Array<Record<string, any>>; next?: string }
    if (!body || !Array.isArray(body.results) || body.next != null && typeof body.next !== 'string') throw new TypeError('invalid transfer history response')
    for (const row of body.results) {
      if (!row || typeof row.from_coldkey !== 'string' || typeof row.to_coldkey !== 'string' || row.from_coldkey !== owner && row.to_coldkey !== owner) throw new TypeError('transfer row does not involve the requested owner')
      if (typeof row.success !== 'boolean' && row.success !== 0 && row.success !== 1) throw new TypeError('invalid transfer success status')
      const direction = row.from_coldkey === row.to_coldkey ? 'self' : row.from_coldkey === owner ? 'out' : 'in'
      const netuid = integer(direction === 'in' ? row.to_netuid : row.from_netuid, 'asset', 65535)
      if (typeof row.timestamp !== 'string') throw new TypeError('invalid transfer timestamp')
      result.push({ blockNumber: integer(row.block_number, 'block number'), extrinsicIndex: integer(row.extrinsic_idx, 'extrinsic index'), timestamp: row.timestamp, amount: Balance.fromRao(row.amount, netuid), netuid, from: row.from_coldkey, to: row.to_coldkey, direction, success: Boolean(row.success), taoPriceUsd: row.tao_price_usd ?? null })
      if (result.length === limit) break
    }
    if (result.length === limit) break
    next = body.next ? new URL(body.next, next) : null
    if (next?.protocol === 'http:' && endpoint.protocol === 'https:') next.protocol = 'https:'
    if (next && next.origin !== endpoint.origin) throw new Error('transfer pagination moved to another origin')
    if (next && !body.results.length) throw new Error('transfer pagination did not advance: empty page has a continuation')
  }
  return result
}
function integer(value: unknown, field: string, max = Number.MAX_SAFE_INTEGER): number {
  if (typeof value !== 'number' && !(typeof value === 'string' && /^\d+$/.test(value))) throw new TypeError(`invalid transfer ${field}`)
  const result = Number(value)
  if (!Number.isSafeInteger(result) || result < 0 || result > max) throw new TypeError(`invalid transfer ${field}`)
  return result
}
