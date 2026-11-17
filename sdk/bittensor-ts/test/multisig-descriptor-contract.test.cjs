const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { NodeMultisig } = require('../dist/node-workflows')
const { IntentCall, Policy } = require('../dist/transaction')
const { encodeIntentCall } = require('../dist/intents')
const { toWire } = require('../dist/wire')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const descriptor = (pallet, call, params = {}) => ({ pallet, call, params })
const registration = descriptor('SubtensorModule', 'burned_register', { netuid: 1, hotkey: bob.ss58Address })
const collateral = descriptor('SubtensorModule', 'add_collateral', { netuid: 1, hotkey: bob.ss58Address, alpha: 1n, limit_price: 2n })
const payment = descriptor('Balances', 'transfer_keep_alive', { dest: bob.ss58Address, value: 1n })

function nodeFixture(t) {
  const client = new sdk.Client('local'), state = { submitted: 0, signed: 0, quotedCarriers: 0, shieldKeyReads: 0 }
  const fromDescriptor = IntentCall.fromDescriptor, fromPolicy = Policy.from
  // Keep native call construction outside the fake chain fixture; the shared
  // shielding policy and real SCALE composer run unchanged.
  IntentCall.fromDescriptor = (_native, call) => ({ pallet: call.pallet, function: call.call, params: call.params })
  Policy.from = () => ({ check: () => [] })
  t.after(async () => { IntentCall.fromDescriptor = fromDescriptor; Policy.from = fromPolicy; await client.close() })
  const signingPlan = (data, publicKey) => ({ runtime: runtime.handle, publicKey, callData: data, cryptoType: 1, feeRao: 1000n, txParams: { era: toWire('00'), nonce: 0n, tip: 0n } })
  const native = {
    accountNextIndex: async () => 0,
    externalSigningPlan: async (data, _address, publicKey) => signingPlan(data, publicKey),
    externalSigningPlanForIntent: async (call, _address, publicKey) => ({ ...signingPlan(await encodeIntentCall(descriptor(call.pallet, call.function, call.params), (p, f, args) => runtime.composeCall(p, f, args)), publicKey), runtime: { encodeSignedExtrinsic: () => ({ bytes: Buffer.alloc(128) }) } }),
  }
  Object.assign(client, {
    runtimeAt: async () => runtime,
    blockHash: async () => '0x' + '11'.repeat(32),
    blockNumber: async () => 40,
    composeCall: async (p, f, args) => { if (p === 'MevShield') { state.quotedCarriers++; return Buffer.alloc(16) } return runtime.composeCall(p, f, args) },
    query: async (p, item) => {
      if (p === 'MevShield') { state.shieldKeyReads++; return Buffer.alloc(0) }
      if (item === 'Multisigs') return null
      if (item === 'Account') return { data: { free: 100_000_000_000n } }
      throw Error(`unexpected query ${p}.${item}`)
    },
    constant: async () => 1000n,
    rpc: async () => ({ weight: { ref_time: 1, proof_size: 0 } }),
    requireNativeClient: async () => native,
    submit: async () => { state.submitted++; return { status: 'included', success: true, message: 'Included', events: [{ pallet: 'SubtensorModule', name: 'GammaMinted' }], data: { gamma_minted: 99n, submission_rejected: false, diagnostic: 'retained' } } },
  })
  const signer = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, sign: () => { state.signed++; return Buffer.alloc(64) } }
  return { group: new NodeMultisig(client, [alice.ss58Address, bob.ss58Address], 1), signer, state }
}

let browserModule
async function initializeBrowser() {
  if (!browserModule) {
    browserModule = await import('../dist/browser.mjs')
    browserModule.configureBrowserWasm(async () => {
      const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
      const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
      bindings.__wbg_set_wasm(instance.exports); instance.exports.__wbindgen_start?.()
      const module = { ...bindings }; delete module.default; return module
    })
    await browserModule.initBrowser()
  }
}
test.before(initializeBrowser)
async function browserFixture(t) {
  await initializeBrowser()
  const client = new browserModule.Client('local'), state = { submitted: 0, signed: 0, shieldEstimates: 0, plainEstimates: 0, shielded: undefined }
  t.after(() => client.close())
  client.prepareMultisig = async ({ call }) => ({ call, depositRao: 0n, availableFreeRao: 100000n, violations: [] })
  client.estimateShieldedFees = async () => { state.shieldEstimates++; return { innerFee: sdk.Balance.fromRao(1n), carrierFee: sdk.Balance.fromRao(1n) } }
  client.estimateFee = async () => { state.plainEstimates++; return sdk.Balance.fromRao(1n) }
  client.submit = async (_call, _signer, options) => { state.submitted++; state.shielded = options.mevShield; return { status: 'submitted', message: 'Submitted', events: [] } }
  const group = client.multisig([alice.ss58Address, bob.ss58Address], 1)
  const signer = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, sign: () => { state.signed++; return Buffer.alloc(64) } }
  return { group, signer, state }
}

