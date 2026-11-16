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
export const ACTIVE_ANNOUNCEMENT_STATUSES: ReadonlySet<string> = new Set(['pending', 'submitting', 'awaiting_approval'])
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
export function announcementCategory(op: string): string {
  if (['transfer', 'transfer_all'].includes(op)) return 'transfer'
  if (op.startsWith('remove_stake') || ['unstake_all', 'unstake_all_alpha'].includes(op)) return 'unstake'
  if (op.startsWith('add_stake') || ['move_stake', 'swap_stake', 'move_swap_stake'].includes(op)) return 'stake'
  if (op.includes('basket')) return 'basket'
  if (['add_proxy', 'remove_proxy', 'remove_proxies', 'kill_pure_proxy'].includes(op)) return 'proxy'
  return op.includes('coldkey') ? 'coldkey-swap' : op.replaceAll('_', '-')
}
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
  sameCommitment(left: Partial<ProxyAnnouncement>, right: Partial<ProxyAnnouncement>): boolean {
    return (left.genesis_hash?.toLowerCase() ?? null) === (right.genesis_hash?.toLowerCase() ?? null) && left.controller_wallet === right.controller_wallet &&
      this.accountIdentity(left.delegate_ss58!) === this.accountIdentity(right.delegate_ss58!) && this.accountIdentity(left.real_ss58!) === this.accountIdentity(right.real_ss58!) && left.call_hash?.toLowerCase() === right.call_hash?.toLowerCase()
  }
  sameIdentity(left: Partial<ProxyAnnouncement>, right: Partial<ProxyAnnouncement>): boolean { return this.sameCommitment(left, right) && (left.announced_at_block ?? 0) === (right.announced_at_block ?? 0) && (left.announcement_ordinal ?? 0) === (right.announcement_ordinal ?? 0) }
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
    const found = rows.findIndex(old => this.sameIdentity(old, row) && !(row.status === 'submitting' && !['submitting', 'awaiting_approval'].includes(old.status)))
    if (found >= 0) {
      const terminal = ['executed', 'cancelled'].includes(rows[found].status) ? rows[found].status : undefined
      Object.assign(row, { ...rows[found], ...row, index: rows[found].index }); delete row.plaintext_missing; delete row.on_chain
      if (terminal) row.status = terminal
      rows[found] = row
      rows.forEach((old, i) => { if (i !== found && this.sameIdentity(old, row)) rows[i] = { ...old, status: 'superseded' } })
    } else rows.push(row)
    return row
  }
  add(input: NewProxyAnnouncement): Promise<ProxyAnnouncement> { const saved = structuredClone(input); return this.mutate(rows => this.append(rows, saved)) }
  /** Reserve before submitting; another caller may not broadcast the same uncertain payload. */
  reserve(input: NewProxyAnnouncement): Promise<ProxyAnnouncement> {
    const saved = structuredClone(input)
    return this.mutate(rows => {
      const existing = rows.find(row => (['submitting', 'awaiting_approval'].includes(row.status) || row.pending_action) && this.sameCommitment(row, saved))
      if (existing) throw new Error(`identical announcement #${existing.index} may still be submitting or awaiting approval`)
      return this.append(rows, { ...saved, status: 'submitting' })
    })
  }
  upsertOnChain(input: Pick<ProxyAnnouncement, 'controller_wallet' | 'delegate_ss58' | 'real_ss58' | 'call_hash' | 'delay_blocks' | 'announced_at_block'> & { genesis_hash: string; announcement_ordinal?: number }): Promise<ProxyAnnouncement> {
    const saved = structuredClone(input)
    return this.mutate(rows => {
      const chain = { controller_wallet: saved.controller_wallet, delegate_ss58: saved.delegate_ss58, real_ss58: saved.real_ss58, delay_blocks: integer(saved.delay_blocks, 'delay'), announced_at_block: integer(saved.announced_at_block, 'height'), genesis_hash: hash(saved.genesis_hash), call_hash: hash(saved.call_hash), announcement_ordinal: integer(saved.announcement_ordinal ?? 0, 'ordinal'), executable_at_block: integer(saved.announced_at_block + saved.delay_blocks, 'executable block'), status: 'pending', on_chain: true }
      const index = rows.findIndex(row => this.sameIdentity(row, chain))
      if (index >= 0) {
        const plaintext = rows.find(row => row.inner_op && this.sameIdentity(row, chain))
        const row = { ...rows[index], ...plaintext, ...chain, ...(['executed', 'cancelled'].includes(rows[index].status) ? { status: rows[index].status } : {}), index: rows[index].index, plaintext_missing: !Boolean(plaintext?.inner_op) }
        rows[index] = row
        rows.forEach((old, i) => { if (i !== index && this.sameIdentity(old, chain)) rows[i] = { ...old, status: 'superseded' } })
        return row
      }
      const matches = rows.filter(row => this.sameCommitment(row, chain))
      const provisional = matches.find(row => ['submitting', 'awaiting_approval'].includes(row.status))
      const template = provisional ?? matches.slice().reverse().find(row => row.inner_op)
      const row: ProxyAnnouncement = { ...template, ...chain, index: provisional?.index ?? Math.max(0, ...rows.map(row => row.index)) + 1, inner_op: template?.inner_op ?? '', inner_args: template?.inner_args ?? {}, category: template?.category ?? 'unknown', force_proxy_type: template?.force_proxy_type ?? 'Any', summary: template?.summary ?? `on-chain announcement ${chain.call_hash}`, plaintext_missing: !Boolean(template?.inner_op), created_at: this.now() }
      if (!provisional) { delete row.pending_action; delete row.action_receipt; delete row.announcement_receipt; delete row.multisig_announcement }
      if (provisional) rows[rows.indexOf(provisional)] = row
      else rows.push(row)
      return row
    })
  }
  update(wallet: string, index: number, updates: Partial<ProxyAnnouncement>, scope: AnnouncementScope = {}): Promise<ProxyAnnouncement | undefined> {
    const saved = normalizeCallSemantics(updates)
    return this.mutate(rows => { const i = rows.findIndex(row => row.index === index && row.controller_wallet === wallet && this.scope(row, scope)); if (i < 0) return undefined; return rows[i] = { ...rows[i], ...saved, index } })
  }
  markStatus(wallet: string, index: number, status: string, scope: AnnouncementScope = {}) { return this.update(wallet, index, { status, updated_at: this.now() }, scope) }
  claimAction(wallet: string, index: number, kind: 'execute' | 'cancel', scope: AnnouncementScope, allowRepeat = false): Promise<ProxyAnnouncement> {
    return this.mutate(rows => {
      const row = rows.find(row => row.index === index && row.controller_wallet === wallet && this.scope(row, scope))
      if (!row || row.status !== 'pending') throw new Error('announcement is not pending')
      if (rows.some(other => other.index !== row.index && other.pending_action && this.sameCommitment(other, row))) throw new Error('an identical commitment has an uncertain action; reconcile it before retrying')
      const executed = kind === 'execute' && !allowRepeat && rows.find(other => other.status === 'executed' && this.sameCommitment(other, row))
      if (executed) throw new Error(`this payload already executed as #${executed.index}; explicit allowRepeat is required`)
      if (row.pending_action && !(row.pending_action.kind === kind && row.pending_action.state === 'awaiting_approval')) throw new Error('a previous action has an uncertain submission outcome; reconcile it before retrying')
      row.pending_action = { kind, state: 'submitting', created_at: this.now() }
      return row
    })
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
