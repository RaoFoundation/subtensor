const test = require('node:test')
const assert = require('node:assert/strict')
const { resolveTransactionFeePayer } = require('../dist/fee-payer.js')
const transfer = { pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: 'dest', value: 1n } }
const weights = { pallet: 'SubtensorModule', call: 'set_weights', params: {} }
const proxy = (real, call) => ({ pallet: 'Proxy', call: 'proxy', params: { real, call } })
const batch = calls => ({ pallet: 'Utility', call: 'batch_all', params: { calls } })
function reader(consents = [], owner = null) {
  return {
    countStorageKeys: async (_, item, keys) => Number(item === 'Owner' ? owner != null : consents.includes(keys.join(':'))),
    query: async (_, item) => { assert.equal(item, 'Owner'); return owner },
    decodeAddress: address => new TextEncoder().encode(address),
  }
}
test('only direct hotkey calls redirect fees and discard the tip', async () => {
  assert.deepEqual(await resolveTransactionFeePayer(reader([], 'coldkey'), weights, 'signer'), { address: 'coldkey', dropsTip: true })
  assert.deepEqual(await resolveTransactionFeePayer(reader(), weights, 'signer'), { address: 'signer', dropsTip: false })
  for (const call of [batch([weights]), proxy('real', weights), { pallet: 'Multisig', call: 'as_multi', params: { call: weights } }]) {
    assert.deepEqual(await resolveTransactionFeePayer(reader([], 'coldkey'), call, 'signer'), { address: 'signer', dropsTip: false })
  }
})
test('proxy sponsorship requires physical consent and stops at three levels or revoked consent', async () => {
  const call = proxy('one', proxy('two', proxy('three', proxy('four', transfer))))
  assert.deepEqual(await resolveTransactionFeePayer(reader(), call, 'signer'), { address: 'signer', dropsTip: false })
  assert.deepEqual(await resolveTransactionFeePayer(reader(['one:signer']), call, 'signer'), { address: 'one', dropsTip: true })
  assert.deepEqual(await resolveTransactionFeePayer(reader(['one:signer', 'two:one', 'three:two', 'four:three']), call, 'signer'), { address: 'three', dropsTip: true })
})
test('homogeneous proxy batches can choose a final payer only inside a consented proxy', async () => {
  const all = batch([proxy('two', transfer), proxy('two', weights)]), consents = ['one:signer', 'two:one']
  assert.deepEqual(await resolveTransactionFeePayer(reader(consents), proxy('one', all), 'signer'), { address: 'two', dropsTip: true })
  assert.deepEqual(await resolveTransactionFeePayer(reader(consents), all, 'signer'), { address: 'signer', dropsTip: false })
  assert.deepEqual(await resolveTransactionFeePayer(reader(consents), proxy('one', batch([proxy('two', transfer), transfer])), 'signer'), { address: 'one', dropsTip: true })
})
test('unit and default values cannot fake consent and storage failures fail closed', async () => {
  await assert.rejects(resolveTransactionFeePayer({ query: async () => null }, proxy('real', transfer), 'signer'), /physical storage-key presence/)
  await assert.rejects(resolveTransactionFeePayer({ countStorageKeys: async () => -1 }, weights, 'signer'), /invalid storage-key count/)
})
