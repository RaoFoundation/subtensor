'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { buildIntent } = require('../dist/intents')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob'), charlie = sdk.Keypair.fromUri('//Charlie')
const controller = sdk.ss58FromPublic(sdk.multisigAccountId([alice.publicKey, bob.publicKey], 2).accountId)
const pure = charlie.ss58Address, recipient = bob.ss58Address
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const blockHash = n => '0x' + n.toString(16).padStart(64, '0'), genesis = blockHash(9999)
const payment = () => buildIntent('transfer', { dest_ss58: recipient, amount_tao: '1' })

async function fixture(kind = 'execute', surface = sdk) {
  const { MemoryJsonStore, ProxyAnnouncementStore, ProtectedAccountStore, ProtectedAnnouncements, ProtectedAccountSession, encodeIntentCall } = surface
  const payment = () => surface.buildIntent('transfer', { dest_ss58: recipient, amount_tao: '1' })
  const store = new MemoryJsonStore(), journal = new ProxyAnnouncementStore(store, address => hex(sdk.publicKeyFromSs58(address)))
  const state = { block: 100, announcements: [], multisigs: new Map(), receipts: new Map(), calls: new Map(), submits: 0, pureRao: 10_000_000_000n, recipientRao: 0n }
  const compose = call => encodeIntentCall(call, async (p, c, args) => runtime.composeCall(p, c, args))
  const record = { controller_wallet: 'group', controller_ss58: controller, protected_ss58: pure, genesis_hash: genesis, delay_blocks: 10, allows: [], reviewed_allows: [], pending_allows: [], setup: { status: 'ready' } }
  const query = async (p, item, params) => {
    if (p === 'Proxy' && item === 'Proxies') return [[{ delegate: controller, proxy_type: 'Any', delay: 10 }], 0n]
    if (p === 'Proxy' && item === 'Announcements') return [structuredClone(state.announcements), 0n]
    if (p === 'Multisig' && item === 'Multisigs') return state.multisigs.get(params[1]) ?? null
    if (p === 'System' && item === 'Account') return { data: { free: 100_000_000_000n } }
    throw Error(`unexpected query ${p}.${item}`)
  }
  const reader = { query, queryMap: async () => [], runtimeCall: async () => null, composeCall: compose, decodeCall: data => runtime.decodeCall(data), hashCallData: sdk.blake2_256, specVersion: () => 419, decodeAddress: sdk.publicKeyFromSs58, blockHash: async () => blockHash(state.block), blockNumber: async () => state.block }
  const backend = {
    accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256,
    snapshot: async () => ({ genesisHash: genesis, hash: blockHash(state.block), block: state.block, specVersion: 419, blockTime: 12, query, queryMap: reader.queryMap, constant: async () => 0n, prepare: (intent, context) => intent.prepare(reader, context), compose, decodeCall: reader.decodeCall, hashCallData: sdk.blake2_256, fee: async () => 1n }),
    blockHash: async height => blockHash(height),
    receipt: async hash => state.receipts.get(hash) ?? null,
    extrinsicCall: async (block, index) => state.calls.get(`${typeof block === 'number' ? blockHash(block) : block}:${index}`) ?? null,
    submit: async intent => {
      state.submits++; state.block++
      const receipt = { status: 'finalized', success: true, blockNumber: state.block, blockHash: blockHash(state.block), extrinsicIndex: 0, extrinsicHash: blockHash(1000 + state.submits), events: [] }
      if (intent.op === 'announce_proxy_call') {
        state.announcements.push({ real: pure, call_hash: intent.args.call_hash, height: state.block })
      } else {
        const inner = (await intent.prepare(reader, { coldkey: controller })).call
        state.timepoint = { height: state.block, index: 0 }
        state.inner = inner
        state.innerHash = hex(sdk.blake2_256(await compose(inner)))
        state.multisigs.set(state.innerHash, { when: state.timepoint })
        state.outer = { pallet: 'Multisig', call: 'as_multi', params: { threshold: 2, other_signatories: [bob.ss58Address], maybe_timepoint: null, call: inner, max_weight: { ref_time: 1n, proof_size: 0n } } }
        state.calls.set(`${receipt.blockHash}:0`, runtime.decodeCall(await compose(state.outer)))
        receipt.events.push({ module_id: 'Multisig', event_id: 'NewMultisig', extrinsic_idx: 0, attributes: [alice.ss58Address, controller, state.innerHash] })
      }
      state.receipts.set(receipt.extrinsicHash, receipt)
      return receipt
    },
  }
  const engine = new ProtectedAnnouncements(backend, journal), records = new ProtectedAccountStore(store, backend)
  await records.upsert(record)
  const session = new ProtectedAccountSession(backend, records, engine, 'group')
  const announced = await engine.announce(record, payment())
  state.block += 10
  const approval = kind === 'execute' ? await session.execute(announced.row.index) : await session.cancel(announced.row.index)
  assert.equal(approval.completed, false)
  async function finish({ cancelMultisig = false, nestedFailure = false, announcementAbsent = true } = {}) {
    state.block++
    state.multisigs.delete(state.innerHash)
    if (announcementAbsent) state.announcements = []
    const result = nestedFailure ? { Err: { BadOrigin: null } } : { Ok: null }
    if (!cancelMultisig && !nestedFailure && kind === 'execute') {
      const transfer = runtime.decodeCall(Buffer.from(announced.row.call_data.slice(2), 'hex'))
      const amount = BigInt(transfer.call_args.find(arg => arg.name === 'value').value)
      state.pureRao -= amount; state.recipientRao += amount
    }
    const call = cancelMultisig
      ? { pallet: 'Multisig', call: 'cancel_as_multi', params: { threshold: 2, other_signatories: [bob.ss58Address], timepoint: state.timepoint, call_hash: state.innerHash } }
      : { ...state.outer, params: { ...state.outer.params, maybe_timepoint: state.timepoint } }
    const receipt = { status: 'finalized', success: true, blockNumber: state.block, blockHash: blockHash(state.block), extrinsicIndex: 1, extrinsicHash: blockHash(2000), events: [] }
    if (!cancelMultisig && kind === 'execute') receipt.events.push({ module_id: 'Proxy', event_id: 'ProxyExecuted', extrinsic_idx: 1, attributes: [result] })
    receipt.events.push({ module_id: 'Multisig', event_id: cancelMultisig ? 'MultisigCancelled' : 'MultisigExecuted', extrinsic_idx: 1, attributes: { approving: bob.ss58Address, multisig: controller, call_hash: state.innerHash, timepoint: state.timepoint, ...(cancelMultisig ? {} : { result: { Ok: null } }) } })
    state.receipts.set(receipt.extrinsicHash, receipt)
    state.calls.set(`${receipt.blockHash}:1`, runtime.decodeCall(await compose(call)))
    return { receipt, call, reference: { extrinsicHash: receipt.extrinsicHash, blockHash: receipt.blockHash } }
  }
  return { state, record, journal, backend, session, engine, finish, index: announced.row.index, saved: () => journal.getByIndex('group', announced.row.index, { genesisHash: genesis }) }
}

