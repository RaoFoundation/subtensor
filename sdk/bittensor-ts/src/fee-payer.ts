import { COLDKEY_PAYS_FEE_CALLS } from './fee-filters'
import type { IntentDescriptor, IntentReader } from './intents'

export interface TransactionFeePayer {
  readonly address: string
  /** The runtime discards signer-chosen tips whenever a sponsor pays. */
  readonly dropsTip: boolean
}
type FeeReader = Pick<IntentReader, 'query' | 'countStorageKeys' | 'decodeAddress' | 'encodeAddress'>

/** Mirror ChargeTransactionPaymentWrapper: bounded consented proxies, then direct hotkey calls. */
export async function resolveTransactionFeePayer(reader: FeeReader, value: unknown, signerAddress: string): Promise<TransactionFeePayer> {
  const call = descriptor(value)
  const proxy = proxyCall(call, reader)
  if (proxy && await exists(reader, 'Proxy', 'RealPaysFeeConsentV1', [proxy.real, signerAddress])) {
    let { real, inner } = proxy
    // Include the outer proxy and at most two additional consented proxy hops.
    for (let depth = 1; depth < 3; depth++) {
      const nested = proxyCall(inner, reader)
      if (!nested) {
        const batch = await batchProxyPayer(reader, inner, real)
        return { address: batch ?? real, dropsTip: true }
      }
      if (!await exists(reader, 'Proxy', 'RealPaysFeeConsentV1', [nested.real, real])) break
      real = nested.real; inner = nested.inner
    }
    return { address: real, dropsTip: true }
  }
  // Unlike UI call-leaf inspection, the runtime's fee filter does not recurse
  // into Utility, Proxy, Multisig, or Sudo wrappers.
  if (COLDKEY_PAYS_FEE_CALLS.has(`${call.pallet}.${call.call}`) && await exists(reader, 'SubtensorModule', 'Owner', [signerAddress])) {
    return { address: address(await reader.query('SubtensorModule', 'Owner', [signerAddress]), reader), dropsTip: true }
  }
  return { address: signerAddress, dropsTip: false }
}

async function exists(reader: FeeReader, pallet: string, item: string, params: unknown[]): Promise<boolean> {
  if (!reader.countStorageKeys) throw new Error('fee-payer resolution requires physical storage-key presence')
  // Option<()> and absent ValueQuery values can decode identically, so inspect
  // key presence instead of treating decoded defaults as consent or ownership.
  const count = await reader.countStorageKeys(pallet, item, params, 1)
  if (count !== 0 && count !== 1) throw new Error(`invalid storage-key count for ${pallet}.${item}`)
  return count === 1
}
async function batchProxyPayer(reader: FeeReader, call: IntentDescriptor, delegate: string): Promise<string | null> {
  if (call.pallet !== 'Utility' || !['batch', 'batch_all', 'force_batch'].includes(call.call)) return null
  const calls = call.params.calls
  if (!Array.isArray(calls) || !calls.length) return null
  let common: string | null = null
  for (const child of calls) {
    const proxy = proxyCall(descriptor(child), reader)
    if (!proxy) return null
    if (common == null) {
      if (!await exists(reader, 'Proxy', 'RealPaysFeeConsentV1', [proxy.real, delegate])) return null
      common = proxy.real
    } else if (!sameAccount(reader, common, proxy.real)) return null
  }
  return common
}
function proxyCall(call: IntentDescriptor, reader: FeeReader): { real: string; inner: IntentDescriptor } | null {
  return call.pallet === 'Proxy' && call.call === 'proxy' ? { real: address(call.params.real, reader), inner: descriptor(call.params.call) } : null
}
function address(value: unknown, reader: FeeReader): string {
  if (typeof value === 'string') return value
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
function descriptor(value: unknown): IntentDescriptor {
  if (Array.isArray(value) && value.length >= 2 && typeof value[0] === 'string' && typeof value[1] === 'string') return { pallet: value[0], call: value[1], params: value[2] ?? {} }
  if (value && typeof value === 'object') {
    const row = value as Record<string, any>
    const pallet = row.pallet ?? row.module ?? row.call_module, call = row.call ?? row.fn ?? row.function ?? row.call_function
    if (typeof pallet === 'string' && typeof call === 'string') return { pallet, call, params: row.params ?? Object.fromEntries((row.call_args ?? []).map((arg: any) => [arg.name, arg.value])) }
    const entries = Object.entries(row)
    if (entries.length === 1 && entries[0][1] && typeof entries[0][1] === 'object') {
      const methods = Object.entries(entries[0][1])
      if (methods.length === 1) return { pallet: entries[0][0], call: methods[0][0], params: methods[0][1] as Record<string, unknown> }
    }
  }
  throw new TypeError('fee-payer resolution requires a decoded call')
}
