const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { Client } = require('../dist/client.js')
const { WorkflowClient, NodeMultisig } = require('../dist/node-workflows.js')
const { NativeChainClient } = require('../dist/transaction.js')
const { Keypair, ss58FromPublic } = require('../dist/keys.js')
const { toWire } = require('../dist/wire.js')
const golden = require('../../python/tests/fixtures/golden.json')

async function fixture(run, options = {}) {
  let chainNonce = 7
  const server = http.createServer(async (request, response) => {
    let body = ''; for await (const chunk of request) body += chunk
    const { id, method } = JSON.parse(body)
    if (method === 'system_accountNextIndex') await options.beforeNonceRpc?.()
    const values = {
      system_properties: { ss58Format: 42 },
      state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 },
      state_call: golden.metadata.v15_hex,
      chain_getBlockHash: '0x' + '11'.repeat(32),
      system_accountNextIndex: chainNonce,
    }
    assert.ok(Object.hasOwn(values, method), `unexpected native RPC: ${method}`)
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ jsonrpc: '2.0', id, result: values[method] }))
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  let client
  try {
    const native = await NativeChainClient.connect(`http://127.0.0.1:${server.address().port}`)
    const keypair = Keypair.fromUri('//Alice'), address = keypair.ss58Address
    const signer = { getAccount: async () => ({ address, publicKey: keypair.publicKey, cryptoType: 1 }) }
    const innerHash = '0x' + '01'.repeat(32), signedNonces = []
    const outer = { status: 'inBlock', success: true, message: 'Success', extrinsicHash: 'outer', blockHash: 'block-100', blockNumber: 100, events: [] }
    client = Object.assign(new Client('local'), {
      requireNativeClient: async () => native,
      runtimeAt: async () => ({ ss58Format: 42 }),
      blockHash: async () => 'block-100',
      query: async () => Buffer.alloc(1184, 0x42),
      signExtrinsic: async (_call, _signer, submission) => {
        signedNonces.push(submission.nonce)
        return { bytes: Buffer.from([signedNonces.length, 2, 3]), hash: Buffer.alloc(32, signedNonces.length) }
      },
      composeCall: async () => Buffer.from([3, 4, 5]),
      submitSigned: async () => {
        if (options.afterCarrier) await options.afterCarrier(native, address)
        chainNonce = 0 // The inner transfer reaps the sender; it has since been funded again.
        return options.carrierOnly ? { ...outer, status: 'submitted', success: undefined, blockHash: undefined, blockNumber: undefined } : outer
      },
      rpc: async (method, params) => {
        if (options.uncertain) throw Error('inner receipt unavailable')
        return method === 'chain_getHeader' ? { number: 100 } : `block-${params[0]}`
      },
      findExtrinsic: async (hash, blockHash) => {
        assert.equal(hash, innerHash)
        if (options.beforeInner) await options.beforeInner(native, address)
        return { status: options.dispatchFailed ? 'failed' : 'inBlock', success: !options.dispatchFailed, message: 'Inner included', extrinsicHash: hash, blockHash, blockNumber: 100, events: [] }
      },
    })
    await run({ native, client, workflow: new WorkflowClient(client), signer, address, signedNonces })
  } finally {
    if (client) await client.close()
    server.closeAllConnections()
    await new Promise(resolve => server.close(resolve))
  }
}

// PROTECTED_REGRESSION: a reaped Shield sender must transact again using its reset chain nonce; changes require explicit human approval.
test('Node Shield retires the included inner nonce after account reaping', async () => {
  for (const dispatchFailed of [false, true]) {
    await fixture(async ({ native, workflow, signer, address, signedNonces }) => {
      const result = await workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true })
      assert.deepEqual(signedNonces, [8, 7])
      assert.equal(result.data.carrier_only, false)
      assert.equal(result.success, !dispatchFailed)
      assert.equal(await native.accountNextIndex(address), 0)
      assert.equal(await native.reserveNonces(address, 1), 0)
    }, { dispatchFailed })
  }
})

test('Node Shield inclusion preserves later detached reservations', async () => {
  await fixture(async ({ native, workflow, signer, address }) => {
    await workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true })
    assert.equal(await native.reserveNonces(address, 1), 10)
  }, { afterCarrier: async (native, address) => assert.equal(await native.reserveNonces(address, 1), 9) })
})

