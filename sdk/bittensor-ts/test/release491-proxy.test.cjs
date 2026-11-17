'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const browser = require('../dist/browser.js')
const { buildIntent, intentSignerRole, encodeIntentCall, checkIntentPolicy } = require('../dist/intents')
const { announcedCallRequiresDirect, decodedCallDescriptor, validateAnnouncedCall } = require('../dist/proxy-announcements')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const nativeRuntime = new sdk.Runtime(metadata, 419, 1, 42)
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const context = { coldkey: alice.ss58Address, hotkey: bob.ss58Address }
const hex = data => '0x' + Buffer.from(data).toString('hex')
const transferArgs = { dest: { Id: bob.publicKey }, value: 9_007_199_254_740_993n }

let wasm
async function wasmRuntime() {
  if (wasm) return wasm
  browser.configureBrowserWasm(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default
    return module
  })
  await browser.initBrowser()
  return wasm = new browser.Runtime(metadata, 419, 1, 42)
}
function reader(runtime = nativeRuntime, overrides = {}) {
  return {
    query: async () => { throw new Error('saved replay must not rebuild a dynamic inner intent') },
    runtimeCall: async () => { throw new Error('saved replay must not requote a dynamic inner intent') },
    decodeAddress: sdk.publicKeyFromSs58,
    composeCall: call => encodeIntentCall(call, async (p, f, args) => runtime.composeCall(p, f, args)),
    decodeCall: async data => runtime.decodeCall(data),
    hashCallData: sdk.blake2_256,
    specVersion: () => 419,
    ...overrides,
  }
}
function savedArgs(data, extra = {}) {
  return {
    delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address,
    inner_op: 'transfer', inner_args: { dest_ss58: bob.ss58Address, amount_tao: '9007199.254740993', keep_alive: false },
    announced_call_data: hex(data), expected_call_hash: hex(sdk.blake2_256(data)), announced_spec_version: 419,
    ...extra,
  }
}

test('saved announcements retain exact native and WASM call bytes for their declared payment', async () => {
  const data = nativeRuntime.composeCall('Balances', 'transfer_allow_death', transferArgs)
  for (const runtime of [nativeRuntime, await wasmRuntime()]) {
    const plan = await buildIntent('execute_proxy_announced', savedArgs(data)).prepare(reader(runtime), context)
    assert.equal(plan.call.params.call.callData, hex(data))
    assert.equal(plan.call.params.call.pallet, 'Balances')
    assert.equal(BigInt(plan.call.params.call.params.value), transferArgs.value)
    const encoded = await encodeIntentCall(plan.call, async (p, f, args) => runtime.composeCall(p, f, args))
    const expected = runtime.composeCall('Proxy', 'proxy_announced', { delegate: alice.ss58Address, real: bob.ss58Address, force_proxy_type: null, call: data })
    assert.deepEqual(Buffer.from(encoded), Buffer.from(expected))
    assert.equal(plan.extras.announced_call_hash, hex(sdk.blake2_256(data)))
    assert.match(checkIntentPolicy(plan, { maxSpendRao: 1n }).join(), /unbounded spend/)
    assert.match(checkIntentPolicy(plan, { allowedNetuids: [1], allowGlobal: true }).join(), /all subnets/)
    assert(Object.isFrozen(plan.call.params.call))
  }
})

test('saved announcement hash, malformed bytes, changed runtime semantics and failed decoding fail closed', async () => {
  const data = nativeRuntime.composeCall('Balances', 'transfer_allow_death', transferArgs), decoded = nativeRuntime.decodeCall(data)
  const prepare = (extra, view = reader()) => buildIntent('execute_proxy_announced', savedArgs(data, extra)).prepare(view, context)
  await assert.rejects(prepare({ expected_call_hash: '0x' + '00'.repeat(32) }), /selected announcement hash/)
  for (const announced_call_data of ['0x', '0x123', '00', '0xgg', '']) await assert.rejects(prepare({ announced_call_data }), /nonempty/)
  await assert.rejects(prepare({ announced_spec_version: 418 }), /original decoded semantics/)
  await assert.rejects(prepare({ announced_spec_version: 418, announced_call_semantics: { ...decoded, call_function: 'transfer_all' } }), /decodes.*differently/)
  assert.equal((await prepare({ announced_spec_version: 418, announced_call_semantics: decoded })).call.params.call.callData, hex(data))
  await assert.rejects(prepare({}, reader(nativeRuntime, { decodeCall: async () => { throw new Error('metadata unavailable') } })), /metadata unavailable/)
  const plan = await prepare({})
  const forged = { ...plan.call.params.call, params: { ...plan.call.params.call.params, value: 1n } }
  await assert.rejects(encodeIntentCall(forged, async (p, f, args) => nativeRuntime.composeCall(p, f, args)), /do not match/)
})