// PROTECTED_REGRESSION: externally completed multisig payments must release protected workflow reservations without a duplicate transfer; changes require explicit human approval.
test('another member completion recovers exact execution or cancellation and unblocks rotation', async () => {
  for (const kind of ['execute', 'cancel']) {
    const f = await fixture(kind), { reference } = await f.finish()
    assert.equal((await f.engine.reconcile(f.record))[0].approval_state, 'action_unknown')
    await assert.rejects(f.session.planRotation('new-controller', alice.ss58Address), /execute or cancel/)
    const before = f.state.submits, recovered = await f.session.recoverAction(f.index, reference)
    assert.equal(recovered.completed, true)
    assert.equal(recovered.row.status, kind === 'execute' ? 'executed' : 'cancelled')
    assert.equal(recovered.row.execution_uncertain, kind === 'cancel' ? true : undefined)
    assert.equal((await f.saved()).pending_action, null)
    assert.deepEqual((await f.session.inspect()).pending, [])
    assert.equal((await f.session.planRotation('new-controller', alice.ss58Address)).toWallet, 'new-controller')
    assert.equal(f.state.submits, before)
    assert.equal(f.state.recipientRao, kind === 'execute' ? 1_000_000_000n : 0n)
    assert.equal(f.state.pureRao, kind === 'execute' ? 9_000_000_000n : 10_000_000_000n)
    assert.equal((await f.session.recoverAction(f.index, reference)).completed, true)
    const next = await f.engine.announce(f.record, payment())
    assert.equal(next.completed, true, 'identical announcements are no longer blocked by the old action')
    if (kind === 'execute') {
      f.state.block += 10
      await assert.rejects(f.session.execute(next.row.index), /already executed/)
      assert.equal(f.state.recipientRao, 1_000_000_000n)
    }
  }
})

