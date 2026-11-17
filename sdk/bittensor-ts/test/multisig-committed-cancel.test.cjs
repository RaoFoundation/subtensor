'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { createHash } = require('node:crypto')
const { buildIntent } = require('../dist/intents')

const timepoint = { height: 42, index: 3 }
const originalIntent = { op: 'add_stake', netuid: 1, hotkey_ss58: 'hotkey', amount_tao: '1' }
const hash = call => createHash('sha256').update(JSON.stringify(call, (_, value) => typeof value === 'bigint' ? value.toString() : value)).digest()
const hex = value => '0x' + Buffer.from(value).toString('hex')
function fixture() {
  const state = { price: 2_000_000_000n, entries: [], innerReads: 0 }
  const reader = {
    query: async () => ({ data: { free: 100_000_000_000n } }),
    runtimeCall: async (_, method) => { assert.equal(method, 'current_alpha_price'); state.innerReads++; return state.price },
    queryMap: async (pallet, item, params) => {
      assert.deepEqual([pallet, item, params], ['Multisig', 'Multisigs', ['multisig']])
      return state.entries
    },
    decodeAddress: address => new Uint8Array(32).fill(address === 'alice' ? 1 : 2),
    multisigAccount: () => ({ address: 'multisig', publicKey: new Uint8Array(32) }),
    hashCall: async call => hash(call),
  }
  return { state, reader }
}

// PROTECTED_REGRESSION: price changes must not prevent cancelling an approval and releasing its deposit; changes require explicit human approval.
test('multisig cancellation targets the committed hash after the subnet price changes', async () => {
  const { state, reader } = fixture()
  const approved = await buildIntent('add_stake', originalIntent).prepare(reader, { coldkey: 'multisig' })
  const committedHash = hash(approved.call)
  state.entries = [[[committedHash], { when: timepoint, deposit: 1000n, depositor: 'alice' }]]
  state.price *= 2n
  const repriced = await buildIntent('add_stake', originalIntent).prepare(reader, { coldkey: 'multisig' })
  assert.notEqual(hex(hash(repriced.call)), hex(committedHash))
  state.innerReads = 0

  const canceled = await buildIntent('multisig_cancel', { threshold: 2, other_signatories: ['bob'], call: originalIntent, timepoint }).prepare(reader, { coldkey: 'alice' })
  assert.equal(hex(canceled.call.params.call_hash), hex(committedHash))
  assert.deepEqual(canceled.call.params.timepoint, timepoint)
  assert.equal(state.innerReads, 0)
  assert.equal(canceled.call.semanticCall, undefined)
  assert.equal(canceled.global, true)
  assert.equal(canceled.spendRao, null)
  assert.deepEqual(canceled.spendByAccount, [])
})

test('an explicit original hash cancels without rebuilding or querying the original call', async () => {
  const { reader, state } = fixture()
  const committedHash = new Uint8Array(32).fill(17)
  reader.queryMap = async () => { throw new Error('unexpected pending lookup') }
  reader.hashCall = async () => { throw new Error('unexpected call hashing') }
  for (const account_mode of ['legacy', 'safe']) {
    const plan = await buildIntent('multisig_cancel', { threshold: 2, other_signatories: ['bob'], timepoint, call_hash: hex(committedHash), account_mode }).prepare(reader, { coldkey: 'alice' })
    assert.equal(hex(plan.call.params.call_hash), hex(committedHash))
    assert.equal(plan.call.call, account_mode === 'safe' ? 'cancel_as_multi_safe' : 'cancel_as_multi')
    assert.equal(plan.extras.multisig_call_hash, hex(committedHash))
  }
  assert.equal(state.innerReads, 0)
})

test('missing and ambiguous pending commitments fail instead of calculating a new hash', async () => {
  const { reader, state } = fixture()
  const args = { threshold: 2, other_signatories: ['bob'], call: originalIntent, timepoint }
  await assert.rejects(buildIntent('multisig_cancel', args).prepare(reader, { coldkey: 'alice' }), /no pending multisig operation/)
  state.entries = [17, 18].map(byte => [new Uint8Array(32).fill(byte), { when: timepoint }])
  await assert.rejects(buildIntent('multisig_cancel', args).prepare(reader, { coldkey: 'alice' }), /multiple pending operations/)
  assert.equal(state.innerReads, 0)
})