test('announced atomic batches are directly replayable but cannot consume announcements inside another batch', async () => {
  const leaf = nativeRuntime.composeCall('Balances', 'transfer_allow_death', transferArgs)
  const data = nativeRuntime.composeCall('Utility', 'batch_all', { calls: [leaf] })
  const args = savedArgs(data), intent = buildIntent('execute_proxy_announced', args)
  const plan = await intent.prepare(reader(), context)
  assert.equal(plan.extras.requires_direct_submission, true)
  const encoded = await encodeIntentCall(plan.call, async (p, f, args) => nativeRuntime.composeCall(p, f, args))
  assert(encoded.length > data.length)
  await assert.rejects(buildIntent('batch', { intents: [intent, { op: 'transfer', dest_ss58: bob.ss58Address, amount_tao: '1' }] }).prepare(reader(), context), /submit it directly/)
  const hidden = { pallet: 'Sudo', call: 'sudo', params: { call: decodedCallDescriptor(nativeRuntime.decodeCall(data)) } }
  assert.equal(announcedCallRequiresDirect(hidden), true)
  assert.equal(announcedCallRequiresDirect({ pallet: 'Crowdloan', call: 'finalize', params: { crowdloan_id: 1 } }), true)
  assert.equal(announcedCallRequiresDirect({ pallet: 'Utility', call: 'if_else', params: { main: decodedCallDescriptor(nativeRuntime.decodeCall(leaf)), fallback: hidden } }), true)
  assert.equal(announcedCallRequiresDirect({ pallet: 'Proxy', call: 'proxy', params: { call: 'opaque' } }), true)
  assert.equal(announcedCallRequiresDirect(decodedCallDescriptor(nativeRuntime.decodeCall(leaf))), false)
})

test('saved payload shielding follows actual calls and nested declarations', async () => {
  const registration = nativeRuntime.composeCall('SubtensorModule', 'burned_register', { netuid: 1, hotkey: alice.ss58Address })
  const declaredRegistration = await buildIntent('execute_proxy_announced', savedArgs(registration, {
    inner_op: 'burned_register', inner_args: { netuid: 1, hotkey_ss58: alice.ss58Address },
  })).prepare(reader(), context)
  assert.equal(declaredRegistration.mevShieldRequired, true)
  const data = nativeRuntime.composeCall('Utility', 'batch_all', { calls: [registration] })
  const nested = await buildIntent('execute_proxy_announced', savedArgs(data, {
    inner_op: 'batch', inner_args: { intents: [{ op: 'burned_register', netuid: 1, hotkey_ss58: alice.ss58Address }] },
  })).prepare(reader(), context)
  assert.equal(nested.mevShieldRequired, true)
})

test('proxy announcements map to exact call contracts and validate commitments', async () => {
  const call_hash = '0x' + '12'.repeat(32)
  for (const [op, fn, account] of [['announce_proxy_call', 'announce', 'real'], ['remove_proxy_announcement', 'remove_announcement', 'real'], ['reject_proxy_announcement', 'reject_announcement', 'delegate']]) {
    const args = { [account + '_ss58']: bob.ss58Address, call_hash }
    const plan = await buildIntent(op, args).prepare(reader(), context)
    assert.deepEqual(plan.call, { pallet: 'Proxy', call: fn, params: { [account]: bob.ss58Address, call_hash } })
    const encoded = await encodeIntentCall(plan.call, async (p, f, args) => nativeRuntime.composeCall(p, f, args))
    assert.equal(nativeRuntime.decodeCall(encoded).call_function, fn)
    await assert.rejects(buildIntent(op, { ...args, call_hash: '0x01' }).prepare(reader(), context), /Blake2-256/)
  }
})

