import { evaluatePolicy } from './policy'
import { hexBytes } from './portable'
import { buildIntent, intentFromJSON, intentSignerRole, type Intent } from './intents'
import {
  ProtectedAccountStore, type ProtectedAccountRecord, type ProxyGrant,
  ALLOW_ITEM_TO_PROXY_TYPE, PROXY_TYPE_INTENT_OPS, allowKeyGrants, announcementProxyGrants,
  controllerAnyDelays, deriveLegacyPureSs58, instantProxyTypeFor,
  parseDelayToBlocks, protectedSetupPending, protectedManagementPath, assertProtectedManagementAllowed, resolveAllowItems, rotationComplete,
} from './protected-account-config'
import { type ProtectedAnnouncements, type ProtectedAnnouncementResult, type AnnouncementActionReceipt } from './protected-announcements'
import type { ProxyAnnouncement } from './announcement-store'
import { nestedDispatchError } from './receipt'
import { isDefiniteSubmissionRejection } from './transport'
import { controllerFunding, planControllerRefill, includeControllerRefill, fundController, type ControllerRefillPlan } from './protected-funding'
import { requireProtectedChain, sameProtectedAccount, shieldedUncertainReceipt, type ProtectedProxyBackend, type ProtectedProxySnapshot, type ProtectedProxyReceipt } from './protected-proxy-types'

const hex = (bytes: Uint8Array) => '0x' + Array.from(bytes, value => value.toString(16).padStart(2, '0')).join('')
const object = (value: unknown): Record<string, any> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, any> : {}
const unique = <T>(values: readonly T[]): T[] => [...new Set(values)]
const sessionQueues = new WeakMap<object, Map<string, Promise<unknown>>>()
const approvalOnly = (receipt: ProtectedProxyReceipt) => evaluatePolicy<{approval_only: boolean}>('protected.receipt_state', receipt).approval_only
const confirmed = (receipt: ProtectedProxyReceipt) => evaluatePolicy<{confirmed: boolean}>('protected.receipt_state', receipt).confirmed
function uncertainReceipt(error: unknown): ProtectedProxyReceipt | undefined {
  const shielded = shieldedUncertainReceipt(error)
  if (shielded) return shielded
  const value = object(error)
  return typeof value.extrinsicHash === 'string' && /^0x[0-9a-f]{64}$/i.test(value.extrinsicHash) ? { status: 'uncertain', success: null, events: [], extrinsicHash: value.extrinsicHash, blockHash: value.blockHash } : undefined
}
function integer(value: unknown, label: string): number {
  const number = Number(value)
  if (value == null || !Number.isSafeInteger(number) || number < 0 || number > 0xffffffff) throw new TypeError(`${label} must be a u32`)
  return number
}

export interface ProtectedPureProvenance {
  address: string
  spawner: string
  height: number
  ext_index: number
  index?: number
  account_mode?: 'legacy' | 'safe' | 'protected'
  block_hash?: string
}
export interface ProtectedSetupOptions {
  controllerAddress: string
  delay?: string | number
  allows?: readonly string[]
  allowKey?: string
  name?: string
  index?: number
  accountMode?: 'legacy' | 'safe'
  /** Reuse requires matching PureCreated provenance and sole-controller live grants. */
  reuse?: ProtectedPureProvenance
  /** Recover a previous submission; it is independently verified against chain events. */
  creationReceipt?: ProtectedProxyReceipt
  /** Retry only a creation with a definitive failed receipt; uncertain submissions stay reserved. */
  retryFailedCreation?: boolean
}
export interface ProtectedSetupPlan {
  genesisHash: string
  controllerAddress: string
  delayBlocks: number
  allowTypes: string[]
  allowKey?: string
  record?: ProtectedAccountRecord
  create?: Intent
  reuse?: ProtectedPureProvenance
  warnings: readonly string[]
}
export interface ProtectedSetupResult {
  status: 'ready' | 'pending' | 'uncertain' | 'awaiting_approval' | 'failed'
  record?: ProtectedAccountRecord
  receipt?: ProtectedProxyReceipt
}
export interface ProtectedSettingsOptions { delay?: number | string; allow?: readonly string[]; disallow?: readonly string[]; allowKey?: string }
export interface ProtectedSettingsAction { intent: Intent; category: string; metadata?: Record<string, unknown> }
export interface ProtectedSettingsPlan {
  record: ProtectedAccountRecord
  actions: ProtectedSettingsAction[]
  cancelIndices: number[]
  allowTypes: string[]
  disallowTypes: string[]
  delegate: string
  delayBlocks: number
}
export interface ProtectedRotationPlan { record: ProtectedAccountRecord; intent: Intent; toWallet: string; toAddress: string; migratedAllows: string[] }
export type ProtectedRoute = { kind: 'direct'; intent: Intent; signerAddress: string } | { kind: 'instant'; intent: Intent; signerAddress: string; proxyFor: string; forceProxyType: string } | { kind: 'delayed'; intent: Intent; signerAddress: string; record: ProtectedAccountRecord }
export interface ProtectedSubmitOptions {
  signerAddress?: string
  explicitProxy?: boolean
  /** Explicit permission to include a controller refill, bounded by this amount. */
  refill?: { maxRefillRao: bigint }
}

/**
 * Delayed pure-proxy lifecycle. Planning never writes local state or broadcasts.
 * Mutations recheck live authority and keep incomplete submissions resumable.
 */
