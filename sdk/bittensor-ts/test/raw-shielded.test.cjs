const test = require('node:test')
const assert = require('node:assert/strict')
const { WorkflowClient } = require('../dist/node-workflows.js')
const { Keypair } = require('../dist/keys.js')
const { Client } = require('../dist/client.js')

function fixture(overrides = {}) {
  const address = Keypair.fromUri('//Alice').ss58Address
  const signed = [], submitted = [], staged = [], reservations = []
  let nonceFloor = 7
  const native = {
    reserveNonces: async (_address, count) => { reservations.push(count); const next = nonceFloor; nonceFloor += count; return next },
    invalidateNonce: async () => { nonceFloor = 7 },
  }
  const signer = {
    getAccount: async () => ({ address, publicKey: Keypair.fromUri('//Alice').publicKey, cryptoType: 1 }),
    beginTwoStageSigning: () => staged.push(true),
  }
  const client = Object.assign(new Client('local'), {
    requireNativeClient: async () => native,
    runtimeAt: async () => ({ ss58Format: 42 }),
    accountNextIndex: async () => 7,
    query: async (pallet, item) => { assert.equal(pallet + '.' + item, 'MevShield.NextKey'); return Buffer.alloc(1184, 0x42) },
    signExtrinsic: async (call, actualSigner, options) => {
      assert.equal((await actualSigner.getAccount({})).address, address)
      signed.push({ call, options })
      return { bytes: Buffer.from([signed.length, 2, 3]), hash: Buffer.alloc(32, signed.length) }
    },
    composeCall: async (pallet, call, params) => {
      assert.equal(pallet + '.' + call, 'MevShield.submit_encrypted')
      assert.ok(params.ciphertext.length > 1000 && params.ciphertext.length <= 8192)
      return Buffer.from([3, 4, 5])
    },
    submitSigned: async (extrinsic, actualAddress, options) => {
      assert.equal(actualAddress, address)
      submitted.push(options)
      return { status: 'submitted', message: 'Submitted', extrinsicHash: 'outer', events: [] }
    },
    ...overrides,
  })
  return { workflow: new WorkflowClient(client), client, signer, address, signed, submitted, staged, reservations }
}

test('raw shielding requires explicit admission and refuses policy claims opaque bytes cannot prove', async () => {
  const { workflow, signer, signed } = fixture({ runtimeAt: async () => { throw Error('must refuse before reading or signing') } })
  await assert.rejects(workflow.submitRawShielded(Buffer.from([1]), signer), /require allowRawCall/)
  for (const policy of [{ maxFeeRao: 1n }, { maxSpendRao: 1n }, { allowedNetuids: [1] }]) {
    await assert.rejects(workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true, policy }), /cannot prove/)
  }
  assert.equal(signed.length, 0)
})

test('raw shielding signs the inner and carrier with consecutive nonces, short eras and default inclusion', async () => {
  const { workflow, signer, signed, submitted, staged } = fixture()
  const result = await workflow.submitRawShielded(Buffer.from([1]), signer, { allowRawCall: true, period: 128, tip: 2n })
  assert.deepEqual(signed.map(value => value.options.nonce), [8, 7])
  assert.deepEqual(signed.map(value => value.options.period), [8, 8])
  assert.deepEqual(signed.map(value => value.options.tip), [2n, 2n])
  assert.equal(signed.some(value => Object.hasOwn(value.options, 'metadataHash')), false)
  assert.equal(submitted[0].waitForInclusion, true)
  assert.equal(staged.length, 1)
  assert.equal(result.innerExtrinsicHash, '0x' + '01'.repeat(32))
  assert.equal(result.data.carrier_only, true)
})

test('raw shielding preserves explicit submission mode, nonce and metadata hash', async () => {
  const { workflow, signer, signed, submitted } = fixture()
  await workflow.submitRawShielded(Buffer.from([1]), signer, { policy: { allowRawCalls: true }, nonce: 12, waitForInclusion: false, metadataHash: null })
  assert.deepEqual(signed.map(value => value.options.nonce), [13, 12])
  assert.deepEqual(signed.map(value => value.options.metadataHash), [null, null])
  assert.equal(submitted[0].waitForInclusion, false)
  assert.equal(Object.hasOwn(signed[1].options, 'policy'), false)
})

test('raw shielding rejects missing encryption keys and unsafe nonces before signing', async () => {
  const missing = fixture({ query: async () => null })
  await assert.rejects(missing.workflow.submitRawShielded(Buffer.from([1]), missing.signer, { allowRawCall: true }), /NextKey/)
  assert.equal(missing.signed.length, 0)
  assert.deepEqual(missing.reservations, [])
  for (const nonce of [-1, 1.5, Number.MAX_SAFE_INTEGER]) {
    const state = fixture()
    await assert.rejects(state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true, nonce }), /invalid shielded nonce/)
    assert.equal(state.signed.length, 0)
  }
})

test('raw shielding serializes two envelopes for the same account', async () => {
  const state = fixture(), order = []
  const sign = state.client.signExtrinsic
  state.client.signExtrinsic = async (...args) => {
    order.push(args[2].nonce)
    await new Promise(resolve => setTimeout(resolve, 2))
    return sign(...args)
  }
  await Promise.all([
    state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true, nonce: 4 }),
    state.workflow.submitRawShielded(Buffer.from([2]), state.signer, { allowRawCall: true, nonce: 6 }),
  ])
  assert.deepEqual(order, [5, 4, 7, 6])
})

