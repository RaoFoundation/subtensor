'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const crypto = require('node:crypto')
const { ProtectedAccountSession } = require('../dist/protected-account-lifecycle')
const { ProtectedAccountStore, deriveLegacyPureSs58 } = require('../dist/protected-account-config')
const { MemoryJsonStore } = require('../dist/preferences')
const { buildIntent } = require('../dist/intents')
const { SubmissionNotSentError } = require('../dist/transport')

const chain = '0x' + '11'.repeat(32), blockHash = '0x' + '22'.repeat(32)
const address = byte => '0x' + byte.toString(16).padStart(2, '0').repeat(32)
const controller = address(1), other = address(2), delegate = address(3), foreign = address(4)
const grant = (key, type, delay = 0) => ({ delegate: key, proxy_type: type, delay })
const success = extra => ({ status: 'finalized', success: true, blockHash, blockNumber: 100, extrinsicIndex: 2, events: [], ...extra })
const transfer = () => buildIntent('transfer', { dest_ss58: other, amount_tao: '1' })
const stake = extra => buildIntent('remove_stake', { hotkey_ss58: other, netuid: 1, amount_alpha: '1', slippage_protection: false, ...extra })

async function fixture({ ready = true, setup = false } = {}) {
  const store = new MemoryJsonStore(), state = { block: 100, grants: [], rows: [], submits: [], actions: [], events: [], swap: undefined, policy: undefined, dispute: undefined, writes: 0, created: false, free: 400n }
  const write = store.write.bind(store)
  store.write = async (...args) => { state.writes++; return write(...args) }
  const backend = {
    accountId: value => {
      if (!/^0x[0-9a-f]{64}$/i.test(value)) throw new TypeError('bad address')
      return Buffer.from(value.slice(2), 'hex')
    },
    ss58Encode: value => '0x' + Buffer.from(value).toString('hex'),
    hash: value => crypto.createHash('sha256').update(value).digest(),
    events: async () => state.events,
    receipt: async () => state.recoveredReceipt,
    snapshot: async () => ({
      genesisHash: chain, hash: blockHash, block: state.block, blockTime: 12, specVersion: 491,
      query: async (pallet, item) => {
        if (state.queryHook) await state.queryHook(pallet, item)
        if (item === 'Proxies') return [state.grants, 0n]
        if (item === 'Account') return { data: { free: state.free, reserved: 0n, frozen: 0n } }
        if (item === 'Announcements') return [[], 0n]
        if (item === 'DerivedAuthorities') return undefined
        if (item === 'ColdkeySwapAnnouncements') return state.swap
        if (item === 'ColdkeySwapV2PolicySnapshots') return state.policy
        if (item === 'ColdkeySwapDisputes') return state.dispute
        throw new Error(`unexpected query ${pallet}.${item}`)
      },
      constant: async (_pallet, item) => ({ AnnouncementDepositBase: 100n, AnnouncementDepositFactor: 10n, ExistentialDeposit: 1n })[item],
      fee: async () => 10n,
    }),
    submit: async (intent, options) => {
      state.submits.push({ intent, options }); state.actions.push(intent.op)
      if (state.submitHook) return state.submitHook(intent, options)
      if (intent.op === 'create_pure_proxy') {
        state.created = true
        state.grants = [grant(controller, 'Any', Number(intent.args.delay))]
        return success({ events: state.events })
      }
      if (intent.op === 'announce_coldkey_swap') {
        state.swap = [120, '0x' + backend.hash(backend.accountId(pure)).toString('hex')]
        return success()
      }
      if (intent.op === 'swap_coldkey_announced') { state.swap = undefined; return success() }
      return success()
    },
  }
  const pure = deriveLegacyPureSs58(backend, controller, { height: 100, ext_index: 2, index: 0 })
  state.events = [{ module_id: 'Proxy', event_id: 'PureCreated', extrinsic_idx: 2, attributes: { pure, who: controller, proxy_type: 'Any', disambiguation_index: 0 } }]
  const proof = { address: pure, spawner: controller, height: 100, ext_index: 2, index: 0, account_mode: 'legacy', block_hash: blockHash }
  const records = new ProtectedAccountStore(store, backend)
  const record = { controller_wallet: 'alice', controller_ss58: controller, protected_ss58: pure, genesis_hash: chain, delay_blocks: 10, allows: [], reviewed_allows: [], pending_allows: [], provenance: proof, setup: { status: setup ? 'pending' : 'ready', allow_types: [] }, coldkey_swap: { status: 'pending' } }
  state.grants = [grant(controller, 'Any', 10)]
  if (ready || setup) await records.upsert(record)
  const engine = {
    journal: {
      moveWallet: async (from, to) => { state.actions.push('move-journal'); for (const row of state.rows) if (row.controller_wallet === from) row.controller_wallet = to },
      bindLegacy: async () => 0,
    },
    reconcile: async () => structuredClone(state.rows.filter(row => ['pending', 'submitting', 'awaiting_approval'].includes(row.status))),
    announce: async (record, intent, options = {}) => {
      const encoded = intent.toJSON(), { op, ...inner_args } = encoded
      const row = { index: state.rows.length + 1, controller_wallet: record.controller_wallet, delegate_ss58: record.controller_ss58, real_ss58: record.protected_ss58, genesis_hash: chain, status: 'pending', inner_op: op, inner_args, category: options.category || op, ...options.metadata }
      state.rows.push(row); state.actions.push(`announce:${row.category}`)
      if (state.announceHook) return state.announceHook(row, intent, options)
      return { row, receipt: success(), completed: true }
    },
    inspect: async (_record, index) => state.rows.find(row => row.index === index),
    execute: async (_record, index) => {
      const row = state.rows.find(row => row.index === index)
      if (state.executeHook) return state.executeHook(row)
      row.status = 'executed'
      return { row, receipt: success(), completed: true }
    },
    cancel: async (_record, index) => {
      const row = state.rows.find(row => row.index === index); state.actions.push(`cancel:${index}`)
      if (state.cancelHook) return state.cancelHook(row)
      row.status = 'cancelled'
      return { row, receipt: success(), completed: true }
    },
  }
  const session = new ProtectedAccountSession(backend, records, engine, 'alice')
  const saved = () => records.getByController('alice', chain)
  const row = (index, spec, extras = {}) => ({ index, controller_wallet: 'alice', delegate_ss58: controller, real_ss58: pure, genesis_hash: chain, status: 'pending', category: 'allow:Staking', inner_op: spec.op, inner_args: { ...spec, op: undefined }, ...extras })
  return { session, state, backend, records, store, engine, record, saved, pure, proof, row }
}

