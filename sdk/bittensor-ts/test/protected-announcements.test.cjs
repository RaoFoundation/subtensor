'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { MemoryJsonStore } = require('../dist/preferences')
const { ProxyAnnouncementStore } = require('../dist/announcement-store')
const { ProtectedAnnouncements, composeAnnounceableCall } = require('../dist/protected-announcements')
const { buildIntent, encodeIntentCall } = require('../dist/intents')
const { SubmissionNotSentError, SubmissionUncertainError } = require('../dist/transport')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const controller = sdk.Keypair.fromUri('//Alice').ss58Address, pure = sdk.Keypair.fromUri('//Bob').ss58Address, recipient = sdk.Keypair.fromUri('//Charlie').ss58Address
const genesis = '0x' + 'aa'.repeat(32), blockHash = n => '0x' + n.toString(16).padStart(64, '0')
const hex = data => '0x' + Buffer.from(data).toString('hex')
const transfer = () => buildIntent('transfer', { dest_ss58: recipient, amount_tao: '1' })
function fixture() {
  const record = { controller_wallet: 'alice', controller_ss58: controller, protected_ss58: pure, genesis_hash: genesis, delay_blocks: 10 }
  const state = { block: 100, genesis, grants: [{ delegate: controller, proxy_type: 'Any', delay: 10 }], announcements: [], multisigs: new Map(), receipts: new Map(), submits: [], hook: null, queryHook: null, recipientRao: 0n, pureRao: 10_000_000_000n }
  const json = new MemoryJsonStore(), journal = new ProxyAnnouncementStore(json, address => hex(sdk.publicKeyFromSs58(address)))
  const compose = call => encodeIntentCall(call, async (p, f, args) => runtime.composeCall(p, f, args))
  const backend = {
    accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256,
    snapshot: async () => {
      const captured = { block: state.block, genesis: state.genesis, grants: structuredClone(state.grants), announcements: structuredClone(state.announcements), multisigs: new Map(state.multisigs) }
      const query = async (p, item, params) => {
        if (state.queryHook) { const value = await state.queryHook(p, item, params); if (value !== undefined) return value }
        if (p === 'Proxy' && item === 'Proxies') return [captured.grants, 0n]
        if (p === 'Proxy' && item === 'Announcements') return [captured.announcements, 0n]
        if (p === 'Multisig' && item === 'Multisigs') return captured.multisigs.get(params[1]) ?? null
        if (p === 'System' && item === 'Account') return { data: { free: 100_000_000_000n } }
        throw new Error(`unexpected query ${p}.${item}`)
      }
      const reader = { query, queryMap: async () => [], runtimeCall: async () => { throw Error('unexpected dynamic quote') }, composeCall: compose, decodeCall: async data => runtime.decodeCall(data), hashCallData: sdk.blake2_256, specVersion: () => 419, decodeAddress: sdk.publicKeyFromSs58, blockHash: async () => blockHash(captured.block), blockNumber: async () => captured.block }
      return { genesisHash: captured.genesis, hash: blockHash(captured.block), block: captured.block, specVersion: 419, blockTime: 12, query, queryMap: reader.queryMap, constant: async () => 0n, prepare: (intent, context) => intent.prepare(reader, context), compose, decodeCall: reader.decodeCall, hashCallData: sdk.blake2_256, fee: async () => 1n }
    },
    receipt: async hash => state.receipts.get(hash) ?? null,
    submit: async (intent, options) => {
      state.submits.push({ intent, options })
      if (state.hook) return state.hook(intent, options)
      state.block++
      const receipt = { status: 'finalized', success: true, blockNumber: state.block, blockHash: blockHash(state.block), extrinsicHash: blockHash(state.submits.length + 1000), events: [] }
      if (intent.op === 'announce_proxy_call') {
        state.announcements.push({ real: intent.args.real_ss58, call_hash: intent.args.call_hash, height: state.block })
        receipt.events.push({ module_id: 'Proxy', event_id: 'Announced', attributes: { real: pure, proxy: controller, call_hash: intent.args.call_hash } })
      } else if (intent.op === 'execute_proxy_announced') {
        const decoded = runtime.decodeCall(Buffer.from(intent.args.announced_call_data.slice(2), 'hex'))
        if (decoded.call_module === 'Balances') { const value = BigInt(decoded.call_args.find(arg => arg.name === 'value').value); state.pureRao -= value; state.recipientRao += value }
        const delay = Math.min(...state.grants.filter(grant => grant.proxy_type === 'Any').map(grant => grant.delay))
        state.announcements = state.announcements.filter(row => row.call_hash !== intent.args.expected_call_hash || row.height + delay > state.block)
        receipt.events.push({ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Ok: null } } })
      } else if (intent.op === 'remove_proxy_announcement') state.announcements = state.announcements.filter(row => row.call_hash !== intent.args.call_hash)
      state.receipts.set(receipt.extrinsicHash, receipt)
      return receipt
    },
  }
  return { state, record, json, journal, backend, engine: new ProtectedAnnouncements(backend, journal) }
}

