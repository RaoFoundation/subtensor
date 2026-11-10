import { u64 } from './wallet-types'
import { rawBytes } from './protected-accounts'
import { bytesHex as hex, concatBytes } from './portable'
import { registeredRecipient } from './receiving'
import type { ProtectedWorkflowAdapter } from './protected-transactions'
import type { IntentDescriptor, IntentReader } from './intents'

function littleU64(value: bigint | number): Uint8Array {
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigUint64(0, u64(value), true)
  return bytes
}

export type VersionedOrder = { V1: Record<string, any> } | { V2: Record<string, any> } | { Authorized: { nonce: bigint | number | string; terms: { V1: Record<string, any> } | { V2: Record<string, any> } } }
export function orderTerms(order: any): Record<string, any> {
  if (!order || typeof order !== 'object' || Object.keys(order).length !== 1) throw new TypeError('expected one complete order variant')
  if ('Authorized' in order) {
    if (!order.Authorized || Object.keys(order.Authorized).sort().join(',') !== 'nonce,terms' || 'Authorized' in order.Authorized.terms) throw new TypeError('invalid Authorized order')
    u64(BigInt(order.Authorized.nonce))
    return orderTerms(order.Authorized.terms)
  }
  const terms = order.V1 ?? order.V2
  if (!terms || typeof terms !== 'object' || !('signer' in terms) || !('hotkey' in terms) || !('fee_recipient' in terms)) throw new TypeError('expected complete V1 or V2 order terms')
  if (!Number.isInteger(terms.netuid) || terms.netuid < 0 || terms.netuid > 65535) throw new RangeError('invalid order subnet')
  return terms
}
export function authorizedOrder(nonce: bigint | number | string, order: VersionedOrder): VersionedOrder {
  if ('Authorized' in order) throw new TypeError('expected original V1 or V2 terms')
  orderTerms(order)
  return { Authorized: { nonce: u64(BigInt(nonce)), terms: structuredClone(order) } }
}
export function validateSignedOrders(input: any[]): any[] {
  if (!Array.isArray(input) || !input.length) throw new TypeError('at least one signed order is required')
  let authorized = 0
  const rows = structuredClone(input)
  for (const row of rows) {
    if (Object.keys(row).sort().join(',') !== 'order,partial_fill,signature') throw new TypeError('signed order requires order, signature and partial_fill')
    orderTerms(row.order)
    const isAuthorized = 'Authorized' in row.order
    if (isAuthorized) authorized++
    if (row.partial_fill != null) row.partial_fill = u64(BigInt(row.partial_fill))
    if (row.signature === 'AccountApproved' || row.signature && Object.keys(row.signature).length === 1 && 'AccountApproved' in row.signature) {
      if (!isAuthorized) throw new TypeError('account approval requires an Authorized order')
      continue
    }
    if (!row.signature || typeof row.signature !== 'object' || Object.keys(row.signature).length !== 1) throw new TypeError('invalid order authorization')
    const [scheme, proof] = Object.entries(row.signature)[0] as [string, any]
    if (['Ed25519','Sr25519','Ecdsa'].includes(scheme)) {
      if (isAuthorized || rawBytes(proof).length !== (scheme === 'Ecdsa' ? 65 : 64)) throw new TypeError('invalid classical order signature')
      row.signature[scheme] = hex(rawBytes(proof)); continue
    }
    throw new TypeError('unknown order authorization')
  }
  if (authorized > 8) throw new RangeError('at most eight authorized orders can be relayed')
  return rows
}
export async function prepareOrderIntent(op: string, args: Record<string, any>, reader: IntentReader, owner: string) {
  const call = (pallet: string, name: string, params: Record<string, unknown>): IntentDescriptor => ({ pallet, call: name, params })
  if (op === 'authorize_order' || op === 'cancel_order') {
    const canonical = await orderForChain(reader,args.order), terms = orderTerms(canonical)
    if (reader.decodeAddress ? hex(reader.decodeAddress(terms.signer)) !== hex(reader.decodeAddress(owner)) : terms.signer !== owner) throw new Error('order owner must authorize this call')
    const order = authorizedOrder(args.nonce, canonical)
    const descriptor = call('LimitOrders', op, {order})
    if (args.order_data) {
      if (!reader.composeCall) throw new Error('runtime call encoding is required to check pinned order data')
      const data = await reader.composeCall(call('LimitOrders','authorize_order',{order}))
      if (data.length <= 11 || data[2] !== 2 || hex(data.slice(3,11)) !== hex(littleU64(BigInt(args.nonce))) || hex(data.slice(11)) !== hex(rawBytes(args.order_data))) throw new Error('pinned economic order bytes changed')
    }
    return { call: descriptor, spend: op === 'authorize_order' ? 'unbounded' as const : null, netuids:[terms.netuid] }
  }
  const orders = validateSignedOrders(args.orders)
  for (const row of orders) row.order = await orderForChain(reader,row.order)
  const netuids = [...new Set(orders.map(row => orderTerms(row.order).netuid))]
  if (op === 'execute_batched_orders') {
    if (args.should_fail !== true || args.netuid <= 0 || args.netuid > 65535 || netuids.some(n => n !== args.netuid)) throw new Error('netted orders must be atomic and target one non-root subnet')
    return { call: call('LimitOrders',op,{netuid:args.netuid,orders}), spend:null, netuids }
  }
  return { call: call('LimitOrders','execute_orders',{orders,should_fail:args.should_fail}), spend:null, netuids }
}