test('setup planning is read-only; successful setup saves provenance and queues allows before migration', async () => {
  const f = await fixture({ ready: false }), before = f.state.writes
  const plan = await f.session.planSetup({ controllerAddress: controller, delay: 10, allows: ['staking'] })
  assert.equal(plan.create.op, 'create_pure_proxy')
  assert.deepEqual(plan.allowTypes, ['Staking'])
  assert.equal(f.state.writes, before)
  assert.equal(f.state.submits.length, 0)
  const result = await f.session.setup({ controllerAddress: controller, delay: 10, allows: ['staking'] })
  assert.equal(result.status, 'ready')
  assert.deepEqual(f.state.actions, ['create_pure_proxy', 'announce:allow:Staking', 'announce_coldkey_swap'])
  assert.equal(result.record.protected_ss58, f.pure)
  assert.deepEqual(result.record.pending_allows, ['Staking'])
  assert.equal(result.record.coldkey_swap.execute_block, 120)
  assert.equal(await f.records.getSetup('alice', chain), undefined)
  await assert.rejects(f.session.executeMigration(), /not ready/)
  f.state.block = 120
  assert.equal((await f.session.executeMigration()).success, true)
  assert.equal((await f.saved()).coldkey_swap.status, 'executed')
})

test('uncertain creation is durably reserved and never creates a second pure on resume', async () => {
  const f = await fixture({ ready: false })
  f.state.submitHook = async () => ({ status: 'uncertain', success: null, extrinsicHash: '0x' + '33'.repeat(32), events: [] })
  const first = await f.session.setup({ controllerAddress: controller, delay: 10, allows: ['staking'], allowKey: delegate })
  assert.equal(first.status, 'uncertain')
  assert.equal((await f.records.getSetup('alice', chain)).status, 'uncertain')
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'uncertain')
  assert.equal(f.state.submits.length, 1)
  await assert.rejects(f.session.resumeSetup({ controllerAddress: controller, allowKey: other }), /allow key/)
  f.state.recoveredReceipt = success({ events: f.state.events })
  f.state.submitHook = undefined
  const resumed = await f.session.resumeSetup({ controllerAddress: controller })
  assert.equal(resumed.status, 'ready')
  assert.deepEqual(resumed.record.reviewed_allows, ['Staking'])
  assert.equal(resumed.record.allow_key_ss58, delegate)
  assert.equal(f.state.submits.filter(row => row.intent.op === 'create_pure_proxy').length, 1)
})