export class ProtectedAccountSession {
  constructor(readonly backend: ProtectedProxyBackend, readonly records: ProtectedAccountStore, readonly announcements: ProtectedAnnouncements, readonly controllerWallet: string) {
    if (!controllerWallet.trim()) throw new TypeError('controller wallet name is required')
  }
  private change<T>(action: () => Promise<T>): Promise<T> {
    let queues = sessionQueues.get(this.records.store)
    if (!queues) { queues = new Map(); sessionQueues.set(this.records.store, queues) }
    const next = (queues.get(this.controllerWallet) ?? Promise.resolve()).catch(() => {}).then(action)
    queues.set(this.controllerWallet, next)
    return next.finally(() => { if (queues!.get(this.controllerWallet) === next) queues!.delete(this.controllerWallet) })
  }
  private same(left: string, right: string) { return sameProtectedAccount(this.backend, left, right) }
  private identity(address: string) { return this.backend.ss58Encode(this.backend.accountId(address)) }
  private async record(snapshot: ProtectedProxySnapshot, ready = true): Promise<ProtectedAccountRecord> {
    const record = await this.records.getByController(this.controllerWallet, snapshot.genesisHash)
    if (!record) {
      if (await this.records.getByController(this.controllerWallet)) throw new Error('legacy protected binding requires explicit network binding')
      throw new Error('controller wallet has no protected account on this chain')
    }
    requireProtectedChain(snapshot.genesisHash, String(record.genesis_hash))
    if (ready && protectedSetupPending(record)) throw new Error('protected-account setup is incomplete; resume setup first')
    return record
  }
  private async grants(snapshot: ProtectedProxySnapshot, address: string): Promise<ProxyGrant[]> {
    const value = await snapshot.query('Proxy', 'Proxies', [address])
    if (!Array.isArray(value) || !Array.isArray(value[0])) throw new TypeError('invalid live proxy grants')
    return value[0].map((row: any) => {
      const type = typeof row.proxy_type === 'string' ? row.proxy_type : Object.keys(object(row.proxy_type))[0]
      if (typeof row.delegate !== 'string' || !type) throw new TypeError('invalid live proxy grant')
      this.backend.accountId(row.delegate)
      return { delegate: row.delegate, proxy_type: type, delay: integer(row.delay, 'proxy delay') }
    })
  }
  private liveDelay(record: ProtectedAccountRecord, grants: ProxyGrant[]): number {
    return evaluatePolicy('protected.live_delay', { delays: controllerAnyDelays(this.backend, grants, record.controller_ss58) })
  }

