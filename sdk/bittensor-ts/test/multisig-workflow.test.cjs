const test = require('node:test')
const assert = require('node:assert/strict')
const { prepareMultisigApproval } = require('../dist/multisig-workflow.js')
const call = { pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: 'dest', value: 10n } }
const request = { call, signatories: ['signer', 'other'], threshold: 2, signerAddress: 'signer' }
function reader(pending = null, free = 10000n) {
  return {
    decodeAddress: address => Uint8Array.from({ length: 32 }, () => address === 'signer' ? 1 : 2),
    multisigAccount: () => ({ address: 'multisig', publicKey: new Uint8Array(32) }),
    hashCall: async () => new Uint8Array(32),
    estimateWeight: async () => ({ ref_time: 5n, proof_size: 1n }),
    estimateFee: async call => { assert.equal(call.pallet, 'Multisig'); return 100n },
    constant: async (_, name) => name === 'DepositBase' ? 1000n : 20n,
    query: async (_, item) => item === 'Multisigs' ? pending : { data: { free } },
  }
}
test('multisig opening computes deposit and later approval uses on-chain timepoint', async () => {
  const first = await prepareMultisigApproval(reader(), request)
  assert.equal(first.opening, true)
  assert.equal(first.depositRao, 1040n)
  assert.equal(first.call.params.maybe_timepoint, null)
  const next = await prepareMultisigApproval(reader({ when: { height: 42, index: 3 } }), request)
  assert.equal(next.opening, false)
  assert.equal(next.depositRao, 0n)
  assert.deepEqual(next.call.params.maybe_timepoint, { height: 42, index: 3 })
})
test('multisig funding, membership and expected-call guards fail before signing', async () => {
  assert.match((await prepareMultisigApproval(reader(null, 100n), request)).violations.join(), /1140/)
  await assert.rejects(prepareMultisigApproval(reader(), { ...request, expectedCallHash: '0x' + '01'.repeat(32) }), /call changed/)
  await assert.rejects(prepareMultisigApproval(reader(), { ...request, signatories: ['signer', 'signer'] }), /duplicate/)
  await assert.rejects(prepareMultisigApproval(reader({}), request), /missing its timepoint/)
})
test('threshold-one approval uses the no-deposit wrapper', async () => {
  const value = await prepareMultisigApproval(reader(), { ...request, threshold: 1 })
  assert.equal(value.call.call, 'as_multi_threshold_1')
  assert.equal(value.depositRao, 0n)
  assert.equal(value.opening, false)
})