test('concurrent sessions reserve exactly one pure creation', async () => {
  const f = await fixture({ ready: false })
  const separateStore = { read: key => f.store.read(key), write: (key, value) => f.store.write(key, value), update: (key, action) => f.store.update(key, action) }
  const second = new ProtectedAccountSession(f.backend, new ProtectedAccountStore(separateStore, f.backend), f.engine, 'alice')
  const outcomes = await Promise.allSettled([f.session.setup({ controllerAddress: controller, delay: 10 }), second.setup({ controllerAddress: controller, delay: 10 })])
  assert.equal(outcomes.filter(result => result.status === 'fulfilled' && result.value.status === 'ready').length, 1)
  assert.equal(f.state.submits.filter(row => row.intent.op === 'create_pure_proxy').length, 1)
})

test('separate stores atomically reserve migration before broadcasting and preserve a concurrent reservation', async () => {
  const f = await fixture({ setup: true })
  const separateStore = { read: key => f.store.read(key), write: (key, value) => f.store.write(key, value), update: (key, action) => f.store.update(key, action) }
  const second = new ProtectedAccountSession(f.backend, new ProtectedAccountStore(separateStore, f.backend), f.engine, 'alice')
  let release, started
  const gate = new Promise(resolve => { release = resolve }), entering = new Promise(resolve => { started = resolve })
  f.state.submitHook = async intent => {
    assert.equal(intent.op, 'announce_coldkey_swap')
    started()
    await gate
    f.state.swap = [120, '0x' + f.backend.hash(f.backend.accountId(f.pure)).toString('hex')]
    return success()
  }
  const first = f.session.resumeSetup({ controllerAddress: controller }), other = second.resumeSetup({ controllerAddress: controller })
  await entering
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(f.state.submits.length, 1)
  assert.equal((await f.saved()).coldkey_swap.status, 'submitting')
  release()
  const outcomes = await Promise.all([first, other])
  assert.equal(outcomes.filter(value => value.status === 'ready').length, 1)
  assert.equal(f.state.submits.length, 1)
})

test('late creation recovery cannot replace a bound record or overwrite its migration reservation', async () => {
  const f = await fixture({ ready: false }), receipt = success({ events: f.state.events })
  await f.records.saveSetup({ controller_wallet: 'alice', controller_ss58: controller, genesis_hash: chain, delay_blocks: 10, index: 0, allow_types: [], status: 'uncertain', receipt })
  const store2 = { read: key => f.store.read(key), write: (key, value) => f.store.write(key, value), update: (key, action) => f.store.update(key, action) }
  const second = new ProtectedAccountSession(f.backend, new ProtectedAccountStore(store2, f.backend), f.engine, 'alice')
  let releaseProof, releaseSubmit, enteredSubmit, eventReads = 0
  const proofGate = new Promise(resolve => { releaseProof = resolve }), submitGate = new Promise(resolve => { releaseSubmit = resolve }), entered = new Promise(resolve => { enteredSubmit = resolve })
  f.backend.events = async () => { if (++eventReads === 2) await proofGate; return f.state.events }
  f.state.submitHook = async intent => { assert.equal(intent.op, 'announce_coldkey_swap'); enteredSubmit(); await submitGate; f.state.swap = [120, '0x' + f.backend.hash(f.backend.accountId(f.pure)).toString('hex')]; return success() }
  const first = f.session.resumeSetup({ controllerAddress: controller }), later = second.resumeSetup({ controllerAddress: controller })
  await entered
  releaseProof()
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(f.state.submits.length, 1)
  assert.equal((await f.saved()).coldkey_swap.status, 'submitting')
  releaseSubmit()
  await Promise.all([first, later])
  assert.equal(f.state.submits.length, 1)
})

