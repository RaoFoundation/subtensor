import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
import type { JsonStore } from './preferences'
import { Intent } from './intents'
import { bytesHex, hexBytes, unsignedInteger } from './portable'
import { sameProtectedAccount, type ProtectedProxyBackend, type ProtectedProxyReceipt } from './protected-proxy-types'
import { formatDuration } from './timelock-workflow'

type Identity = Pick<ProtectedProxyBackend, 'accountId'>
export interface ProxyGrant { delegate: string; proxy_type: string; delay: number }
export interface ProtectedAccountRecord {
  controller_wallet: string
  controller_ss58: string
  protected_ss58: string
  delay_blocks: number
  genesis_hash?: string
  name?: string
  allows?: string[]
  reviewed_allows?: string[]
  pending_allows?: string[]
  allow_key_ss58?: string | null
  allow_key_address?: string | null
  setup?: { status: string; allow_types?: string[]; [key: string]: any }
  coldkey_swap?: { status: string; execute_block?: number | null; new_coldkey_ss58?: string; [key: string]: any }
  pending_delay_blocks?: number
  pending_delay_index?: number
  pending_rotate?: { index?: number; to_wallet?: string; to_ss58?: string; migrated_allows?: string[]; [key: string]: any }
  spawner?: string
  account_mode?: 'legacy' | 'safe' | 'protected'
  height?: number | null
  ext_index?: number | null
  index?: number
  [key: string]: any
}
export interface ProtectedSetupReservation {
  controller_wallet: string
  controller_ss58: string
  genesis_hash: string
  delay_blocks: number
  index: number
  allow_types: string[]
  allow_key_ss58?: string | null
  status: 'submitting' | 'uncertain' | 'awaiting_approval' | 'failed'
  extrinsic_hash?: string
  block_hash?: string
  receipt?: ProtectedProxyReceipt
  [key: string]: any
}
export interface ProtectedAnnouncementPolicyRow {
  index?: number | null
  category?: string
  status?: string
  inner_op?: string
  inner_args?: Record<string, any>
  plaintext_missing?: boolean
  rotate_to_ss58?: string
}

export const ALLOW_ITEM_TO_PROXY_TYPE: Readonly<Record<string, string>> = Object.freeze(POLICY_CATALOG.protected.ALLOW_ITEM_TO_PROXY_TYPE)
export const FORBIDDEN_ALLOW_ITEMS: Readonly<Record<string, string>> = Object.freeze(POLICY_CATALOG.protected.FORBIDDEN_ALLOW_ITEMS)
export const PROXY_TYPE_INTENT_OPS: Readonly<Record<string, readonly string[]>> = Object.freeze(Object.fromEntries(Object.entries(POLICY_CATALOG.protected.PROXY_TYPE_INTENT_OPS).map(([name, ops]) => [name, Object.freeze(ops)])))
export const DEFEATING_ZERO_DELAY_TYPES: readonly string[] = Object.freeze(POLICY_CATALOG.protected.DEFEATING_ZERO_DELAY_TYPES)
export const PROTECTED_MANAGEMENT_OPS: readonly string[] = Object.freeze(POLICY_CATALOG.protected.PROTECTED_MANAGEMENT_OPS)
export const PROTECTED_BYPASS_OPS: readonly string[] = Object.freeze(POLICY_CATALOG.protected.PROTECTED_BYPASS_OPS)

const queues = new WeakMap<JsonStore, Promise<unknown>>()
function serialize<T>(store: JsonStore, action: () => Promise<T>): Promise<T> {
  const next = (queues.get(store) ?? Promise.resolve()).catch(() => {}).then(action)
  queues.set(store, next)
  return next.finally(() => { if (queues.get(store) === next) queues.delete(store) })
}
function scope(value?: string): string | undefined {
  if (value == null) return undefined
  if (!/^0x[0-9a-f]{64}$/i.test(value)) throw new TypeError('genesis_hash must be a 32-byte hash')
  return value.toLowerCase()
}
function walletName(value: unknown): asserts value is string { if (typeof value !== 'string' || !value.trim()) throw new TypeError('controller_wallet must be nonempty') }
function blockCount(value: number, positive = false): number {
  const count = Number(unsignedInteger(value, 32, 'delay blocks'))
  if (positive && count === 0) throw new RangeError('delay must be greater than 0 blocks')
  return count
}
function clone<T>(value: T): T { return structuredClone(value) }
/** JSON journals retain exact decoded balances and bytes without backend-specific objects. */
export function protectedJsonValue(value: unknown, depth = 0): any {
  if (depth > 128) throw new RangeError('protected journal value nesting exceeds 128')
  if (typeof value === 'bigint') return value.toString()
  if (value instanceof Uint8Array) return bytesHex(value)
  if (Array.isArray(value)) return value.map(child => protectedJsonValue(child, depth + 1))
  if (value != null && typeof value === 'object') return Object.fromEntries(Object.entries(value).filter(([, child]) => child !== undefined).map(([key, child]) => [key, protectedJsonValue(child, depth + 1)]))
  if (typeof value === 'function' || typeof value === 'symbol' || typeof value === 'number' && !Number.isFinite(value)) throw new TypeError('protected journal requires JSON-compatible values')
  return value
}

