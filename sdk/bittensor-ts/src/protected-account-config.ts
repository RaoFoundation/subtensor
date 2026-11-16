import type { JsonStore } from './preferences'
import { Intent } from './intents'
import { bytesHex, concatBytes, integerLE, unsignedInteger } from './portable'
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

export const ALLOW_ITEM_TO_PROXY_TYPE: Readonly<Record<string, string>> = Object.freeze({
  staking: 'Staking', childkeys: 'ChildKeys', childkey: 'ChildKeys', basket: 'BasketTrading',
  'basket-swaps': 'BasketTrading', basket_swaps: 'BasketTrading', gamma: 'GammaSpend',
  'root-claim': 'RootClaim', root_claim: 'RootClaim', rootclaim: 'RootClaim', validate: 'Validate',
  weights: 'Weights', owner: 'Owner', 'swap-hotkey': 'SwapHotkey', swap_hotkey: 'SwapHotkey',
  'subnet-lease': 'SubnetLeaseBeneficiary', subnet_lease: 'SubnetLeaseBeneficiary',
})
export const FORBIDDEN_ALLOW_ITEMS: Readonly<Record<string, string>> = Object.freeze({
  stake: "use 'staking' after reviewing the complete Staking authority", unstake: "use 'staking' after reviewing the complete Staking authority",
  registration: 'Registration can burn protected-account TAO immediately', lock: 'Lock can freeze protected-account stake permanently',
  any: 'Any can transfer and change proxies immediately', transfer: 'Transfer moves free balance immediately',
  smalltransfer: 'SmallTransfer moves free balance immediately', 'small-transfer': 'SmallTransfer moves free balance immediately', small_transfer: 'SmallTransfer moves free balance immediately',
  nontransfer: 'NonTransfer can manage proxies and defeat the delay', 'non-transfer': 'NonTransfer can manage proxies and defeat the delay', non_transfer: 'NonTransfer can manage proxies and defeat the delay',
  noncritical: 'NonCritical can transfer free balance immediately', 'non-critical': 'NonCritical can transfer free balance immediately', non_critical: 'NonCritical can transfer free balance immediately',
})
export const PROXY_TYPE_INTENT_OPS: Readonly<Record<string, readonly string[]>> = Object.freeze(Object.fromEntries(Object.entries({
  Staking: ['add_stake', 'remove_stake', 'move_stake', 'swap_stake', 'add_stake_limit', 'remove_stake_limit', 'unstake_all', 'unstake_all_alpha', 'stake_into_basket', 'claim_root', 'claim_root_with_hotkey'],
  ChildKeys: ['set_children', 'set_childkey_take'], BasketTrading: ['swap_basket', 'swap_basket_many'],
  Registration: ['burned_register', 'pow_register'], Lock: ['lock_stake', 'move_lock', 'set_perpetual_lock'], GammaSpend: ['spend_gamma'],
  RootClaim: ['claim_root', 'claim_root_with_hotkey'], Validate: ['serve_axon', 'serve_axon_tls', 'serve_prometheus', 'reset_axon', 'associate_evm_key'],
  Weights: ['set_weights', 'commit_weights', 'reveal_weights'], Owner: ['set_hyperparameter', 'set_subnet_identity'],
  SwapHotkey: ['swap_hotkey'], SubnetLeaseBeneficiary: ['start_call', 'set_subnet_identity', 'set_hyperparameter', 'set_service'],
}).map(([name, ops]) => [name, Object.freeze(ops)])))
export const DEFEATING_ZERO_DELAY_TYPES = Object.freeze(['Any', 'NonCritical', 'Transfer', 'SmallTransfer', 'NonTransfer'])
export const PROTECTED_MANAGEMENT_OPS = Object.freeze(['add_proxy', 'remove_proxy', 'remove_proxies', 'kill_pure_proxy'])
export const PROTECTED_BYPASS_OPS = Object.freeze(['announce_proxy_call', 'remove_proxy_announcement', 'reject_proxy_announcement', 'execute_proxy_announced', 'create_pure_proxy', 'announce_coldkey_swap', 'swap_coldkey_announced', 'clear_coldkey_swap_announcement', 'dispute_coldkey_swap', 'release_own_coldkey_swap_destination_reservation'])

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
  upsert(record: ProtectedAccountRecord): Promise<ProtectedAccountRecord> {
    const saved = clone(record); this.validate(saved)
    if (saved.genesis_hash != null) saved.genesis_hash = scope(saved.genesis_hash)
    return this.change('protected', current => {
      const rows = this.records(current).filter(row => !(row.controller_wallet === saved.controller_wallet && scope(row.genesis_hash) === scope(saved.genesis_hash)))
      return { value: [...rows, saved], result: clone(saved) }
    })
  }
  /** Recover creation once without replacing another process's setup or migration progress. */
  bindCreated(record: ProtectedAccountRecord): Promise<ProtectedAccountRecord> {
    const saved = clone(record); this.validate(saved)
    const selected = scope(saved.genesis_hash)
    if (!selected) throw new TypeError('created protected account requires genesis_hash')
    saved.genesis_hash = selected
    return this.change('protected', current => {
      const rows = this.records(current), old = rows.find(row => row.controller_wallet === saved.controller_wallet && scope(row.genesis_hash) === selected)
      if (old) {
        if (!sameProtectedAccount(this.identity, old.controller_ss58, saved.controller_ss58) || !sameProtectedAccount(this.identity, old.protected_ss58, saved.protected_ss58)) throw new Error('creation recovery conflicts with an existing protected account binding; both identities preserved')
        return { value: rows, result: old }
      }
      return { value: [...rows, saved], result: saved }
    })
  }
  /** Merge workflow progress without overwriting a concurrent migration claim. */
  patch(wallet: string, genesisHash: string, fields: Partial<ProtectedAccountRecord>, expectedMigration?: { status: string; extrinsicHash?: string }): Promise<ProtectedAccountRecord> {
    const selected = scope(genesisHash), saved = clone(fields)
    if (!selected) throw new TypeError('protected update requires genesis_hash')
    return this.change('protected', current => {
      const rows = this.records(current), index = rows.findIndex(row => row.controller_wallet === wallet && scope(row.genesis_hash) === selected)
      if (index < 0) throw new Error('protected account binding not found on this network')
      const old = rows[index]
      if (expectedMigration && (old.coldkey_swap?.status !== expectedMigration.status || old.coldkey_swap?.receipt?.extrinsicHash !== expectedMigration.extrinsicHash)) return { value: rows, result: old }
      if (Object.hasOwn(saved, 'controller_wallet') && saved.controller_wallet !== old.controller_wallet || Object.hasOwn(saved, 'genesis_hash') && scope(saved.genesis_hash) !== selected || Object.hasOwn(saved, 'protected_ss58') && (typeof saved.protected_ss58 !== 'string' || !sameProtectedAccount(this.identity, saved.protected_ss58, old.protected_ss58))) throw new Error('protected account identity cannot change in a workflow update')
      const updated = { ...old, ...saved, controller_wallet: old.controller_wallet, genesis_hash: old.genesis_hash, protected_ss58: old.protected_ss58 }
      this.validate(updated); rows[index] = updated
      return { value: rows, result: updated }
    })
  }
  /** Reserve a coldkey migration before broadcasting; uncertain work is never retried implicitly. */
  reserveMigration(wallet: string, genesisHash: string, destination: string): Promise<{ record: ProtectedAccountRecord; created: boolean }> {
    const selected = scope(genesisHash)
    if (!selected) throw new TypeError('migration reservation requires genesis_hash')
    this.identity.accountId(destination)
    return this.change<{ record: ProtectedAccountRecord; created: boolean }>('protected', current => {
      const rows = this.records(current), index = rows.findIndex(row => row.controller_wallet === wallet && scope(row.genesis_hash) === selected)
      if (index < 0) throw new Error('protected account binding not found on this network')
      const old = rows[index]
      if (!sameProtectedAccount(this.identity, old.protected_ss58, destination)) throw new Error('migration destination differs from the bound protected account')
      if (['submitting', 'uncertain', 'awaiting_approval', 'announced', 'executed'].includes(old.coldkey_swap?.status ?? '')) return { value: rows, result: { record: old, created: false } }
      const updated = { ...old, coldkey_swap: { status: 'submitting', new_coldkey_ss58: destination } }
      rows[index] = updated
      return { value: rows, result: { record: updated, created: true } }
    })
  }
  remove(wallet: string, genesisHash?: string): Promise<boolean> {
    const selected = scope(genesisHash)
    return this.change('protected', current => { const rows = this.records(current), retained = rows.filter(row => !(row.controller_wallet === wallet && scope(row.genesis_hash) === selected)); return { value: retained, result: retained.length !== rows.length } })
  }
  moveController(fromWallet: string, toWallet: string, options: { controller_ss58?: string; genesis_hash?: string } = {}): Promise<ProtectedAccountRecord | undefined> {
    walletName(toWallet); const selected = scope(options.genesis_hash)
    if (options.controller_ss58 != null) this.identity.accountId(options.controller_ss58)
    return this.change('protected', current => {
      const rows = this.records(current), record = rows.find(row => row.controller_wallet === fromWallet && scope(row.genesis_hash) === selected)
      if (!record) return { value: rows, result: undefined }
      const conflict = rows.find(row => row.controller_wallet === toWallet && scope(row.genesis_hash) === selected && !sameProtectedAccount(this.identity, row.protected_ss58, record.protected_ss58))
      if (conflict) throw new Error(`wallet ${toWallet} already controls another protected account on this network; both bindings preserved`)
      const moved = { ...record, controller_wallet: toWallet, ...(options.controller_ss58 ? { controller_ss58: options.controller_ss58 } : {}) }
      return { value: [...rows.filter(row => !([fromWallet, toWallet].includes(row.controller_wallet) && scope(row.genesis_hash) === selected)), moved], result: clone(moved) }
    })
  }
  /** Call only after explicit chain validation; legacy data is never selected as a fallback. */
  bindLegacy(wallet: string, genesisHash: string): Promise<ProtectedAccountRecord | undefined> {
    const selected = scope(genesisHash)
    if (!selected) throw new TypeError('legacy binding requires genesis_hash')
    return this.change('protected', current => {
      const rows = this.records(current), old = rows.find(row => row.controller_wallet === wallet && scope(row.genesis_hash) === undefined)
      const scoped = rows.find(row => row.controller_wallet === wallet && scope(row.genesis_hash) === selected)
      if (!old) return { value: rows, result: scoped }
      if (scoped) throw new Error('wallet already has a protected account on this network; legacy and scoped records preserved')
      const bound = { ...old, genesis_hash: selected }
      return { value: rows.map(row => row === old ? bound : row), result: clone(bound) }
    })
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
    return this.change<{ reservation: ProtectedSetupReservation; created: boolean }>('protected_setups', current => {
      const rows = this.setups(current), old = rows.find(row => row.controller_wallet === saved.controller_wallet && scope(row.genesis_hash) === saved.genesis_hash)
      if (old) return { value: rows, result: { reservation: clone(old), created: false } }
      return { value: [...rows.filter(row => row !== old), saved], result: { reservation: clone(saved), created: true } }
    })
  }
  saveSetup(reservation: ProtectedSetupReservation): Promise<ProtectedSetupReservation> {
    const saved = clone(reservation); saved.genesis_hash = scope(saved.genesis_hash)!
    this.setups([saved])
    return this.change('protected_setups', current => { const rows = this.setups(current).filter(row => !(row.controller_wallet === saved.controller_wallet && scope(row.genesis_hash) === saved.genesis_hash)); return { value: [...rows, saved], result: clone(saved) } })
  }
  removeSetup(wallet: string, genesisHash: string): Promise<boolean> { const selected = scope(genesisHash); return this.change('protected_setups', current => { const rows = this.setups(current), retained = rows.filter(row => !(row.controller_wallet === wallet && scope(row.genesis_hash) === selected)); return { value: retained, result: rows.length !== retained.length } }) }
}

