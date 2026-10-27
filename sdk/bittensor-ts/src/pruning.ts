import { buildIntent, type Intent } from './intents'
import { eventName } from './receipt'
import type { ReadView } from './reads'

export interface PruningState { block: number; genesis: string; owner: string; population: number; pendingTarget: number | null; mechanisms: number; minimum: number; maximum: number; mode: string; frozen: boolean; yumaCeiling: number }
export interface PruningRecord { netuid: number; target: number; operation: string; state: PruningState }
/** reserve must atomically refuse an existing record, including from another process/tab. */
export interface PruningJournal { pending(): Promise<PruningRecord | undefined>; reserve(record: PruningRecord): Promise<void>; clear(): Promise<void> }
export interface PruningClient { at(): Promise<ReadView>; mapHasEntries(pallet: string, item: string, params: unknown[], block: number): Promise<boolean> }
export function pruningClient(client: { at(): Promise<ReadView>; queryMap(pallet: string, item: string, params: any[], block: number, options: { pageSize: number; maxResults: number }): Promise<unknown[]> }): PruningClient {
  return { at: () => client.at(), mapHasEntries: async (pallet, item, params, block) => (await client.queryMap(pallet, item, params, block, { pageSize: 1, maxResults: 1 })).length > 0 }
}
export async function pruningState(client: Pick<PruningClient, 'at'>, netuid: number): Promise<PruningState> {
  const view = await client.at(), block = typeof view.block === 'function' ? await view.block() : view.block!
  const names = ['NetworksAdded', 'SubnetOwner', 'SubnetworkN', 'NullPruningTarget', 'MechanismCountCurrent', 'MinAllowedUids', 'MaxAllowedUids', 'SubnetEpochConsensus', 'Tempo', 'LastEpochBlock', 'PendingEpochAt']
  const [values, window, genesis] = await Promise.all([Promise.all(names.map(name => view.query(['SubtensorModule', name], [netuid]))), view.query(['SubtensorModule', 'AdminFreezeWindow']), view.query(['System', 'BlockHash'], [0])])
  const [exists, owner, n, target, count, minimum, maximum, consensus, tempo, last, pending] = values
  if (exists !== true || !Number.isInteger(Number(count)) || Number(count) < 1 || Number(count) > 16) throw new Error('subnet is absent or has an invalid mechanism count')
  if (typeof owner !== 'string' || !owner || typeof genesis !== 'string' || !genesis) throw new Error('invalid pruning chain or owner identity')
  const height = pruningInteger(block, 'block'), population = pruningInteger(n, 'population', 65535), pendingTarget = target == null ? null : pruningInteger(target, 'target', 65535)
  const mechanisms = pruningInteger(count, 'mechanism count', 16), min = pruningInteger(minimum, 'minimum', 65535), max = pruningInteger(maximum, 'maximum', 65535)
  const epochTempo = pruningInteger(tempo, 'tempo'), epochLast = pruningInteger(last, 'last epoch'), epochPending = pruningInteger(pending ?? 0, 'pending epoch'), freezeWindow = pruningInteger(window, 'freeze window')
  const rawMode = consensus && typeof consensus === 'object' ? Object.keys(consensus)[0] : consensus
  return { block: height, genesis, owner, population, pendingTarget, mechanisms, minimum: min, maximum: max, mode: ({ 0: 'Yuma', 1: 'Null' } as Record<string, string>)[String(rawMode)] ?? String(rawMode), frozen: Boolean(epochTempo) && (epochPending > height || Math.max(0, epochLast + epochTempo - height) < freezeWindow), yumaCeiling: Math.floor(256 / mechanisms) }
}
export async function pruningSwitchWaitReason(client: PruningClient, netuid: number): Promise<string | null> {
  const view = await client.at(), block = typeof view.block === 'function' ? await view.block() : view.block!
  for (let mechanism = 0; mechanism < 16; mechanism++) if (await client.mapHasEntries('SubtensorModule', 'TimelockedWeightCommits', [mechanism * 4096 + netuid], block)) return 'pending timelocked commitments'
  const [last, tempo, epochs] = await Promise.all([view.query(['SubtensorModule', 'LastRateLimitedBlock'], [{ OwnerHyperparamUpdate: [netuid, 'EpochConsensus'] }]), view.query(['SubtensorModule', 'Tempo'], [netuid]), view.query(['SubtensorModule', 'OwnerHyperparamRateLimit'])])
  return Number(last) > 0 && block - Number(last) < Number(tempo) * Number(epochs) ? 'consensus-switch cooldown' : null
}
export interface PruningReceipt { success?: boolean; finalized?: boolean; blockHash?: string; extrinsicIndex?: number; events: unknown[]; message?: string; chainError?: { name?: string }; error?: unknown }
export interface PruningOptions {
  netuid: number; owner: string; target?: number; switchToYuma?: boolean; maxBatches?: number; waitTimeoutMs?: number; pollIntervalMs?: number; signal?: AbortSignal
  journal: PruningJournal
  /** Must wait for finalization and return the dispatch result. */
  submit(intent: Intent): Promise<PruningReceipt>
  onProgress?(state: PruningState, batches: number): void | Promise<void>
}
/** Bounded restartable pruning. An uncertain submission leaves the journal reserved for reconciliation. */
export async function pruneNullSubnet(client: PruningClient, options: PruningOptions): Promise<{ state: PruningState; batches: number }> {
  if (!Number.isInteger(options.netuid) || options.netuid <= 0 || options.netuid > 65535) throw new RangeError('pruning requires a non-root subnet')
  if (await options.journal.pending()) throw new Error('unresolved pruning submission; reconcile inclusion or expiry before clearing its journal')
  const initial = await pruningState(client, options.netuid), target = options.target ?? initial.pendingTarget ?? initial.yumaCeiling
  const limit = options.maxBatches ?? 100, waitTimeout = options.waitTimeoutMs ?? 120_000, poll = options.pollIntervalMs ?? 3000
  if (!Number.isInteger(target) || !Number.isInteger(limit) || limit <= 0 || !Number.isFinite(waitTimeout) || waitTimeout < 0 || !Number.isFinite(poll) || poll <= 0) throw new RangeError('invalid pruning limits')
  let batches = 0, stalled = 0, deadline: number | undefined
  for (;;) {
    options.signal?.throwIfAborted()
    if (await options.journal.pending()) throw new Error('unresolved pruning submission; refusing to repeat')
    const state = await pruningState(client, options.netuid)
    if (state.genesis !== initial.genesis || state.owner !== options.owner || state.mechanisms !== initial.mechanisms) throw new Error('chain, owner, or mechanism count changed; re-plan pruning')
    if (state.pendingTarget != null && state.pendingTarget !== target) throw new Error('conflicting on-chain pruning target')
    if (state.mode === 'Yuma') { if (state.population > state.yumaCeiling) throw new Error('Yuma population exceeds its ceiling'); return { state, batches } }
    if (state.mode !== 'Null' || target < state.minimum || target > Math.min(state.maximum, state.yumaCeiling)) throw new Error('invalid Null pruning state or target')
    let complete = state.population <= target && state.pendingTarget == null
    if (complete && !options.switchToYuma) return { state, batches }
    const reason = complete ? await pruningSwitchWaitReason(client, options.netuid) : null
    if (reason === 'pending timelocked commitments') complete = false
    if (state.frozen || (reason != null && reason !== 'pending timelocked commitments')) {
      deadline ??= performance.now() + waitTimeout
      if (performance.now() >= deadline) throw new Error(`pruning wait timed out: ${reason ?? 'administration window closed'}`)
      await pause(Math.min(poll, deadline - performance.now()), options.signal); continue
    }
    if (!complete && batches >= limit) throw new Error('pruning batch limit reached; resume from on-chain state')
    const intent = complete ? buildIntent('set_hyperparameter', { netuid: options.netuid, name: 'epoch_consensus', value: 'Yuma' }) : buildIntent('trim_null_subnet_batch', { netuid: options.netuid, target })
    await options.journal.reserve({ netuid: options.netuid, target, operation: intent.op, state })
    const result = await options.submit(intent)
    if (!result.finalized || !result.blockHash || result.extrinsicIndex == null) throw new Error('pruning inclusion is unresolved; journal retained')
    await options.journal.clear()
    if (!result.success) {
      const name = result.chainError?.name ?? (result.error && typeof result.error === 'object' ? (result.error as { name?: string }).name : undefined)
      // A finalized administration-window race establishes delivery and can be
      // retried after fresh state. Other paid failures never retry automatically.
      if (name !== 'AdminActionProhibitedDuringWeightsWindow') throw new Error(`pruning failed: ${result.message ?? 'dispatch failed'}; stopping without retry`)
      deadline ??= performance.now() + waitTimeout
      if (performance.now() >= deadline) throw new Error('pruning wait timed out: administration window closed before inclusion')
      await pause(Math.min(poll, deadline - performance.now()), options.signal); continue
    }
    const after = await pruningState(client, options.netuid)
    if (complete) { if (after.mode !== 'Yuma') throw new Error('receipt did not switch consensus'); return { state: after, batches } }
    const event = result.events.find(event => eventName(event) === 'SubtensorModule.NullUidsPruningProgress') as { attributes?: Record<string, unknown> } | undefined
    const progress = event?.attributes
    if (!progress || Number(progress.netuid) !== options.netuid || Number(progress.target) !== target) throw new Error('pruning receipt has no matching progress event')
    const cleaned = after.population <= target && after.pendingTarget == null
    const cleared = pruningInteger(progress.cleared_commits, 'cleared commitments')
    stalled = after.population >= state.population && cleared === 0 && !cleaned ? stalled + 1 : 0
    batches++; await options.onProgress?.(after, batches)
    if (stalled >= 3) throw new Error('three pruning batches made no progress')
    deadline = undefined
  }
}
function pruningInteger(value: unknown, name: string, maximum = Number.MAX_SAFE_INTEGER): number {
  if (!(typeof value === 'number' || typeof value === 'bigint' || typeof value === 'string' && /^\d+$/.test(value))) throw new Error(`invalid pruning ${name}`)
  const n = Number(value)
  if (!Number.isSafeInteger(n) || n < 0 || n > maximum) throw new Error(`invalid pruning ${name}`)
  return n
}
async function pause(ms: number, signal?: AbortSignal): Promise<void> {
  signal?.throwIfAborted()
  await new Promise<void>((resolve, reject) => { const abort = () => { clearTimeout(timer); reject(signal?.reason) }; const timer = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve() }, ms); signal?.addEventListener('abort', abort, { once: true }) })
}