  private refreshed(record: ProtectedAccountRecord, grants: ProxyGrant[]): ProtectedAccountRecord {
    const delegate = record.allow_key_ss58 || record.controller_ss58
    const allows = unique(allowKeyGrants(this.backend, grants, delegate).map(grant => grant.proxy_type).filter(type => type in PROXY_TYPE_INTENT_OPS))
    return { ...record, delay_blocks: this.liveDelay(record, grants), allows }
  }
  async inspect() {
    const snapshot = await this.backend.snapshot(), stored = await this.record(snapshot, false), grants = await this.grants(snapshot, stored.protected_ss58)
    const delays = controllerAnyDelays(this.backend, grants, stored.controller_ss58), observedDelayBlocks = delays.length ? Math.min(...delays) : null
    const protectionWarnings = observedDelayBlocks == null ? ['The recorded controller no longer holds Any authority.'] : observedDelayBlocks === 0 ? ['A zero-delay Any grant defeats the protected waiting window.'] : []
    const record = observedDelayBlocks != null && observedDelayBlocks > 0 ? this.refreshed(stored, grants) : stored
    const pending = await this.announcements.reconcile(record, { persist: false })
    return { record, grants, pending, block: snapshot.block, blockHash: snapshot.hash, observedDelayBlocks, protectionWarnings, setupPending: protectedSetupPending(record) }
  }
  async planSetup(options: ProtectedSetupOptions): Promise<ProtectedSetupPlan> {
    const snapshot = await this.backend.snapshot(), record = await this.records.getByController(this.controllerWallet, snapshot.genesisHash)
    const reservation = record ? undefined : await this.records.getSetup(this.controllerWallet, snapshot.genesisHash)
    const controller = this.identity(options.controllerAddress)
    if (!record && await this.records.getByController(this.controllerWallet)) throw new Error('bind the legacy protected record to its original network before setup')
    if (record && !this.same(record.controller_ss58, controller)) throw new Error('controller does not match interrupted setup')
    if (reservation && !this.same(reservation.controller_ss58, controller)) throw new Error('controller does not match creation reservation')
    if (record && !protectedSetupPending(record)) throw new Error('protected account is already configured; use updateSettings')
    const delay = parseDelayToBlocks(options.delay ?? record?.delay_blocks ?? reservation?.delay_blocks ?? '36h', snapshot.blockTime)
    integer(delay, 'controller delay')
    const prior = object(record?.setup).allow_types ?? record?.pending_allows ?? reservation?.allow_types ?? []
    const allowTypes = unique([...prior, ...resolveAllowItems([...(options.allows ?? [])])])
    const allowKey = options.allowKey ?? record?.allow_key_address ?? record?.allow_key_ss58 ?? reservation?.allow_key_ss58 ?? undefined
    if (allowKey) this.backend.accountId(allowKey)
    if (reservation && options.allowKey && !this.same(options.allowKey, reservation.allow_key_ss58 || controller)) throw new Error('finish creation before changing the allow key')
    if (record) {
      if (options.allowKey && !this.same(options.allowKey, record.allow_key_ss58 || controller)) throw new Error('finish setup before changing the allow key')
      if (delay !== this.liveDelay(record, await this.grants(snapshot, record.protected_ss58))) throw new Error('finish setup before changing controller delay')
    }
    return { genesisHash: snapshot.genesisHash, controllerAddress: controller, delayBlocks: delay, allowTypes, allowKey, record,
      create: record || options.reuse ? undefined : buildIntent('create_pure_proxy', { proxy_type: 'Any', delay, index: options.index ?? 0, account_mode: options.accountMode ?? 'legacy' }), reuse: options.reuse,
      warnings: ['Coldkey migration moves available TAO, stake and ownership to the protected account. Its controller needs a separately funded balance for fees and announcement deposits.'],
    }
  }
  private async verifyProvenance(proof: ProtectedPureProvenance, controller: string): Promise<void> {
    const height = integer(proof.height, 'pure creation height'), extIndex = integer(proof.ext_index, 'pure creation extrinsic index'), index = integer(proof.index ?? 0, 'pure index')
    if (!this.same(proof.spawner, controller)) throw new Error('pure spawner does not match the controller')
    if ((proof.account_mode ?? 'legacy') === 'legacy') {
      const expected = deriveLegacyPureSs58(this.backend, proof.spawner, { height, ext_index: extIndex, index })
      if (!this.same(expected, proof.address)) throw new Error('pure address does not match creation coordinates')
    }
    if (!this.backend.events) throw new Error('historical events are required to verify pure creation')
    const events = await this.backend.events(proof.block_hash ?? height)
    if (!events.some(event => event.module_id === 'Proxy' && event.event_id === 'PureCreated' && event.extrinsic_idx === extIndex && typeof event.attributes?.pure === 'string' && typeof event.attributes?.who === 'string' && this.same(event.attributes.pure, proof.address) && this.same(event.attributes.who, controller) && (event.attributes.proxy_type === 'Any' || Object.hasOwn(object(event.attributes.proxy_type), 'Any')) && integer(event.attributes.disambiguation_index, 'pure event index') === index)) throw new Error('matching PureCreated provenance is unavailable')
  }
  private proof(record: ProtectedAccountRecord): ProtectedPureProvenance {
    const proof = object(record.provenance)
    return { address: record.protected_ss58, spawner: String(proof.spawner ?? record.spawner ?? record.controller_ss58), height: proof.height ?? record.height, ext_index: proof.ext_index ?? record.ext_index, index: proof.index ?? record.index ?? 0, account_mode: proof.account_mode ?? record.account_mode ?? 'legacy', block_hash: proof.block_hash }
  }
  private validateSetupGrants(record: ProtectedAccountRecord, grants: ProxyGrant[], allows: readonly string[]): void {
    evaluatePolicy('protected.validate_setup_grants', { controller: hex(this.backend.accountId(record.controller_ss58)), allow_key: record.allow_key_ss58 ? hex(this.backend.accountId(record.allow_key_ss58)) : null, delay_blocks: record.delay_blocks, allows, grants: grants.map(grant => ({ ...grant, delegate: hex(this.backend.accountId(grant.delegate)) })) })
  }

