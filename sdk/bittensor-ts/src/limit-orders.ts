import { evaluatePolicy } from './policy'
import { u64 } from './wallet-types'
import { rawBytes } from './protected-accounts'
import { bytesHex as hex } from './portable'
import { registeredRecipient } from './receiving'
import type { ProtectedWorkflowAdapter } from './protected-transactions'
import type { IntentDescriptor, IntentReader } from './intents'


export type VersionedOrder = { V1: Record<string, any> } | { V2: Record<string, any> } | { Authorized: { nonce: bigint | number | string; terms: { V1: Record<string, any> } | { V2: Record<string, any> } } }
export function orderTerms(order: any): Record<string, any> {
  evaluatePolicy('financial.orders.terms', { order })
  const terms = 'Authorized' in order ? order.Authorized.terms : order
  return terms.V1 ?? terms.V2
}
export function authorizedOrder(nonce: bigint | number | string, order: VersionedOrder): VersionedOrder {
  const result = evaluatePolicy<{ Authorized: { nonce: string } }>('financial.orders.authorized', { nonce, order })
  return { Authorized: { nonce: BigInt(result.Authorized.nonce), terms: structuredClone(order) as { V1: Record<string, any> } | { V2: Record<string, any> } } }
}
export function validateSignedOrders(input: any[]): any[] {
  const checked = evaluatePolicy<any[]>('financial.orders.signed', { orders: input })
  return checked.map((row, index) => ({ ...row, order: structuredClone(input[index].order), partial_fill: row.partial_fill == null ? null : BigInt(row.partial_fill) }))
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
      evaluatePolicy('financial.orders.pinned', { encoded: data, nonce: args.nonce, terms: rawBytes(args.order_data) })
    }
    return { call: descriptor, spend: op === 'authorize_order' ? 'unbounded' as const : null, netuids:[terms.netuid] }
  }
  const orders = validateSignedOrders(args.orders)
  for (const row of orders) row.order = await orderForChain(reader,row.order)
  const netuids = [...new Set(orders.map(row => orderTerms(row.order).netuid))]
  if (op === 'execute_batched_orders') {
    evaluatePolicy('financial.orders.netted', { should_fail: args.should_fail, netuid: args.netuid, netuids })
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
    this.orderId = hex(hash(rawBytes(evaluatePolicy<string>('financial.orders.transcript', { genesis, owner: ownerBytes, nonce, encoded }))))
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
  const terms = rawBytes(evaluatePolicy<string>('financial.orders.layout', { encoded: data, nonce: normalized }))
  const genesis = rawBytes(await client.blockHash(0))
  if (genesis.length !== 32) throw new Error('invalid chain genesis')
  return new PreparedOrder(normalized,canonical,terms,genesis,client.decodeAddress(orderTerms(canonical).signer),client.hash)
}