test('prepare captures real-codec bytes under the protected account and announce reserves before submit', async () => {
  const f = fixture(), payload = await composeAnnounceableCall(await f.backend.snapshot(), transfer(), { coldkey: pure, feePayer: controller })
  assert.equal(payload.call_hash, hex(sdk.blake2_256(Buffer.from(payload.call_data.slice(2), 'hex'))))
  const plan = await f.engine.prepare(f.record, transfer())
  assert.equal(plan.prepared.dispatchOrigin, pure)
  assert(Object.isFrozen(plan.row))
  const submit = f.backend.submit
  f.backend.submit = async (...args) => { assert.equal((await f.journal.all())[0].status, 'submitting'); return submit(...args) }
  const result = await f.engine.announce(f.record, plan)
  assert.equal(result.completed, true)
  assert.equal(result.row.call_data, plan.call_data)
  assert.equal(result.row.announced_at_block, 101)
  assert.equal(f.state.submits[0].options.signerAddress, controller)
  const replay = await f.engine.announce(f.record, plan)
  assert.equal(replay.recovered, true)
  assert.equal(replay.row.index, result.row.index)
  assert.equal(f.state.submits.length, 1)
})

test('live shortest Any delay controls readiness and same-block duplicates keep stable indices', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.announcements.push({ ...f.state.announcements[0] })
  f.state.grants.push({ delegate: controller, proxy_type: 'Any', delay: 2 })
  f.state.block = 102
  let rows = await f.engine.reconcile(f.record)
  assert.equal(rows.length, 2); assert.notEqual(rows[0].index, rows[1].index)
  assert(rows.every(row => row.executable_at_block === 103 && row.ready === false))
  f.state.block = 103
  rows = await f.engine.reconcile(f.record)
  assert(rows.every(row => row.ready))
  assert.equal(rows[0].index, queued.row.index)
})

test('dry discovery never changes persistence or advertises an unsaved cancellation index', async () => {
  const f = fixture(), prepared = await f.engine.prepare(f.record, transfer())
  f.state.announcements.push({ real: pure, call_hash: prepared.call_hash, height: 90 })
  const before = await f.json.read('proxy_announcements'), rows = await f.engine.reconcile(f.record, { persist: false })
  assert.equal(rows[0].index, undefined); assert.equal(rows[0].provisional, true)
  assert.deepEqual(await f.json.read('proxy_announcements'), before)
  const persisted = await f.engine.reconcile(f.record)
  assert.equal(persisted[0].index, 1); assert.equal(persisted[0].plaintext_missing, true)
  await assert.rejects(f.engine.execute(f.record, 1), /plaintext/)
  const cancelled = await f.engine.cancel(f.record, 1)
  assert.equal(cancelled.completed, true)
})

