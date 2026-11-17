const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { Client, SubmissionUncertainError } = require('../dist/client')
const { NativeChainClient } = require('../dist/transaction')
const { Keypair } = require('../dist/keys')
const { blake2_256 } = require('../dist/crypto')
const golden = require('../../python/tests/fixtures/golden.json')
const genesis = '0x' + '11'.repeat(32)
const planOptions = { allowRawCall: true, period: null, metadataHash: null }

async function fixture(t) {
  let received, release
  const handoff = new Promise(resolve => { received = resolve })
  const held = new Promise(resolve => { release = resolve })
  const hashes = []
  const server = http.createServer(async (request, response) => {
    let body = ''; for await (const part of request) body += part
    const { id, method, params } = JSON.parse(body)
    const values = {
      system_properties: { ss58Format: 42 },
      state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 },
      state_call: golden.metadata.v15_hex, chain_getBlockHash: genesis,
      chain_getHeader: { number: '0x0' }, system_accountNextIndex: 7,
      payment_queryInfo: { partialFee: '10' },
    }
    let result
    if (method === 'author_submitExtrinsic') {
      result = '0x' + blake2_256(Buffer.from(params[0].slice(2), 'hex')).toString('hex')
      hashes.push(result); received(); await held
    } else { assert.ok(Object.hasOwn(values, method), method); result = values[method] }
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ jsonrpc: '2.0', id, result }))
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const client = new Client('local')
  t.after(async () => { release(); await client.close(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)) })
  const native = await NativeChainClient.connect(`http://127.0.0.1:${server.address().port}`)
  const runtime = native.runtime(), key = Keypair.fromUri('//Alice')
  Object.assign(client, { requireNativeClient: async () => native, runtimeAt: async () => runtime, genesisHash: async () => genesis })
  const call = runtime.composeCall('Balances', 'transfer_keep_alive', { dest: Keypair.fromUri('//Bob').ss58Address, value: 1000000000n })
  return { client, native, key, call, hashes, handoff, release }
}

// PROTECTED_REGRESSION: aborting after detached broadcast must retain the exact reconciliation hash and nonce to prevent a duplicate payment; changes require explicit human approval.
test('aborted detached broadcast exposes its signed hash and retains the consumed nonce', async t => {
  const f = await fixture(t), controller = new AbortController()
  const prepared = await f.client.prepareCall(f.call, f.key, planOptions)
  const signature = f.key.sign(prepared.payload)
  const submit = f.native.submitExternal.bind(f.native)
  let nativeResult
  f.native.submitExternal = (...args) => { nativeResult = submit(...args); return nativeResult }
  const pending = f.client.submitSignature(prepared, signature, { signal: controller.signal })
  const rejected = assert.rejects(pending, error => {
    assert.ok(error instanceof SubmissionUncertainError)
    assert.equal(error.extrinsicHash, f.hashes[0])
    assert.equal(error.submissionUncertain, true)
    assert.equal(error.cause.name, 'RequestAbortedError')
    return true
  })
  await f.handoff; controller.abort(); await rejected
  f.release(); await nativeResult
  await assert.rejects(f.client.cancelPreparedCall(prepared), /no longer usable/)
  const next = await f.client.prepareCall(f.call, f.key, planOptions)
  assert.equal(Number(next.txParams.nonce), 8)
  assert.equal(f.hashes.length, 1)
  await f.client.cancelPreparedCall(next)
})

// PROTECTED_REGRESSION: rejected signatures and cancellation before handoff must not broadcast or consume a detached plan; changes require explicit human approval.
test('detached failures before handoff remain definite and leave the plan cancellable', async t => {
  const f = await fixture(t), prepared = await f.client.prepareCall(f.call, f.key, planOptions)
  await assert.rejects(f.client.submitSignature(prepared, Buffer.alloc(64)), error => !(error instanceof SubmissionUncertainError))
  const assemble = f.native.assembleExternal.bind(f.native)
  const controller = new AbortController()
  f.native.assembleExternal = async (...args) => { const signed = await assemble(...args); controller.abort(); return signed }
  await assert.rejects(f.client.submitSignature(prepared, f.key.sign(prepared.payload), { signal: controller.signal }), error => !(error instanceof SubmissionUncertainError))
  assert.deepEqual(f.hashes, [])
  await f.client.cancelPreparedCall(prepared)
  const next = await f.client.prepareCall(f.call, f.key, planOptions)
  assert.equal(Number(next.txParams.nonce), 7)
  await f.client.cancelPreparedCall(next)
})

// PROTECTED_REGRESSION: native reply and receipt-decoding failures after detached broadcast must retain the payment hash for reconciliation; changes require explicit human approval.
test('post-handoff native and receipt errors preserve the signed transaction hash', async t => {
  for (const failure of ['native reply', 'receipt decoding']) {
    const f = await fixture(t), prepared = await f.client.prepareCall(f.call, f.key, planOptions)
    const submit = f.native.submitExternal.bind(f.native)
    f.native.submitExternal = async (...args) => {
      const result = await submit(...args)
      if (failure === 'native reply') throw new Error('native reply lost')
      return { ...result, events: [{ module_id: 'SubtensorModule', event_id: 'GammaMinted' }] }
    }
    f.release()
    await assert.rejects(f.client.submitSignature(prepared, f.key.sign(prepared.payload), { waitForInclusion: false }), error => {
      assert.ok(error instanceof SubmissionUncertainError)
      assert.equal(error.extrinsicHash, f.hashes[0])
      assert.match(error.cause.message, /native reply lost|invalid Gamma event attributes/)
      return true
    })
    await assert.rejects(f.client.cancelPreparedCall(prepared), /no longer usable/)
    assert.equal(f.hashes.length, 1)
  }
})