export async function orderForChain(reader: IntentReader, order: VersionedOrder): Promise<VersionedOrder> {
  const copy = structuredClone(order), terms = orderTerms(copy)
  for (const name of ['signer','hotkey']) terms[name] = reader.parseRecipient?.(terms[name]).account ?? terms[name]
  terms.fee_recipient = await registeredRecipient(reader,terms.fee_recipient)
  if (terms.relayer != null) terms.relayer = terms.relayer.map((address: string) => reader.parseRecipient?.(address).account ?? address)
  return copy
}
export class PreparedOrder {
  private readonly terms: VersionedOrder
  private readonly data: Uint8Array
  private readonly genesis: Uint8Array
  readonly nonce: bigint
  readonly owner: string
  readonly orderId: string
  constructor(nonce: bigint, order: VersionedOrder, encoded: Uint8Array, genesis: Uint8Array, ownerBytes: Uint8Array, hash: (bytes: Uint8Array) => Uint8Array) {
    this.nonce = u64(nonce); this.terms = structuredClone(order); this.data = encoded.slice(); this.genesis = genesis.slice()
    this.owner = orderTerms(order).signer
    this.orderId = hex(hash(concatBytes(new TextEncoder().encode('bittensor/ms/v2/order-id'),genesis,ownerBytes,littleU64(nonce),encoded)))
    Object.freeze(this)
  }
  get order(): VersionedOrder { return structuredClone(this.terms) }
  get encoded(): Uint8Array { return this.data.slice() }
  get genesisHash(): Uint8Array { return this.genesis.slice() }
  accountApproved(partialFill: bigint | number | null = null) {
    return {order:authorizedOrder(this.nonce,this.terms),signature:'AccountApproved',partial_fill:partialFill == null ? null : u64(partialFill)}
  }
}
/**
 * Pin order terms with a nonce above the owner's ApprovedOrders.highest_nonce.
 * An existing System account can hold eight live approvals. Cancellation or
 * expiry frees a slot on the next approval without resetting the nonce watermark.
 * Account reaping removes all approvals. Preparation does not reserve a slot.
 */
export async function prepareOrder(client: ProtectedWorkflowAdapter, nonce: bigint | number, order: VersionedOrder): Promise<PreparedOrder> {
  const reader = await client.intentReader(await client.finalizedHead()), canonical = await orderForChain(reader,order)
  const normalized = u64(nonce), data = await reader.composeCall({pallet:'LimitOrders',call:'authorize_order',params:{order:authorizedOrder(normalized,canonical)}})
  if (data.length <= 11 || data[2] !== 2 || hex(data.slice(3,11)) !== hex(littleU64(normalized))) throw new Error('incompatible authorized order layout')
  const genesis = rawBytes(await client.blockHash(0))
  if (genesis.length !== 32) throw new Error('invalid chain genesis')
  return new PreparedOrder(normalized,canonical,data.slice(11),genesis,client.decodeAddress(orderTerms(canonical).signer),client.hash)
}