export function deriveLegacyPureAccountId(backend: Pick<ProtectedProxyBackend, 'hash'>, spawner: Uint8Array, options: { height: number; ext_index: number; proxy_type_scale?: number; index?: number }): Uint8Array {
  if (spawner.length !== 32) throw new RangeError('spawner public key must be 32 bytes')
  const prefix = new TextEncoder().encode('modlpy/proxy____')
  const result = backend.hash(concatBytes(prefix, spawner, integerLE(options.height, 4), integerLE(options.ext_index, 4), integerLE(options.proxy_type_scale ?? 0, 1), integerLE(options.index ?? 0, 2)))
  if (result.length !== 32) throw new RangeError('pure account hash must be 32 bytes')
  return result
}
export function deriveLegacyPureSs58(backend: Pick<ProtectedProxyBackend, 'hash' | 'accountId' | 'ss58Encode'>, spawner: string, options: { height: number; ext_index: number; index?: number; proxy_type?: string }): string {
  if (options.proxy_type != null && options.proxy_type !== 'Any') throw new RangeError('legacy pure address derivation supports ProxyType.Any only')
  return backend.ss58Encode(deriveLegacyPureAccountId(backend, backend.accountId(spawner), options))
}
export function parseDelayToBlocks(raw: string | number, blockTime = 12): number {
  if (!Number.isFinite(blockTime) || blockTime <= 0) throw new RangeError('block time must be positive')
  if (typeof raw === 'number') return blockCount(raw, true)
  const match = /^\s*(\d+(?:\.\d+)?)\s*(h|hr|hrs|hour|hours|d|day|days|m|min|mins|minute|minutes|s|sec|secs|second|seconds|b|block|blocks)?\s*$/i.exec(raw)
  if (!match) throw new TypeError('invalid delay; use blocks or a duration such as 36h, 2d or 90m')
  const amount = Number(match[1]), unit = (match[2] ?? 'b').toLowerCase()
  if (['b', 'block', 'blocks'].includes(unit)) return blockCount(Math.trunc(amount), true)
  const factor = unit.startsWith('d') ? 86400 : unit.startsWith('h') ? 3600 : unit.startsWith('m') ? 60 : 1
  const value = amount * factor / blockTime, whole = Math.floor(value), fraction = value - whole
  return blockCount(fraction === 0.5 ? whole + whole % 2 : Math.round(value), true)
}
export function resolveAllowItem(item: string): string {
  const key = item.trim().toLowerCase().replaceAll(' ', '-')
  if (Object.hasOwn(FORBIDDEN_ALLOW_ITEMS, key)) throw new Error(`allow ${item} refused: ${FORBIDDEN_ALLOW_ITEMS[key]}; transfers and proxy changes must remain delayed`)
  if (!Object.hasOwn(ALLOW_ITEM_TO_PROXY_TYPE, key)) throw new Error(`allow ${item} has no suitably narrow existing ProxyType`)
  return ALLOW_ITEM_TO_PROXY_TYPE[key]
}
export function resolveAllowItems(items: readonly string[]): string[] { return [...new Set(items.map(resolveAllowItem))] }
function encoded(value?: Intent | Record<string, any>): Record<string, any> { return value instanceof Intent ? value.toJSON() : value ?? {} }
export function instantProxyTypeFor(backend: Identity, intentOp: string, record: Pick<ProtectedAccountRecord, 'allows' | 'pending_allows' | 'allow_key_ss58'>, options: { dispatcher_ss58?: string; intent?: Intent | Record<string, any> } = {}): string | undefined {
  const intent = encoded(options.intent)
  if (['remove_stake', 'remove_stake_limit', 'move_stake', 'unstake_all'].includes(intentOp) && intent.claim === true) return undefined
  if (intentOp === 'set_hyperparameter' && intent.name === 'registration_allowed') return undefined
  if (record.allow_key_ss58 && options.dispatcher_ss58 && !sameProtectedAccount(backend, record.allow_key_ss58, options.dispatcher_ss58)) return undefined
  return record.allows?.find(type => PROXY_TYPE_INTENT_OPS[type]?.includes(intentOp))
}
export function defeatingZeroDelayProxies(backend: Identity, grants: readonly ProxyGrant[], controller: string): string[] { return grants.filter(row => sameProtectedAccount(backend, row.delegate, controller) && row.delay === 0 && DEFEATING_ZERO_DELAY_TYPES.includes(row.proxy_type)).map(row => row.proxy_type) }
export function controllerGrantsBeforeDelay(backend: Identity, grants: readonly ProxyGrant[], controller: string, delayBlocks: number): ProxyGrant[] { return grants.filter(row => sameProtectedAccount(backend, row.delegate, controller) && row.delay < delayBlocks) }
export function controllerAnyDelays(backend: Identity, grants: readonly ProxyGrant[], controller: string): number[] { return grants.filter(row => sameProtectedAccount(backend, row.delegate, controller) && row.proxy_type === 'Any').map(row => row.delay) }
export function controllerHoldsAllows(backend: Identity, record: Pick<ProtectedAccountRecord, 'allow_key_ss58'>, controller: string): boolean { return !record.allow_key_ss58 || sameProtectedAccount(backend, record.allow_key_ss58, controller) }
export function controllerOwnedAllowTypes(backend: Identity, record: Pick<ProtectedAccountRecord, 'allow_key_ss58' | 'allows'>, controller: string): string[] { return controllerHoldsAllows(backend, record, controller) ? [...new Set(record.allows ?? [])] : [] }
export function allowKeyGrants(backend: Identity, grants: readonly ProxyGrant[], allowKey: string): ProxyGrant[] { return grants.filter(row => sameProtectedAccount(backend, row.delegate, allowKey) && row.proxy_type !== 'Any' && row.delay === 0) }
export function protectedSetupPending(record: Pick<ProtectedAccountRecord, 'setup' | 'coldkey_swap'>): boolean { return record.setup != null ? record.setup.status !== 'ready' : record.coldkey_swap?.status === 'pending' }

