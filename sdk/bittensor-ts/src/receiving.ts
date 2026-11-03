import { bytesHex as hex } from './portable'
import { descriptorBytes, rawBytes, registrationDescriptor } from './protected-accounts'
import type { IntentDescriptor, IntentReader } from './intents'

export interface Recipient { account: string; descriptor?: Uint8Array }
export function containsAccountRegistration(call: IntentDescriptor): boolean {
  if (call.pallet === 'HashedAccounts' && ['register','register_derived','register_derived_sponsored'].includes(call.call) || call.pallet === 'Proxy' && call.call === 'create_protected_pure' || call.pallet === 'Utility' && call.call === 'as_protected_derivative') return true
  const single = call.pallet === 'Proxy' && ['proxy','proxy_announced'].includes(call.call) || call.pallet === 'Multisig' && ['as_multi','as_multi_safe','as_multi_threshold_1','as_multi_threshold_1_safe'].includes(call.call) || call.pallet === 'Utility' && ['as_derivative','as_safe_derivative'].includes(call.call) || call.pallet === 'Sudo' && call.call === 'sudo'
  const children = call.pallet === 'Utility' && ['batch','batch_all','force_batch'].includes(call.call) ? call.params.calls as IntentDescriptor[] : single ? [call.params.call as IntentDescriptor] : []
  return children?.some(child => child && !(child instanceof Uint8Array) && containsAccountRegistration(child)) ?? false
}
export async function registrationShieldDefault(reader: IntentReader, call: IntentDescriptor): Promise<boolean> {
  if (!containsAccountRegistration(call)) return false
  const key = await reader.query('MevShield','NextKey',[])
  if (!(typeof key === 'string' || key instanceof Uint8Array || Array.isArray(key))) return false
  return rawBytes(key).length === 1184
}
export function parseRecipient(address: string, decode: (address: string) => Uint8Array, fromDescriptor: (descriptor: Uint8Array) => { ss58Address: string }, decodeAccount: (address: string) => Uint8Array, explicit?: Uint8Array): Recipient {
  const encoded = address.startsWith('tao_') || address.startsWith('bth2_')
  const descriptor = encoded ? decode(address) : explicit?.slice()
  if (encoded && explicit && hex(descriptor!) !== hex(explicit)) throw new Error('receiving address and descriptor disagree')
  if (!descriptor) return { account: address }
  const account = fromDescriptor(descriptor).ss58Address
  if (!encoded && hex(decodeAccount(account)) !== hex(decodeAccount(address))) throw new Error('descriptor does not match receiving account')
  // SS58 prefixes can differ. Callers compare decoded account IDs for bare addresses.
  return { account: encoded ? account : address, descriptor }
}
export async function registeredRecipient(reader: IntentReader, address: string): Promise<string> {
  const recipient = reader.parseRecipient?.(address) ?? { account: address }
  if (recipient.descriptor) {
    if (!reader.finalizedHead) throw new Error('finalized recipient lookup is required')
    const block = await reader.finalizedHead()
    const record = await reader.query('HashedAccounts', recipient.descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts', [recipient.account], block) as any
    if (!record || hex(descriptorBytes(record.descriptor)) !== hex(recipient.descriptor)) throw new Error('recipient registration is not finalized or differs from its descriptor')
  }
  return recipient.account
}
const targets: Record<string, string> = { transfer: 'dest_ss58', transfer_all: 'dest_ss58', announce_coldkey_swap: 'new_coldkey_ss58', swap_coldkey_announced: 'new_coldkey_ss58', swap_hotkey: 'new_hotkey_ss58', burned_register: 'hotkey_ss58', pow_register: 'hotkey_ss58', root_register: 'hotkey_ss58', register_subnet: 'hotkey_ss58', associate_hotkey: 'hotkey_ss58' }
/** Bind receiving descriptors before funds or ownership reach a rotating account. */
export async function prepareRecipients(op: string, args: Record<string, any>, reader: IntentReader, owner: string) {
  const normalized = { ...args }, guards: IntentDescriptor[] = []
  let deposit = 0n
  const sponsorships: Array<{recipient:string;depositRao:bigint}> = []
  const target = targets[op]
  for (const [name, value] of Object.entries(args)) {
    if (typeof value !== 'string' || !(name.endsWith('_ss58'))) continue
    const explicit = name === target && args.hashed_descriptor ? rawBytes(args.hashed_descriptor) : undefined
    if (!explicit && !value.startsWith('tao_') && !value.startsWith('bth2_')) continue
    if (!reader.parseRecipient || !reader.decodeAddress) throw new Error('receiving address support unavailable')
    const recipient = reader.parseRecipient(value, explicit)
    normalized[name] = recipient.account
    const descriptor = recipient.descriptor
    if (!descriptor) continue
    const fixed = descriptor[0] === 2 && [5, 7, 9].includes(descriptor[1])
    if (await reader.query('AdminUtils', 'HashedAccountsEnabled', []) !== true) throw new Error('protected accounts are disabled')
    if (descriptor[0] === 2 && !await reader.supportsPqProfile?.(descriptor[1])) throw new Error('runtime does not support receiving profile')
    if (fixed) continue
    const direct = ['swap_coldkey_announced','pow_register','execute_proxy_announced'].includes(op)
    const block = direct ? await reader.finalizedHead?.() : undefined
    if (direct && !block) throw new Error('finalized recipient lookup is required')
    const storage = descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts'
    let record = await reader.query('HashedAccounts', storage, [recipient.account], block) as any
    if (op === 'announce_coldkey_swap' && record) {
      const finalized = await reader.finalizedHead?.()
      if (!finalized) throw new Error('finalized recipient lookup is required')
      record = await reader.query('HashedAccounts', storage, [recipient.account], finalized)
      if (!record) throw new Error('coldkey destination registration must finalize first')
    }
    if (record && hex(descriptorBytes(record.descriptor)) !== hex(descriptor)) throw new Error('registered descriptor differs from recipient')
    if (op === 'associate_hotkey') {
      const pq = await reader.query('HashedAccounts','MlDsaAccounts',[owner]) || await reader.query('HashedAccounts','DerivedAuthorities',[owner])
      if (!pq && (descriptor[0] === 2 || !await reader.query('HashedAccounts','Accounts',[owner]))) throw new Error('hashed hotkey requires a compatible protected coldkey owner')
    }
    if (!record) {
      if (direct || name !== target || op === 'associate_hotkey') throw new Error('recipient must be registered and finalized first')
      const amount = await reader.constant?.('HashedAccounts', descriptor[0] === 2 ? 'MlDsaRegistrationDeposit' : 'RegistrationDeposit')
      if (amount == null) throw new Error('registration deposit unavailable')
      deposit += BigInt(amount as any)
      sponsorships.push({recipient:recipient.account,depositRao:BigInt(amount as any)})
    }
    if (!direct && !(op === 'announce_coldkey_swap' && record)) guards.push({ pallet: 'HashedAccounts', call: record ? 'check_registered' : 'register', params: { descriptor: registrationDescriptor(descriptor) } })
  }
  return { args: normalized, guards, deposit, sponsorships }
}
