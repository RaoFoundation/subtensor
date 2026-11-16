import { evaluatePolicy } from './policy'
import type { ProtectedAccountClient, ProtectedKeypair } from './protected-accounts'
import type { IntentDescriptor } from './intents'
import { bytesHex as hex } from './portable'

export type AccountMode = 'legacy' | 'safe'
export async function protectedAccountRecord(client: ProtectedAccountClient, account: string, block?: string) {
  block ??= await client.finalizedHead()
  for (const [kind, storage] of [['classical','Accounts'],['pq','MlDsaAccounts'],['derived','DerivedAuthorities']] as const) {
    const record = await client.query('HashedAccounts',storage,[account],block)
    if (record != null) return {kind,record,block}
  }
  return {kind:null,record:null,block}
}
export async function derivativeCall(client: ProtectedAccountClient, parent: string | ProtectedKeypair, index: number, call: IntentDescriptor | Uint8Array, mode?: AccountMode): Promise<IntentDescriptor> {
  if (!Number.isSafeInteger(index)) throw new RangeError('derivative index must fit u16')
  const address = typeof parent === 'string' ? parent : parent.ss58Address
  const request = { index, mode, crypto_type: typeof parent === 'string' ? undefined : parent.cryptoType }
  const selected = evaluatePolicy<{ mode: AccountMode; call: string }>('transaction.derivative', request)
  const protectedParent = selected.mode === 'legacy' ? await protectedAccountRecord(client,address) : null
  const fn = evaluatePolicy<{ call: string }>('transaction.derivative', { ...request, pq_required: protectedParent?.kind === 'pq' || protectedParent?.kind === 'derived' }).call
  return {pallet:'Utility',call:fn,params:{index,call:structuredClone(call)}}
}
export async function multisigProtection(client: ProtectedAccountClient, signatories: string[], threshold: number, codec: { decodeAddress(address: string): Uint8Array; encodeAddress(bytes: Uint8Array): string; multisigAccount(members: string[], threshold: number): {address:string} }, depositPayer?: string) {
  const ids = signatories.map(codec.decodeAddress)
  if (new Set(ids.map(hex)).size !== ids.length || ids.length < 2 || ids.length > 100 || !Number.isInteger(threshold) || threshold < 1 || threshold > signatories.length) throw new RangeError('invalid multisig members or threshold')
  signatories = ids.sort((a,b) => hex(a).localeCompare(hex(b))).map(codec.encodeAddress)
  const block = await client.finalizedHead(), address = codec.multisigAccount(signatories,threshold).address
  if (await client.query('AdminUtils','HashedAccountsEnabled',[],block) !== true) throw new Error('protected accounts are disabled')
  let weak = 0
  for (const member of signatories) {
    const {kind} = await protectedAccountRecord(client,member,block)
    if (kind !== 'pq' && kind !== 'derived') weak++
  }
  const {kind,record} = await protectedAccountRecord(client,address,block)
  const eligible = evaluatePolicy<boolean>('transaction.multisig_eligible', { count: signatories.length, weak, threshold })
  const descriptor = {Multisig:{signatories:[...signatories],threshold}}
  if (kind === 'derived') {
    const registered = record.descriptor?.Multisig
    if (!registered || Number(registered.threshold) !== threshold || registered.signatories.length !== signatories.length || registered.signatories.some((member: string,index: number) => hex(codec.decodeAddress(member)) !== hex(codec.decodeAddress(signatories[index])))) throw new Error('registered derived authority differs from multisig')
  }
  const call: IntentDescriptor = {pallet:'HashedAccounts',call:kind === 'derived' ? 'check_derived' : 'register_derived_sponsored',params:kind === 'derived' ? {descriptor} : {descriptor,deposit_payer:depositPayer}}
  return {address,eligible,registered:kind === 'derived',block,call:eligible && (kind === 'derived' || depositPayer) ? call : undefined}
}
