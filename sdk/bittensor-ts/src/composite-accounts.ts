import { isPqCrypto, isRotatingCrypto } from './wallet-types'
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
  if (!Number.isInteger(index) || index < 0 || index > 65535) throw new RangeError('derivative index must fit u16')
  if (mode != null && mode !== 'safe' && mode !== 'legacy') throw new RangeError('invalid account mode')
  const address = typeof parent === 'string' ? parent : parent.ss58Address
  mode ??= typeof parent !== 'string' && isPqCrypto(parent.cryptoType) && !isRotatingCrypto(parent.cryptoType) ? 'safe' : 'legacy'
  const protectedParent = mode === 'legacy' ? await protectedAccountRecord(client,address) : null
  const fn = mode === 'safe' ? 'as_safe_derivative' : protectedParent?.kind === 'pq' || protectedParent?.kind === 'derived' ? 'as_protected_derivative' : 'as_derivative'
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
  const eligible = signatories.length >= 2 && weak < threshold
  const descriptor = {Multisig:{signatories:[...signatories],threshold}}
  if (kind === 'derived') {
    const registered = record.descriptor?.Multisig
    if (!registered || Number(registered.threshold) !== threshold || registered.signatories.length !== signatories.length || registered.signatories.some((member: string,index: number) => hex(codec.decodeAddress(member)) !== hex(codec.decodeAddress(signatories[index])))) throw new Error('registered derived authority differs from multisig')
  }
  const call: IntentDescriptor = {pallet:'HashedAccounts',call:kind === 'derived' ? 'check_derived' : 'register_derived_sponsored',params:kind === 'derived' ? {descriptor} : {descriptor,deposit_payer:depositPayer}}
  return {address,eligible,registered:kind === 'derived',block,call:eligible && (kind === 'derived' || depositPayer) ? call : undefined}
}