test('Node Shield does not retire an uncertain or carrier-only inner nonce', async () => {
  for (const uncertain of [false, true]) {
    await fixture(async ({ native, workflow, signer, address }) => {
      const pending = workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true })
      if (uncertain) await assert.rejects(pending, /inner receipt unavailable/)
      else assert.equal((await pending).data.carrier_only, true)
      assert.equal(await native.reserveNonces(address, 1), 9)
    }, { uncertain, carrierOnly: !uncertain })
  }
})

test('a delayed Shield receipt cannot retire reservations from a new account epoch', async () => {
  await fixture(async ({ native, workflow, signer, address }) => {
    await workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true })
    assert.equal(await native.reserveNonces(address, 1), 1)
  }, { beforeInner: async (native, address) => {
    await native.invalidateNonce(address)
    assert.equal(await native.reserveNonces(address, 1), 0)
  } })
})

test('native inclusion observations use AccountId identity and tolerate duplicate receipts', async () => {
  await fixture(async ({ native, address }) => {
    const alternate = ss58FromPublic(Keypair.fromUri('//Alice').publicKey, 0)
    assert.equal(await native.reserveNonces(address, 2), 7)
    const observed = await native.observeNonce(alternate, 8)
    assert.ok(observed)
    await native.invalidateNonce(address)
    assert.equal(await native.reserveNonces(address, 1), 7)
    await observed.included()
    await observed.included()
    assert.equal(await native.reserveNonces(address, 1), 8)
  })
})

// PROTECTED_REGRESSION: shielded multisig signers must recover their reset nonce after reaping; changes require explicit human approval.
test('Node multisig Shield retires the included inner reservation', async () => {
  await fixture(async ({ native, client, signer, address, signedNonces }) => {
    const runtime = native.runtime(), receiptRpc = client.rpc
    Object.assign(client, {
      runtimeAt: async () => runtime,
      blockNumber: async () => 100,
      constant: async () => 100n,
      query: async (_pallet, item) => item === 'NextKey' ? Buffer.alloc(1184, 0x42) : item === 'Multisigs' ? null : { data: { free: 1000000n } },
      composeCall: async (...args) => runtime.composeCall(...args.slice(0, 3)),
      rpc: async (method, params) => method === 'payment_queryInfo' ? { weight: { ref_time: 1, proof_size: 0 } } : receiptRpc(method, params),
    })
    // Keep signing/fee data deterministic; reserve/observe/include all use the real Rust client.
    native.externalSigningPlan = async (data, _address, publicKey) => ({ runtime: runtime.handle, publicKey, callData: data, feeRao: 10n, txParams: { era: toWire('00'), nonce: 7n, tip: 0n } })
    native.externalSigningPlanForIntent = async () => ({ feeRao: 10n })
    const multisig = new NodeMultisig(client, [address, Keypair.fromUri('//Bob').ss58Address], 2)
    const result = await multisig.approve({ pallet: 'System', call: 'remark', params: { remark: Buffer.from([1]) } }, signer, { allowRawCall: true, mevShield: true })
    assert.deepEqual(signedNonces, [8, 7])
    assert.equal(result.data.carrier_only, false)
    assert.equal(await native.reserveNonces(address, 1), 0)
  })
})

test('native nonce observations allow RPC progress during a concurrent reservation', { timeout: 5000 }, async () => {
  const options = {}
  await fixture(async ({ native, address }) => {
    assert.equal(await native.reserveNonces(address, 2), 7)
    const observed = await native.observeNonce(address, 8)
    let started, release
    const entered = new Promise(resolve => { started = resolve })
    const resume = new Promise(resolve => { release = resolve })
    options.beforeNonceRpc = async () => { started(); await resume }
    const pending = native.reserveNonces(address, 1)
    await entered
    const captured = native.observeNonce(address, 9)
    const retired = observed.included()
    release()
    assert.equal(await pending, 9)
    assert.ok(await captured)
    await retired
    assert.equal(await native.reserveNonces(address, 1), 10)
  }, options)
})
