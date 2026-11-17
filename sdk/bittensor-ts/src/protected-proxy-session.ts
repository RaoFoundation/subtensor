import type { JsonStore } from './preferences'
import type { Intent } from './intents'
import { bytesHex } from './portable'
import { ProtectedAccountStore } from './protected-account-config'
import { ProxyAnnouncementStore } from './announcement-store'
import { ProtectedAnnouncements } from './protected-announcements'
import { ProtectedAccountSession } from './protected-account-lifecycle'
import type { ProtectedProxyBackend, ProtectedProxySubmission } from './protected-proxy-types'
import type { ProtectedProxyPublicKey } from './protected-proxy-adapter'

export interface ProtectedAccountClientOptions<Signer = unknown, TransactionOptions = unknown> {
  controllerWallet: string
  store: JsonStore
  /** Resolve the explicitly selected controller, allow key, or execution payer. */
  signerFor?(address: string): Signer | Promise<Signer>
  /** Supply this for controllers authorized by multisig or an external transaction service. */
  submit?(intent: Intent, request: ProtectedProxySubmission): Promise<unknown>
  /** Public descriptors for fee estimation; this callback is never used to sign. */
  publicKeyFor?(account: string): ProtectedProxyPublicKey | undefined | Promise<ProtectedProxyPublicKey | undefined>
  /** Supply an inclusion block for uncertain transactions older than the bounded history search. */
  receiptBlockFor?(extrinsicHash: string): string | null | undefined | Promise<string | null | undefined>
  receiptSearchBlocks?: number
  transactionOptions?: TransactionOptions
}

/** Bind an explicit persistence adapter. Creating a session performs no reads or submissions. */
export function createProtectedAccountSession(backend: ProtectedProxyBackend, store: JsonStore, controllerWallet: string): ProtectedAccountSession {
  const records = new ProtectedAccountStore(store, backend)
  const journal = new ProxyAnnouncementStore(store, address => bytesHex(backend.accountId(address)))
  return new ProtectedAccountSession(backend, records, new ProtectedAnnouncements(backend, journal), controllerWallet)
}