  setup(options: ProtectedSetupOptions): Promise<ProtectedSetupResult> { return this.change(() => this.setupInternal(options)) }
  resumeSetup(options: ProtectedSetupOptions): Promise<ProtectedSetupResult> { return this.setup(options) }
  private async setupInternal(options: ProtectedSetupOptions): Promise<ProtectedSetupResult> {
    const plan = await this.planSetup(options)
    let record = plan.record, receipt: ProtectedProxyReceipt | undefined
    if (!record) {
      let proof = plan.reuse
      const saved = await this.records.getSetup(this.controllerWallet, plan.genesisHash)
      if (proof && saved && !options.creationReceipt) throw new Error('resolve the existing creation reservation before reusing another pure')
      if (!proof) {
        if (saved && (saved.controller_ss58 !== plan.controllerAddress || saved.delay_blocks !== plan.delayBlocks)) throw new Error('creation reservation does not match this setup')
        if (saved) {
          receipt = options.creationReceipt ?? (saved.extrinsic_hash && this.backend.receipt ? await this.backend.receipt(saved.extrinsic_hash, saved.block_hash) ?? undefined : undefined) ?? saved.receipt
          if (options.retryFailedCreation && receipt?.success === false && ['failed', 'in_block', 'finalized', 'finalised'].includes(receipt.status)) {
            const allows = plan.allowTypes.map(type => {
              const token = Object.entries(ALLOW_ITEM_TO_PROXY_TYPE).find(([, value]) => value === type)?.[0]
              if (!token) throw new Error(`unreviewed setup permission ${type}`)
              return token
            })
            await this.records.removeSetup(this.controllerWallet, plan.genesisHash)
            return this.setupInternal({ ...options, delay: plan.delayBlocks, allows, allowKey: plan.allowKey, index: saved.index, accountMode: saved.account_mode === 'safe' ? 'safe' : 'legacy', creationReceipt: undefined, retryFailedCreation: false })
          }
          if (!receipt || !confirmed(receipt)) return { status: receipt && approvalOnly(receipt) ? 'awaiting_approval' : receipt?.success === false ? 'failed' : 'uncertain', receipt }
        } else {
          if (options.creationReceipt) throw new Error('creation receipt requires a saved setup reservation')
          const reserved = await this.records.reserveSetup({ controller_wallet: this.controllerWallet, controller_ss58: plan.controllerAddress, genesis_hash: plan.genesisHash, delay_blocks: plan.delayBlocks, index: options.index ?? 0, allow_types: plan.allowTypes, allow_key_ss58: plan.allowKey, account_mode: options.accountMode ?? 'legacy', status: 'submitting' })
          if (!reserved.created) return { status: reserved.reservation.status === 'awaiting_approval' ? 'awaiting_approval' : 'uncertain', receipt: reserved.reservation.receipt }
          try { receipt = await this.backend.submit(plan.create!, { signerAddress: plan.controllerAddress, purpose: 'create protected pure proxy' }) }
          catch (error) {
            if (isDefiniteSubmissionRejection(error)) await this.records.removeSetup(this.controllerWallet, plan.genesisHash)
            else {
              const receipt = uncertainReceipt(error)
              await this.records.saveSetup({ ...(await this.records.getSetup(this.controllerWallet, plan.genesisHash))!, status: 'uncertain', receipt, extrinsic_hash: receipt?.extrinsicHash ?? undefined, block_hash: receipt?.blockHash ?? undefined })
            }
            throw error
          }
          await this.records.saveSetup({ ...(await this.records.getSetup(this.controllerWallet, plan.genesisHash))!, status: approvalOnly(receipt) ? 'awaiting_approval' : confirmed(receipt) ? 'submitting' : receipt.success === false ? 'failed' : 'uncertain', extrinsic_hash: receipt.extrinsicHash ?? undefined, block_hash: receipt.blockHash ?? undefined, receipt })
          if (!confirmed(receipt)) return { status: approvalOnly(receipt) ? 'awaiting_approval' : receipt.success === false ? 'failed' : 'uncertain', receipt }
        }
        const created = receipt.events.filter(event => event.module_id === 'Proxy' && event.event_id === 'PureCreated' && typeof event.attributes?.who === 'string' && this.same(event.attributes.who, plan.controllerAddress))
        if (created.length !== 1 || typeof created[0].attributes?.pure !== 'string') throw new Error('cannot recover a unique PureCreated; setup reservation retained')
        const event = created[0], pure = String(event.attributes.pure), reservation = await this.records.getSetup(this.controllerWallet, plan.genesisHash)
        // Protected creators may be automatically routed to create_protected_pure.
        const snapshot = await this.backend.snapshot()
        const derived = await snapshot.query('HashedAccounts', 'DerivedAuthorities', [pure])
        proof = { address: pure, spawner: plan.controllerAddress, height: integer(receipt.blockNumber, 'creation block'), ext_index: integer(event.extrinsic_idx ?? receipt.extrinsicIndex, 'creation extrinsic'), index: reservation?.index ?? options.index ?? 0, account_mode: derived ? 'protected' : reservation?.account_mode ?? options.accountMode ?? 'legacy', block_hash: receipt.blockHash ?? undefined }
      }
      await this.verifyProvenance(proof, plan.controllerAddress)
      record = { controller_wallet: this.controllerWallet, controller_ss58: plan.controllerAddress, protected_ss58: proof.address, genesis_hash: plan.genesisHash, delay_blocks: plan.delayBlocks, name: options.name ?? `${this.controllerWallet}-protected`, allows: [], reviewed_allows: [], pending_allows: [], allow_key_ss58: plan.allowKey ? this.identity(plan.allowKey) : undefined, allow_key_address: plan.allowKey, provenance: proof, setup: { status: 'pending', allow_types: plan.allowTypes }, coldkey_swap: { status: 'pending' } }
      this.validateSetupGrants(record, await this.grants(await this.backend.snapshot(), record.protected_ss58), [])
      record = await this.records.bindCreated(record)
      await this.records.removeSetup(this.controllerWallet, plan.genesisHash)
    } else await this.verifyProvenance(this.proof(record), plan.controllerAddress)
    record = await this.records.patch(this.controllerWallet, plan.genesisHash, { setup: { status: 'pending', allow_types: plan.allowTypes } })
    const grants = await this.grants(await this.backend.snapshot(), record.protected_ss58)
    this.validateSetupGrants(record, grants, plan.allowTypes)
    const delegate = plan.allowKey || record.controller_ss58, pending = await this.announcements.reconcile(record)
    const active = allowKeyGrants(this.backend, grants, delegate).map(grant => grant.proxy_type)
    for (const proxyType of plan.allowTypes) {
      if (active.includes(proxyType)) continue
      const existing = pending.find(row => this.rowGrants(row, delegate, proxyType))
      if (!existing) {
        const result = await this.announcements.announce(record, buildIntent('add_proxy', { delegate_ss58: delegate, proxy_type: proxyType, delay: 0, from_hotkey: false }), { category: `allow:${proxyType}`, allowManagement: true })
        pending.push(result.row)
        if (result.receipt && result.receipt.success === false) return { status: 'failed', record, receipt: result.receipt }
      }
    }
    record = await this.records.patch(this.controllerWallet, plan.genesisHash, { allows: active, reviewed_allows: unique([...(record.reviewed_allows ?? []), ...plan.allowTypes]), pending_allows: plan.allowTypes.filter(type => !active.includes(type)) })
    if (pending.some(row => row.status === 'awaiting_approval' || row.status === 'submitting')) return { status: pending.some(row => row.status === 'awaiting_approval') ? 'awaiting_approval' : 'uncertain', record }
    return this.announceMigration(record)
  }
  private async migration(snapshot: ProtectedProxySnapshot, record: ProtectedAccountRecord) {
    requireProtectedChain(snapshot.genesisHash, String(record.genesis_hash))
    const [value, dispute, policy] = await Promise.all([
      snapshot.query('SubtensorModule', 'ColdkeySwapAnnouncements', [record.controller_ss58]),
      snapshot.query('SubtensorModule', 'ColdkeySwapDisputes', [record.controller_ss58]),
      snapshot.query('SubtensorModule', 'ColdkeySwapV2PolicySnapshots', [record.controller_ss58]),
    ])
    if (policy != null && typeof policy !== 'boolean') throw new TypeError('invalid coldkey policy snapshot')
    if (dispute != null) throw new Error('coldkey migration is disputed or recovery locked')
    if (value == null) return undefined
    if (!Array.isArray(value) || value.length !== 2) throw new TypeError('invalid coldkey migration announcement')
    const account = this.backend.accountId(record.protected_ss58)
    const expected = evaluatePolicy<string[]>('protected.migration_preimages', { account, policy }).map(preimage => hex(this.backend.hash(hexBytes(preimage))))
    if (!expected.includes(String(value[1]).toLowerCase())) throw new Error('controller has a different coldkey migration')
    return { execute_block: integer(value[0], 'migration execution block'), new_coldkey_hash: String(value[1]), import_source_policies: policy, ready: snapshot.block >= Number(value[0]) }
  }
  private async announceMigration(record: ProtectedAccountRecord): Promise<ProtectedSetupResult> {
    let live = await this.migration(await this.backend.snapshot(), record), swap = object(record.coldkey_swap)
    if (!live && swap.receipt?.extrinsicHash && this.backend.receipt && ['submitting', 'uncertain', 'awaiting_approval'].includes(swap.status)) {
      let recovered: ProtectedProxyReceipt | null = null
      try { recovered = await this.backend.receipt(swap.receipt.extrinsicHash, swap.receipt.blockHash ?? undefined) } catch { /* Unknown inclusion must not cause another migration announcement. */ }
      if (recovered && (recovered.blockHash != null || recovered.blockNumber != null) && !['uncertain', 'unknown', 'submitted', 'pending'].includes(recovered.status)) {
        const failed = recovered.success === false || nestedDispatchError([...recovered.events]) != null
        const nextSwap = { ...swap, receipt: recovered, status: failed ? 'failed' : approvalOnly(recovered) ? 'awaiting_approval' : confirmed(recovered) ? 'announced' : swap.status }
        record = await this.records.patch(this.controllerWallet, String(record.genesis_hash), { coldkey_swap: nextSwap }, { status: swap.status, extrinsicHash: swap.receipt.extrinsicHash })
        swap = object(record.coldkey_swap)
        live = await this.migration(await this.backend.snapshot(), record)
      }
    }
    if (!live && ['submitting', 'uncertain', 'awaiting_approval', 'announced'].includes(swap.status)) return { status: swap.status === 'awaiting_approval' ? 'awaiting_approval' : 'uncertain', record }
    if (!live) {
      const reserved = await this.records.reserveMigration(this.controllerWallet, String(record.genesis_hash), record.protected_ss58)
      record = reserved.record
      if (!reserved.created) return { status: record.coldkey_swap?.status === 'awaiting_approval' ? 'awaiting_approval' : record.coldkey_swap?.status === 'executed' ? 'ready' : 'uncertain', record }
      let receipt: ProtectedProxyReceipt
      try { receipt = await this.backend.submit(buildIntent('announce_coldkey_swap', { new_coldkey_ss58: record.protected_ss58 }), { signerAddress: record.controller_ss58, purpose: 'announce protected coldkey migration' }) }
      catch (error) { await this.records.patch(this.controllerWallet, String(record.genesis_hash), { coldkey_swap: { ...object(record.coldkey_swap), status: isDefiniteSubmissionRejection(error) ? 'failed' : 'uncertain', receipt: uncertainReceipt(error) } }); throw error }
      record = await this.records.patch(this.controllerWallet, String(record.genesis_hash), { coldkey_swap: { ...object(record.coldkey_swap), status: approvalOnly(receipt) ? 'awaiting_approval' : confirmed(receipt) ? 'announced' : receipt.success === false ? 'failed' : 'uncertain', receipt } })
      if (!confirmed(receipt)) return { status: approvalOnly(receipt) ? 'awaiting_approval' : receipt.success === false ? 'failed' : 'uncertain', record, receipt }
      live = await this.migration(await this.backend.snapshot(), record)
      if (!live) return { status: 'uncertain', record, receipt }
    }
    record = await this.records.patch(this.controllerWallet, String(record.genesis_hash), { coldkey_swap: { ...object(record.coldkey_swap), ...live, status: 'announced', new_coldkey_ss58: record.protected_ss58 }, setup: { ...object(record.setup), status: 'ready' } })
    return { status: 'ready', record }
  }
  async migrationStatus() { const snapshot = await this.backend.snapshot(), record = await this.record(snapshot, false); return { record, announcement: await this.migration(snapshot, record), block: snapshot.block } }
  executeMigration(): Promise<ProtectedProxyReceipt> { return this.change(async () => {
    const snapshot = await this.backend.snapshot(), record = await this.record(snapshot), live = await this.migration(snapshot, record)
    if (!live?.ready) throw new Error('protected coldkey migration is not ready')
    const receipt = await this.backend.submit(buildIntent('swap_coldkey_announced', { new_coldkey_ss58: record.protected_ss58 }), { signerAddress: record.controller_ss58, purpose: 'execute protected coldkey migration' })
    if (confirmed(receipt)) await this.records.upsert({ ...record, coldkey_swap: { ...object(record.coldkey_swap), status: 'executed', receipt } })
    return receipt
  }) }
  async route(intent: Intent, options: { signerAddress?: string; explicitProxy?: boolean } = {}): Promise<ProtectedRoute> {
    const snapshot = await this.backend.snapshot(), stored = await this.record(snapshot), signer = options.signerAddress ?? stored.controller_ss58
    const gate = evaluatePolicy<string>('protected.route_gate', { intent: intent.toJSON(), hotkey: intentSignerRole(intent) === 'hotkey', explicit_signer: options.signerAddress != null, explicit_proxy: options.explicitProxy })
    if (gate === 'direct') return { kind: 'direct', intent, signerAddress: signer }
    const record = this.refreshed(stored, await this.grants(snapshot, stored.protected_ss58))
    const route = evaluatePolicy<{kind: 'instant' | 'delayed'; proxy_type?: string}>('protected.route', { op: intent.op, intent: intent.toJSON(), allows: record.allows, allow_key: record.allow_key_ss58 ? hex(this.backend.accountId(record.allow_key_ss58)) : null, dispatcher: hex(this.backend.accountId(signer)), controller: hex(this.backend.accountId(record.controller_ss58)) })
    if (route.kind === 'instant') return { kind: 'instant', intent, signerAddress: signer, proxyFor: record.protected_ss58, forceProxyType: route.proxy_type! }
    return { kind: 'delayed', intent, signerAddress: signer, record }
  }

