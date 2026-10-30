import { publicKeyFromSs58 } from './keys'
import type { ChainSigner, SignerLike } from './client'

declare const permitBrand: unique symbol
export type SignerQueuePermit = { readonly [permitBrand]: true }
const permitOption = Symbol('internal signer queue permit')
const queues = new WeakMap<object, Map<string, Promise<unknown>>>()
const permits = new WeakMap<SignerQueuePermit, { client: object; account: string; active: boolean }>()

/** Forward a held queue permit only to the workflow's own nested client operations. */
export function signerQueueOptions<T extends object>(options: T, permit: SignerQueuePermit): T {
  return { ...options, [permitOption]: permit }
}

/** Serialize nonce-sensitive work across ordinary, shielded, and workflow calls. */
export async function runSignerQueue<T>(client: object, address: string, action: (permit: SignerQueuePermit) => Promise<T>, options?: object): Promise<T> {
  const account = publicKeyFromSs58(address).toString('hex')
  const inherited = (options as Record<symbol, SignerQueuePermit> | undefined)?.[permitOption]
  const held = inherited && permits.get(inherited)
  if (inherited && held?.active && held.client === client && held.account === account) return action(inherited)
  let queue = queues.get(client)
  if (!queue) { queue = new Map(); queues.set(client, queue) }
  const permit = Object.freeze({}) as SignerQueuePermit
  const state = { client, account, active: false }
  permits.set(permit, state)
  const prior = queue.get(account) ?? Promise.resolve()
  const next = prior.catch(() => {}).then(async () => {
    state.active = true
    try { return await action(permit) } finally { state.active = false }
  })
  queue.set(account, next)
  try { return await next } finally { if (queue.get(account) === next) queue.delete(account) }
}

export interface BoundSignerIdentity {
  address: string
  publicKey: Uint8Array
  cryptoType: number
  requiresMetadataProof: boolean
}

/** Fail before signing if an extension or hardware account changes after preflight. */
export function bindSignerIdentity(signer: SignerLike, identity: BoundSignerIdentity): ChainSigner {
  const original = signer as ChainSigner & { beginTwoStageSigning?(): void }
  const publicKey = Buffer.from(identity.publicKey)
  if (!publicKeyFromSs58(identity.address).equals(publicKey)) throw new TypeError('signer identity public key differs from its address')
  const bound: ChainSigner & { beginTwoStageSigning?(): void } = {
    address: identity.address, publicKey, cryptoType: identity.cryptoType, requiresMetadataProof: identity.requiresMetadataProof,
    async getAccount(context) {
      const current = original.getAccount ? await original.getAccount(context) : original
      const address = current.ss58Address ?? current.address
      const key = address == null ? null : publicKeyFromSs58(address)
      const cryptoType = current.cryptoType ?? original.cryptoType ?? 1
      const requiresMetadataProof = Boolean(current.requiresMetadataProof ?? current.requiresMetadataHash ?? original.requiresMetadataProof ?? original.requiresMetadataHash)
      if (key == null || !key.equals(publicKey) || current.publicKey != null && !Buffer.from(current.publicKey).equals(publicKey) || cryptoType !== identity.cryptoType || requiresMetadataProof !== identity.requiresMetadataProof) throw new Error('signer account changed after preflight')
      return { address: identity.address, publicKey, cryptoType: identity.cryptoType, requiresMetadataProof: identity.requiresMetadataProof }
    },
  }
  if (original.signBytes) bound.signBytes = (payload, context) => original.signBytes!.call(signer, payload, context)
  if (original.sign) bound.sign = (payload, context) => original.sign!.call(signer, payload, context)
  if (original.signRaw) bound.signRaw = request => original.signRaw!.call(signer, request)
  if (original.signPayload) bound.signPayload = (payload, context) => original.signPayload!.call(signer, payload, context)
  if (original.beginTwoStageSigning) bound.beginTwoStageSigning = () => original.beginTwoStageSigning!.call(signer)
  return Object.freeze(bound)
}
