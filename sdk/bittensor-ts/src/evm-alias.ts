import { descriptorBytes, rawBytes } from './protected-accounts'
import { bytesHex as hex } from './portable'
import type { IntentReader } from './intents'
import type { Recipient } from './receiving'

/** Resolve immutable protected aliases at finality before displaying a native deposit route. */
export async function resolveEvmRecipient(reader: IntentReader, evmAddress: string): Promise<Recipient> {
  if (!/^0x[\da-f]{40}$/i.test(evmAddress)) throw new TypeError('expected H160')
  if (!reader.finalizedHead || !reader.evmMirror || !reader.decodeAddress || !reader.encodeAddress || !reader.descriptorAddress) throw new Error('reader lacks alias resolution capabilities')
  const block = await reader.finalizedHead(), address = evmAddress.toLowerCase()
  const bound = await reader.query('HashedAccounts','EvmAliases',[address],block)
  if (bound == null) return {account:reader.evmMirror(address)}
  const bytes = typeof bound === 'string' && !bound.startsWith('0x') ? reader.decodeAddress(bound) : rawBytes(bound)
  if (bytes.length !== 32 || hex(bytes.slice(0,20)) !== address) throw new Error('invalid protected EVM alias')
  const account = reader.encodeAddress(bytes)
  for (const storage of ['Accounts','MlDsaAccounts','DerivedAuthorities']) {
    const record = await reader.query('HashedAccounts',storage,[account],block) as any
    if (!record) continue
    if (storage === 'DerivedAuthorities') return {account}
    const descriptor = descriptorBytes(record.descriptor)
    if (hex(reader.decodeAddress(reader.descriptorAddress(descriptor))) !== hex(bytes)) throw new Error('EVM alias descriptor differs from bound account')
    return {account,descriptor}
  }
  throw new Error('EVM alias has no finalized authority')
}