test('failed local submissions release reservations while uncertain outcomes block identical rebroadcast', async () => {
  const f = fixture()
  f.state.hook = async () => { throw new SubmissionNotSentError(Error('user declined')) }
  await assert.rejects(f.engine.announce(f.record, transfer()), /user declined/)
  assert.equal((await f.journal.all())[0].status, 'failed')
  f.state.hook = async () => { throw new SubmissionUncertainError(blockHash(700), Error('lost response')) }
  await assert.rejects(f.engine.announce(f.record, transfer()), /submission uncertain/)
  const count = f.state.submits.length
  await assert.rejects(f.engine.announce(f.record, transfer()), /identical announcement/)
  assert.equal(f.state.submits.length, count)
  const rows = await f.engine.reconcile(f.record)
  assert.equal(rows[0].approval_state, 'submission_unknown')
  assert.equal(rows[0].announcement_receipt.extrinsicHash, blockHash(700))
})

test('multisig approval stays distinct from announcement inclusion and absence requires verifiable identity', async () => {
  const f = fixture(), outer = blockHash(800)
  f.state.multisigs.set(outer, { when: { height: 100, index: 1 } })
  f.state.hook = async () => ({ status: 'finalized', success: true, blockNumber: 100, blockHash: blockHash(100), events: [{ module_id: 'Multisig', event_id: 'NewMultisig', attributes: [recipient, controller, outer] }] })
  const result = await f.engine.announce(f.record, transfer())
  assert.equal(result.completed, false); assert.equal(result.row.status, 'awaiting_approval')
  assert.equal(result.row.multisig_announcement.call_hash, outer)
  assert.equal((await f.engine.reconcile(f.record))[0].approval_state, 'pending')
  await assert.rejects(f.engine.execute(f.record, result.row.index), /not landed/)
  await assert.rejects(f.engine.cancel(f.record, result.row.index), /underlying commitment/)
  f.state.queryHook = async (p) => { if (p === 'Multisig') throw Error('archive pruned') }
  assert.equal((await f.engine.reconcile(f.record))[0].approval_state, 'unknown')
  f.state.queryHook = null; f.state.multisigs.delete(outer)
  assert.deepEqual(await f.engine.reconcile(f.record), [])
  assert.equal((await f.journal.getByIndex('alice', result.row.index, { genesisHash: genesis })).status, 'gone')
})

// PROTECTED_REGRESSION: uncertain delayed transfer execution and younger duplicate announcements must not cause a second funds transfer without explicit repeat approval; changes require explicit human approval.
test('delayed replay transfers the saved amount once and retains younger duplicate protection', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.block = 111
  f.state.announcements.push({ ...f.state.announcements[0], height: 110 })
  const first = await f.engine.execute(f.record, queued.row.index)
  assert.equal(first.completed, true); assert.equal(f.state.recipientRao, 1_000_000_000n)
  assert.equal(f.state.pureRao, 9_000_000_000n)
  const younger = (await f.engine.reconcile(f.record))[0]
  assert.equal(younger.announced_at_block, 110); assert.equal(younger.call_data, queued.row.call_data)
  f.state.block = 121
  await assert.rejects(f.engine.execute(f.record, younger.index), /already executed/)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
  assert.equal((await f.engine.execute(f.record, younger.index, { allowRepeat: true })).completed, true)
  assert.equal(f.state.recipientRao, 2_000_000_000n)
})

test('uncertain execution is recovered from its exact receipt without resubmitting', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.block = 111
  const submit = f.backend.submit
  f.backend.submit = async (...args) => { const receipt = await submit(...args); throw new SubmissionUncertainError(receipt.extrinsicHash, Error('response lost after inclusion')) }
  await assert.rejects(f.engine.execute(f.record, queued.row.index), /submission uncertain/)
  const count = f.state.submits.length
  assert.deepEqual(await f.engine.reconcile(f.record), [])
  assert.equal((await f.journal.getByIndex('alice', queued.row.index, { genesisHash: genesis })).status, 'executed')
  assert.equal(f.state.submits.length, count); assert.equal(f.state.recipientRao, 1_000_000_000n)
})