test('native cancellation keeps fee and global policy while needing no inner spending authority', () => {
  const { IntentCall, Policy } = require('../dist/transaction')
  const params = { threshold: 2, other_signatories: ['bob'], timepoint, call_hash: new Uint8Array(32).fill(17) }
  for (const suffix of ['', '_safe']) {
    const call = IntentCall.operation('Multisig.cancel_as_multi' + suffix, params)
    assert.deepEqual(new Policy({ maxSpendRao: 0n }).check(call), [])
    assert.match(new Policy({ maxFeeRao: 10n }).check(call, 11n).join(), /fee/)
    assert.match(new Policy({ allowedNetuids: [1] }).check(call).join(), /global/)
    assert.deepEqual(new Policy({ allowedNetuids: [1], allowGlobal: true }).check(call), [])
  }
  assert.throws(() => IntentCall.operation('Multisig.cancel_as_multi', { ...params, call_hash: new Uint8Array(1) }), /32 bytes/)
  assert.throws(() => IntentCall.operation('Multisig.approve_as_multi', { ...params, maybe_timepoint: timepoint, max_weight: { ref_time: 1n, proof_size: 0n } }), /unknown trusted operation/)
})

test('native standalone and batched cancellation accept the original hash without semantic call data', async t => {
  const sdk = require('../dist')
  const { Worker } = require('node:worker_threads')
  const golden = require('../../python/tests/fixtures/golden.json')
  const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
  const runtime = new sdk.Runtime(metadata, 419, 1, 42)
  const worker = new Worker(`
    const http = require('node:http')
    const { parentPort, workerData } = require('node:worker_threads')
    const server = http.createServer(async (request, response) => {
      let body = ''; for await (const chunk of request) body += chunk
      const { id, method } = JSON.parse(body)
      const values = {
        system_properties: { ss58Format: 42 },
        state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 },
        state_call: workerData,
        chain_getBlockHash: '0x' + '11'.repeat(32),
      }
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ jsonrpc: '2.0', id, result: values[method] }))
    })
    server.listen(0, '127.0.0.1', () => parentPort.postMessage(server.address().port))
  `, { eval: true, workerData: golden.metadata.v15_hex })
  t.after(() => worker.terminate())
  const port = await new Promise((resolve, reject) => { worker.once('message', resolve); worker.once('error', reject) })
  const client = await sdk.NativeChainClient.connect(`http://127.0.0.1:${port}`)
  const params = { threshold: 2, other_signatories: [sdk.Keypair.fromUri('//Bob').ss58Address], timepoint, call_hash: new Uint8Array(32).fill(17) }
  const descriptor = { pallet: 'Multisig', call: 'cancel_as_multi', params }
  const standalone = sdk.IntentCall.fromDescriptor(client, descriptor)
  assert.equal(standalone.function, 'cancel_as_multi')
  const batched = sdk.IntentCall.fromDescriptor(client, { pallet: 'Utility', call: 'batch_all', params: { calls: [descriptor] } })
  assert.deepEqual(Buffer.from(batched.params.calls[0]), Buffer.from(runtime.composeCall('Multisig', 'cancel_as_multi', params)))
  const { timepoint: _, ...approval } = params
  assert.throws(() => sdk.IntentCall.fromDescriptor(client, { pallet: 'Multisig', call: 'approve_as_multi', params: { ...approval, maybe_timepoint: timepoint, max_weight: { ref_time: 1n, proof_size: 0n } } }), /missing field semanticCall/)
})

test('native and browser WASM select the same original commitment and reject ambiguity', async () => {
  const fs = require('node:fs/promises')
  const path = require('node:path')
  const { pathToFileURL } = require('node:url')
  const { policyEvaluate } = require('../dist/policy')
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  const committed = '0x' + '11'.repeat(32), other = '0x' + '22'.repeat(32)
  const pending = [{ call_hash: other, timepoint: { height: 41, index: 3 } }, { call_hash: committed, timepoint }]
  for (const evaluate of [policyEvaluate, binding.policyEvaluate]) {
    assert.deepEqual(JSON.parse(evaluate('transaction.multisig_cancel', JSON.stringify({ timepoint, pending }))), { version: 1, value: { call_hash: committed, timepoint } })
    assert.match(JSON.parse(evaluate('transaction.multisig_cancel', JSON.stringify({ timepoint, pending: [...pending, { call_hash: other, timepoint }] }))).error.message, /multiple pending operations/)
  }
})
