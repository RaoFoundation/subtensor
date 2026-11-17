'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { followShieldedResult } = require('../dist/shielded-receipt')
const { ProtectedWalletClient } = require('../dist/protected-transactions')
const hash = number => '0x' + number.toString(16).padStart(64, '0')
const transfer = { pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: sdk.Keypair.fromUri('//Bob').ss58Address, value: 1n } }

function fixture(crypto = sdk.CRYPTO_HASHED) {
  const key = sdk.Keypair.fromSeed(Buffer.alloc(32, 45), crypto)
  const state = { generation: 2n, nonce: 3, head: 100, finalized: 100, disconnected: true, canonicalCarrier: true, encoded: [], submissions: [], recipient: 0n, sender: 100n }
  const runtime = {
    supportsMldsaProfile: () => true,
    composeCall: (pallet, call, params) => Buffer.from(JSON.stringify({ pallet, call, params }, (_, value) => typeof value === 'bigint' ? value.toString() : value)),
    signaturePayloadParts: params => ({ includedInExtrinsic: Buffer.from([Number(params.nonce)]), includedInSignedData: Buffer.alloc(32, 8) }),
    encodeSignedExtrinsic: (call, account, proof, crypto, params) => {
      const bytes = Buffer.concat([Buffer.from(proof), Buffer.from(call)])
      const signed = { bytes, hash: sdk.blake2_256(bytes) }
      state.encoded.push({ ...signed, nonce: Number(params.nonce), call: JSON.parse(Buffer.from(call).toString()) })
      return signed
    },
  }
  const chain = {
    rpc: async (method, args) => {
      if (state.disconnected) throw new Error('tracking connection lost')
      if (method === 'chain_getFinalizedHead') return hash(state.finalized)
      if (method === 'chain_getHeader') return { number: args.length ? Number(BigInt(args[0])) : state.head }
      if (method === 'chain_getBlockHash') return hash(args[0] === 100 && state.forkedCarrier ? 999 : args[0])
      throw new Error(`unexpected RPC ${method}`)
    },
    findExtrinsic: async (txHash, blockHash, finalized) => {
      if (state.canonicalCarrier && txHash === state.carrier.extrinsicHash && blockHash === state.carrier.blockHash) return { ...state.carrier, finalized, ...(state.finalizedCarrierFailure ? { success: false, status: 'failed' } : {}) }
      if (state.inner && txHash === state.inner.extrinsicHash && blockHash === state.inner.blockHash) return { ...state.inner, finalized }
      return null
    },
  }
  const adapter = {
    decodeAddress: sdk.publicKeyFromSs58, hash: sdk.blake2_256,
    finalizedHead: async () => hash(state.finalized), blockHash: async number => hash(number === 100 && state.forkedCarrier ? 999 : number ?? state.head),
    blockNumber: async block => Number(BigInt(block)), runtimeAt: async () => runtime,
    accountNextIndex: async () => state.nonce, eraBirth: () => 96,
    query: async (_pallet, item) => {
      if (item === 'HashedAccountsEnabled') return true
      if (item === 'Account') return { nonce: state.nonce, data: { free: state.sender } }
      if (item === 'Accounts') return { descriptor: key.authorizationDescriptor(), generation: state.generation, commitment: key.atGeneration(state.generation).authorizationCurrentCommitment() }
      if (item === 'NextKey') return Buffer.alloc(1184, 1)
      return null
    },
    rpc: chain.rpc, intentReader: async () => ({}), seal: () => Buffer.alloc(1234, 1), verify: sdk.verifyAuthorization,
    submitBytes: async bytes => {
      const signed = state.encoded.find(item => Buffer.from(item.bytes).equals(bytes))
      assert.equal(signed.nonce, state.nonce)
      state.submissions.push(signed)
      state.generation++; state.nonce++
      const result = { status: 'finalized', success: true, message: 'included', extrinsicHash: '0x' + Buffer.from(signed.hash).toString('hex'), blockHash: hash(100), blockNumber: 100, finalized: true, events: [] }
      if (signed.call.pallet === 'MevShield') state.carrier = result
      else { state.recipient += BigInt(signed.call.params.value); state.sender -= BigInt(signed.call.params.value) }
      return result
    },
    followShielded: (carrier, inner, period, options) => followShieldedResult(carrier, inner, period, chain, options),
    findExtrinsic: chain.findExtrinsic,
  }
  const owner = {}
  return { key, state, adapter, chain, owner, session: new ProtectedWalletClient(adapter, key, owner) }
}

async function interrupted(f) {
  let error
  await assert.rejects(f.session.submit(transfer, { allowRawCall: true, mevShield: true }), failure => {
    error = failure
    assert.equal(failure.submissionUncertain, true)
    return /tracking connection lost/.test(failure.message)
  })
  assert.equal(f.state.recipient, 0n)
  assert.equal(f.state.generation, 3n)
  return error
}

// PROTECTED_REGRESSION: finalized Shield expiry must recover a rotating wallet without replaying its expired payment; changes require explicit human approval.
test('rotating wallet reconciles interrupted Shield expiry and can spend again on the same client', async () => {
  const f = fixture(), error = await interrupted(f)
  f.state.disconnected = false; f.state.head = f.state.finalized = 108
  const verifiedElsewhere = await followShieldedResult(error.carrierReceipt, error.innerExtrinsicHash, 8, f.chain, { waitForFinalization: true })
  assert.equal(verifiedElsewhere.data.inner_expiry_finalized, true)
  error.carrierReceipt.blockNumber = 999
  error.carrierReceipt.extrinsicHash = hash(999)
  await assert.rejects(f.session.submit(transfer, { allowRawCall: true }), /not reconciled/)
  const recovered = await f.session.reconcileShielded()
  assert.equal(recovered.data.inner_expiry_finalized, true)
  assert.equal(recovered.innerExtrinsicHash, error.innerExtrinsicHash)
  assert.equal(recovered.carrierExtrinsicHash, error.carrierExtrinsicHash)
  await f.session.submit(transfer, { allowRawCall: true })
  assert.equal(f.state.recipient, 1n)
  assert.equal(f.state.sender, 99n)
  assert.deepEqual(f.state.submissions.map(row => row.nonce), [3, 4])
  await assert.rejects(f.session.reconcileShielded(), /no pending Shield/)
})

