import { evaluatePolicy } from './policy'
import { descriptorBytes, rawBytes, registrationDescriptor } from './protected-accounts'
import type { IntentDescriptor, IntentReader } from './intents'
import { quoteRegistrationDeposit, registrationDepositGuard } from './registration-deposit'

export interface Recipient { account: string; descriptor?: Uint8Array }
export function containsAccountRegistration(call: IntentDescriptor): boolean {
  const node = evaluatePolicy<{ found: boolean; fields: string[] }>('protected.registration_node', { pallet: call.pallet, call: call.call })
  if (node.found) return true
  return node.fields.some(field => {
    const value = call.params[field], children = field === 'calls' ? value as IntentDescriptor[] ?? [] : [value]
    return children.some(child => child && !(child instanceof Uint8Array) && containsAccountRegistration(child as IntentDescriptor))
  })
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
  if (encoded && explicit && evaluatePolicy('transaction.recipient_identity', { descriptor, explicit }) != null) throw new Error('receiving address and descriptor disagree')
  if (!descriptor) return { account: address }
  const account = fromDescriptor(descriptor).ss58Address
  if (!encoded && evaluatePolicy('transaction.recipient_identity', { account: decodeAccount(address), descriptor_account: decodeAccount(account) }) != null) throw new Error('descriptor does not match receiving account')
  // SS58 prefixes can differ. Callers compare decoded account IDs for bare addresses.
  return { account: encoded ? account : address, descriptor }
}
export async function registeredRecipient(reader: IntentReader, address: string): Promise<string> {
  const recipient = reader.parseRecipient?.(address) ?? { account: address }
  if (recipient.descriptor) {
    if (!reader.finalizedHead) throw new Error('finalized recipient lookup is required')
    const block = await reader.finalizedHead()
    if (!block) throw new Error('finalized recipient lookup is required')
    if (evaluatePolicy<boolean>('transaction.fixed_descriptor', { descriptor: recipient.descriptor, header_only: true })) {
      if (await reader.query('AdminUtils', 'HashedAccountsEnabled', [], block) !== true) throw new Error('protected accounts are disabled')
      if (!await reader.supportsPqProfile?.(recipient.descriptor[1])) throw new Error('runtime does not support receiving profile')
      return recipient.account
    }
    const record = await reader.query('HashedAccounts', recipient.descriptor[0] === 2 ? 'MlDsaAccounts' : 'Accounts', [recipient.account], block) as any
    if (!record || evaluatePolicy('transaction.recipient_identity', { descriptor: descriptorBytes(record.descriptor), explicit: recipient.descriptor }) != null) throw new Error('recipient registration is not finalized or differs from its descriptor')
  }
  return recipient.account
}

/** Bind receiving descriptors before funds or ownership reach a rotating account. */
export async function prepareRecipients(op: string, args: Record<string, any>, reader: IntentReader, owner: string) {
  const normalized = { ...args }, guards: IntentDescriptor[] = []
  let deposit = 0n
  const sponsorships: Array<{recipient:string;depositRao:bigint}> = []
  const target = evaluatePolicy<string | null>('transaction.receiving_target', { op })
  for (const [name, value] of Object.entries(args)) {
    if (typeof value !== 'string' || !(name.endsWith('_ss58'))) continue
    const explicit = name === target && args.hashed_descriptor ? rawBytes(args.hashed_descriptor) : undefined
    if (!explicit && !value.startsWith('tao_') && !value.startsWith('bth2_')) continue
    if (!reader.parseRecipient || !reader.decodeAddress) throw new Error('receiving address support unavailable')
    const recipient = reader.parseRecipient(value, explicit)
    normalized[name] = recipient.account
    const descriptor = recipient.descriptor
    if (!descriptor) continue
    const fixed = evaluatePolicy<boolean>('transaction.fixed_descriptor', { descriptor, header_only: true })
    if (await reader.query('AdminUtils', 'HashedAccountsEnabled', []) !== true) throw new Error('protected accounts are disabled')
    if (descriptor[0] === 2 && !await reader.supportsPqProfile?.(descriptor[1])) throw new Error('runtime does not support receiving profile')
    if (fixed) continue
    const direct = evaluatePolicy<boolean>('transaction.receiving_direct', { op })
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
    if (record && evaluatePolicy('transaction.recipient_identity', { descriptor: descriptorBytes(record.descriptor), explicit: descriptor }) != null) throw new Error('registered descriptor differs from recipient')
    if (op === 'associate_hotkey') {
      const pq = await reader.query('HashedAccounts','MlDsaAccounts',[owner]) || await reader.query('HashedAccounts','DerivedAuthorities',[owner])
      if (!evaluatePolicy<boolean>('transaction.protected_owner', { kind: pq ? 'mldsa' : await reader.query('HashedAccounts','Accounts',[owner]) ? 'classical' : null, requires_pq: descriptor[0] === 2 })) throw new Error('hashed hotkey requires a compatible protected coldkey owner')
    }
    if (!record) {
      if (!evaluatePolicy<{ may_register: boolean }>('transaction.receiving_registration', { op, field: name, direct }).may_register) throw new Error('recipient must be registered and finalized first')
      const quote = await quoteRegistrationDeposit(reader, descriptor[0] === 2 ? 'MlDsaRegistrationDeposit' : 'RegistrationDeposit')
      deposit += quote.amountRao
      sponsorships.push({recipient:recipient.account,depositRao:quote.amountRao})
      if (quote.configurable) guards.push(registrationDepositGuard(quote))
    }
    if (!direct && !(op === 'announce_coldkey_swap' && record)) guards.push({ pallet: 'HashedAccounts', call: record ? 'check_registered' : 'register', params: { descriptor: registrationDescriptor(descriptor) } })
  }
  return { args: normalized, guards, deposit, sponsorships }
}