/** Explicit, chain-scoped configuration. A failed write never changes a binding in memory. */
export class ProtectedAccountStore {
  constructor(readonly store: JsonStore, private readonly identity: Identity) {}
  private validate(record: ProtectedAccountRecord): void {
    if (!record || typeof record !== 'object') throw new TypeError('invalid protected-account record')
    walletName(record.controller_wallet)
    this.identity.accountId(record.controller_ss58); this.identity.accountId(record.protected_ss58)
    scope(record.genesis_hash); blockCount(record.delay_blocks, true)
    if (record.allow_key_ss58) this.identity.accountId(record.allow_key_ss58)
    for (const name of ['allows', 'reviewed_allows', 'pending_allows'] as const) if (record[name] != null && (!Array.isArray(record[name]) || record[name]!.some(item => typeof item !== 'string' || !item))) throw new TypeError(`invalid ${name}`)
  }
  private records(rows: unknown): ProtectedAccountRecord[] {
    if (rows == null) return []
    if (!Array.isArray(rows)) throw new TypeError('invalid protected-account store')
    const keys = new Set<string>()
    for (const row of rows) {
      this.validate(row)
      const key = JSON.stringify([row.controller_wallet, scope(row.genesis_hash)])
      if (keys.has(key)) throw new TypeError('duplicate protected-account controller binding')
      keys.add(key)
    }
    return clone(rows)
  }
  async list(): Promise<ProtectedAccountRecord[]> { return this.records(await this.store.read('protected')) }
  private change<R>(key: string, action: (current: unknown) => { value: unknown; result: R }): Promise<R> {
    const update = (current: unknown) => { const { value, result } = action(current); return { value: protectedJsonValue(value), result: protectedJsonValue(result) as R } }
    if (this.store.update) return this.store.update(key, update)
    return serialize(this.store, async () => { const { value, result } = update(await this.store.read(key)); await this.store.write(key, value); return clone(result) })
  }
  async getByController(wallet: string, genesisHash?: string): Promise<ProtectedAccountRecord | undefined> { const selected = scope(genesisHash); return (await this.list()).find(row => row.controller_wallet === wallet && scope(row.genesis_hash) === selected) }
  async getByProtectedAddress(address: string, genesisHash?: string): Promise<ProtectedAccountRecord | undefined> { const selected = scope(genesisHash); return (await this.list()).find(row => scope(row.genesis_hash) === selected && sameProtectedAccount(this.identity, row.protected_ss58, address)) }
  private transition<R>(rows: ProtectedAccountRecord[] | ProtectedSetupReservation[], input: Record<string, any>): {value: any[]; result: R} {
    const addresses = [...rows, input.record, input.fields].filter(Boolean).flatMap(row => [row.controller_ss58, row.protected_ss58]).concat(input.destination, input.controller_ss58).filter((value): value is string => typeof value === 'string')
    const identities = Object.fromEntries(addresses.map(address => [address, bytesHex(this.identity.accountId(address))]))
    const result = evaluatePolicy<{value: any[]; result: R | null}>('protected.config_transition', { ...input, rows, identities })
    return { value: result.value, result: (result.result === null ? undefined : result.result) as R }
  }
  upsert(record: ProtectedAccountRecord): Promise<ProtectedAccountRecord> {
    const saved = clone(record); this.validate(saved)
    if (saved.genesis_hash != null) saved.genesis_hash = scope(saved.genesis_hash)
    return this.change('protected', current => this.transition(this.records(current), { action: 'upsert', record: saved }))
  }
  /** Recover creation once without replacing another process's setup or migration progress. */
  bindCreated(record: ProtectedAccountRecord): Promise<ProtectedAccountRecord> {
    const saved = clone(record); this.validate(saved)
    const selected = scope(saved.genesis_hash)
    if (!selected) throw new TypeError('created protected account requires genesis_hash')
    saved.genesis_hash = selected
    return this.change('protected', current => this.transition(this.records(current), { action: 'bind_created', record: saved }))
  }
  /** Merge workflow progress without overwriting a concurrent migration claim. */
  patch(wallet: string, genesisHash: string, fields: Partial<ProtectedAccountRecord>, expectedMigration?: { status: string; extrinsicHash?: string }): Promise<ProtectedAccountRecord> {
    const selected = scope(genesisHash), saved = clone(fields)
    if (!selected) throw new TypeError('protected update requires genesis_hash')
    return this.change('protected', current => {
      const result = this.transition<ProtectedAccountRecord>(this.records(current), { action: 'patch', wallet, genesis_hash: selected, fields: saved, undefined_fields: Object.keys(saved).filter(key => (saved as any)[key] === undefined), expected_migration: expectedMigration })
      this.validate(result.result)
      return result
    })
  }
  /** Reserve a coldkey migration before broadcasting; uncertain work is never retried implicitly. */
  reserveMigration(wallet: string, genesisHash: string, destination: string): Promise<{ record: ProtectedAccountRecord; created: boolean }> {
    const selected = scope(genesisHash)
    if (!selected) throw new TypeError('migration reservation requires genesis_hash')
    this.identity.accountId(destination)
    return this.change('protected', current => this.transition(this.records(current), { action: 'reserve_migration', wallet, genesis_hash: selected, destination }))
  }
  remove(wallet: string, genesisHash?: string): Promise<boolean> { const selected = scope(genesisHash); return this.change('protected', current => this.transition(this.records(current), { action: 'remove', wallet, genesis_hash: selected })) }
  moveController(fromWallet: string, toWallet: string, options: { controller_ss58?: string; genesis_hash?: string } = {}): Promise<ProtectedAccountRecord | undefined> {
    walletName(toWallet); const selected = scope(options.genesis_hash)
    if (options.controller_ss58 != null) this.identity.accountId(options.controller_ss58)
    return this.change('protected', current => this.transition(this.records(current), { action: 'move', wallet: fromWallet, to_wallet: toWallet, genesis_hash: selected, controller_ss58: options.controller_ss58 }))
  }
  /** Call only after explicit chain validation; legacy data is never selected as a fallback. */
  bindLegacy(wallet: string, genesisHash: string): Promise<ProtectedAccountRecord | undefined> {
    const selected = scope(genesisHash)
    if (!selected) throw new TypeError('legacy binding requires genesis_hash')
    return this.change('protected', current => this.transition(this.records(current), { action: 'bind_legacy', wallet, genesis_hash: selected }))
  }
  private setups(rows: unknown): ProtectedSetupReservation[] {
    if (rows == null) return []
    if (!Array.isArray(rows)) throw new TypeError('invalid protected setup journal')
    const keys = new Set<string>()
    for (const row of rows) {
      walletName(row?.controller_wallet); this.identity.accountId(row.controller_ss58); scope(row.genesis_hash); blockCount(row.delay_blocks, true)
      if (!row.genesis_hash || !['submitting', 'uncertain', 'awaiting_approval', 'failed'].includes(row.status) || !Array.isArray(row.allow_types) || row.allow_types.some((value: unknown) => typeof value !== 'string')) throw new TypeError('invalid protected setup reservation')
      unsignedInteger(row.index, 16, 'pure account index')
      if (row.allow_key_ss58) this.identity.accountId(row.allow_key_ss58)
      const key = JSON.stringify([row.controller_wallet, scope(row.genesis_hash)])
      if (keys.has(key)) throw new TypeError('duplicate protected setup reservation')
      keys.add(key)
    }
    return clone(rows)
  }
  async getSetup(wallet: string, genesisHash: string): Promise<ProtectedSetupReservation | undefined> { const selected = scope(genesisHash); return this.setups(await this.store.read('protected_setups')).find(row => row.controller_wallet === wallet && scope(row.genesis_hash) === selected) }
  reserveSetup(reservation: ProtectedSetupReservation): Promise<{ reservation: ProtectedSetupReservation; created: boolean }> {
    const saved = clone(reservation); saved.genesis_hash = scope(saved.genesis_hash)!
    this.setups([saved])
    return this.change('protected_setups', current => this.transition(this.setups(current), { action: 'reserve_setup', record: saved }))
  }

