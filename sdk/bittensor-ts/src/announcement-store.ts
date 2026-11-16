import { POLICY_CATALOG } from './policy-catalog'
import { evaluatePolicy } from './policy'
import type { JsonStore } from './preferences'

export interface AnnouncementMultisig { account_ss58: string; call_hash: string; timepoint?: { height: number; index: number } }
export interface ProxyAnnouncement {
  index: number
  controller_wallet: string
  delegate_ss58: string
  real_ss58: string
  call_hash: string
  genesis_hash?: string
  inner_op: string
  inner_args: Record<string, any>
  category: string
  delay_blocks: number
  announced_at_block: number
  announcement_ordinal: number
  executable_at_block: number
  force_proxy_type?: string | null
  summary: string
  status: string
  created_at: number
  call_data?: string
  runtime_spec_version?: number
  call_semantics?: unknown
  plaintext_missing?: boolean
  on_chain?: boolean
  ready?: boolean
  multisig_announcement?: AnnouncementMultisig
  [key: string]: any
}
export interface NewProxyAnnouncement {
  controller_wallet: string; delegate_ss58: string; real_ss58: string; call_hash: string
  genesis_hash?: string; inner_op: string; inner_args: Record<string, any>
  delay_blocks: number; announced_at_block: number
  announcement_ordinal?: number; status?: string; summary?: string; category?: string
  [key: string]: any
}
export interface AnnouncementScope { genesisHash?: string | null }
export const ACTIVE_ANNOUNCEMENT_STATUSES: ReadonlySet<string> = new Set(POLICY_CATALOG.protected.ACTIVE_ANNOUNCEMENT_STATUSES)
const queues = new WeakMap<JsonStore, Promise<unknown>>()
type AtomicStore = JsonStore & { update?<T>(key: string, action: (current: unknown) => { value: unknown; result: T }): Promise<T> }

/** JSON-safe exact values for persisted decoded calls and intent arguments. */
export function normalizeCallSemantics(value: any): any {
  if (typeof value === 'bigint') return value.toString()
  if (value instanceof Uint8Array) return '0x' + Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('')
  if (Array.isArray(value)) return value.map(normalizeCallSemantics)
  if (value != null && typeof value === 'object') return Object.fromEntries(Object.entries(value).filter(([, child]) => child !== undefined).map(([key, child]) => [key, normalizeCallSemantics(child)]))
  if (typeof value === 'function' || typeof value === 'symbol' || typeof value === 'number' && !Number.isFinite(value)) throw new TypeError('announcement values must be JSON serializable')
  return value
}
export function announcementCategory(op: string): string { return evaluatePolicy('protected.announcement_category', { op }) }
function hash(value: string): string { if (!/^0x[0-9a-f]{64}$/i.test(value)) throw new TypeError('announcement requires a 32-byte hash'); return value.toLowerCase() }
function integer(value: number, label: string): number { if (!Number.isSafeInteger(value) || value < 0) throw new TypeError(`${label} must be a nonnegative safe integer`); return value }
function document(value: unknown): ProxyAnnouncement[] {
  if (value == null) return []
  const doc = value as any
  if (doc.version !== 1 || !Array.isArray(doc.announcements)) throw new TypeError('invalid proxy_announcements journal')
  const rows = structuredClone(doc.announcements) as ProxyAnnouncement[]
  const indices = new Set<number>()
  for (const row of rows) {
    if (!row || integer(row.index, 'announcement index') === 0 || indices.has(row.index) || typeof row.controller_wallet !== 'string' || !row.controller_wallet || typeof row.delegate_ss58 !== 'string' || typeof row.real_ss58 !== 'string') throw new TypeError('invalid announcement record')
    indices.add(row.index); hash(row.call_hash)
    if (row.genesis_hash != null) hash(row.genesis_hash)
    integer(row.announced_at_block, 'announcement block'); integer(row.announcement_ordinal ?? 0, 'announcement ordinal')
    integer(row.delay_blocks, 'announcement delay')
  }
  return rows
}

