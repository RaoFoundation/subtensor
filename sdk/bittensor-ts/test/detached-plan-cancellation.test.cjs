const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { Client } = require('../dist/client')
const { NativeChainClient } = require('../dist/transaction')
const { Keypair } = require('../dist/keys')
const { blake2_256 } = require('../dist/crypto')
const { runSignerQueue } = require('../dist/signer-queue')
const golden = require('../../python/tests/fixtures/golden.json')
const genesis = '0x' + '11'.repeat(32)
const options = { allowRawCall: true, period: null, metadataHash: null }

async function fixture(run, { uncertain = false } = {}) {
  let runtime, chainNonce = 7
  const submitted = [], requests = []
  const server = http.createServer(async (request, response) => {
    let body = ''; for await (const chunk of request) body += chunk
    const { id, method, params } = JSON.parse(body)
    requests.push(method)
    const values = {
      system_properties: { ss58Format: 42 },
      state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 },
      state_call: golden.metadata.v15_hex,
      chain_getBlockHash: genesis,
      chain_getHeader: { number: '0x0' },
      system_accountNextIndex: chainNonce,
      payment_queryInfo: { partialFee: '10' },
    }
    let result
    if (method === 'author_submitExtrinsic') {
      const bytes = Buffer.from(params[0].slice(2), 'hex'), decoded = runtime.decodeExtrinsic(bytes)
      submitted.push(decoded.nonce)
      if (uncertain) { response.writeHead(503); response.end('{}'); return }
      assert.equal(decoded.nonce, chainNonce, 'automatic submission must fill the cancelled nonce')
      chainNonce++
      result = '0x' + blake2_256(bytes).toString('hex')
    } else {
      assert.ok(Object.hasOwn(values, method), `unexpected native RPC ${method}`)
      result = values[method]
    }
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ jsonrpc: '2.0', id, result }))
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const client = new Client('local'), other = new Client('local')
  try {
    const native = await NativeChainClient.connect(`http://127.0.0.1:${server.address().port}`)
    runtime = native.runtime()
    Object.assign(client, { requireNativeClient: async () => native, runtimeAt: async () => runtime, genesisHash: async () => genesis })
    const key = Keypair.fromUri('//Alice'), call = runtime.composeCall('System', 'remark', { remark: Buffer.from('detached cancellation') })
    await run({ client, other, native, key, call, submitted, requests })
  } finally {
    await client.close(); await other.close()
    server.closeAllConnections()
    await new Promise(resolve => server.close(resolve))
  }
}

// PROTECTED_REGRESSION: abandoning an unused detached plan must unblock automatic submissions without making the old plan reusable; changes require explicit human approval.
test('cancelPreparedCall releases a gap and the next real automatic submission fills it', async () => {
  await fixture(async ({ client, other, key, call, submitted, requests }) => {
    const abandoned = await client.prepareCall(call, key, options)
    assert.equal(Number(abandoned.txParams.nonce), 7)
    const later = await client.prepareCall(call, key, options)
    assert.equal(Number(later.txParams.nonce), 8)
    await assert.rejects(other.cancelPreparedCall(abandoned), /prepared by this client/)
    const requestCount = requests.length
    await client.cancelPreparedCall(abandoned)
    assert.equal(requests.length, requestCount, 'cancellation is local and never broadcasts')
    await assert.rejects(client.cancelPreparedCall(abandoned), /prepared by this client/)
    assert.throws(() => client.exportPreparedCall(abandoned), /prepared or imported/)
    await assert.rejects(client.submitSignature(abandoned, key.sign(abandoned.payload)), /prepared by this client/)
    const result = await client.submit(call, key, { ...options, waitForInclusion: false, waitForFinalization: false })
    assert.equal(result.status, 'submitted')
    assert.deepEqual(submitted, [7])
    await client.cancelPreparedCall(later)
    const next = await client.prepareCall(call, key, options)
    assert.equal(Number(next.txParams.nonce), 8)
    await client.cancelPreparedCall(next)
  })
})

// PROTECTED_REGRESSION: cancelling an explicit nonce must not release another detached plan's reservation at the same nonce; changes require explicit human approval.
test('explicit-nonce cancellation invalidates only that plan and rejects foreign/imported plans', async () => {
  await fixture(async ({ client, other, key, call }) => {
    const automatic = await client.prepareCall(call, key, options)
    const explicit = await client.prepareCall(call, key, { ...options, nonce: 7 })
    await assert.rejects(other.cancelPreparedCall(explicit), /prepared by this client/)
    await client.cancelPreparedCall(explicit)
    await assert.rejects(client.submitSignature(explicit, key.sign(explicit.payload)), /prepared by this client/)
    const next = await client.prepareCall(call, key, options)
    assert.equal(Number(next.txParams.nonce), 8)
    const imported = await client.importPreparedCall(client.exportPreparedCall(automatic), { allowRawCall: true })
    await assert.rejects(client.cancelPreparedCall(imported), /prepared by this client/)
    await client.cancelPreparedCall(automatic)
    await client.cancelPreparedCall(next)
  })
})

// PROTECTED_REGRESSION: cancelling an uncertain submission must not allow a conflicting transaction to reuse its reserved nonce; changes require explicit human approval.
test('cancelPreparedCall refuses a submitted plan when delivery is uncertain', async () => {
  await fixture(async ({ client, key, call, submitted }) => {
    const prepared = await client.prepareCall(call, key, options)
    const result = await client.submitSignature(prepared, key.sign(prepared.payload), { waitForInclusion: false, waitForFinalization: false })
    assert.equal(result.status, 'unknown')
    assert.equal(result.data.submission_uncertain, true)
    await assert.rejects(client.cancelPreparedCall(prepared), /no longer usable/)
    const next = await client.prepareCall(call, key, options)
    assert.equal(Number(next.txParams.nonce), 8)
    assert.deepEqual(submitted, [7])
    await client.cancelPreparedCall(next)
  }, { uncertain: true })
})

// PROTECTED_REGRESSION: a signature queued before cancellation completes must not reuse a released nonce; changes require explicit human approval.
test('cancellation invalidates a previously captured submitSignature queue entry', async () => {
  await fixture(async ({ client, key, call, submitted }) => {
    const prepared = await client.prepareCall(call, key, options)
    let release, entered
    const waiting = new Promise(resolve => { entered = resolve })
    const held = runSignerQueue(client, key.ss58Address, async () => { entered(); await new Promise(resolve => { release = resolve }) })
    await waiting
    const cancelling = client.cancelPreparedCall(prepared)
    const submitting = client.submitSignature(prepared, key.sign(prepared.payload), { waitForInclusion: false, waitForFinalization: false })
    const rejected = assert.rejects(submitting, /no longer usable/)
    release()
    await Promise.all([held, cancelling, rejected])
    assert.deepEqual(submitted, [])
    const next = await client.prepareCall(call, key, options)
    assert.equal(Number(next.txParams.nonce), 7)
    await client.cancelPreparedCall(next)
  })
})
