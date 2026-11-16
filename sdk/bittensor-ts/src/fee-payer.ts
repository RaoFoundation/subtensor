import { evaluatePolicy } from './policy'
import { hexBytes } from './portable'
import type { IntentDescriptor, IntentReader } from './intents'

export interface TransactionFeePayer {
  readonly address: string
  /** The runtime discards signer-chosen tips whenever a sponsor pays. */
  readonly dropsTip: boolean
}
type FeeReader = Pick<IntentReader, 'query' | 'countStorageKeys' | 'decodeAddress' | 'encodeAddress'>

/** Chain reads only; the core chooses consent traversal and the resulting fee payer. */
export async function resolveTransactionFeePayer(reader: FeeReader, call: unknown, signerAddress: string): Promise<TransactionFeePayer> {
  const state: Record<string, unknown> = {}
  for (;;) {
    const next = evaluatePolicy<any>('financial.fee.payer', { call, signer: signerAddress, state })
    if (next.kind === 'result') return { address: address(next.address, reader), dropsTip: next.dropsTip }
    const params = next.params.map((value: unknown) => address(value, reader))
    if (next.kind === 'identity') state[next.key] = sameAccount(reader, params[0], params[1])
    else if (next.kind === 'query') state[next.key] = await reader.query(next.pallet, next.item, params)
    else {
      if (!reader.countStorageKeys) throw new Error('fee-payer resolution requires physical storage-key presence')
      state[next.key] = await reader.countStorageKeys(next.pallet, next.item, params, 1)
    }
  }
}
function address(value: unknown, reader: FeeReader): string {
  if (typeof value === 'string') {
    if (/^0x[0-9a-f]{64}$/i.test(value) && reader.encodeAddress) return reader.encodeAddress(hexBytes(value))
    return value
  }
  if (value instanceof Uint8Array || Array.isArray(value) && value.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
    if (!reader.encodeAddress) throw new Error('fee-payer address encoding unavailable')
    return reader.encodeAddress(Uint8Array.from(value as ArrayLike<number>))
  }
  if (value && typeof value === 'object' && 'Id' in value) return address((value as { Id: unknown }).Id, reader)
  throw new TypeError('fee-payer account is not an AccountId')
}
function sameAccount(reader: FeeReader, left: string, right: string): boolean {
  if (left === right) return true
  if (!reader.decodeAddress) throw new Error('fee-payer account comparison requires address decoding')
  const a = reader.decodeAddress(left), b = reader.decodeAddress(right)
  return a.length === b.length && a.every((byte, index) => byte === b[index])
}