  async funding(options: { feePayer?: string } = {}) {
    const snapshot = await this.backend.snapshot(), record = await this.record(snapshot, false)
    return controllerFunding(this.backend, record, { ...options, snapshot })
  }
  async planRefill(intent: Intent, options: { maxRefillRao?: bigint; enabled?: boolean } = {}) {
    const snapshot = await this.backend.snapshot(), record = await this.record(snapshot)
    const [funding, pending] = await Promise.all([controllerFunding(this.backend, record, { snapshot }), this.announcements.reconcile(record, { persist: false })])
    const refill: ControllerRefillPlan = planControllerRefill(this.backend, record, intent, funding, { ...options, pending })
    return { funding, refill, intent: includeControllerRefill(intent, refill) }
  }
  fund(fromAddress: string, options: { maxFundingRao?: bigint; expectedFundingRao?: bigint } = {}) { return this.change(async () => {
    const record = await this.record(await this.backend.snapshot())
    return fundController(this.backend, record, fromAddress, options)
  }) }
  submit(intent: Intent, options: ProtectedSubmitOptions = {}): Promise<ProtectedProxyReceipt | ProtectedAnnouncementResult> { return this.change(async () => {
    let route = await this.route(intent, options)
    if (options.refill != null) {
      if (typeof options.refill.maxRefillRao !== 'bigint' || options.refill.maxRefillRao < 0n) throw new TypeError('explicit refill requires nonnegative bigint maxRefillRao')
      if (route.kind === 'direct') throw new Error('controller refill requires a protected delayed operation')
      const plan = await this.planRefill(intent, options.refill)
      if (plan.refill.reason === 'external_funding_required') throw new Error('controller cannot afford an announcement; fund it from a separate account first')
      if (plan.refill.reason === 'unknown_pending_payload') throw new Error('pending announcement plaintext is unknown; refuse a potentially duplicate controller refill')
      if (plan.refill.offered) {
        route = await this.route(plan.intent, options)
        if (route.kind !== 'delayed') throw new Error('a controller refill must use the delayed Any path')
      }
    }
    return route.kind === 'delayed' ? this.announcements.announce(route.record, route.intent) : this.backend.submit(route.intent, route)
  }) }
  private rowGrants(row: ProxyAnnouncement, delegate: string, type: string): boolean {
    return announcementProxyGrants(row).some(grant => grant.delay === 0 && grant.proxy_type === type && grant.delegate_ss58 != null && this.same(grant.delegate_ss58, delegate))
  }
  async planSettings(options: ProtectedSettingsOptions = {}): Promise<ProtectedSettingsPlan> {
    const snapshot = await this.backend.snapshot(), stored = await this.record(snapshot), grants = await this.grants(snapshot, stored.protected_ss58), record = this.refreshed(stored, grants)
    const pending = await this.announcements.reconcile(record, { persist: false })
    const allow = resolveAllowItems([...(options.allow ?? [])]), disallow = resolveAllowItems([...(options.disallow ?? [])])
    const oldDelegate = record.allow_key_ss58 || record.controller_ss58, delegate = options.allowKey ?? record.allow_key_address ?? oldDelegate
    const id = (address: string) => hex(this.backend.accountId(address))
    const delay = options.delay == null ? record.delay_blocks : parseDelayToBlocks(options.delay, snapshot.blockTime)
    const plan = evaluatePolicy<{actions: Array<{intent: Record<string, any>; category: string; metadata?: Record<string, unknown>}>; cancel_indices: number[]}>('protected.settings_plan', { record, allow, disallow, old_delegate: id(oldDelegate), delegate: id(delegate), delegate_address: delegate, delay_blocks: delay, controller_delays: controllerAnyDelays(this.backend, grants, record.controller_ss58), grants: grants.map(grant => ({ ...grant, delegate: id(grant.delegate) })), pending: pending.map(row => ({ ...row, grants: announcementProxyGrants(row).map(grant => ({ ...grant, delegate_ss58: grant.delegate_ss58 ? id(grant.delegate_ss58) : null })) })) })
    return { record, actions: plan.actions.map(action => ({ ...action, intent: intentFromJSON(action.intent) })), cancelIndices: plan.cancel_indices, allowTypes: allow, disallowTypes: disallow, delegate, delayBlocks: delay }
  }

