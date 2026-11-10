'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { encodeIntentCall } = require('../dist/intents')
const { estimateWalletRegistrationFee, snapshotWalletIdentity } = require('../dist/wallet-registration')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)

// The committed metadata predates HashedAccounts. Its real RuntimeCall enum and
// Utility.batch_all codec still reproduce the nested registration encoding bug.
// The live release test exercises the actual deposit guard and register leaves.
function fixture() {
  const payer = sdk.Keypair.fromUri('//Alice')
  const leaves = ['approved reserve guard', 'register the selected account'].map(text => ({ pallet: 'System', call: 'remark', params: { remark: Buffer.from(text) } }))
  const call = { pallet: 'Utility', call: 'batch_all', params: { calls: leaves } }
  const expected = runtime.composeCall('Utility', 'batch_all', { calls: leaves.map(leaf => runtime.composeCall(leaf.pallet, leaf.call, leaf.params)) })
  return { payer, call, expected }
}

function assertAtomicChildren(bytes) {
  const decoded = runtime.decodeCall(bytes)
  assert.equal(decoded.call_module, 'Utility')
  assert.equal(decoded.call_function, 'batch_all')
  const children = decoded.call_args.find(arg => arg.name === 'calls').value
  assert.equal(children.length, 2)
  assert.deepEqual(children.map(child => [child.call_module, child.call_function, child.call_args[0].value]), [
    ['System', 'remark', 'approved reserve guard'],
    ['System', 'remark', 'register the selected account'],
  ])
}

test('registration fee quotation recursively composes a real atomic RuntimeCall before querying fees', async () => {
  const { payer, call, expected } = fixture(), quoted = []
  const fee = await estimateWalletRegistrationFee({
    runtimeAt: async block => { assert.equal(block, 'quoted'); return runtime },
    blockNumber: async block => { assert.equal(block, 'quoted'); return 10 },
    rpc: async (method, [bytes, block]) => {
      assert.equal(method, 'payment_queryInfo'); assert.equal(block, 'quoted')
      quoted.push(runtime.decodeExtrinsic(Buffer.from(bytes.slice(2), 'hex')))
      return { partialFee: '17' }
    },
  }, call, payer, 'quoted', sdk.publicKeyFromSs58)
  assert.equal(fee, 34n)
  assert.equal(quoted.length, 2)
  assert.equal(quoted[0].call.call_hash, runtime.decodeCall(expected).call_hash)
  assert.equal(quoted[0].call.call_args[0].value.length, 2)
  assert.equal(quoted[1].call.call_module, 'MevShield')
  assert.equal(quoted[1].call.call_function, 'submit_encrypted')
  const encoded = await encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
  assert.deepEqual(Buffer.from(encoded), expected)
  assertAtomicChildren(encoded)
})

// PROTECTED_REGRESSION: sponsored wallet registration must preserve and encode the reserve guard and registration as one atomic call; changes require explicit human approval.
test('Node registration sends encoded atomic child calls to native Shield submission', async t => {
  const { payer, call, expected } = fixture(), client = new sdk.Client('local')
  t.after(() => client.close())
  let selectedRuntimes = 0, submitted = 0
  client.runtimeAt = async () => { selectedRuntimes++; return runtime }
  client.submitRawShielded = async (encoded, signer, options) => {
    submitted++
    assert.ok(encoded instanceof Uint8Array, 'native raw signing must never receive nested semantic descriptors')
    assert.deepEqual(Buffer.from(encoded), expected)
    assertAtomicChildren(encoded)
    assert.equal(signer.ss58Address, payer.ss58Address)
    assert.equal(options.mevShield, true)
    assert.equal(options.waitForFinalization, true)
    assert.equal(options.allowRawCall, true)
    return { success: true, finalized: true }
  }
  const adapter = client.walletRegistrationAdapter()
  const result = await adapter.submit(call, payer, snapshotWalletIdentity(await adapter.publicKey(payer, 'coldkey')), {})
  assert.equal(result.success, true)
  assert.equal(submitted, 1)
  assert.equal(selectedRuntimes, 1, 'all nested calls must use one selected metadata instance')
  assert.equal(call.params.calls[0].pallet, 'System', 'encoding must not mutate the approved descriptor')
})