test('multisig approval-only creation and nested dispatch failure never establish a ready binding', async () => {
  for (const [events, status] of [
    [[{ module_id: 'Multisig', event_id: 'NewMultisig', attributes: {} }], 'awaiting_approval'],
    [[{ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Err: 'BadOrigin' } } }], 'uncertain'],
  ]) {
    const f = await fixture({ ready: false })
    f.state.submitHook = async () => success({ events })
    assert.equal((await f.session.setup({ controllerAddress: controller, delay: 10 })).status, status)
    assert.equal(await f.saved(), undefined)
    assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, status)
    assert.equal(f.state.submits.length, 1)
  }
})

test('a definitive failed creation requires explicit retry and is never confused with uncertainty', async () => {
  const f = await fixture({ ready: false })
  f.state.submitHook = async () => success({ status: 'failed', success: false })
  assert.equal((await f.session.setup({ controllerAddress: controller, delay: 10 })).status, 'failed')
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'failed')
  assert.equal(f.state.submits.length, 1)
  f.state.submitHook = undefined
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller, retryFailedCreation: true })).status, 'ready')
  assert.equal(f.state.submits.filter(row => row.intent.op === 'create_pure_proxy').length, 2)
})

test('known unsent creation releases its reservation while ambiguous errors retain the transaction hash', async () => {
  const f = await fixture({ ready: false })
  f.state.submitHook = async () => { throw new SubmissionNotSentError(new Error('user rejected signer')) }
  await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10 }), /user rejected/)
  assert.equal(await f.records.getSetup('alice', chain), undefined)
  const transaction = '0x' + '55'.repeat(32)
  f.state.submitHook = async () => { throw Object.assign(new Error('connection lost'), { extrinsicHash: transaction }) }
  await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10 }), /connection lost/)
  assert.equal((await f.records.getSetup('alice', chain)).extrinsic_hash, transaction)
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'uncertain')
  assert.equal(f.state.submits.length, 2)
})

test('interrupted post-creation RPC resumes the saved pure instead of recreating it', async () => {
  const f = await fixture({ ready: false })
  f.state.queryHook = async (_pallet, item) => { if (item === 'ColdkeySwapAnnouncements') throw new Error('RPC unavailable') }
  await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10 }), /RPC unavailable/)
  assert.equal((await f.saved()).protected_ss58, f.pure)
  f.state.queryHook = undefined
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'ready')
  assert.equal(f.state.submits.filter(row => row.intent.op === 'create_pure_proxy').length, 1)
})

// PROTECTED_REGRESSION: migration must not move funds to a forged or competing-authority pure; changes require explicit human approval.
test('setup refuses false provenance, shorter Any siblings and foreign proxy authority before migration', async () => {
  for (const mutate of [f => { f.state.events = [] }, f => { f.state.grants.push(grant(controller, 'Any', 1)) }, f => { f.state.grants.push(grant(foreign, 'NonFungible')) }]) {
    const f = await fixture({ ready: false }); mutate(f)
    await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10, reuse: f.proof }), /provenance|delay|authority/)
    assert.equal(f.state.submits.length, 0)
  }
  const f = await fixture({ ready: false })
  await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10, reuse: { ...f.proof, address: foreign } }), /creation coordinates/)
})

test('resumed migration accepts authenticated false V2 policy and refuses disputes including block zero', async () => {
  const f = await fixture({ setup: true })
  f.state.policy = false
  f.state.swap = [120, '0x' + f.backend.hash(Buffer.concat([Buffer.from('coldkey-swap-v2-policy'), f.backend.accountId(f.pure), Buffer.from([0])])).toString('hex')]
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'ready')
  assert.equal(f.state.submits.length, 0)
  f.state.dispute = 0
  await assert.rejects(f.session.migrationStatus(), /recovery locked/)
})