  saveSetup(reservation: ProtectedSetupReservation): Promise<ProtectedSetupReservation> {
    const saved = clone(reservation); saved.genesis_hash = scope(saved.genesis_hash)!
    this.setups([saved])
    return this.change('protected_setups', current => this.transition(this.setups(current), { action: 'save_setup', record: saved }))
  }
  removeSetup(wallet: string, genesisHash: string): Promise<boolean> { const selected = scope(genesisHash); return this.change('protected_setups', current => this.transition(this.setups(current), { action: 'remove', wallet, genesis_hash: selected })) }
}

export function deriveLegacyPureAccountId(backend: Pick<ProtectedProxyBackend, 'hash'>, spawner: Uint8Array, options: { height: number; ext_index: number; proxy_type_scale?: number; index?: number }): Uint8Array {
  const result = backend.hash(hexBytes(evaluatePolicy<string>('protected.pure_preimage', { spawner, ...options })))
  if (result.length !== 32) throw new RangeError('pure account hash must be 32 bytes')
  return result
}
export function deriveLegacyPureSs58(backend: Pick<ProtectedProxyBackend, 'hash' | 'accountId' | 'ss58Encode'>, spawner: string, options: { height: number; ext_index: number; index?: number; proxy_type?: string }): string {
  if (options.proxy_type != null && options.proxy_type !== 'Any') throw new RangeError('legacy pure address derivation supports ProxyType.Any only')
  return backend.ss58Encode(deriveLegacyPureAccountId(backend, backend.accountId(spawner), options))
}
export function parseDelayToBlocks(raw: string | number, blockTime = 12): number { return evaluatePolicy('protected.delay', { raw, block_time: blockTime }) }
export function resolveAllowItem(item: string): string {
  const result = evaluatePolicy<{kind: string; reason?: string; proxy_type?: string}>('protected.allow_resolution', { item })
  if (result.kind === 'forbidden') throw new Error(`allow ${item} refused: ${result.reason}; transfers and proxy changes must remain delayed`)
  if (result.kind === 'unknown') throw new Error(`allow ${item} has no suitably narrow existing ProxyType`)
  return result.proxy_type!
}
export function resolveAllowItems(items: readonly string[]): string[] { return [...new Set(items.map(resolveAllowItem))] }
function encoded(value?: Intent | Record<string, any>): Record<string, any> { return value instanceof Intent ? value.toJSON() : value ?? {} }
const identity = (backend: Identity, address?: string | null): string | null => address ? bytesHex(backend.accountId(address)) : null
function grantIndices(backend: Identity, grants: readonly ProxyGrant[], controller: string, mode: string, delayBlocks?: number): number[] { return evaluatePolicy('protected.grant_indices', { mode, grants: grants.map(row => ({ ...row, delegate: identity(backend, row.delegate) })), controller: identity(backend, controller), delay_blocks: delayBlocks }) }
export function instantProxyTypeFor(backend: Identity, intentOp: string, record: Pick<ProtectedAccountRecord, 'allows' | 'pending_allows' | 'allow_key_ss58'>, options: { dispatcher_ss58?: string; intent?: Intent | Record<string, any> } = {}): string | undefined {
  return evaluatePolicy<string | null>('protected.instant_proxy', { op: intentOp, allows: record.allows, allow_key: identity(backend, record.allow_key_ss58), dispatcher: identity(backend, options.dispatcher_ss58), intent: encoded(options.intent) }) ?? undefined
}
export function defeatingZeroDelayProxies(backend: Identity, grants: readonly ProxyGrant[], controller: string): string[] { return grantIndices(backend, grants, controller, 'defeating').map(index => grants[index].proxy_type) }
export function controllerGrantsBeforeDelay(backend: Identity, grants: readonly ProxyGrant[], controller: string, delayBlocks: number): ProxyGrant[] { return grantIndices(backend, grants, controller, 'before_delay', delayBlocks).map(index => grants[index]) }
export function controllerAnyDelays(backend: Identity, grants: readonly ProxyGrant[], controller: string): number[] { return grantIndices(backend, grants, controller, 'any').map(index => grants[index].delay) }
export function controllerHoldsAllows(backend: Identity, record: Pick<ProtectedAccountRecord, 'allow_key_ss58'>, controller: string): boolean { return evaluatePolicy('protected.holds_allows', { allow_key: identity(backend, record.allow_key_ss58), controller: identity(backend, controller) }) }
export function controllerOwnedAllowTypes(backend: Identity, record: Pick<ProtectedAccountRecord, 'allow_key_ss58' | 'allows'>, controller: string): string[] { return evaluatePolicy('protected.owned_allows', { allow_key: identity(backend, record.allow_key_ss58), controller: identity(backend, controller), allows: record.allows }) }
export function allowKeyGrants(backend: Identity, grants: readonly ProxyGrant[], allowKey: string): ProxyGrant[] { return grantIndices(backend, grants, allowKey, 'allow').map(index => grants[index]) }
export function protectedSetupPending(record: Pick<ProtectedAccountRecord, 'setup' | 'coldkey_swap'>): boolean { return evaluatePolicy('protected.setup_pending', record) }
export function announcementProxyGrants(row: ProtectedAnnouncementPolicyRow): Array<{ delegate_ss58?: string; proxy_type: string; delay: number }> { return evaluatePolicy('protected.announcement_grants', row) }
export function pendingAllowGrantLabels(backend: Identity, rows: readonly ProtectedAnnouncementPolicyRow[], allowKey?: string): string[] {
  const grants = rows.map(row => announcementProxyGrants(row))
  const selected = evaluatePolicy<Array<{ row_index: number; grant_index: number }>>('protected.pending_allow_grants', { rows: rows.map((row, index) => ({ status: row.status, grants: grants[index].map(grant => ({ ...grant, identity: identity(backend, grant.delegate_ss58) })) })), allow_key: identity(backend, allowKey), include_uncertain: true })
  return [...new Set(selected.map(({ row_index, grant_index }) => { const grant = grants[row_index][grant_index]; return `#${rows[row_index].index} allow:${grant.proxy_type} → ${grant.delegate_ss58 ?? 'unknown-delegate'}` }))]
}
export function allowKeyChangeBlockers(backend: Identity, options: { old_allow_key_ss58: string; new_allow_key_ss58: string; allows: readonly string[]; pending_allows: readonly string[]; delegations: readonly ProxyGrant[]; pending_allow_announcements?: readonly string[] }): string[] {
  const selected = evaluatePolicy<{ kinds: string[]; live_indices: number[] }>('protected.allow_key_blockers', { old: identity(backend, options.old_allow_key_ss58), new: identity(backend, options.new_allow_key_ss58), allows: options.allows, pending_allows: options.pending_allows, pending_announcements: options.pending_allow_announcements, grants: options.delegations.map(row => ({ ...row, delegate: identity(backend, row.delegate) })) })
  const labels: Record<string, string> = {
    allows: `confirmed allows on old key: ${options.allows.join(', ')}`,
    pending_allows: `pending allows on old key: ${options.pending_allows.join(', ')}`,
    pending_announcements: `outstanding allow announcements: ${options.pending_allow_announcements?.join(', ')}`,
    live: `on-chain zero-delay grants on old key: ${selected.live_indices.map(index => `${options.delegations[index].proxy_type}@0`).join(', ')}`,
  }
  return selected.kinds.map(kind => labels[kind])
}
export function rotationComplete(backend: Identity, grants: readonly ProxyGrant[], options: { new_controller_ss58: string; old_controller_ss58?: string | null; delay_blocks: number }): boolean {
  return evaluatePolicy('protected.rotation_complete', { grants: grants.map(row => ({ ...row, delegate: identity(backend, row.delegate) })), new_controller: identity(backend, options.new_controller_ss58), old_controller: identity(backend, options.old_controller_ss58), delay_blocks: options.delay_blocks })
}
export function protectedManagementPath(value: Intent | Record<string, any>, depth = 0): string | undefined { return evaluatePolicy<string | null>('protected.management_path', { intent: encoded(value), depth }) ?? undefined }
export function isProtectedManagementReceipt(row: ProtectedAnnouncementPolicyRow, value: Intent | Record<string, any>): boolean { return evaluatePolicy('protected.management_receipt', { row, intent: encoded(value) }) }
export function assertProtectedManagementAllowed(row: ProtectedAnnouncementPolicyRow, value: Intent | Record<string, any>): void { const path = protectedManagementPath(value); if (path && !isProtectedManagementReceipt(row, value)) throw new Error(`refusing unmanaged ${path} on a keyless protected account; use protected settings or controller rotation`) }
export function defaultProtectedName(wallet: string): string { walletName(wallet); return `${wallet}-protected` }
export function blocksToLocalEta(blocks: number, blockTime = 12, now = new Date()): string {
  if (blocks <= 0) return 'now'
  if (!Number.isFinite(blocks) || !Number.isFinite(blockTime) || blockTime <= 0) throw new RangeError('invalid block timing')
  const seconds = blocks * blockTime, eta = new Date(now.getTime() + seconds * 1000)
  return `~${formatDuration(seconds >= 60 ? Math.round(seconds / 60) * 60 : seconds)} (${eta.toLocaleString()})`
}
