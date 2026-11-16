import { evaluatePolicy } from './policy'
import { Intent, intentFromJSON, intentSignerRole, type IntentContext, type PreparedIntent } from './intents'
import { ProxyAnnouncementStore, normalizeCallSemantics, announcementCategory, type NewProxyAnnouncement, type ProxyAnnouncement, type AnnouncementMultisig } from './announcement-store'
import { decodedCallDescriptor, sameCallSemantics, validateAnnouncedCall, announcedCallLeaves } from './proxy-announcements'
import { protectedManagementPath, assertProtectedManagementAllowed } from './protected-account-config'
import { requireProtectedChain, sameProtectedAccount, shieldedUncertainReceipt, type ProtectedProxyBackend, type ProtectedProxySnapshot, type ProtectedProxyReceipt, type ProtectedProxyEvent } from './protected-proxy-types'
import { bytesHex as hex, hexBytes } from './portable'
import { isDefiniteSubmissionRejection } from './transport'

export interface AnnouncementAccount {
  controller_wallet: string; controller_ss58: string; protected_ss58: string; genesis_hash?: string; delay_blocks: number
  [key: string]: any
}
export interface AnnounceableCall {
  call_data: string; call_hash: string; runtime_spec_version: number; call_semantics: unknown; prepared: PreparedIntent
}
export interface PreparedAnnouncement extends AnnounceableCall { readonly record: AnnouncementAccount; readonly row: NewProxyAnnouncement }
export interface AnnounceOptions { category?: string; summary?: string; purpose?: string; allowManagement?: boolean; metadata?: Record<string, unknown> }
export interface AnnouncementActionOptions { signerAddress?: string; allowRepeat?: boolean; allowManagement?: boolean }
export interface AnnouncementActionReceipt { extrinsicHash: string; blockHash?: string }
export interface ProtectedAnnouncementResult { row: ProxyAnnouncement; receipt?: ProtectedProxyReceipt; recovered?: boolean; completed: boolean }
const preparedAnnouncements = new WeakSet<object>()

/** Capture approved bytes once under the real account; replay never requotes dynamic inputs. */
export async function composeAnnounceableCall(snapshot: ProtectedProxySnapshot, intent: Intent, context: IntentContext): Promise<AnnounceableCall> {
  const prepared = await snapshot.prepare(intent, context), bytes = await snapshot.compose(prepared.call)
  const decoded = await snapshot.decodeCall(bytes)
  validateAnnouncedCall(decodedCallDescriptor(decoded))
  return { call_data: hex(bytes), call_hash: hex(snapshot.hashCallData(bytes)), runtime_spec_version: snapshot.specVersion, call_semantics: normalizeCallSemantics(decoded), prepared }
}

function freeze<T>(value: T): T { if (value && typeof value === 'object' && !(value instanceof Uint8Array)) { for (const child of Object.values(value)) freeze(child); Object.freeze(value) } return value }
function number(value: unknown, label: string): number { const n = typeof value === 'bigint' ? Number(value) : value; if (typeof n !== 'number' || !Number.isSafeInteger(n) || n < 0) throw new TypeError(`${label} is not a nonnegative safe integer`); return n }
function hash(value: unknown): string { if (typeof value !== 'string' || !/^0x[0-9a-f]{64}$/i.test(value)) throw new TypeError('invalid announcement hash'); return value.toLowerCase() }
function attrs(event: ProtectedProxyEvent, names: string[]): Record<string, any> { return Array.isArray(event.attributes) ? Object.fromEntries(names.map((name, i) => [name, event.attributes[i]])) : event.attributes ?? {} }
function moduleEvents(receipt: ProtectedProxyReceipt, module: string, name: string): ProtectedProxyEvent[] { return receipt.events.filter(event => event.module_id === module && event.event_id === name) }
function successful(receipt: ProtectedProxyReceipt): boolean { return evaluatePolicy<{successful: boolean}>('protected.receipt_state', receipt).successful }
function approvalOnly(receipt: ProtectedProxyReceipt): boolean { return evaluatePolicy<{approval_only: boolean}>('protected.receipt_state', receipt).approval_only }
function included(receipt: ProtectedProxyReceipt): boolean { return evaluatePolicy<{included: boolean}>('protected.receipt_state', receipt).included }
function retired(status: string): Partial<ProxyAnnouncement> { return evaluatePolicy('protected.announcement_retirement', { status }) }
function uncertainReceipt(error: unknown): ProtectedProxyReceipt | undefined {
  const shielded = shieldedUncertainReceipt(error)
  if (shielded) return shielded
  const value = error as any
  return typeof value?.extrinsicHash === 'string' && /^0x[0-9a-f]{64}$/i.test(value.extrinsicHash) ? { status: 'unknown', success: null, events: [], extrinsicHash: value.extrinsicHash, blockHash: value.blockHash ?? undefined } : undefined
}