test('migration lost response resolves by receipt hash and retries only proven failure', async () => {
  const f = await fixture({ setup: true }), transaction = '0x' + '66'.repeat(32)
  f.state.submitHook = async () => { throw Object.assign(new Error('lost migration response'), { extrinsicHash: transaction }) }
  await assert.rejects(f.session.resumeSetup({ controllerAddress: controller }), /lost migration response/)
  assert.equal((await f.saved()).coldkey_swap.receipt.extrinsicHash, transaction)
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'uncertain')
  assert.equal(f.state.submits.length, 1)
  f.state.recoveredReceipt = success({ status: 'failed', success: false })
  f.state.submitHook = undefined
  assert.equal((await f.session.resumeSetup({ controllerAddress: controller })).status, 'ready')
  assert.equal(f.state.submits.length, 2)
})

test('routing uses live grants, matches separate allow key and preserves argument-dependent delayed calls', async () => {
  const f = await fixture()
  await f.records.upsert({ ...f.record, allows: ['Staking'], allow_key_ss58: delegate })
  assert.equal((await f.session.route(stake())).kind, 'delayed')
  f.state.grants.push(grant(delegate, 'Staking'))
  assert.equal((await f.session.route(stake())).kind, 'delayed')
  assert.equal((await f.session.route(stake(), { signerAddress: delegate })).kind, 'instant')
  await assert.rejects(f.session.route(stake({ claim: true }), { signerAddress: delegate }), /requires the protected controller/)
  assert.equal((await f.session.route(stake({ claim: true }))).kind, 'delayed')
  assert.equal((await f.session.route(transfer())).kind, 'delayed')
})

// PROTECTED_REGRESSION: ordinary nested management must not orphan or install immediate authority on a keyless account; changes require explicit human approval.
test('ordinary management is blocked through batches and nested multisig intents', async () => {
  const f = await fixture(), dangerous = buildIntent('remove_proxies', {})
  for (const intent of [dangerous, buildIntent('batch', { intents: [dangerous] }), buildIntent('multisig_threshold_1', { other_signatories: [other], call: dangerous })]) {
    await assert.rejects(f.session.submit(intent), /ordinary/)
  }
  assert.equal(f.state.submits.length, 0)
  assert.equal(f.state.rows.length, 0)
})

test('PoW routing refuses the delayed path and permits only a live Registration grant', async () => {
  const f = await fixture(), pow = buildIntent('pow_register', { netuid: 1, work_block: 100, nonce: 0, work_hex: '0x' + '00'.repeat(32), hotkey_ss58: other })
  await assert.rejects(f.session.route(pow), /five blocks/)
  f.state.grants.push(grant(controller, 'Registration'))
  assert.equal((await f.session.route(pow)).forceProxyType, 'Registration')
})

test('hotkey operations never silently use the protected coldkey controller', async () => {
  const f = await fixture(), intent = buildIntent('add_proxy', { delegate_ss58: delegate, proxy_type: 'Weights', from_hotkey: true })
  await assert.rejects(f.session.submit(intent), /explicit signerAddress/)
  await f.session.submit(intent, { signerAddress: other })
  assert.equal(f.state.submits[0].options.signerAddress, other)
})

// PROTECTED_REGRESSION: revocation must cancel every executable grant commitment before queuing removal; changes require explicit human approval.
test('settings cancel duplicate and nested grants before revoking a permission installed during cancellation', async () => {
  const f = await fixture(), add = { op: 'add_proxy', delegate_ss58: controller, proxy_type: 'Staking', delay: 0, from_hotkey: false }
  f.state.rows = [f.row(1, add), f.row(2, { op: 'batch', intents: [add] }, { category: 'rotate-controller' })]
  f.state.cancelHook = async row => { row.status = 'cancelled'; f.state.grants = [grant(controller, 'Any', 10), grant(controller, 'Staking')]; return { row, completed: true, receipt: success() } }
  const plan = await f.session.planSettings({ disallow: ['staking'] })
  assert.deepEqual(plan.cancelIndices, [1, 2])
  assert.equal(plan.actions.length, 0)
  const before = f.state.actions.length
  const result = await f.session.updateSettings({ disallow: ['staking'] })
  assert.deepEqual(f.state.actions.slice(before), ['cancel:1', 'cancel:2', 'announce:disallow:Staking'])
  assert.deepEqual(result.record.allows, ['Staking'], 'live permission remains until delayed removal executes')
})