// PROTECTED_REGRESSION: cancellation of a replacement announcement must retain the original payment's replay guard; changes require explicit human approval.
test('successful cancellation permits rotation but a replacement announcement cannot hide an earlier payment', async () => {
  const f = await fixture('cancel'), original = structuredClone(f.state.announcements[0])
  // A permissionless executor consumes the original and pays its recipient.
  f.state.announcements = []
  f.state.pureRao -= 1_000_000_000n; f.state.recipientRao += 1_000_000_000n
  // Another client announces the same hash before the old cancellation clears.
  f.state.announcements.push({ ...original, height: ++f.state.block })
  const { reference } = await f.finish()
  const recovered = await f.session.recoverAction(f.index, reference)
  assert.equal(recovered.completed, true)
  assert.equal(recovered.row.status, 'cancelled')
  assert.equal((await f.saved()).execution_uncertain, true)
  assert.equal((await f.saved()).pending_action, null)
  assert.equal((await f.session.planRotation('new-controller', alice.ss58Address)).toWallet, 'new-controller')
  const next = await f.engine.announce(f.record, payment()), submissions = f.state.submits
  f.state.block += 10
  assert.equal(await f.journal.executedCommitmentIndex(next.row), f.index)
  await assert.rejects(f.session.execute(next.row.index), /may have executed.*allowRepeat/)
  await assert.rejects(f.journal.claimAction('group', next.row.index, 'execute', { genesisHash: genesis }), /may have executed.*allowRepeat/)
  assert.equal(f.state.submits, submissions)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
  assert.equal(f.state.pureRao, 9_000_000_000n)
  assert.equal((await f.session.execute(next.row.index, { allowRepeat: true })).completed, false)
  assert.equal(f.state.submits, submissions + 1, 'a deliberate repeat still permits a new approval')
})

// PROTECTED_REGRESSION: unrelated or unfinalized receipts must not clear uncertain payments and permit a duplicate transfer; changes require explicit human approval.
test('recovery rejects wrong timepoint, account, hash, effect, finality, and event phase', async () => {
  for (const scenario of ['timepoint', 'account', 'hash', 'effect', 'unfinalized', 'fork', 'hint', 'phase', 'still-pending']) {
    const f = await fixture(), { receipt, call, reference } = await f.finish(), before = await f.saved()
    const event = receipt.events.find(event => event.module_id === 'Multisig')
    if (scenario === 'timepoint') event.attributes.timepoint = { ...f.state.timepoint, index: 9 }
    if (scenario === 'account') event.attributes.multisig = alice.ss58Address
    if (scenario === 'hash') event.attributes.call_hash = blockHash(4444)
    if (scenario === 'effect') f.state.calls.set(`${receipt.blockHash}:1`, { ...call, params: { ...call.params, call: { ...f.state.inner, params: { ...f.state.inner.params, real: alice.ss58Address } } } })
    if (scenario === 'unfinalized') receipt.status = 'inBlock'
    if (scenario === 'fork') receipt.blockHash = blockHash(5555)
    if (scenario === 'hint') reference.blockHash = blockHash(5555)
    if (scenario === 'phase') event.extrinsic_idx = 0
    if (scenario === 'still-pending') f.state.multisigs.set(f.state.innerHash, { when: f.state.timepoint })
    await assert.rejects(f.session.recoverAction(f.index, reference))
    assert.deepEqual(await f.saved(), before)
    await assert.rejects(f.session.planRotation('new-controller', alice.ss58Address), /execute or cancel/)
  }
})