const wrapped = [
  ['direct', registration],
  ['batch', descriptor('Utility', 'batch_all', { calls: [payment, registration] })],
  ['proxy', descriptor('Proxy', 'proxy', { real: bob.ss58Address, force_proxy_type: null, call: registration })],
  ['multisig', descriptor('Multisig', 'as_multi_threshold_1', { other_signatories: [bob.ss58Address], call: registration })],
  ['collateral', collateral],
]

// PROTECTED_REGRESSION: descriptor-based multisig operations must not expose mandatory-shielded registration or collateral calls; changes require explicit human approval.
test('Node multisig descriptors refuse shielding opt-outs and choose Shield by default before signing', async t => {
  for (const [name, call] of wrapped) await t.test(name, async t => {
    const f = nodeFixture(t)
    await assert.rejects(f.group.approve(call, f.signer, { mevShield: false }), /requires MEV shielding/)
    assert.equal(f.state.submitted, 0); assert.equal(f.state.signed, 0)
    await assert.rejects(f.group.approve(call, f.signer), /MevShield.NextKey is unavailable/)
    assert.equal(f.state.shieldKeyReads, 1); assert.ok(f.state.quotedCarriers > 0)
    assert.equal(f.state.submitted, 0); assert.equal(f.state.signed, 0)
  })
})

// PROTECTED_REGRESSION: browser descriptor wrappers must retain the same mandatory shielding as native workflows; changes require explicit human approval.
test('browser multisig descriptors refuse opt-outs and forward required shielding to submission', async t => {
  for (const [name, call] of wrapped) await t.test(name, async t => {
    const f = await browserFixture(t)
    await assert.rejects(f.group.approve(call, f.signer, { allowRawCall: true, mevShield: false }), /requires MEV shielding/)
    assert.equal(f.state.submitted, 0); assert.equal(f.state.signed, 0)
    await f.group.approve(call, f.signer, { allowRawCall: true })
    assert.equal(f.state.submitted, 1); assert.equal(f.state.shielded, true)
    assert.equal(f.state.shieldEstimates, 1); assert.equal(f.state.plainEstimates, 0)
  })
})

test('unshielded Node multisig keeps Gamma outcomes and submission diagnostics alongside coordinates', async t => {
  const f = nodeFixture(t), result = await f.group.approve(payment, f.signer, { mevShield: false })
  assert.equal(result.success, true)
  assert.equal(result.data.gamma_minted, 99n)
  assert.equal(result.data.submission_rejected, false)
  assert.equal(result.data.diagnostic, 'retained')
  assert.equal(result.events[0].name, 'GammaMinted')
  assert.equal(result.data.multisig_account, f.group.address)
  assert.match(result.data.multisig_call_hash, /^0x[0-9a-f]{64}$/)
  assert.equal(result.data.opening, false)
  assert.equal(f.state.submitted, 1)
})

test('shared descriptor shielding covers every mandatory call in the semantic catalog', () => {
  const { evaluatePolicy } = require('../dist/policy'), { INTENT_CATALOG } = require('../dist/intent-catalog')
  const required = INTENT_CATALOG.filter(intent => intent.mev_shield_required)
  assert.ok(required.length > 0)
  for (const intent of required) for (const [pallet, call] of intent.wraps) {
    assert.equal(evaluatePolicy('transaction.descriptor_shielding', { call: descriptor(pallet, call) }).required, true, `${intent.name}: ${pallet}.${call}`)
  }
})

test('ordinary browser multisig descriptors retain unshielded defaults', async t => {
  const f = await browserFixture(t)
  await f.group.approve(payment, f.signer, { allowRawCall: true })
  assert.equal(f.state.shielded, false)
  assert.equal(f.state.shieldEstimates, 0)
  assert.equal(f.state.plainEstimates, 1)
})

// PROTECTED_REGRESSION: opaque raw calls may replay only with explicit shielding when their nested requirements cannot be inspected; changes require explicit human approval.
test('opaque descriptors remain usable with explicit shielding and cannot silently opt out', async t => {
  const data = runtime.composeCall(registration.pallet, registration.call, registration.params)
  const call = descriptor('Proxy', 'proxy', { real: bob.ss58Address, force_proxy_type: null, call: data })
  await t.test('Node', async t => {
    const f = nodeFixture(t)
    for (const mevShield of [undefined, false]) await assert.rejects(f.group.approve(call, f.signer, { allowRawCall: true, mevShield }), /RuntimeCall|announced/)
    assert.equal(f.state.signed, 0); assert.equal(f.state.submitted, 0)
    await assert.rejects(f.group.approve(call, f.signer, { allowRawCall: true, mevShield: true }), /MevShield.NextKey is unavailable/)
    assert.equal(f.state.shieldKeyReads, 1); assert.ok(f.state.quotedCarriers > 0)
  })
  await t.test('browser', async t => {
    const f = await browserFixture(t)
    for (const mevShield of [undefined, false]) await assert.rejects(f.group.approve(call, f.signer, { allowRawCall: true, mevShield }), /RuntimeCall|announced/)
    assert.equal(f.state.signed, 0); assert.equal(f.state.submitted, 0)
    await f.group.approve(call, f.signer, { allowRawCall: true, mevShield: true })
    assert.equal(f.state.shielded, true); assert.equal(f.state.submitted, 1)
  })
})