test('uncertain cancellation and unknown plaintext stop revocation and allow-key changes', async () => {
  const f = await fixture(), add = { op: 'add_proxy', delegate_ss58: controller, proxy_type: 'Staking', delay: 0 }
  f.state.rows = [f.row(1, add)]
  f.state.cancelHook = async row => ({ row, completed: false, receipt: { status: 'uncertain', success: null, events: [] } })
  await assert.rejects(f.session.updateSettings({ disallow: ['staking'] }), /not confirmed cancelled/)
  assert.equal(f.state.rows.length, 1)
  await assert.rejects(f.session.planSettings({ allowKey: delegate }), /old allow key/)
  f.state.rows = [f.row(2, { op: '' }, { plaintext_missing: true, category: 'unknown' })]
  await assert.rejects(f.session.planSettings({ disallow: ['staking'] }), /missing plaintext/)
})

test('pending delay updates retain the live window and planning never saves requested settings', async () => {
  const f = await fixture(), before = await f.store.read('protected')
  const plan = await f.session.planSettings({ delay: 20 })
  assert.equal(plan.record.delay_blocks, 10)
  assert.deepEqual(await f.store.read('protected'), before)
  await f.session.updateSettings({ delay: 20 })
  assert.equal((await f.saved()).delay_blocks, 10)
  assert.equal((await f.saved()).pending_delay_blocks, 20)
})

test('inspection reports defeated protection without mutating the stored policy or allowing delayed submissions', async () => {
  const f = await fixture(), original = await f.store.read('protected')
  f.state.grants.push(grant(controller, 'Any', 0))
  const report = await f.session.inspect()
  assert.equal(report.observedDelayBlocks, 0)
  assert.match(report.protectionWarnings.join(), /zero-delay Any/)
  assert.deepEqual(await f.store.read('protected'), original)
  await assert.rejects(f.session.submit(transfer()), /nonzero Any delay/)
})

// PROTECTED_REGRESSION: controller rotation must clear planted grants and retain only reviewed live policy; changes require explicit human approval.
test('rotation resets all authority and migrates only reviewed live allows', async () => {
  const f = await fixture()
  await f.records.upsert({ ...f.record, reviewed_allows: ['Staking'], allows: ['Staking', 'Owner'] })
  f.state.grants.push(grant(controller, 'Staking'), grant(controller, 'Owner'), grant(foreign, 'Transfer'))
  const plan = await f.session.planRotation('bob', other), children = plan.intent.toJSON().intents
  assert.deepEqual(children.map(child => child.op), ['remove_proxies', 'add_proxy', 'add_proxy'])
  assert.equal(children[1].delegate_ss58, other)
  assert.equal(children[1].delay, 10)
  assert.equal(children[2].proxy_type, 'Staking')
  assert.equal(children[2].from_hotkey, false)
  assert.deepEqual(plan.migratedAllows, ['Staking'])
  await f.session.rotate('bob', other)
  await assert.rejects(f.session.finalizeRotation('bob', other), /rotation is incomplete/)
  f.state.grants = [grant(other, 'Any', 10), grant(other, 'Staking'), grant(controller, 'Weights')]
  await assert.rejects(f.session.finalizeRotation('bob', other), /old controller retains/)
  f.state.grants.pop()
  const moved = await f.session.finalizeRotation('bob', other)
  assert.equal(moved.controller_wallet, 'bob')
  assert.equal(moved.controller_ss58, other)
  assert.equal(await f.saved(), undefined)
  assert.equal(moved.pending_rotate, undefined)
  assert.deepEqual(moved.allows, ['Staking'])
})

test('rotation refuses pending commitments and conflicting destination bindings', async () => {
  const f = await fixture()
  f.state.rows = [f.row(1, transfer().toJSON(), { category: 'transfer' })]
  await assert.rejects(f.session.planRotation('bob', other), /all old-controller announcements/)
  f.state.rows = []
  await f.records.upsert({ ...f.record, controller_wallet: 'bob', controller_ss58: other, protected_ss58: foreign })
  await assert.rejects(f.session.planRotation('bob', other), /already controls another/)
})