/** A plaintext journal. Supply an atomic JsonStore.update for cross-process reservations. */
export class ProxyAnnouncementStore {
  readonly key = 'proxy_announcements'
  constructor(readonly store: JsonStore, private readonly accountIdentity: (address: string) => string = address => address, private readonly now: () => number = () => Math.floor(Date.now() / 1000)) {}
  async all(): Promise<ProxyAnnouncement[]> { return document(await this.store.read(this.key)) }
  private scope(row: ProxyAnnouncement, scope: AnnouncementScope): boolean { return (row.genesis_hash?.toLowerCase() ?? null) === (scope.genesisHash?.toLowerCase() ?? null) }
  private canonical(row: Partial<ProxyAnnouncement>): Partial<ProxyAnnouncement> { return { ...row, delegate_ss58: this.accountIdentity(row.delegate_ss58!), real_ss58: this.accountIdentity(row.real_ss58!) } }
  sameCommitment(left: Partial<ProxyAnnouncement>, right: Partial<ProxyAnnouncement>): boolean { return evaluatePolicy('protected.same_commitment', { left: this.canonical(left), right: this.canonical(right) }) }
  sameIdentity(left: Partial<ProxyAnnouncement>, right: Partial<ProxyAnnouncement>): boolean { return evaluatePolicy('protected.same_identity', { left: this.canonical(left), right: this.canonical(right) }) }
  private async mutate<T>(action: (rows: ProxyAnnouncement[]) => T): Promise<T> {
    const transform = (current: unknown) => { const rows = document(current), result = action(rows), value = { version: 1, announcements: normalizeCallSemantics(rows) }; document(value); return { value, result: structuredClone(result) } }
    const atomic = this.store as AtomicStore
    if (atomic.update) return atomic.update(this.key, transform)
    const previous = queues.get(this.store) ?? Promise.resolve()
    const next = previous.catch(() => {}).then(async () => { const { value, result } = transform(await this.store.read(this.key)); await this.store.write(this.key, value); return result })
    queues.set(this.store, next)
    try { return await next } finally { if (queues.get(this.store) === next) queues.delete(this.store) }
  }
  async listForWallet(wallet: string, options: AnnouncementScope & { includeDone?: boolean } = {}): Promise<ProxyAnnouncement[]> { return (await this.all()).filter(row => row.controller_wallet === wallet && this.scope(row, options) && (options.includeDone || ACTIVE_ANNOUNCEMENT_STATUSES.has(row.status))).sort((a, b) => a.index - b.index) }
  async getByIndex(wallet: string, index: number, scope: AnnouncementScope = {}): Promise<ProxyAnnouncement | undefined> { return (await this.listForWallet(wallet, { ...scope, includeDone: true })).find(row => row.index === index) }
  private append(rows: ProxyAnnouncement[], input: NewProxyAnnouncement): ProxyAnnouncement {
    const saved = normalizeCallSemantics(input), delay = integer(saved.delay_blocks, 'delay'), height = integer(saved.announced_at_block, 'height')
    const row: ProxyAnnouncement = { ...saved, index: Math.max(0, ...rows.map(row => row.index)) + 1, call_hash: hash(saved.call_hash), ...(saved.genesis_hash == null ? {} : { genesis_hash: hash(saved.genesis_hash) }),
      announcement_ordinal: integer(saved.announcement_ordinal ?? 0, 'ordinal'), executable_at_block: integer(delay + height, 'executable block'), status: saved.status ?? 'pending', category: saved.category ?? announcementCategory(saved.inner_op), summary: saved.summary || saved.category || saved.inner_op, created_at: this.now() }
    return this.transition(rows, { action: 'add', candidate: row })
  }
  private transition(rows: ProxyAnnouncement[], input: Record<string, unknown>): ProxyAnnouncement {
    const addresses = [...rows, input.candidate as Partial<ProxyAnnouncement> | undefined].filter(Boolean).flatMap(row => [row!.delegate_ss58, row!.real_ss58]).filter((value): value is string => typeof value === 'string')
    const identities = Object.fromEntries(addresses.map(address => [address, this.accountIdentity(address)]))
    const result = evaluatePolicy<{rows: ProxyAnnouncement[]; row: ProxyAnnouncement}>('protected.journal_transition', { ...input, rows, identities, now: this.now(), null_defaults: true })
    rows.splice(0, rows.length, ...result.rows)
    return result.row
  }

  add(input: NewProxyAnnouncement): Promise<ProxyAnnouncement> { const saved = structuredClone(input); return this.mutate(rows => this.append(rows, saved)) }
  /** Reserve before submitting; another caller may not broadcast the same uncertain payload. */
  reserve(input: NewProxyAnnouncement): Promise<ProxyAnnouncement> {
    const saved = structuredClone(input)
    return this.mutate(rows => {
      // Construct the same public row shape; reserve and append atomically in core.
      const delay = integer(saved.delay_blocks, 'delay'), height = integer(saved.announced_at_block, 'height')
      const candidate = { ...normalizeCallSemantics(saved), call_hash: hash(saved.call_hash), ...(saved.genesis_hash == null ? {} : {genesis_hash: hash(saved.genesis_hash)}), announcement_ordinal: integer(saved.announcement_ordinal ?? 0, 'ordinal'), executable_at_block: integer(delay + height, 'executable block'), status: 'submitting', category: saved.category ?? announcementCategory(saved.inner_op), summary: saved.summary || saved.category || saved.inner_op, created_at: this.now() }
      return this.transition(rows, { action: 'reserve', candidate })
    })
  }