export function announcementProxyGrants(row: ProtectedAnnouncementPolicyRow): Array<{ delegate_ss58?: string; proxy_type: string; delay: number }> {
  const grants: Array<{ delegate_ss58?: string; proxy_type: string; delay: number }> = []
  const visit = (value: Record<string, any>, depth: number) => {
    if (depth > 64) throw new RangeError('protected intent nesting exceeds 64')
    if (value.op === 'add_proxy') grants.push({ delegate_ss58: value.delegate_ss58, proxy_type: value.proxy_type ?? 'Staking', delay: blockCount(Number(value.delay ?? 0)) })
    else if (value.op === 'batch') for (const child of value.intents ?? []) visit(encoded(child), depth + 1)
    else if (value.op === 'execute_proxy_announced') visit({ ...value.inner_args, op: value.inner_op }, depth + 1)
    else if (['multisig_approve', 'multisig_execute', 'multisig_threshold_1'].includes(value.op) && value.call) visit(encoded(value.call), depth + 1)
  }
  const args = { ...row.inner_args }, op = row.inner_op ?? (row.category?.startsWith('allow:') ? 'add_proxy' : undefined)
  if (!row.inner_op && row.category?.startsWith('allow:')) args.proxy_type ??= row.category.slice(6)
  visit({ ...args, op }, 0); return grants
}
export function pendingAllowGrantLabels(backend: Identity, rows: readonly ProtectedAnnouncementPolicyRow[], allowKey?: string): string[] {
  const labels: string[] = []
  for (const row of rows) {
    if (row.status != null && !['pending', 'awaiting_approval', 'submitting', 'uncertain'].includes(row.status)) continue
    for (const grant of announcementProxyGrants(row)) if (grant.delay === 0 && !(allowKey && grant.delegate_ss58 && !sameProtectedAccount(backend, allowKey, grant.delegate_ss58))) labels.push(`#${row.index} allow:${grant.proxy_type} → ${grant.delegate_ss58 ?? 'unknown-delegate'}`)
  }
  return [...new Set(labels)]
}
export function allowKeyChangeBlockers(backend: Identity, options: { old_allow_key_ss58: string; new_allow_key_ss58: string; allows: readonly string[]; pending_allows: readonly string[]; delegations: readonly ProxyGrant[]; pending_allow_announcements?: readonly string[] }): string[] {
  if (sameProtectedAccount(backend, options.old_allow_key_ss58, options.new_allow_key_ss58)) return []
  const blockers: string[] = []
  if (options.allows.length) blockers.push(`confirmed allows on old key: ${options.allows.join(', ')}`)
  if (options.pending_allows.length) blockers.push(`pending allows on old key: ${options.pending_allows.join(', ')}`)
  if (options.pending_allow_announcements?.length) blockers.push(`outstanding allow announcements: ${options.pending_allow_announcements.join(', ')}`)
  const live = allowKeyGrants(backend, options.delegations, options.old_allow_key_ss58)
  if (live.length) blockers.push(`on-chain zero-delay grants on old key: ${live.map(row => `${row.proxy_type}@0`).join(', ')}`)
  return blockers
}
export function rotationComplete(backend: Identity, grants: readonly ProxyGrant[], options: { new_controller_ss58: string; old_controller_ss58?: string | null; delay_blocks: number }): boolean {
  const newGrant = grants.some(row => sameProtectedAccount(backend, row.delegate, options.new_controller_ss58) && row.proxy_type === 'Any' && row.delay === options.delay_blocks)
  return newGrant && (!options.old_controller_ss58 || sameProtectedAccount(backend, options.old_controller_ss58, options.new_controller_ss58) || !grants.some(row => sameProtectedAccount(backend, row.delegate, options.old_controller_ss58!)))
}
export function protectedManagementPath(value: Intent | Record<string, any>, depth = 0): string | undefined {
  if (depth > 64) throw new RangeError('protected intent nesting exceeds 64')
  const spec = encoded(value)
  if (PROTECTED_MANAGEMENT_OPS.includes(spec.op)) return spec.op
  if (spec.op === 'batch') for (const [index, child] of (spec.intents ?? []).entries()) { const found = protectedManagementPath(child, depth + 1); if (found) return `batch[${index}].${found}` }
  if (spec.op === 'execute_proxy_announced') { const found = protectedManagementPath({ ...spec.inner_args, op: spec.inner_op }, depth + 1); if (found) return `execute_proxy_announced.${found}` }
  if (['multisig_approve', 'multisig_execute', 'multisig_threshold_1'].includes(spec.op) && spec.call) { const found = protectedManagementPath(spec.call, depth + 1); if (found) return `${spec.op}.${found}` }
  return undefined
}
export function isProtectedManagementReceipt(row: ProtectedAnnouncementPolicyRow, value: Intent | Record<string, any>): boolean {
  const spec = encoded(value), category = row.category ?? '', children = spec.intents ?? []
  if (category.startsWith('allow:') || category.startsWith('disallow:')) return spec.op === (category.startsWith('allow:') ? 'add_proxy' : 'remove_proxy') && spec.proxy_type === category.split(':')[1] && Number(spec.delay ?? 0) === 0
  if (category === 'delay-change') return spec.op === 'batch' && children.length === 2 && children[0].op === 'remove_proxy' && children[1].op === 'add_proxy' && children.every((child: any) => child.proxy_type === 'Any') && children[0].delegate_ss58 === children[1].delegate_ss58
  if (category === 'rotate-controller') return spec.op === 'batch' && Boolean(row.rotate_to_ss58) && children.length >= 2 && children[0].op === 'remove_proxies' && children.slice(1).every((child: any) => child.op === 'add_proxy')
  return false
}
export function assertProtectedManagementAllowed(row: ProtectedAnnouncementPolicyRow, value: Intent | Record<string, any>): void { const path = protectedManagementPath(value); if (path && !isProtectedManagementReceipt(row, value)) throw new Error(`refusing unmanaged ${path} on a keyless protected account; use protected settings or controller rotation`) }
export function defaultProtectedName(wallet: string): string { walletName(wallet); return `${wallet}-protected` }
export function blocksToLocalEta(blocks: number, blockTime = 12, now = new Date()): string {
  if (blocks <= 0) return 'now'
  if (!Number.isFinite(blocks) || !Number.isFinite(blockTime) || blockTime <= 0) throw new RangeError('invalid block timing')
  const seconds = blocks * blockTime, eta = new Date(now.getTime() + seconds * 1000)
  return `~${formatDuration(seconds >= 60 ? Math.round(seconds / 60) * 60 : seconds)} (${eta.toLocaleString()})`
}
