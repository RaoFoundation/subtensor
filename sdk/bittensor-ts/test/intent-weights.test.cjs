'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const { prepareWeights, normalizeValidatorWeights, clipToMaxWeight } = require('../dist/intent-weights.js')

function reader(overrides = {}) {
  const state = {
    Uids: 0, CommitRevealWeightsEnabled: false, WeightsSetRateLimit: 10,
    LastUpdate: [90], MinAllowedWeights: 2, MaxWeightsLimit: 65535,
    SubnetEpochConsensus: 'Yuma', ValidatorPermit: [true], Tempo: 30,
    RevealPeriodEpochs: 2, LastEpochBlock: 70, PendingEpochAt: 111,
    SubnetEpochIndex: 9, BlocksSinceLastStep: 30, ...overrides,
  }
  const reads = [], encrypted = []
  return {
    reads, encrypted,
    async query(pallet, item, params, block) { reads.push({ pallet, item, params, block }); return state[item] },
    async runtimeCall() { throw new Error('unexpected runtime API') },
    async blockNumber() { return 100 }, async blockHash(n) { return `block-${n}` }, async blockTime() { return 0.25 },
    async encryptWeightsV2(options) { encrypted.push(options); return { commit: new Uint8Array([1, 2]), revealRound: 42n } },
  }
}
const context = { coldkey: 'cold', hotkey: 'hot', hotkeyPublicKey: new Uint8Array(32) }

test('validator normalization matches Python ties-to-even and drops zero weights', () => {
  assert.deepEqual(normalizeValidatorWeights([1, 2, 3], [65535, 2.5, 3.5]), [[1, 2, 3], [65535, 2, 4]])
  assert.deepEqual(normalizeValidatorWeights([1, 2], [0, 0]), [[], []])
  const clipped = clipToMaxWeight([1, 1, 100], 0.4)
  assert.ok(Math.abs(clipped.reduce((sum, n) => sum + n, 0) - 1) < 1e-12)
  assert.ok(Math.max(...clipped) <= 0.4 + 1e-7)
  assert.throws(() => normalizeValidatorWeights([1], [NaN]), /finite/)
})

test('plaintext weights pin preflight reads and use mechanism storage index', async () => {
  const source = reader()
  const prepared = await prepareWeights('set_weights', { netuid: 5, mechid: 2, weights: { 2: 1, 1: 0.5 } }, source, context)
  assert.equal(prepared.call.call, 'set_mechanism_weights')
  assert.deepEqual(prepared.call.params.dests, [1, 2])
  assert.deepEqual(prepared.call.params.weights, [32768, 65535])
  assert.deepEqual(source.reads.find(r => r.item === 'LastUpdate').params, [8197])
  assert.ok(source.reads.every(r => r.block === 'block-100'))
  assert.equal(source.encrypted.length, 0)
})

test('Null raw-u16 keeps ratios and requires sole validator permit even for self weights', async () => {
  const args = { netuid: 5, uids: [1, 2], weights: [2, 3], raw_u16: true }
  const result = await prepareWeights('set_weights', args, reader({ SubnetEpochConsensus: { Null: null } }), context)
  assert.deepEqual(result.call.params.weights, [2, 3])
  await assert.rejects(prepareWeights('set_weights', { netuid: 5, weights: { 0: 1 } }, reader({ SubnetEpochConsensus: 'Null', ValidatorPermit: [false] }), context), { code: 'NOT_AUTHORIZED' })
  await assert.rejects(prepareWeights('set_weights', args, reader(), context), /raw_u16/)
})

test('weights detect registration, rate, minimum count, and raw maximum ratio failures', async () => {
  const args = { netuid: 5, weights: { 1: 1, 2: 2 } }
  await assert.rejects(prepareWeights('set_weights', args, reader({ Uids: null }), context), { code: 'NOT_REGISTERED' })
  await assert.rejects(prepareWeights('set_weights', args, reader({ LastUpdate: [91] }), context), { code: 'RATE_LIMITED' })
  await assert.rejects(prepareWeights('set_weights', args, reader({ MinAllowedWeights: 3 }), context), { code: 'INVALID_ARGUMENT' })
  await assert.rejects(prepareWeights('set_weights', { ...args, raw_u16: true }, reader({ SubnetEpochConsensus: 'Null', MaxWeightsLimit: 32768 }), context), /max_weight_limit/)
})

test('timelocked weights use all pinned epoch state and fractional block seconds', async () => {
  const source = reader({ CommitRevealWeightsEnabled: true })
  const result = await prepareWeights('set_weights', { netuid: 5, mechid: 2, weights: { 1: 1, 2: 2 }, version_key: '9007199254740993' }, source, context)
  assert.equal(result.call.call, 'commit_timelocked_mechanism_weights')
  assert.deepEqual(result.extras, { reveal_round: 42n })
  assert.deepEqual(source.encrypted[0].state, {
    lastEpochBlock: 70n, pendingEpochAt: 111n, subnetEpochIndex: 9n,
    tempo: 30, blocksSinceLastStep: 30n, currentBlock: 100n,
  })
  assert.equal(source.encrypted[0].blockTime, 0.25)
  assert.equal(source.encrypted[0].versionKey, 9007199254740993n)
  for (const read of source.reads.filter(r => ['Tempo', 'RevealPeriodEpochs', 'LastEpochBlock', 'PendingEpochAt', 'SubnetEpochIndex', 'BlocksSinceLastStep'].includes(r.item))) {
    assert.deepEqual(read.params, [5])
    assert.equal(read.block, 'block-100')
  }
})