test('failed execution can release its multisig reservation while the original announcement is live', async () => {
  const f = await fixture(), { receipt, reference } = await f.finish({ nestedFailure: true, announcementAbsent: false })
  receipt.events = receipt.events.filter(event => event.module_id !== 'Proxy')
  receipt.events[0].attributes.result = { Err: { BadOrigin: null } }
  const recovered = await f.session.recoverAction(f.index, reference)
  assert.equal(recovered.completed, false)
  assert.equal(recovered.row.status, 'pending')
  assert.equal(recovered.row.pending_action, null)
  assert.equal(f.state.recipientRao, 0n)
  assert.equal((await f.session.cancel(f.index)).completed, false, 'the original announcement remains actionable')
})

test('cancelling the multisig approval does not claim its protected action executed', async () => {
  const f = await fixture(), { reference } = await f.finish({ cancelMultisig: true, announcementAbsent: false })
  const recovered = await f.session.recoverAction(f.index, reference)
  assert.equal(recovered.completed, false)
  assert.equal(recovered.row.status, 'pending')
  assert.equal(recovered.row.pending_action, null)
  assert.equal(f.state.recipientRao, 0n)
})

// PROTECTED_REGRESSION: cancellation or failure after a permissionless payment must not permit replaying the same transfer; changes require explicit human approval.
test('an absent announcement remains uncertain after multisig cancellation or failed execution', async () => {
  for (const outcome of ['cancelled', 'outer-failed', 'inner-failed']) {
    const f = await fixture()
    // Anyone may execute proxy_announced. Its separate successful receipt is
    // unavailable here, so the later multisig outcome cannot prove no payment.
    f.state.announcements = []
    f.state.pureRao -= 1_000_000_000n; f.state.recipientRao += 1_000_000_000n
    const { receipt, reference } = await f.finish({ cancelMultisig: outcome === 'cancelled', nestedFailure: true })
    if (outcome === 'outer-failed') {
      receipt.events = receipt.events.filter(event => event.module_id !== 'Proxy')
      receipt.events[0].attributes.result = { Err: { Module: { error: 'Unannounced' } } }
    }
    const before = await f.saved(), submissions = f.state.submits
    await assert.rejects(f.session.recoverAction(f.index, reference), /does not prove/)
    assert.deepEqual(await f.saved(), before)
    await assert.rejects(f.engine.announce(f.record, payment()), /may still be submitting or awaiting approval/)
    await assert.rejects(f.session.execute(f.index), /unresolved|pending|uncertain|reconciled|has not landed/)
    assert.equal(f.state.submits, submissions)
    assert.equal(f.state.recipientRao, 1_000_000_000n)
    assert.equal(f.state.pureRao, 9_000_000_000n)
  }
})

// PROTECTED_REGRESSION: an unrelated later batch failure must not make an executed payment retryable; changes require explicit human approval.
test('wrapped completion uses its own result despite an unrelated later failure', async () => {
  for (const wrapper of ['batch', 'proxy']) {
    const f = await fixture(), { receipt, call, reference } = await f.finish()
    const wrapped = wrapper === 'batch'
      ? { pallet: 'Utility', call: 'batch', params: { calls: [call, { pallet: 'System', call: 'remark', params: { remark: '0x' } }] } }
      : { pallet: 'Proxy', call: 'proxy', params: { real: alice.ss58Address, force_proxy_type: null, call } }
    f.state.calls.set(`${receipt.blockHash}:1`, wrapped)
    receipt.events.push(wrapper === 'batch'
      ? { module_id: 'Utility', event_id: 'BatchInterrupted', attributes: { index: 1, error: { BadOrigin: null } }, extrinsic_idx: 1 }
      : { module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: [{ Ok: null }], extrinsic_idx: 1 })
    receipt.success = wrapper !== 'batch'
    const recovered = await f.session.recoverAction(f.index, reference)
    assert.equal(recovered.completed, true)
    assert.equal(recovered.row.status, 'executed')
    assert.equal(f.state.recipientRao, 1_000_000_000n)
    assert.equal((await f.session.planRotation('new-controller', alice.ss58Address)).toWallet, 'new-controller')
  }
})