test('cancellation approval is not cancellation and actual absence confirms completion', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.hook = async () => ({ status: 'finalized', success: true, blockNumber: f.state.block, events: [{ module_id: 'Multisig', event_id: 'MultisigApproval', attributes: [recipient, { height: 100, index: 0 }, controller, blockHash(900)] }] })
  assert.equal((await f.engine.cancel(f.record, queued.row.index)).completed, false)
  assert.equal(f.state.announcements.length, 1)
  f.state.hook = null
  assert.equal((await f.engine.cancel(f.record, queued.row.index)).completed, true)
  assert.equal((await f.journal.getByIndex('alice', queued.row.index, { genesisHash: genesis })).status, 'cancelled')
})

test('chain mismatch, revoked controller, zero delay and tampered saved payloads fail closed', async () => {
  const f = fixture()
  await assert.rejects(f.engine.prepare({ ...f.record, genesis_hash: undefined }, transfer()), /another chain/)
  f.state.grants = []
  await assert.rejects(f.engine.prepare(f.record, transfer()), /no live Any/)
  f.state.grants = [{ delegate: controller, proxy_type: 'Any', delay: 0 }]
  await assert.rejects(f.engine.prepare(f.record, transfer()), /zero-delay/)
  f.state.grants[0].delay = 10
  const queued = await f.engine.announce(f.record, transfer())
  await f.journal.update('alice', queued.row.index, { call_data: '0x1234' }, { genesisHash: genesis })
  await assert.rejects(f.engine.inspect(f.record, queued.row.index), /do not match/)
  f.state.genesis = blockHash(99)
  await assert.rejects(f.engine.reconcile(f.record), /another chain/)
})

test('reviewed management metadata must describe the exact saved proxy calls', async () => {
  const f = fixture(), intent = buildIntent('add_proxy', { delegate_ss58: recipient, proxy_type: 'Staking', delay: 0, from_hotkey: false })
  await assert.rejects(f.engine.prepare(f.record, intent), /explicitly reviewed/)
  const queued = await f.engine.announce(f.record, intent, { allowManagement: true, category: 'allow:Staking' })
  const data = runtime.composeCall('Proxy', 'remove_proxies', {})
  await f.journal.update('alice', queued.row.index, { call_data: hex(data), call_hash: hex(sdk.blake2_256(data)), call_semantics: runtime.decodeCall(data) }, { genesisHash: genesis })
  await assert.rejects(f.engine.inspect(f.record, queued.row.index), /management bytes differ/)
})

test('outer success with ProxyExecuted error never marks a transfer executed', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.block = 111
  f.state.hook = async () => ({ status: 'finalized', success: true, blockNumber: 111, events: [{ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Err: { BadOrigin: null } } } }] })
  assert.equal((await f.engine.execute(f.record, queued.row.index)).completed, false)
  assert.equal(f.state.recipientRao, 0n)
  assert.notEqual((await f.journal.getByIndex('alice', queued.row.index, { genesisHash: genesis })).status, 'executed')
})

