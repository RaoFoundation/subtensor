'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { Worker } = require('node:worker_threads')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { Intent, buildIntent, encodeIntentCall } = require('../dist/intents')
const { policyEvaluate } = require('../dist/policy')
const golden = require('../../python/tests/fixtures/golden.json')

const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const runtime = new sdk.Runtime(metadata, 419, 1, 42)
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const charlie = sdk.Keypair.fromUri('//Charlie')
const context = { coldkey: alice.ss58Address }
const compose = call => encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
const view = {
  query: async (_, name) => name === 'Account' ? { data: { free: 1_000_000_000_000n } } : null,
  decodeAddress: sdk.publicKeyFromSs58,
  multisigAccount: () => ({ address: charlie.ss58Address, publicKey: charlie.publicKey }),
  hashCall: async call => sdk.blake2_256(await compose(call)),
  estimateWeight: async () => ({ ref_time: 1000n, proof_size: 0n }),
  estimateFee: async () => 1000n,
  decodeCall: async data => runtime.decodeCall(data),
  hashCallData: sdk.blake2_256,
  specVersion: () => 419,
}

async function nativeClient(t) {
  // Native descriptor construction refreshes metadata synchronously. Keep the
  // fixture RPC on another thread so these real native calls cannot deadlock it.
  const server = new Worker(`
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
  t.after(() => server.terminate())
  const port = await new Promise((resolve, reject) => {
    server.once('message', resolve)
    server.once('error', reject)
  })
  return sdk.NativeChainClient.connect(`http://127.0.0.1:${port}`)
}

for (const operation of ['multisig_cancel', 'multisig_approve']) {
  test(`batch retains the verified semantic call for ${operation}`, async t => {
    const client = await nativeClient(t)
    const spec = {
      op: operation, threshold: 2, other_signatories: [bob.ss58Address],
      timepoint: { height: 1, index: 0 },
      call: { op: 'batch', intents: [{ op: 'transfer', dest_ss58: bob.ss58Address, amount_tao: '1.25' }] },
    }
    let intent
    if (operation === 'multisig_cancel') {
      const saved = await buildIntent(spec.call.op, spec.call).prepare(view, { coldkey: charlie.ss58Address })
      spec.call_hash = '0x' + Buffer.from(await view.hashCall(saved.call)).toString('hex')
      // Cancellation needs only its original commitment. Attach optional saved
      // semantics before flattening to retain this descriptor-preservation test.
      class SavedCancellation extends Intent {
        async prepare(reader, context) {
          const plan = await super.prepare(reader, context)
          return { ...plan, call: { ...plan.call, semanticCall: saved.call } }
        }
      }
      intent = new SavedCancellation(operation, spec)
    } else intent = buildIntent(operation, spec)
    const standalone = await intent.prepare(view, context)
    const original = sdk.IntentCall.fromDescriptor(client, standalone.call)
    assert.equal(original.function, operation === 'multisig_cancel' ? 'cancel_as_multi' : 'approve_as_multi')

    const batched = await buildIntent('batch', { intents: [operation === 'multisig_cancel' ? intent : spec] }).prepare(view, context)
    const native = sdk.IntentCall.fromDescriptor(client, batched.call)
    const expected = await compose(standalone.call)
    assert.deepEqual(Buffer.from(native.params.calls[0]), Buffer.from(expected))
    assert.deepEqual(batched.call.params.calls[0].semanticCall, standalone.call.semanticCall)

    const forged = structuredClone(batched.call)
    forged.params.calls[0].semanticCall.params.calls[0].params.value = 1n
    assert.throws(() => sdk.IntentCall.fromDescriptor(client, forged), /hash does not match its semantic call/)
  })
}

test('batch retains explicit proxy signer roles', async t => {
  const client = await nativeClient(t)
  const plan = await buildIntent('batch', { intents: [{ op: 'add_proxy', delegate_ss58: alice.ss58Address, proxy_type: 'Any', from_hotkey: true }] }).prepare(view, { ...context, hotkey: bob.ss58Address })
  assert.equal(plan.call.params.calls[0].signerRole, 'hotkey')
  assert.equal(sdk.IntentCall.fromDescriptor(client, plan.call).signerRole, 'hotkey')
})

test('batch retains saved call bytes inside wrappers and native validation still enforces them', async t => {
  const client = await nativeClient(t)
  const data = runtime.composeCall('Balances', 'transfer_keep_alive', { dest: { Id: bob.publicKey }, value: 9007199254740993n })
  const hex = value => '0x' + Buffer.from(value).toString('hex')
  const args = {
    delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address,
    inner_op: 'transfer', inner_args: { dest_ss58: bob.ss58Address, amount_tao: '1.25' },
    announced_call_data: hex(data), expected_call_hash: hex(sdk.blake2_256(data)), announced_spec_version: 419,
  }
  const plan = await buildIntent('batch', { intents: [{ op: 'execute_proxy_announced', ...args }] }).prepare(view, context)
  const retained = plan.call.params.calls[0].params.call
  assert.equal(retained.callData, hex(data))
  assert.equal(BigInt(retained.params.value), 9007199254740993n)
  const native = sdk.IntentCall.fromDescriptor(client, plan.call)
  assert.deepEqual(Buffer.from(native.params.calls[0]), Buffer.from(await compose(plan.call.params.calls[0])))
  const forged = structuredClone(plan.call)
  forged.params.calls[0].params.call.params.value = 1n
  assert.throws(() => sdk.IntentCall.fromDescriptor(client, forged), /saved call bytes do not match/)
})

test('native and WASM select the same original leaves and reject unsafe proxy batches', async () => {
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  const leaf = { pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: 'account', value: '9007199254740993' }, callData: '0x010203' }
  const call = { pallet: 'Multisig', call: 'cancel_as_multi', params: { call_hash: '0x0102' }, semanticCall: leaf }
  const batch = { pallet: 'Utility', call: 'batch_all', params: { calls: [call] } }
  const input = JSON.stringify({ calls: [batch, leaf] })
  for (const evaluate of [policyEvaluate, binding.policyEvaluate]) {
    assert.deepEqual(JSON.parse(evaluate('protected.flatten_batch_paths', input)), { version: 1, value: [[0, 0], [1]] })
    const unsafe = JSON.stringify({ calls: [{ pallet: 'Proxy', call: 'proxy', params: { call: batch } }] })
    assert.match(JSON.parse(evaluate('protected.flatten_batch_paths', unsafe)).error.message, /submit the proxy intent directly/)
  }
})