test('rotation finalization resumes journal rebinding after config has already moved', async () => {
  const f = await fixture()
  await f.session.rotate('bob', other)
  f.state.grants = [grant(other, 'Any', 10)]
  const move = f.engine.journal.moveWallet
  f.engine.journal.moveWallet = async () => { throw new Error('journal disk unavailable') }
  await assert.rejects(f.session.finalizeRotation('bob', other), /journal disk unavailable/)
  assert.equal(await f.saved(), undefined)
  assert.equal((await f.records.getByController('bob', chain)).rotation_rebind.status, 'pending')
  f.engine.journal.moveWallet = move
  const result = await f.session.finalizeRotation('bob', other)
  assert.equal(result.rotation_rebind.status, 'complete')
  assert.equal(f.state.rows[0].controller_wallet, 'bob')
  assert.equal((await f.session.finalizeRotation('bob', other)).controller_wallet, 'bob')
  await assert.rejects(f.session.finalizeRotation('carol', foreign), /matching resumable/)
})

test('legacy binding requires explicit historical proof and selects the current chain only', async () => {
  const f = await fixture()
  await f.records.remove('alice', chain)
  await f.records.upsert({ ...f.record, genesis_hash: undefined })
  await assert.rejects(f.session.route(transfer()), /explicit network binding/)
  f.state.events = []
  await assert.rejects(f.session.bindLegacy(), /provenance/)
  assert.equal((await f.records.getByController('alice')).genesis_hash, undefined)
  f.state.events = [{ module_id: 'Proxy', event_id: 'PureCreated', extrinsic_idx: 2, attributes: { pure: f.pure, who: controller, proxy_type: 'Any', disambiguation_index: 0 } }]
  assert.equal((await f.session.bindLegacy()).genesis_hash, chain)
  assert.equal(await f.records.getByController('alice'), undefined)
})

// PROTECTED_REGRESSION: controller refill must stay capped and delayed instead of bypassing protected spending authority; changes require explicit human approval.
test('funding and refill plans are read-only and capped refill forces an atomic delayed route', async () => {
  const f = await fixture(), original = await f.store.read('protected')
  f.state.free = 140n
  f.state.grants.push(grant(controller, 'Staking'))
  const quote = await f.session.funding()
  assert.equal(quote.target_rao, 701n)
  assert.equal(quote.top_up_rao, 561n)
  assert.equal(quote.can_announce, true)
  const plan = await f.session.planRefill(stake(), { maxRefillRao: 600n })
  assert.equal(plan.refill.offered, true)
  assert.deepEqual(plan.intent.toJSON().intents.map(child => child.op), ['remove_stake', 'transfer'])
  assert.deepEqual(await f.store.read('protected'), original)
  assert.equal(f.state.submits.length, 0)
  await assert.rejects(f.session.submit(stake(), { refill: { maxRefillRao: 500n } }), /maxRefillRao/)
  assert.equal(f.state.rows.length, 0)
  await f.session.submit(stake(), { refill: { maxRefillRao: 600n } })
  assert.equal(f.state.submits.length, 0, 'refill cannot use the live instant Staking grant')
  assert.equal(f.state.rows.length, 1)
  const payload = f.state.rows[0].inner_args.intents
  assert.deepEqual(payload.map(child => child.op), ['remove_stake', 'transfer'])
  assert.equal(payload[1].dest_ss58, controller)
  assert.equal(payload[1].amount_tao, '0.000000561')
  assert.equal((await f.session.planRefill(stake())).refill.reason, 'already_pending')
})

test('external funding is explicit, refuses in-flight migration and checks exact amount before transfer', async () => {
  const f = await fixture()
  f.state.free = 140n
  f.state.swap = [120, '0x' + f.backend.hash(f.backend.accountId(f.pure)).toString('hex')]
  await assert.rejects(f.session.fund(other), /complete the coldkey migration/)
  f.state.swap = undefined
  await assert.rejects(f.session.fund(other, { expectedFundingRao: 560n }), /changed since approval/)
  assert.equal(f.state.submits.length, 0)
  const result = await f.session.fund(other, { expectedFundingRao: 561n, maxFundingRao: 600n })
  assert.equal(result.amount_rao, 561n)
  assert.equal(f.state.submits[0].options.signerAddress, other)
  assert.equal(f.state.submits[0].intent.args.dest_ss58, controller)
  f.state.free = 1n
  await assert.rejects(f.session.submit(transfer(), { refill: { maxRefillRao: 1000n } }), /fund it from a separate account/)
})