  updateSettings(options: ProtectedSettingsOptions = {}): Promise<{ record: ProtectedAccountRecord; results: ProtectedAnnouncementResult[] }> { return this.change(async () => {
    let plan = await this.planSettings(options)
    for (const index of plan.cancelIndices) {
      const result = await this.announcements.cancel(plan.record, index)
      if (!result.completed) throw new Error(`grant announcement ${index} is not confirmed cancelled; revocation was not queued`)
    }
    // Re-read after cancellations: an executor may have installed a grant meanwhile.
    plan = await this.planSettings(options)
    if (plan.cancelIndices.length) throw new Error('new pending grant commitments appeared; retry the settings review')
    let record: ProtectedAccountRecord = { ...plan.record, reviewed_allows: unique([...(plan.record.reviewed_allows ?? plan.record.allows ?? []), ...plan.allowTypes]), allow_key_ss58: this.identity(plan.delegate), allow_key_address: plan.delegate }
    await this.records.upsert(record)
    const results: ProtectedAnnouncementResult[] = []
    for (const action of plan.actions) {
      const result = await this.announcements.announce(record, action.intent, { category: action.category, metadata: action.metadata, allowManagement: true })
      results.push(result)
      if (action.category === 'delay-change') record = { ...record, pending_delay_blocks: plan.delayBlocks, pending_delay_index: result.row.index }
      if (action.category.startsWith('allow:')) record = { ...record, pending_allows: unique([...(record.pending_allows ?? []), action.category.slice(6)]) }
      await this.records.upsert(record)
      if (result.receipt && !confirmed(result.receipt)) break
    }
    return { record, results }
  }) }
  async planRotation(toWallet: string, toAddress: string): Promise<ProtectedRotationPlan> {
    const snapshot = await this.backend.snapshot(), stored = await this.record(snapshot), grants = await this.grants(snapshot, stored.protected_ss58), record = this.refreshed(stored, grants)
    if (!toWallet.trim() || toWallet === this.controllerWallet || this.same(toAddress, record.controller_ss58)) throw new Error('rotation requires a different controller wallet and account')
    const destination = await this.records.getByController(toWallet, snapshot.genesisHash)
    if (destination && !this.same(destination.protected_ss58, record.protected_ss58)) throw new Error('destination wallet already controls another protected account')
    const pending = await this.announcements.reconcile(record, { persist: false })
    if (pending.length) throw new Error('execute or cancel all old-controller announcements before rotation')
    const plan = evaluatePolicy<{ children: Record<string, any>[]; migrated: string[] }>('protected.rotation_plan', { record, controller: hex(this.backend.accountId(record.controller_ss58)), allow_key: record.allow_key_ss58 ? hex(this.backend.accountId(record.allow_key_ss58)) : null, allow_address: record.allow_key_address, new_address: toAddress, grants: grants.map(grant => ({ ...grant, address: grant.delegate, delegate: hex(this.backend.accountId(grant.delegate)) })) })
    return { record, intent: buildIntent('batch', { intents: plan.children.map(intentFromJSON) }), toWallet, toAddress, migratedAllows: plan.migrated }
  }