// PROTECTED_REGRESSION: a finalized failed carrier consumes only its own generation and must not strand the unused inner generation; changes require explicit human approval.
test('finalized carrier failure after interrupted tracking releases only the unused inner generation', async () => {
  const f = fixture()
  await interrupted(f)
  f.state.disconnected = false
  f.state.finalizedCarrierFailure = true
  const result = await f.session.reconcileShielded()
  assert.equal(result.finalized, true)
  assert.equal(result.success, false)
  assert.equal(result.data.carrier_only, true)
  await f.session.submit(transfer, { allowRawCall: true })
  assert.deepEqual(f.state.submissions.map(row => row.nonce), [3, 4])
  assert.equal(f.state.recipient, 1n)
})

test('Shield reconciliation bounds canonical-carrier reads by the request deadline', async () => {
  const f = fixture()
  await interrupted(f)
  f.adapter.findExtrinsic = async () => new Promise(() => {})
  await assert.rejects(f.session.reconcileShielded({ timeoutMs: 20 }), /timed out/)
  await assert.rejects(f.session.submit(transfer, { allowRawCall: true }), /not reconciled/)
  assert.equal(f.state.submissions.length, 1)
})

// PROTECTED_REGRESSION: incomplete, reorged, or mismatched Shield evidence must never release a live inner authorization; changes require explicit human approval.
test('Shield reconciliation preserves the generation reservation until exact canonical finalized evidence exists', async () => {
  for (const outcome of ['unfinalized', 'missing-carrier', 'reorg', 'wrong-carrier', 'wrong-hash', 'abort']) {
    const f = fixture(), error = await interrupted(f)
    f.state.disconnected = false; f.state.head = 108
    if (outcome !== 'unfinalized') f.state.finalized = 108
    if (outcome === 'missing-carrier') f.state.canonicalCarrier = false
    if (outcome === 'reorg') f.state.forkedCarrier = true
    if (outcome === 'wrong-carrier') f.adapter.findExtrinsic = async () => ({ ...f.state.carrier, extrinsicHash: hash(999), finalized: true })
    if (outcome === 'wrong-hash') f.adapter.followShielded = async () => ({ innerExtrinsicHash: hash(999), carrierExtrinsicHash: error.carrierExtrinsicHash, data: { inner_expiry_finalized: true, carrier_only: true } })
    const controller = new AbortController()
    if (outcome === 'abort') controller.abort(new Error('cancelled reconciliation'))
    if (outcome === 'missing-carrier' || outcome === 'reorg') assert.equal((await f.session.reconcileShielded()).data.submission_uncertain, true)
    else await assert.rejects(f.session.reconcileShielded({ timeoutMs: 20, signal: controller.signal }), /timed out|different|cancelled/)
    await assert.rejects(f.session.submit(transfer, { allowRawCall: true }), /not reconciled/)
    assert.equal(f.state.submissions.length, 1)
    assert.equal(f.state.recipient, 0n)
    assert.equal(f.state.generation, 3n)
  }
})

// PROTECTED_REGRESSION: finalized expiry must not retain a stale nonce floor after a fixed wallet was reaped; changes require explicit human approval.
test('fixed wallet reconciliation reads a fresh nonce after finalized expiry and account reaping', async () => {
  const f = fixture(sdk.CRYPTO_FALCON512)
  await interrupted(f)
  f.state.disconnected = false; f.state.head = f.state.finalized = 108
  f.state.nonce = 0
  assert.equal((await f.session.reconcileShielded()).data.inner_expiry_finalized, true)
  await f.session.submit(transfer, { allowRawCall: true })
  assert.deepEqual(f.state.submissions.map(row => row.nonce), [3, 0])
  assert.equal(f.state.recipient, 1n)
})

// PROTECTED_REGRESSION: recovered finalized inclusion must not clear a newer wallet reservation or duplicate the original payment; changes require explicit human approval.
test('a new session on the same client reconciles the original included Shield payment without rebroadcast', async () => {
  const f = fixture(), error = await interrupted(f)
  f.state.disconnected = false; f.state.head = f.state.finalized = 108
  f.state.inner = { status: 'finalized', success: true, message: 'payment included', extrinsicHash: error.innerExtrinsicHash, blockHash: hash(105), blockNumber: 105, events: [], finalized: true }
  f.state.nonce = 5; f.state.generation = 4n; f.state.recipient = 1n; f.state.sender = 99n
  const session = new ProtectedWalletClient(f.adapter, f.key, f.owner)
  const result = await session.reconcileShielded()
  assert.equal(result.extrinsicHash, error.innerExtrinsicHash)
  assert.equal(result.finalized, true)
  assert.equal(f.state.submissions.length, 1)
  await session.submit(transfer, { allowRawCall: true })
  assert.deepEqual(f.state.submissions.map(row => row.nonce), [3, 5])
  assert.equal(f.state.recipient, 2n)
  await assert.rejects(f.session.reconcileShielded(), /no pending Shield/)
})