test('implicit shielding allocates disjoint pairs above earlier detached reservations even before inclusion', async () => {
  const state = fixture()
  assert.equal(await state.client.reserveNonces(state.address, 1), 7)
  await Promise.all([
    state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true, waitForInclusion: false }),
    state.workflow.submitRawShielded(Buffer.from([2]), state.signer, { allowRawCall: true, waitForInclusion: false }),
  ])
  assert.deepEqual(state.reservations, [1, 2, 2])
  assert.deepEqual(state.signed.map(value => value.options.nonce), [9, 8, 11, 10])
  assert.equal(await state.client.reserveNonces(state.address, 1), 12)
})

test('uncertain delivery retains the shielded pair until explicit reconciled recovery', async () => {
  const state = fixture({ submitSigned: async () => { throw new Error('broadcast timed out; inclusion unknown') } })
  await assert.rejects(state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true }), /inclusion unknown/)
  assert.deepEqual(state.signed.map(value => value.options.nonce), [8, 7])
  assert.equal(await state.client.reserveNonces(state.address, 1), 9)
  await state.client.invalidateNonce(state.address)
  assert.equal(await state.client.reserveNonces(state.address, 1), 7)
})

test('raw shielding releases both nonces on signing failure and definite rejection', async () => {
  for (const failedAt of ['sign', 'reject', 'uncertain']) {
    const f = fixture(), native = await f.client.requireNativeClient(), releases = []
    native.releaseNonces = async (...args) => releases.push(args)
    if (failedAt === 'sign') f.client.signExtrinsic = async () => { throw new Error('user declined') }
    else f.client.submitSigned = async () => ({ status: failedAt === 'reject' ? 'failed' : 'unknown', success: false, extrinsicHash: 'outer', events: [], data: { [failedAt === 'reject' ? 'submission_rejected' : 'submission_uncertain']: true } })
    const result = f.workflow.submitRawShielded(Buffer.from([1]), f.signer, { allowRawCall: true })
    if (failedAt === 'sign') await assert.rejects(result, /user declined/); else await result
    assert.deepEqual(releases, failedAt === 'uncertain' ? [] : [[f.address,7,2]])
  }
})


test('Node shielding reuses the inner nonce only after canonical finalized expiry', async () => {
  for (const canonical of [false, true]) {
    const outer = { status: 'inBlock', success: true, message: 'Success', extrinsicHash: 'outer', blockNumber: 100, events: [] }
    const state = fixture({
      submitSigned: async () => outer,
      rpc: async (method, params) => method === 'chain_getHeader' ? { number: 108 } : method === 'chain_getFinalizedHead' ? 'final' : `block-${params[0]}`,
      findExtrinsic: async hash => hash === 'outer' && canonical ? outer : null,
    })
    const native = await state.client.requireNativeClient(), released = []
    native.releaseNonces = async (...args) => released.push(args)
    const result = await state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true })
    assert.equal(result.data.inner_expiry_finalized === true, canonical)
    assert.deepEqual(released, canonical ? [[state.address, 8, 1]] : [])
    await state.client.close()
  }
})


test('a receipt RPC error after carrier acceptance cannot release submitted nonces', async () => {
  const { JsonRpcError } = require('../dist/transport')
  const state = fixture({
    submitSigned: async () => ({ success: true, extrinsicHash: 'outer', blockNumber: 100, events: [] }),
    rpc: async () => { throw new JsonRpcError('read method unavailable', -32601) },
  })
  const native = await state.client.requireNativeClient(), released = []
  native.releaseNonces = async (...args) => released.push(args)
  await assert.rejects(state.workflow.submitRawShielded(Buffer.from([1]), state.signer, { allowRawCall: true }), /read method unavailable/)
  assert.deepEqual(released, [])
  await state.client.close()
})


test('Node multisig shielding releases the expired inner reservation', async () => {
  const sdk = require('../dist'), { NodeMultisig } = require('../dist/node-workflows'), { toWire } = require('../dist/wire')
  const golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const outer = { status: 'inBlock', success: true, message: 'Success', extrinsicHash: 'outer', blockNumber: 100, events: [] }
  const state = fixture({
    runtimeAt: async () => runtime, blockHash: async () => '0x' + '11'.repeat(32), blockNumber: async () => 100,
    constant: async () => 100n,
    query: async (_pallet, item) => item === 'NextKey' ? Buffer.alloc(1184, 0x42) : item === 'Multisigs' ? null : { data: { free: 1000000n } },
    composeCall: async (...args) => runtime.composeCall(...args.slice(0, 3)),
    submitSigned: async () => outer,
    rpc: async (method, params) => method === 'payment_queryInfo' ? { weight: { ref_time: 1, proof_size: 0 } } : method === 'chain_getHeader' ? { number: 108 } : method === 'chain_getFinalizedHead' ? 'final' : `block-${params[0]}`,
    findExtrinsic: async hash => hash === 'outer' ? outer : null,
  })
  const native = await state.client.requireNativeClient(), released = []
  native.accountNextIndex = async () => 7
  native.externalSigningPlan = async (data, _address, publicKey) => ({ runtime: runtime.handle, publicKey, callData: data, feeRao: 10n, txParams: { era: toWire('00'), nonce: 7n, tip: 0n } })
  native.externalSigningPlanForIntent = async () => ({ feeRao: 10n })
  native.releaseNonces = async (...args) => released.push(args)
  const multisig = new NodeMultisig(state.client, [state.address, Keypair.fromUri('//Bob').ss58Address], 2)
  const result = await multisig.approve({ pallet: 'System', call: 'remark', params: { remark: Buffer.from([1]) } }, state.signer, { allowRawCall: true, mevShield: true })
  assert.equal(result.data.inner_expiry_finalized, true)
  assert.deepEqual(state.signed.map(value => value.options.nonce), [8, 7])
  assert.deepEqual(released, [[state.address, 8, 1]])
  await state.client.close()
})