test('legacy multisig approvals recover only the unique historical embedded announce operation', async () => {
  const f = fixture(), plan = await f.engine.prepare(f.record, transfer()), outerHash = blockHash(850)
  const row = await f.journal.add({ ...plan.row, status: 'awaiting_approval', announced_at_block: 90 })
  const opening = await encodeIntentCall({ pallet: 'Multisig', call: 'as_multi', params: {
    threshold: 2, other_signatories: [recipient], maybe_timepoint: null, max_weight: { ref_time: 1n, proof_size: 0n },
    call: { pallet: 'Proxy', call: 'announce', params: { real: { Id: pure }, call_hash: plan.call_hash } },
  } }, async (p, c, params) => runtime.composeCall(p, c, params))
  const calls = []
  f.backend.blockHash = async height => { assert.equal(height, 90); return blockHash(height) }
  f.backend.events = async block => {
    assert.equal(block, blockHash(90))
    return [{ module_id: 'Multisig', event_id: 'MultisigApproval', attributes: [recipient, { height: 80, index: 2 }, controller, outerHash], extrinsic_idx: 4 }]
  }
  f.backend.extrinsicCall = async (height, index) => { calls.push([height, index]); return runtime.decodeCall(opening) }
  f.state.multisigs.set(outerHash, { when: { height: 80, index: 2 } })
  assert.equal((await f.engine.reconcile(f.record, { persist: false }))[0].approval_state, 'pending')
  assert.equal((await f.journal.getByIndex('alice', row.index, { genesisHash: genesis })).multisig_announcement, undefined)
  assert.equal((await f.engine.reconcile(f.record))[0].approval_state, 'pending')
  assert.deepEqual(calls, [[80, 2], [80, 2]])
  assert.deepEqual((await f.journal.getByIndex('alice', row.index, { genesisHash: genesis })).multisig_announcement, { account_ss58: controller, call_hash: outerHash, timepoint: { height: 80, index: 2 } })
  f.state.multisigs.delete(outerHash)
  assert.deepEqual(await f.engine.reconcile(f.record), [])
  assert.equal((await f.journal.getByIndex('alice', row.index, { genesisHash: genesis })).status, 'gone')
})

test('unavailable, ambiguous or unrelated historical multisig calls retain legacy approval plaintext', async () => {
  for (const scenario of ['pruned', 'missing', 'wrong-real', 'wrong-hash', 'sudo', 'ambiguous']) {
    const f = fixture(), plan = await f.engine.prepare(f.record, transfer()), outerHash = blockHash(860)
    const row = await f.journal.add({ ...plan.row, status: 'awaiting_approval', announced_at_block: 90 })
    f.backend.events = async () => {
      if (scenario === 'pruned') throw Error('archive unavailable')
      return (scenario === 'ambiguous' ? [outerHash, blockHash(861)] : [outerHash]).map((callHash, index) => ({ module_id: 'Multisig', event_id: 'NewMultisig', attributes: [recipient, controller, callHash], extrinsic_idx: index }))
    }
    f.backend.extrinsicCall = async () => scenario === 'missing' ? null : ({ pallet: scenario === 'sudo' ? 'Sudo' : 'Multisig', call: scenario === 'sudo' ? 'sudo' : 'as_multi', params: { call: { pallet: 'Proxy', call: 'announce', params: { real: { Id: scenario === 'wrong-real' ? recipient : pure }, call_hash: scenario === 'wrong-hash' ? blockHash(999) : plan.call_hash } } } })
    const pending = await f.engine.reconcile(f.record)
    assert.equal(pending.length, 1, scenario)
    assert.equal(pending[0].approval_state, 'unknown', scenario)
    const saved = await f.journal.getByIndex('alice', row.index, { genesisHash: genesis })
    assert.equal(saved.status, 'awaiting_approval', scenario)
    assert.equal(saved.call_data, plan.call_data, scenario)
  }
})

test('legacy plaintext is executable only when current composition exactly matches its saved hash', async () => {
  const f = fixture(), prepared = await f.engine.prepare(f.record, transfer())
  const legacy = { ...prepared.row, announced_at_block: 80 }
  delete legacy.call_data
  const row = await f.journal.add(legacy)
  f.state.announcements.push({ real: pure, call_hash: prepared.call_hash, height: 80 })
  assert.equal((await f.engine.inspect(f.record, row.index)).call_data, prepared.call_data)
  await f.journal.update('alice', row.index, { inner_args: { ...legacy.inner_args, amount_tao: '2' } }, { genesisHash: genesis })
  await assert.rejects(f.engine.execute(f.record, row.index), /rebuilt legacy call differs/)
  assert.equal(f.state.submits.length, 0)
  await f.journal.update('alice', row.index, { inner_args: legacy.inner_args }, { genesisHash: genesis })
  assert.equal((await f.engine.execute(f.record, row.index)).completed, true)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
})