test('ambiguous wrapper histories and outer failures preserve the uncertain payment', async () => {
  for (const scenario of ['duplicate-call', 'ambiguous-proxy', 'prior-failure', 'outer-failure']) {
    const f = await fixture(), { receipt, call, reference } = await f.finish(), before = await f.saved()
    if (scenario === 'duplicate-call') f.state.calls.set(`${receipt.blockHash}:1`, { pallet: 'Utility', call: 'batch', params: { calls: [call, call] } })
    if (scenario === 'ambiguous-proxy') receipt.events.unshift({ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: [{ Ok: null }], extrinsic_idx: 1 })
    if (scenario === 'prior-failure') receipt.events.unshift({ module_id: 'Utility', event_id: 'ItemFailed', attributes: { error: { BadOrigin: null } }, extrinsic_idx: 1 })
    if (scenario === 'outer-failure') receipt.events.push({ module_id: 'System', event_id: 'ExtrinsicFailed', attributes: {}, extrinsic_idx: 1 })
    await assert.rejects(f.session.recoverAction(f.index, reference))
    assert.deepEqual(await f.saved(), before)
    assert.equal(f.state.recipientRao, 1_000_000_000n)
  }
})

test('a receipt block hint must still be canonical and finalized before recovery', async () => {
  const { createProtectedProxyBackend } = require('../dist/protected-proxy-adapter')
  for (const state of ['finalized', 'unfinalized', 'fork']) {
    const transaction = blockHash(7000), inclusion = blockHash(50), finalized = blockHash(60)
    const receipt = { status: 'inBlock', success: true, blockHash: inclusion, blockNumber: 50, extrinsicIndex: 1, extrinsicHash: transaction, events: [] }
    const client = {
      findExtrinsic: async (hash, block) => { assert.equal(hash, transaction); assert.equal(block, inclusion); return receipt },
      finalizedHead: async () => finalized,
      blockNumber: async hash => { assert.equal(hash, finalized); return state === 'unfinalized' ? 49 : 60 },
      blockHash: async height => { assert.equal(height, 50); return state === 'fork' ? blockHash(51) : inclusion },
    }
    const backend = createProtectedProxyBackend(client, { crypto: { accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256 }, reader: async () => ({}), submit: async () => { throw Error('must not submit') } })
    assert.equal((await backend.receipt(transaction, inclusion)).status, 'inBlock')
    const recovered = await backend.finalizedReceipt(transaction, inclusion)
    if (state === 'finalized') assert.equal(recovered.status, 'finalized')
    else assert.equal(recovered, null)
    assert.equal(receipt.status, 'inBlock')
  }
})

test('browser recovery uses the same shared receipt policy', async () => {
  const fs = require('node:fs'), path = require('node:path'), { pathToFileURL } = require('node:url')
  const browser = await import(pathToFileURL(path.join(__dirname, '../dist/browser.mjs')).href)
  await browser.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports); instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default; return module
  })
  for (const kind of ['execute', 'cancel']) {
    const f = await fixture(kind, browser), { reference } = await f.finish()
    const result = await f.session.recoverAction(f.index, reference)
    assert.equal(result.completed, true)
    assert.equal(result.row.status, kind === 'execute' ? 'executed' : 'cancelled')
    assert.equal(result.row.execution_uncertain, kind === 'cancel' ? true : undefined)
    assert.equal(await f.journal.executedCommitmentIndex(result.row), f.index)
    assert.equal(f.state.recipientRao, kind === 'execute' ? 1_000_000_000n : 0n)
    assert.equal((await f.session.planRotation('new-controller', alice.ss58Address)).toWallet, 'new-controller')
  }
})