/** Delayed pure-proxy announcements, with explicit application persistence and submission. */
export class ProtectedAnnouncements {
  constructor(readonly backend: ProtectedProxyBackend, readonly journal: ProxyAnnouncementStore) {}
  private scope(record: AnnouncementAccount) { return { genesisHash: record.genesis_hash } }
  private same(left: string, right: string): boolean { return sameProtectedAccount(this.backend, left, right) }
  private async snapshot(record: AnnouncementAccount): Promise<ProtectedProxySnapshot> { const view = await this.backend.snapshot(); requireProtectedChain(view.genesisHash, record.genesis_hash ?? ''); return view }
  private assertRow(record: AnnouncementAccount, row: ProxyAnnouncement): void {
    requireProtectedChain(row.genesis_hash ?? '', record.genesis_hash ?? '')
    if (row.controller_wallet !== record.controller_wallet || !this.same(row.delegate_ss58, record.controller_ss58) || !this.same(row.real_ss58, record.protected_ss58)) throw new Error('announcement does not belong to this protected account and controller')
  }
  async liveDelay(record: AnnouncementAccount, snapshot?: ProtectedProxySnapshot, requireGrant = true): Promise<number> {
    const view = snapshot ?? await this.snapshot(record); requireProtectedChain(view.genesisHash, record.genesis_hash ?? '')
    const stored = await view.query('Proxy', 'Proxies', [record.protected_ss58])
    if (!Array.isArray(stored) || !Array.isArray(stored[0])) throw new Error('proxy grants unavailable')
    const delays = stored[0].filter((grant: any) => this.same(String(grant.delegate), record.controller_ss58) && (typeof grant.proxy_type === 'string' ? grant.proxy_type : Object.keys(grant.proxy_type ?? {})[0]) === 'Any').map((grant: any) => number(grant.delay, 'proxy delay'))
    if (!delays.length) { if (requireGrant) throw new Error('controller has no live Any grant on the protected account'); return number(record.delay_blocks, 'saved proxy delay') }
    return Math.min(...delays)
  }
  private async onChain(record: AnnouncementAccount, snapshot: ProtectedProxySnapshot, delay: number): Promise<Array<NewProxyAnnouncement>> {
    const stored = await snapshot.query('Proxy', 'Announcements', [record.controller_ss58])
    if (!Array.isArray(stored) || !Array.isArray(stored[0])) throw new Error('proxy announcements unavailable')
    const counts = new Map<string, number>(), rows: NewProxyAnnouncement[] = []
    for (const item of stored[0]) {
      if (!this.same(String(item.real), record.protected_ss58)) continue
      const callHash = hash(item.call_hash), height = number(item.height, 'announcement height'), identity = `${callHash}:${height}`, ordinal = counts.get(identity) ?? 0
      counts.set(identity, ordinal + 1)
      rows.push({ controller_wallet: record.controller_wallet, genesis_hash: record.genesis_hash, delegate_ss58: record.controller_ss58, real_ss58: record.protected_ss58, call_hash: callHash, announced_at_block: height, announcement_ordinal: ordinal, delay_blocks: delay, inner_op: '', inner_args: {} })
    }
    return rows.sort((a, b) => a.announced_at_block - b.announced_at_block || a.call_hash.localeCompare(b.call_hash) || (a.announcement_ordinal ?? 0) - (b.announcement_ordinal ?? 0))
  }
  async prepare(record: AnnouncementAccount, value: Intent | Record<string, unknown>, options: AnnounceOptions = {}): Promise<PreparedAnnouncement> {
    record = structuredClone(record)
    const intent = intentFromJSON(value), view = await this.snapshot(record), delay = await this.liveDelay(record, view)
    if (delay <= 0) throw new Error('controller has a zero-delay Any grant; protection is not active')
    if (intent.op === 'pow_register') throw new Error('PoW registration cannot wait behind a protected controller delay')
    if (protectedManagementPath(intent) && !options.allowManagement) throw new Error('protected management requires an explicitly reviewed settings or rotation workflow')
    const context: IntentContext = { coldkey: record.protected_ss58, feePayer: record.controller_ss58, ...(intentSignerRole(intent) === 'hotkey' ? { hotkey: record.protected_ss58, hotkeyPublicKey: this.backend.accountId(record.protected_ss58) } : {}) }
    const payload = await composeAnnounceableCall(view, intent, context)
    if (payload.prepared.violations.length) throw new Error(payload.prepared.violations.join('; '))
    const { op, ...args } = intent.toJSON()
    const row: NewProxyAnnouncement = { ...normalizeCallSemantics(options.metadata ?? {}), controller_wallet: record.controller_wallet, delegate_ss58: record.controller_ss58, real_ss58: record.protected_ss58, genesis_hash: record.genesis_hash, inner_op: String(op), inner_args: args, category: options.category ?? announcementCategory(intent.op), summary: options.summary ?? payload.prepared.effects[0] ?? intent.op, delay_blocks: delay, announced_at_block: view.block, force_proxy_type: 'Any', management_approved: options.allowManagement === true, call_data: payload.call_data, call_hash: payload.call_hash, runtime_spec_version: payload.runtime_spec_version, call_semantics: payload.call_semantics }
    const result = freeze({ ...payload, record, row }); preparedAnnouncements.add(result); return result
  }
  async announce(record: AnnouncementAccount, value: Intent | PreparedAnnouncement | Record<string, unknown>, options: AnnounceOptions = {}): Promise<ProtectedAnnouncementResult> {
    const prepared = preparedAnnouncements.has(value) ? value as PreparedAnnouncement : await this.prepare(record, value as Intent, options)
    if (prepared.record.controller_wallet !== record.controller_wallet || !this.same(prepared.record.protected_ss58, record.protected_ss58) || !this.same(prepared.record.controller_ss58, record.controller_ss58)) throw new Error('prepared announcement account changed')
    requireProtectedChain(prepared.record.genesis_hash ?? '', record.genesis_hash ?? '')
    const view = await this.snapshot(record), delay = await this.liveDelay(record, view)
    if (delay <= 0) throw new Error('controller has a zero-delay Any grant; protection is not active')
    await this.validatePayload(prepared.row, view)
    const live = await this.onChain(record, view, delay)
    const matching = live.find(row => row.call_hash === prepared.call_hash)
    if (matching) {
      const discovered = await this.journal.upsertOnChain(matching as any)
      const row = await this.journal.add({ ...prepared.row, ...matching, inner_op: prepared.row.inner_op, inner_args: prepared.row.inner_args })
      return { row: { ...row, index: discovered.index, on_chain: true }, recovered: true, completed: true }
    }
    const row = await this.journal.reserve({ ...prepared.row, delay_blocks: delay, announced_at_block: view.block })
    let receipt: ProtectedProxyReceipt
    try { receipt = await this.backend.submit(intentFromJSON({ op: 'announce_proxy_call', real_ss58: row.real_ss58, call_hash: row.call_hash }), { signerAddress: record.controller_ss58, purpose: options.purpose ?? `announce ${row.inner_op} for protected account` }) }
    catch (error) {
      if (isDefiniteSubmissionRejection(error)) await this.journal.markStatus(record.controller_wallet, row.index, 'failed', this.scope(record))
      else { const receipt = uncertainReceipt(error); if (receipt) await this.journal.update(record.controller_wallet, row.index, { announcement_receipt: receipt }, this.scope(record)) }
      throw error
    }
    const updates: Partial<ProxyAnnouncement> = { announcement_receipt: receipt, announced_at_block: receipt.blockNumber ?? row.announced_at_block, executable_at_block: (receipt.blockNumber ?? row.announced_at_block) + delay }
    if (approvalOnly(receipt)) { updates.status = 'awaiting_approval'; updates.multisig_announcement = this.multisigIdentity(receipt, row.delegate_ss58) }
    else if (successful(receipt) && included(receipt)) updates.status = 'pending'
    else if (receipt.success === false && included(receipt)) updates.status = 'failed'
    const saved = await this.journal.update(record.controller_wallet, row.index, updates, this.scope(record)) ?? row
    return { row: saved, receipt, completed: saved.status === 'pending' }
  }
  private multisigIdentity(receipt: ProtectedProxyReceipt, delegate: string): AnnouncementMultisig | undefined {
    for (const event of receipt.events) {
      if (event.module_id !== 'Multisig' || !['NewMultisig', 'MultisigApproval'].includes(event.event_id)) continue
      const a = attrs(event, event.event_id === 'NewMultisig' ? ['approving', 'multisig', 'call_hash'] : ['approving', 'timepoint', 'multisig', 'call_hash'])
      if (typeof a.multisig === 'string' && this.same(a.multisig, delegate)) return { account_ss58: delegate, call_hash: hash(a.call_hash), ...(a.timepoint ? { timepoint: a.timepoint } : receipt.blockNumber != null && receipt.extrinsicIndex != null ? { timepoint: { height: receipt.blockNumber, index: receipt.extrinsicIndex } } : {}) }
    }
    return undefined
  }
  private callAccount(value: unknown): string | undefined {
    if (typeof value === 'string') return value
    if (value instanceof Uint8Array && value.length === 32) return this.backend.ss58Encode(value)
    if (value && typeof value === 'object' && Object.keys(value).length === 1 && 'Id' in value) return this.callAccount((value as any).Id)
    return undefined
  }
  private async recoverApprovalIdentity(row: ProxyAnnouncement): Promise<AnnouncementMultisig | undefined> {
    if (!row.announced_at_block || !this.backend.events || !this.backend.extrinsicCall) return undefined
    const height = number(row.announced_at_block, 'approval inclusion height')
    const block = this.backend.blockHash ? await this.backend.blockHash(height) : height
    const identities = new Map<string, AnnouncementMultisig>()
    for (const event of await this.backend.events(block)) {
      const identity = this.multisigIdentity({ status: 'finalized', success: true, events: [event] }, row.delegate_ss58)
      if (!identity) continue
      const timepoint = identity.timepoint ?? { height, index: event.extrinsic_idx! }
      const openingHeight = number(timepoint.height, 'multisig opening height'), openingIndex = number(timepoint.index, 'multisig opening index')
      if (openingHeight > height) continue
      const opening = await this.backend.extrinsicCall(openingHeight, openingIndex)
      if (!opening) continue
      const outer = decodedCallDescriptor(opening)
      // Only a directly embedded announce proves the original operation. A
      // proxy, sudo or unrelated nested call cannot lend its event identity.
      if (outer.pallet !== 'Multisig' || !['as_multi', 'as_multi_safe'].includes(outer.call)) continue
      const call = decodedCallDescriptor(outer.params.call), real = this.callAccount(call.params.real)
      if (call.pallet !== 'Proxy' || call.call !== 'announce' || !real || !this.same(real, row.real_ss58) || hash(call.params.call_hash) !== hash(row.call_hash)) continue
      identities.set(identity.call_hash, { ...identity, timepoint: { height: openingHeight, index: openingIndex } })
    }
    return identities.size === 1 ? identities.values().next().value : undefined
  }
  private async approvalState(row: ProxyAnnouncement, view: ProtectedProxySnapshot): Promise<{ state: 'pending' | 'absent' | 'unknown'; identity?: AnnouncementMultisig }> {
    try {
      if (view.block < row.announced_at_block) return { state: 'unknown' }
      let identity = row.multisig_announcement
      if (!identity && row.announcement_receipt?.extrinsicHash && this.backend.receipt) {
        const receipt = await this.backend.receipt(row.announcement_receipt.extrinsicHash, row.announcement_receipt.blockHash)
        if (receipt) identity = this.multisigIdentity(receipt, row.delegate_ss58)
      }
      identity ??= await this.recoverApprovalIdentity(row)
      if (!identity || !this.same(identity.account_ss58, row.delegate_ss58)) return { state: 'unknown' }
      return { state: await view.query('Multisig', 'Multisigs', [identity.account_ss58, hash(identity.call_hash)]) == null ? 'absent' : 'pending', identity }
    } catch { return { state: 'unknown' } }
  }
  private async resolveAction(row: ProxyAnnouncement, view: ProtectedProxySnapshot, onChain: boolean): Promise<ProxyAnnouncement> {
    const action = row.pending_action
    if (!action) return row
    let receipt: ProtectedProxyReceipt | undefined = action.receipt
    if (receipt?.extrinsicHash && this.backend.receipt) {
      try { receipt = await this.backend.receipt(receipt.extrinsicHash, receipt.blockHash ?? undefined) ?? receipt } catch { return row }
    }
    if (!receipt || !included(receipt) || receipt.blockNumber != null && view.block < receipt.blockNumber) return row
    if (approvalOnly(receipt)) {
      const identity = action.multisig
      if (identity && onChain) {
        try {
          if (await view.query('Multisig', 'Multisigs', [identity.account_ss58, identity.call_hash]) == null) return { ...row, pending_action: null, action_receipt: receipt }
        } catch { /* Retain the uncertain approval. */ }
      }
      return row
    }
    if (!successful(receipt)) return { ...row, ...(onChain ? {} : retired('failed')), pending_action: null, action_receipt: receipt }
    if (!onChain && (action.kind === 'cancel' || moduleEvents(receipt, 'Proxy', 'ProxyExecuted').length > 0)) return { ...row, ...retired(action.kind === 'execute' ? 'executed' : 'cancelled'), pending_action: null, action_receipt: receipt }
    return row
  }
  /** Recover an action completed by another multisig member using its final transaction. */
  async recoverAction(record: AnnouncementAccount, index: number, reference: AnnouncementActionReceipt): Promise<ProtectedAnnouncementResult> {
    const row = await this.journal.getByIndex(record.controller_wallet, index, this.scope(record))
    if (!row) throw new Error('announcement index not found on this chain')
    this.assertRow(record, row)
    const action = row.pending_action
    if (!action) return { row, recovered: true, completed: ['executed', 'cancelled'].includes(row.status) }
    const identity = action.multisig as AnnouncementMultisig | undefined
    if (!identity?.timepoint) throw new Error('original multisig account, call hash, and timepoint are required for action recovery')
    const lookup = this.backend.finalizedReceipt ?? this.backend.receipt
    if (!lookup || !this.backend.blockHash || !this.backend.extrinsicCall) throw new Error('finalized receipt and historical call lookup are required for action recovery')
    const transaction = hash(reference.extrinsicHash)
    const receipt = await lookup.call(this.backend, transaction, reference.blockHash == null ? undefined : hash(reference.blockHash))
    if (!receipt || receipt.status !== 'finalized' || !receipt.blockHash || receipt.blockNumber == null || receipt.extrinsicIndex == null || hash(receipt.extrinsicHash) !== transaction) throw new Error('multisig action receipt is not finalized or cannot be located')
    if (reference.blockHash != null && hash(reference.blockHash) !== hash(receipt.blockHash)) throw new Error('multisig action receipt does not match the requested inclusion block')
    const height = number(receipt.blockNumber, 'completion block'), indexInBlock = number(receipt.extrinsicIndex, 'completion extrinsic index')
    if (hash(await this.backend.blockHash(height)) !== hash(receipt.blockHash)) throw new Error('multisig action receipt is not in the canonical chain')
    const view = await this.snapshot(record), delay = await this.liveDelay(record, view, false)
    if (view.block < height) throw new Error('snapshot has not observed the completed multisig action')
    if (await view.query('Multisig', 'Multisigs', [identity.account_ss58, hash(identity.call_hash)]) != null) throw new Error('multisig commitment is still pending; its completion cannot be recovered')
    const account = (value: unknown): string => {
      const address = this.callAccount(value)
      if (!address) throw new Error('multisig action account cannot be decoded')
      return hex(this.backend.accountId(address))
    }
    const call = (value: unknown) => {
      const normalize = (value: any): any => {
        if (value instanceof Uint8Array) return value
        if (Array.isArray(value)) return value.map(normalize)
        if (value == null || typeof value !== 'object') return value
        const result = Object.fromEntries(Object.entries(value).map(([key, value]) => [key, normalize(value)]))
        if (result.pallet === 'Proxy' && result.params) for (const key of ['real', 'delegate']) if (result.params[key] != null) result.params[key] = account(result.params[key])
        return result
      }
      return normalize(decodedCallDescriptor(value))
    }
    const outer = call(await this.backend.extrinsicCall(receipt.blockHash, indexInBlock))
    const cancelled = moduleEvents(receipt, 'Multisig', 'MultisigCancelled').length > 0
    const opening = cancelled ? call(await this.backend.extrinsicCall(identity.timepoint.height, identity.timepoint.index)) : undefined
    const events = receipt.events.map(event => {
      if (event.extrinsic_idx != null && event.extrinsic_idx !== indexInBlock) throw new Error('multisig action receipt contains events from another transaction')
      if (event.module_id === 'Multisig' && ['MultisigExecuted', 'MultisigCancelled'].includes(event.event_id)) {
        const value = attrs(event, ['approving', 'timepoint', 'multisig', 'call_hash', 'result'])
        return { ...event, attributes: { ...value, multisig: account(value.multisig) } }
      }
      return event.module_id === 'Proxy' && event.event_id === 'ProxyExecuted' ? { ...event, attributes: attrs(event, ['result']) } : event
    })
    const onChain = (await this.onChain(record, view, delay)).some(item => this.journal.sameIdentity(item, row))
    const committed = row.call_semantics ?? (row.call_data ? await view.decodeCall(hexBytes(row.call_data)) : undefined)
    const outcome = evaluatePolicy<{ status: string; execution_uncertain?: boolean }>('protected.multisig_action_result', {
      kind: action.kind, on_chain: onChain, call: outer, opening_call: opening,
      identity: { account: account(identity.account_ss58), call_hash: hash(identity.call_hash), timepoint: identity.timepoint },
      announcement: { real: account(row.real_ss58), delegate: account(row.delegate_ss58), call_hash: hash(row.call_hash), call: committed == null ? null : call(committed) },
      receipt: { ...receipt, events },
    })
    const saved = await this.journal.settleAction(record.controller_wallet, row.index, action, { ...outcome, pending_action: null, action_receipt: receipt }, this.scope(record))
    if (!saved || saved.pending_action) throw new Error('announcement action changed during recovery')
    return { row: saved, receipt, recovered: true, completed: ['executed', 'cancelled'].includes(saved.status) }
  }
  async reconcile(record: AnnouncementAccount, options: { persist?: boolean } = {}): Promise<ProxyAnnouncement[]> {
    const view = await this.snapshot(record), delay = await this.liveDelay(record, view, false), persist = options.persist !== false
    const live = await this.onChain(record, view, delay)
    const history = await this.journal.listForWallet(record.controller_wallet, { ...this.scope(record), includeDone: true })
    // Dry projection uses a private journal so discovered indexes are never advertised as reusable.
    const journal = persist ? this.journal : new ProxyAnnouncementStore({ read: async () => ({ version: 1, announcements: structuredClone(history) }), write: async () => {} }, address => hex(this.backend.accountId(address)))
    const merged: ProxyAnnouncement[] = []
    for (const item of live) {
      let saved = await journal.upsertOnChain(item as any)
      if (saved.pending_action) {
        const before = saved, resolved = await this.resolveAction(saved, view, true)
        saved = resolved
        if (persist && resolved !== before) saved = await this.journal.settleAction(record.controller_wallet, saved.index, before.pending_action, { status: resolved.status, execution_uncertain: resolved.execution_uncertain, pending_action: resolved.pending_action, action_receipt: resolved.action_receipt }, this.scope(record)) ?? resolved
      }
      const exact = history.find(row => this.journal.sameIdentity(row, item))
      merged.push({ ...saved, ...(!persist && !exact ? { index: undefined as any, provisional: true } : {}), on_chain: true, ready: view.block >= saved.executable_at_block })
    }
    const local = persist ? await this.journal.listForWallet(record.controller_wallet, this.scope(record)) : history.filter(row => ['pending', 'submitting', 'awaiting_approval'].includes(row.status))
    for (let row of local) {
      if (!this.same(row.delegate_ss58, record.controller_ss58) || !this.same(row.real_ss58, record.protected_ss58)) continue
      if (live.some(item => this.journal.sameIdentity(row, item))) continue
      if (row.pending_action) {
        const before = row, resolved = await this.resolveAction(row, view, false)
        row = resolved
        if (persist && resolved !== before) row = await this.journal.settleAction(record.controller_wallet, row.index, before.pending_action, { status: resolved.status, execution_uncertain: resolved.execution_uncertain, pending_action: resolved.pending_action, action_receipt: resolved.action_receipt }, this.scope(record)) ?? resolved
        if (row.pending_action) { merged.push({ ...row, on_chain: false, ready: false, approval_state: 'action_unknown' }); continue }
        if (['executed', 'cancelled', 'failed'].includes(row.status)) continue
      }
      if (row.status === 'submitting') {
        const original = row.announcement_receipt
        if (original?.extrinsicHash && this.backend.receipt) {
          try { const receipt = await this.backend.receipt(original.extrinsicHash, original.blockHash); if (receipt && included(receipt)) {
            const status = approvalOnly(receipt) ? 'awaiting_approval' : successful(receipt) ? 'pending' : receipt.success === false ? 'failed' : row.status
            row = { ...row, status, announced_at_block: receipt.blockNumber ?? row.announced_at_block, executable_at_block: (receipt.blockNumber ?? row.announced_at_block) + delay, announcement_receipt: receipt, multisig_announcement: this.multisigIdentity(receipt, row.delegate_ss58) ?? row.multisig_announcement }
            if (persist) await this.journal.update(record.controller_wallet, row.index, row, this.scope(record))
          } } catch { /* An unavailable historical receipt is not proof of failure. */ }
        }
        if (row.status === 'submitting') { merged.push({ ...row, on_chain: false, ready: false, approval_state: 'submission_unknown' }); continue }
      }
      if (row.status === 'awaiting_approval') {
        const { state, identity } = await this.approvalState(row, view)
        if (identity) {
          row = { ...row, multisig_announcement: identity }
          if (persist) await this.journal.update(record.controller_wallet, row.index, { multisig_announcement: identity }, this.scope(record))
        }
        if (state !== 'absent') { merged.push({ ...row, on_chain: false, ready: false, approval_state: state }); continue }
      }
      if (row.status === 'pending' && view.block < row.announced_at_block) { merged.push({ ...row, on_chain: false, ready: false, approval_state: 'inclusion_not_observed' }); continue }
      if (row.status !== 'failed' && persist) await this.journal.markStatus(record.controller_wallet, row.index, 'gone', this.scope(record))
    }
    return merged.sort((a, b) => (a.index ?? Number.MAX_SAFE_INTEGER) - (b.index ?? Number.MAX_SAFE_INTEGER))
  }
  async list(record: AnnouncementAccount, options: { persist?: boolean } = {}) { return this.reconcile(record, options) }
  private async validatePayload(row: Pick<ProxyAnnouncement, 'call_hash' | 'inner_op' | 'inner_args'> & Partial<ProxyAnnouncement>, view: ProtectedProxySnapshot): Promise<void> {
    if (!('call_data' in row) && row.inner_op) {
      const intent = intentFromJSON({ ...row.inner_args, op: row.inner_op }), context: IntentContext = { coldkey: row.real_ss58!, feePayer: row.delegate_ss58!, ...(intentSignerRole(intent) === 'hotkey' ? { hotkey: row.real_ss58!, hotkeyPublicKey: this.backend.accountId(row.real_ss58!) } : {}) }
      const legacy = await composeAnnounceableCall(view, intent, context)
      if (legacy.call_hash !== hash(row.call_hash)) throw new Error('rebuilt legacy call differs from the announcement hash; cancel and re-announce')
      row.call_data = legacy.call_data
    }
    if (typeof row.call_data !== 'string' || !/^0x(?:[0-9a-f]{2})+$/i.test(row.call_data)) throw new Error('announcement has no valid saved call bytes; cancel or explicitly prepare a new announcement')
    const bytes = hexBytes(row.call_data)
    if (hex(view.hashCallData(bytes)) !== hash(row.call_hash)) throw new Error('saved call bytes do not match the selected announcement hash')
    const decoded = await view.decodeCall(bytes), descriptor = decodedCallDescriptor(decoded)
    validateAnnouncedCall(descriptor)
    if (row.runtime_spec_version != null && row.runtime_spec_version !== view.specVersion && (row.call_semantics == null || !sameCallSemantics(decoded, row.call_semantics))) throw new Error('runtime changed announced call semantics; cancel and re-announce')
    const encoded = { ...row.inner_args, op: row.inner_op }
    const sensitive = [...announcedCallLeaves(descriptor)].some(call => call.pallet === 'Proxy' && ['add_proxy', 'remove_proxy', 'remove_proxies', 'kill_pure', 'kill_safe_pure'].includes(call.call))
    const management = protectedManagementPath(encoded)
    if (sensitive && !management) throw new Error('saved management call differs from its declared protected intent')
    if (management) {
      if (row.management_approved !== true) throw new Error('saved management call lacks explicit protected workflow approval')
      assertProtectedManagementAllowed(row, encoded)
      // Settings/rotation calls are fixed operations. Bind their declared review
      // metadata to the exact bytes, rather than trusting an editable category.
      const intent = intentFromJSON(encoded), context: IntentContext = { coldkey: row.real_ss58!, feePayer: row.delegate_ss58!, ...(intentSignerRole(intent) === 'hotkey' ? { hotkey: row.real_ss58!, hotkeyPublicKey: this.backend.accountId(row.real_ss58!) } : {}) }
      const approved = await view.prepare(intent, context)
      if (hex(await view.compose(approved.call)) !== row.call_data.toLowerCase()) throw new Error('saved management bytes differ from the reviewed settings or rotation intent')
    }
    const recomposed = await view.compose({ ...descriptor, callData: row.call_data })
    if (hex(recomposed) !== row.call_data.toLowerCase()) throw new Error('saved call no longer encodes identically')
  }
  async inspect(record: AnnouncementAccount, index: number): Promise<ProxyAnnouncement> {
    const row = await this.journal.getByIndex(record.controller_wallet, index, this.scope(record))
    if (!row) throw new Error('announcement index not found on this chain')
    this.assertRow(record, row)
    const view = await this.snapshot(record), delay = await this.liveDelay(record, view, false), live = await this.onChain(record, view, delay)
    if (row.inner_op) await this.validatePayload(row, view)
    const onChain = live.some(item => this.journal.sameIdentity(item, row))
    return { ...row, delay_blocks: delay, executable_at_block: row.announced_at_block + delay, on_chain: onChain, ready: onChain && view.block >= row.announced_at_block + delay, payload_valid: Boolean(row.inner_op), current_block: view.block }
  }
  async execute(record: AnnouncementAccount, index: number, options: AnnouncementActionOptions = {}): Promise<ProtectedAnnouncementResult> {
    const rows = await this.reconcile(record), row = rows.find(row => row.index === index)
    if (!row) throw new Error('announcement is no longer pending')
    this.assertRow(record, row)
    if (!row.on_chain || row.status !== 'pending') throw new Error('announcement has not landed on chain yet')
    if (row.plaintext_missing || !row.inner_op) throw new Error('announcement plaintext is missing; cancel instead')
    const view = await this.snapshot(record), delay = await this.liveDelay(record, view)
    if (view.block < row.announced_at_block + delay) throw new Error(`announcement is not ready until block ${row.announced_at_block + delay}`)
    await this.validatePayload(row, view)
    const live = await this.onChain(record, view, delay)
    if (!live.some(item => this.journal.sameIdentity(item, row))) throw new Error('selected announcement is no longer on chain')
    if (row.category === 'rotate-controller' && (live.length !== 1 || !this.journal.sameIdentity(live[0], row))) throw new Error('old controller queue changed since rotation was reviewed')
    const prior = await this.journal.executedCommitmentIndex(row)
    if (prior != null && !options.allowRepeat) {
      const previous = await this.journal.getByIndex(record.controller_wallet, prior, this.scope(record))
      throw new Error(`this payload ${previous?.status === 'executed' && !previous.execution_uncertain ? 'already executed' : 'may have executed'} as #${prior}; explicit allowRepeat is required`)
    }
    const intent = intentFromJSON({ op: 'execute_proxy_announced', delegate_ss58: row.delegate_ss58, real_ss58: row.real_ss58, inner_op: row.inner_op, inner_args: row.inner_args, force_proxy_type: row.force_proxy_type ?? 'Any', announced_call_data: row.call_data, expected_call_hash: row.call_hash, announced_spec_version: row.runtime_spec_version, announced_call_semantics: row.call_semantics })
    return this.action(record, row, 'execute', intent, options.signerAddress ?? record.controller_ss58, options.allowRepeat === true)
  }
  async cancel(record: AnnouncementAccount, index: number, options: AnnouncementActionOptions = {}): Promise<ProtectedAnnouncementResult> {
    const original = await this.journal.getByIndex(record.controller_wallet, index, this.scope(record))
    if (!original) throw new Error('announcement index not found on this chain')
    this.assertRow(record, original)
    const rows = await this.reconcile(record), row = rows.find(row => row.index === index)
    if (!row) return { row: await this.journal.getByIndex(record.controller_wallet, index, this.scope(record)) ?? original, completed: true, recovered: true }
    if (!row.on_chain || row.status !== 'pending') throw new Error('announcement submission or multisig approval remains uncertain; resolve the underlying commitment before cancellation')
    const signer = options.signerAddress ?? record.controller_ss58
    if (!this.same(signer, row.delegate_ss58)) throw new Error('cancellation must be authorized by the announcing delegate')
    return this.action(record, row, 'cancel', intentFromJSON({ op: 'remove_proxy_announcement', real_ss58: row.real_ss58, call_hash: row.call_hash }), signer)
  }
  private async action(record: AnnouncementAccount, row: ProxyAnnouncement, kind: 'execute' | 'cancel', intent: Intent, signer: string, allowRepeat = false): Promise<ProtectedAnnouncementResult> {
    await this.journal.claimAction(record.controller_wallet, row.index, kind, this.scope(record), allowRepeat)
    let receipt: ProtectedProxyReceipt
    try { receipt = await this.backend.submit(intent, { signerAddress: signer, purpose: `${kind} protected announcement #${row.index}` }) }
    catch (error) {
      if (isDefiniteSubmissionRejection(error)) await this.journal.update(record.controller_wallet, row.index, { pending_action: null }, this.scope(record))
      else { const receipt = uncertainReceipt(error); if (receipt) await this.journal.update(record.controller_wallet, row.index, { pending_action: { kind, state: 'submission_unknown', receipt } }, this.scope(record)) }
      throw error
    }
    if (approvalOnly(receipt)) {
      const saved = await this.journal.update(record.controller_wallet, row.index, { pending_action: { kind, state: 'awaiting_approval', receipt, multisig: this.multisigIdentity(receipt, signer) } }, this.scope(record)) ?? row
      return { row: saved, receipt, completed: false }
    }
    // Preserve the exact submission before any follow-up RPC can fail. An
    // unavailable post-submit snapshot must remain recoverable without replay.
    const pendingAction = { kind, state: 'submission_unknown', receipt }
    await this.journal.update(record.controller_wallet, row.index, { pending_action: pendingAction }, this.scope(record))
    let complete = false
    if (successful(receipt) && included(receipt)) {
      const view = await this.snapshot(record), delay = await this.liveDelay(record, view, false), live = await this.onChain(record, view, delay)
      const absent = !live.some(item => this.journal.sameIdentity(item, row))
      complete = absent && (receipt.blockNumber == null || view.block >= receipt.blockNumber) && (kind === 'cancel' || moduleEvents(receipt, 'Proxy', 'ProxyExecuted').length > 0)
    }
    const updates = complete ? { ...retired(kind === 'execute' ? 'executed' : 'cancelled'), pending_action: null, action_receipt: receipt } : receipt.success === false && included(receipt) ? { pending_action: null, action_receipt: receipt } : { pending_action: { kind, state: 'submission_unknown', receipt } }
    const saved = await this.journal.settleAction(record.controller_wallet, row.index, pendingAction, updates, this.scope(record)) ?? row
    return { row: saved, receipt, completed: complete }
  }
}