  upsertOnChain(input: Pick<ProxyAnnouncement, 'controller_wallet' | 'delegate_ss58' | 'real_ss58' | 'call_hash' | 'delay_blocks' | 'announced_at_block'> & { genesis_hash: string; announcement_ordinal?: number }): Promise<ProxyAnnouncement> {
    const saved = structuredClone(input)
    return this.mutate(rows => {
      const chain = { controller_wallet: saved.controller_wallet, delegate_ss58: saved.delegate_ss58, real_ss58: saved.real_ss58, delay_blocks: integer(saved.delay_blocks, 'delay'), announced_at_block: integer(saved.announced_at_block, 'height'), genesis_hash: hash(saved.genesis_hash), call_hash: hash(saved.call_hash), announcement_ordinal: integer(saved.announcement_ordinal ?? 0, 'ordinal'), executable_at_block: integer(saved.announced_at_block + saved.delay_blocks, 'executable block'), status: 'pending', on_chain: true }
      return this.transition(rows, { action: 'upsert', candidate: chain })
    })
  }

  update(wallet: string, index: number, updates: Partial<ProxyAnnouncement>, scope: AnnouncementScope = {}): Promise<ProxyAnnouncement | undefined> {
    const saved = normalizeCallSemantics(updates)
    return this.mutate(rows => { const i = rows.findIndex(row => row.index === index && row.controller_wallet === wallet && this.scope(row, scope)); if (i < 0) return undefined; return rows[i] = { ...rows[i], ...saved, index } })
  }
  markStatus(wallet: string, index: number, status: string, scope: AnnouncementScope = {}) { return this.update(wallet, index, { status, updated_at: this.now() }, scope) }
  claimAction(wallet: string, index: number, kind: 'execute' | 'cancel', scope: AnnouncementScope, allowRepeat = false): Promise<ProxyAnnouncement> {
    return this.mutate(rows => this.transition(rows, { action: 'claim', wallet, index, kind, genesis_hash: scope.genesisHash, allow_repeat: allowRepeat }))
  }
  /** A delayed historical RPC must not overwrite a newer action reservation. */
  settleAction(wallet: string, index: number, expectedAction: unknown, updates: Partial<ProxyAnnouncement>, scope: AnnouncementScope): Promise<ProxyAnnouncement | undefined> {
    const expected = JSON.stringify(normalizeCallSemantics(expectedAction)), saved = normalizeCallSemantics(updates)
    return this.mutate(rows => {
      const i = rows.findIndex(row => row.index === index && row.controller_wallet === wallet && this.scope(row, scope))
      if (i < 0) return undefined
      if (JSON.stringify(rows[i].pending_action) !== expected) return rows[i]
      return rows[i] = { ...rows[i], ...saved, index }
    })
  }
  remove(wallet: string, index: number, scope: AnnouncementScope = {}): Promise<boolean> { return this.mutate(rows => { const i = rows.findIndex(row => row.index === index && row.controller_wallet === wallet && this.scope(row, scope)); if (i < 0) return false; rows.splice(i, 1); return true }) }
  async unconfirmedCommitment(input: Pick<ProxyAnnouncement, 'controller_wallet' | 'delegate_ss58' | 'real_ss58' | 'call_hash'> & { genesis_hash: string }): Promise<ProxyAnnouncement | undefined> { return (await this.all()).find(row => ['submitting', 'awaiting_approval'].includes(row.status) && this.sameCommitment(row, input)) }
  async executedCommitmentIndex(row: ProxyAnnouncement): Promise<number | undefined> { return (await this.all()).find(saved => saved.status === 'executed' && this.sameCommitment(saved, row))?.index }
  moveWallet(from: string, to: string, scope: AnnouncementScope = {}): Promise<number> { return this.mutate(rows => { let count = 0; rows.forEach((row, i) => { if (row.controller_wallet === from && this.scope(row, scope) && ACTIVE_ANNOUNCEMENT_STATUSES.has(row.status)) { rows[i] = { ...row, controller_wallet: to }; count++ } }); return count }) }
  bindLegacy(wallet: string, genesisHash: string, real: string): Promise<number> {
    const genesis = hash(genesisHash)
    return this.mutate(rows => {
      const matching = rows.filter(row => row.controller_wallet === wallet && row.genesis_hash == null && this.accountIdentity(row.real_ss58) === this.accountIdentity(real))
      for (const row of matching) if (rows.some(other => this.sameIdentity(other, { ...row, genesis_hash: genesis }))) throw new Error('matching scoped commitment exists; both stored copies have been preserved')
      for (const row of matching) row.genesis_hash = genesis
      return matching.length
    })
  }
}