// PROTECTED_REGRESSION: loss of post-submit RPC access must retain the exact included transfer receipt and recover without a second funds transfer; changes require explicit human approval.
test('post-submit snapshot failure preserves the transfer receipt before any follow-up RPC', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer())
  f.state.block = 111
  const snapshot = f.backend.snapshot, submit = f.backend.submit
  f.backend.submit = async (...args) => { const receipt = await submit(...args); f.backend.snapshot = async () => { throw Error('snapshot disconnected') }; return receipt }
  await assert.rejects(f.engine.execute(f.record, queued.row.index), /snapshot disconnected/)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
  const pending = await f.journal.getByIndex('alice', queued.row.index, { genesisHash: genesis })
  assert.equal(pending.pending_action.receipt.extrinsicHash, blockHash(1002))
  f.backend.snapshot = snapshot
  assert.deepEqual(await f.engine.reconcile(f.record), [])
  assert.equal((await f.journal.getByIndex('alice', queued.row.index, { genesisHash: genesis })).status, 'executed')
  assert.equal(f.state.submits.length, 2)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
})

// PROTECTED_REGRESSION: an uncertain transfer locks every same-payload announcement until its receipt is resolved; younger duplicates cannot transfer the funds twice without explicit repeat approval; changes require explicit human approval.
test('uncertain execution locks younger commitment identities without copying its action state', async () => {
  for (const discoveredBefore of [true, false]) {
    const f = fixture(), queued = await f.engine.announce(f.record, transfer())
    if (discoveredBefore) f.state.announcements.push({ real: pure, call_hash: queued.row.call_hash, height: 110 })
    f.state.block = 111
    const receiptReader = f.backend.receipt, submit = f.backend.submit
    f.backend.receipt = async () => null
    f.backend.submit = async (...args) => { const receipt = await submit(...args); throw new SubmissionUncertainError(receipt.extrinsicHash, Error('lost response')) }
    await assert.rejects(f.engine.execute(f.record, queued.row.index), /submission uncertain/)
    if (!discoveredBefore) f.state.announcements.push({ real: pure, call_hash: queued.row.call_hash, height: 112 })
    f.state.block = 122
    const younger = (await f.engine.reconcile(f.record)).find(row => row.on_chain)
    assert.equal(younger.pending_action, undefined)
    await assert.rejects(f.engine.execute(f.record, younger.index), /identical commitment.*uncertain/)
    assert.equal(f.state.submits.length, 2)
    assert.equal(f.state.recipientRao, 1_000_000_000n)
    f.backend.receipt = receiptReader; f.backend.submit = submit
    await f.engine.reconcile(f.record)
    await assert.rejects(f.engine.execute(f.record, younger.index), /already executed/)
    assert.equal((await f.engine.execute(f.record, younger.index, { allowRepeat: true })).completed, true)
    assert.equal(f.state.recipientRao, 2_000_000_000n)
  }
})

test('late action settlement cannot clear a retry claimed after concurrent reconciliation', async () => {
  const f = fixture(), queued = await f.engine.announce(f.record, transfer()), scope = { genesisHash: genesis }
  f.state.block = 111
  f.state.hook = async () => ({ status: 'failed', success: false, blockNumber: 111, blockHash: blockHash(111), extrinsicHash: blockHash(901), events: [{ module_id: 'System', event_id: 'ExtrinsicFailed', attributes: { dispatch_error: 'BadOrigin' } }] })
  const update = f.journal.update.bind(f.journal)
  let retry
  f.journal.update = async (...args) => {
    const saved = await update(...args)
    if (args[2].pending_action?.state === 'submission_unknown') {
      await f.engine.reconcile(f.record)
      retry = await f.journal.claimAction('alice', queued.row.index, 'execute', scope)
    }
    return saved
  }
  const result = await f.engine.execute(f.record, queued.row.index)
  assert.equal(result.completed, false)
  assert.equal(f.state.recipientRao, 0n)
  assert.equal(f.state.submits.length, 2)
  assert.equal(retry.pending_action.state, 'submitting')
  assert.deepEqual((await f.journal.getByIndex('alice', queued.row.index, scope)).pending_action, retry.pending_action)
})
