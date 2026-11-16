'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { buildIntent, encodeIntentCall } = require('../dist/intents')
const { MemoryJsonStore } = require('../dist/preferences')
const { ProxyAnnouncementStore } = require('../dist/announcement-store')
const { ProtectedAnnouncements } = require('../dist/protected-announcements')
const { protectedManagementPath } = require('../dist/protected-account-config')
const { evaluatePolicy } = require('../dist/policy')
const { decodedCallDescriptor, announcedCallLeaves, announcedCallRequiresDirect } = require('../dist/proxy-announcements')
const golden = require('../../python/tests/fixtures/golden.json')

const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const controller = sdk.Keypair.fromUri('//Alice').ss58Address
const pure = sdk.Keypair.fromUri('//Bob').ss58Address
const recipient = sdk.Keypair.fromUri('//Charlie').ss58Address
const genesis = '0x' + 'aa'.repeat(32)
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const crowdloan = call => ({ op: 'create_crowdloan', deposit_tao: '1', min_contribution_tao: '1', cap_tao: '2', end: 200, call })

function fixture() {
  const state = { submits: [], announcements: [], grants: [{ delegate: controller, proxy_type: 'Any', delay: 10 }] }
  const record = { controller_wallet: 'alice', controller_ss58: controller, protected_ss58: pure, genesis_hash: genesis, delay_blocks: 10 }
  const journal = new ProxyAnnouncementStore(new MemoryJsonStore(), address => hex(sdk.publicKeyFromSs58(address)))
  const compose = call => encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
  const query = async (pallet, name) => {
    if (pallet === 'Proxy' && name === 'Proxies') return [state.grants, 0n]
    if (pallet === 'Proxy' && name === 'Announcements') return [state.announcements, 0n]
    if (pallet === 'System' && name === 'Account') return { data: { free: 10_000_000_000n } }
    throw Error(`unexpected query ${pallet}.${name}`)
  }
  const reader = { query, runtimeCall: async () => { throw Error('unexpected runtime call') }, decodeAddress: sdk.publicKeyFromSs58 }
  const view = {
    genesisHash: genesis, hash: '0x' + 'bb'.repeat(32), block: 100, specVersion: 419, blockTime: 12,
    query, queryMap: async () => [], constant: async () => 0n,
    prepare: (intent, context) => intent.prepare(reader, context), compose,
    decodeCall: async bytes => runtime.decodeCall(bytes), hashCallData: sdk.blake2_256, fee: async () => 1n,
  }
  const backend = {
    accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256,
    snapshot: async () => view,
    submit: async intent => { state.submits.push(intent); throw Error('unsafe submission reached') },
  }
  return { state, record, journal, compose, view, engine: new ProtectedAnnouncements(backend, journal) }
}

// PROTECTED_REGRESSION: deferred crowdloan calls must not remove a pure account's controllers; changes require explicit human approval.
test('protected announcement refuses management hidden inside a crowdloan', async () => {
  const f = fixture()
  const dangerous = crowdloan({ op: 'batch', intents: [{ op: 'remove_proxies' }] })
  await assert.rejects(f.engine.prepare(f.record, buildIntent('create_crowdloan', dangerous)), /protected management/)
  assert.equal((await f.journal.all()).length, 0)
  assert.equal(f.state.submits.length, 0)
  assert.equal(f.state.grants.length, 1)
})

// PROTECTED_REGRESSION: saved crowdloan payloads must not bypass management approval and strand pure assets; changes require explicit human approval.
test('protected execution rejects saved crowdloan management and forged harmless intent metadata', async () => {
  for (const forged of [false, true]) {
    const f = fixture()
    const spec = crowdloan({ op: 'remove_proxies' })
    const plan = await buildIntent('create_crowdloan', spec).prepare({ query: f.view.query, runtimeCall: async () => null }, { coldkey: pure })
    const data = await f.compose(plan.call), callHash = hex(sdk.blake2_256(data))
    const { op, ...args } = forged ? { op: 'transfer', dest_ss58: recipient, amount_tao: '1' } : spec
    const row = await f.journal.add({
      controller_wallet: 'alice', delegate_ss58: controller, real_ss58: pure, genesis_hash: genesis,
      inner_op: op, inner_args: args, category: 'create_crowdloan', management_approved: false,
      delay_blocks: 10, announced_at_block: 80, call_hash: callHash, call_data: hex(data), runtime_spec_version: 419,
    })
    f.state.announcements = [{ real: pure, call_hash: callHash, height: 80 }]
    await assert.rejects(f.engine.execute(f.record, row.index), /management/)
    assert.equal(f.state.submits.length, 0)
    assert.equal(f.state.grants.length, 1)
  }
})

test('crowdloan authority traversal reaches nested intent and decoded wrappers', () => {
  const nested = crowdloan({ op: 'multisig_execute', call: { op: 'batch', intents: [{ op: 'remove_proxies' }] } })
  assert.equal(protectedManagementPath(nested), 'create_crowdloan.multisig_execute.batch[0].remove_proxies')
  const grant = crowdloan({ op: 'add_proxy', delegate_ss58: recipient, proxy_type: 'Any', delay: 0 })
  const grants = evaluatePolicy('protected.announcement_grants', { inner_op: grant.op, inner_args: grant })
  assert.deepEqual(grants, [{ delegate_ss58: recipient, proxy_type: 'Any', delay: 0 }])
  for (const wrapper of ['as_multi', 'as_multi_safe', 'as_multi_threshold_1', 'as_multi_threshold_1_safe']) {
    const decoded = decodedCallDescriptor({ Crowdloan: { create: { target_address: null, call: { Multisig: { [wrapper]: { call: { Proxy: { remove_proxies: {} } } } } } } } })
    assert.deepEqual([...announcedCallLeaves(decoded)].map(call => `${call.pallet}.${call.call}`), ['Proxy.remove_proxies'])
  }
})

test('crowdloan target transfers and harmless deferred batches remain supported', async () => {
  const f = fixture()
  const target = crowdloan(null)
  target.target_ss58 = recipient
  assert.equal(protectedManagementPath(target), undefined)
  assert.equal((await f.engine.prepare(f.record, buildIntent('create_crowdloan', target))).row.management_approved, false)
  const deferred = crowdloan({ op: 'batch', intents: [{ op: 'transfer', dest_ss58: recipient, amount_tao: '1' }] })
  const payload = await f.engine.prepare(f.record, buildIntent('create_crowdloan', deferred))
  assert.equal(announcedCallRequiresDirect(payload.prepared.call), false)
  assert.deepEqual([...announcedCallLeaves(payload.prepared.call)].map(call => `${call.pallet}.${call.call}`), ['Balances.transfer_keep_alive'])
})

test('native and browser Rust policy inspect the same deferred crowdloan authority', async () => {
  const fs = require('node:fs/promises')
  const path = require('node:path')
  const { pathToFileURL } = require('node:url')
  const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
  bindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  const intent = crowdloan({ op: 'remove_proxies' })
  const call = { pallet: 'Crowdloan', call: 'create', params: { call: { pallet: 'Proxy', call: 'remove_proxies', params: {} } } }
  for (const [operation, input, expected] of [
    ['protected.management_path', { intent }, 'create_crowdloan.remove_proxies'],
    ['protected.call_leaves', { call }, [call.params.call]],
    ['protected.requires_direct', { call }, false],
  ]) {
    assert.deepEqual(evaluatePolicy(operation, input), expected)
    assert.deepEqual(JSON.parse(bindings.policyEvaluate(operation, JSON.stringify(input))).value, expected)
  }
})