test('add_proxy explicit account choice overrides only its historical type default', async () => {
  for (const [proxy_type, from_hotkey, expected] of [['Weights', undefined, 'hotkey'], ['Weights', false, 'coldkey'], ['Staking', true, 'hotkey'], ['Staking', false, 'coldkey']]) {
    const intent = buildIntent('add_proxy', { delegate_ss58: alice.ss58Address, proxy_type, ...(from_hotkey === undefined ? {} : { from_hotkey }) })
    assert.equal(intentSignerRole(intent), expected)
    const plan = await intent.prepare(reader(), context)
    assert.equal(plan.signerRole, expected)
    assert.equal(plan.dispatchOrigin, expected === 'hotkey' ? bob.ss58Address : alice.ss58Address)
    assert.equal(plan.call.signerRole, from_hotkey === undefined ? undefined : expected)
    assert.equal(plan.call.params.from_hotkey, undefined)
  }
  assert.throws(() => buildIntent('add_proxy', { delegate_ss58: alice.ss58Address, proxy_type: 'Weights', from_hotkey: 'false' }), /boolean/)
})

test('browser saved replay cannot smuggle an unsupported raw call through a declared transfer', async () => {
  const runtime = await wasmRuntime(), client = new browser.Client('local')
  const data = runtime.composeCall('System', 'remark', { remark: Uint8Array.of(1, 2, 3) })
  const block = '0x' + '11'.repeat(32)
  client.blockHash = async () => block
  client.at = async () => ({ hash: block, query: async () => ({ data: { free: 1_000_000_000_000n } }) })
  client.intentReader = async () => reader(runtime)
  client.peekNonce = async () => 0
  let estimates = 0
  client.estimateFee = async () => { estimates++; return sdk.Balance.fromRao(1n) }
  try {
    await assert.rejects(client.plan(buildIntent('execute_proxy_announced', savedArgs(data)), context, {}, { allowRawCall: false }), /unsupported saved announcement operation System\.remark/)
    assert.equal(estimates, 0, 'unsupported saved bytes must be rejected before fee or signing preparation')
    assert.throws(() => sdk.IntentCall.operation('System.remark', { remark: Uint8Array.of(1, 2, 3) }), /unknown trusted operation/)
  } finally { await client.close() }
})

test('saved call validation preserves native wrapper constraints and rejects opaque hash-only dispatch', async () => {
  const leaf = nativeRuntime.composeCall('Balances', 'transfer_allow_death', transferArgs)
  for (const call of ['batch', 'force_batch']) {
    const data = nativeRuntime.composeCall('Utility', call, { calls: [leaf] })
    await assert.rejects(buildIntent('execute_proxy_announced', savedArgs(data)).prepare(reader(), context), /unsupported saved announcement operation Utility\./)
  }
  const sudo = nativeRuntime.composeCall('Sudo', 'sudo', { call: leaf })
  await assert.rejects(buildIntent('execute_proxy_announced', savedArgs(sudo)).prepare(reader(), context), /trusted sudo only wraps declared root/)
  const rootCall = { pallet: 'AdminUtils', call: 'sudo_set_subnet_emission_enabled', params: { netuid: 1, enabled: true } }
  assert.doesNotThrow(() => validateAnnouncedCall({ pallet: 'Sudo', call: 'sudo', params: { call: { pallet: 'Utility', call: 'batch_all', params: { calls: [rootCall] } } } }))
  for (const call of ['approve_as_multi', 'approve_as_multi_safe', 'cancel_as_multi', 'cancel_as_multi_safe']) {
    assert.throws(() => validateAnnouncedCall({ pallet: 'Multisig', call, params: {} }), /unsupported saved announcement/)
  }
  const unknown = { pallet: 'System', call: 'remark', params: { remark: Uint8Array.of(1) } }
  assert.throws(() => validateAnnouncedCall({ pallet: 'Crowdloan', call: 'create', params: { call: unknown, target_address: null } }), /unsupported saved announcement operation System\.remark/)
})