  rotate(toWallet: string, toAddress: string): Promise<ProtectedAnnouncementResult> { return this.change(async () => {
    const plan = await this.planRotation(toWallet, toAddress)
    const result = await this.announcements.announce(plan.record, plan.intent, { category: 'rotate-controller', allowManagement: true, metadata: { rotate_to_wallet: toWallet, rotate_to_ss58: this.identity(toAddress), rotate_migrated_allows: plan.migratedAllows } })
    await this.records.upsert({ ...plan.record, pending_rotate: { index: result.row.index, to_wallet: toWallet, to_ss58: this.identity(toAddress), migrated_allows: plan.migratedAllows } })
    return result
  }) }
  finalizeRotation(toWallet: string, toAddress: string): Promise<ProtectedAccountRecord> { return this.change(async () => {
    const snapshot = await this.backend.snapshot(), source = await this.records.getByController(this.controllerWallet, snapshot.genesisHash), destination = await this.records.getByController(toWallet, snapshot.genesisHash)
    let record = source
    if (!record) {
      const marker = object(destination?.rotation_rebind)
      if (!destination || marker.from_wallet !== this.controllerWallet || marker.to_wallet !== toWallet || typeof marker.from_ss58 !== 'string' || typeof marker.to_ss58 !== 'string' || !this.same(marker.to_ss58, toAddress)) throw new Error('no matching resumable controller rotation')
      record = { ...destination, controller_ss58: marker.from_ss58 }
    }
    requireProtectedChain(snapshot.genesisHash, String(record.genesis_hash))
    const grants = await this.grants(snapshot, record.protected_ss58)
    if (!rotationComplete(this.backend, grants, { new_controller_ss58: toAddress, old_controller_ss58: record.controller_ss58, delay_blocks: record.delay_blocks })) throw new Error('rotation is incomplete: new delayed Any missing or old controller retains authority')
    if (destination && !this.same(destination.protected_ss58, record.protected_ss58)) throw new Error('destination wallet already controls another protected account')
    const marker = { from_wallet: this.controllerWallet, from_ss58: record.controller_ss58, to_wallet: toWallet, to_ss58: this.identity(toAddress), status: 'pending' }
    if (source) await this.records.upsert({ ...source, rotation_rebind: marker })
    const moved = source ? await this.records.moveController(this.controllerWallet, toWallet, { controller_ss58: this.identity(toAddress), genesis_hash: snapshot.genesisHash }) : destination
    if (!moved) throw new Error('protected binding disappeared')
    delete moved.pending_rotate
    if (!moved.allow_key_ss58 || this.same(moved.allow_key_ss58, record.controller_ss58)) { moved.allow_key_ss58 = this.identity(toAddress); delete moved.allow_key_address }
    moved.pending_allows = []
    const refreshed = { ...this.refreshed(moved, grants), rotation_rebind: marker }
    await this.records.upsert(refreshed)
    await this.announcements.journal.moveWallet(this.controllerWallet, toWallet, { genesisHash: snapshot.genesisHash })
    const completed = { ...refreshed, rotation_rebind: { ...marker, status: 'complete' } }
    await this.records.upsert(completed)
    return completed
  }) }
  bindLegacy(): Promise<ProtectedAccountRecord> { return this.change(async () => {
    const snapshot = await this.backend.snapshot(), record = await this.records.getByController(this.controllerWallet)
    if (!record) throw new Error('no legacy protected binding')
    if (await this.records.getByController(this.controllerWallet, snapshot.genesisHash)) throw new Error('a scoped protected binding already exists')
    await this.verifyProvenance(this.proof(record), record.controller_ss58)
    this.validateSetupGrants(record, await this.grants(snapshot, record.protected_ss58), record.reviewed_allows ?? record.allows ?? [])
    await this.announcements.journal.bindLegacy(this.controllerWallet, snapshot.genesisHash, record.protected_ss58)
    const bound = await this.records.bindLegacy(this.controllerWallet, snapshot.genesisHash)
    if (!bound) throw new Error('legacy protected binding disappeared')
    return bound
  }) }
  execute(index: number, options: { signerAddress?: string; allowRepeat?: boolean } = {}): Promise<ProtectedAnnouncementResult> { return this.change(async () => {
    const record = await this.record(await this.backend.snapshot()), row = await this.announcements.inspect(record, index)
    const intent = intentFromJSON({ ...object(row.inner_args), op: row.inner_op }), management = protectedManagementPath(intent)
    assertProtectedManagementAllowed(row, intent)
    const result = await this.announcements.execute(record, index, { ...options, allowManagement: Boolean(management) })
    if (result.completed) await this.refreshRecord(record)
    return result
  }) }
  cancel(index: number): Promise<ProtectedAnnouncementResult> { return this.change(async () => {
    const record = await this.record(await this.backend.snapshot(), false), result = await this.announcements.cancel(record, index)
    if (result.completed) await this.refreshRecord(record)
    return result
  }) }
  /** Supply the final transaction when another multisig member completed an action. */
  recoverAction(index: number, receipt: AnnouncementActionReceipt): Promise<ProtectedAnnouncementResult> { return this.change(async () => {
    const record = await this.record(await this.backend.snapshot(), false)
    const result = await this.announcements.recoverAction(record, index, receipt)
    await this.refreshRecord(record)
    return result
  }) }
  private async refreshRecord(record: ProtectedAccountRecord): Promise<void> {
    const snapshot = await this.backend.snapshot(), grants = await this.grants(snapshot, record.protected_ss58)
    // A completed controller rotation must be finalized against its explicit destination.
    if (!controllerAnyDelays(this.backend, grants, record.controller_ss58).length) return
    const latest = await this.records.getByController(this.controllerWallet, snapshot.genesisHash)
    if (!latest || !this.same(latest.protected_ss58, record.protected_ss58)) return
    const pending = await this.announcements.reconcile(latest), delegate = latest.allow_key_ss58 || latest.controller_ss58
    const flags = unique(pending.flatMap(row => announcementProxyGrants(row).filter(grant => grant.delay === 0 && grant.delegate_ss58 != null && this.same(grant.delegate_ss58, delegate)).map(grant => grant.proxy_type)))
    const updated = { ...this.refreshed(latest, grants), pending_allows: pending.some(row => row.plaintext_missing) ? unique([...(latest.pending_allows ?? []), ...flags]) : flags }
    if (latest.pending_delay_index != null && !pending.some(row => row.index === latest.pending_delay_index)) { delete updated.pending_delay_index; delete updated.pending_delay_blocks }
    await this.records.upsert(updated)
  }
}
