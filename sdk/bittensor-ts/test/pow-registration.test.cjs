'use strict'
const assert = require('node:assert/strict')
const test = require('node:test')
const { mineRegistration, registrationPrefix, effectiveDifficulty } = require('../dist/pow-registration.js')

const hash = block => `0x${BigInt(block).toString(16).padStart(64, '0')}`
const decode = address => new Uint8Array(32).fill(address === 'hot' ? 2 : 3)
const miner = { decodeAddress: decode, seal: () => new Uint8Array(32), async mine() { return { nonce: 7n, work: new Uint8Array(32) } } }
function source(heads = [100, 100]) {
  const queries = [], state = { NetworkPowRegistrationAllowed: true, Difficulty: 1n, MinDifficulty: 1n, MaxDifficulty: 100n, Uids: null, LastPowRegistrationBlock: 1, RegistrationsThisBlock: 0, MaxRegistrationsPerBlock: 2 }
  let index = 0
  return { queries, state, async blockNumber() { return heads[Math.min(index++, heads.length - 1)] }, async blockHash(block) { return hash(block) }, async query(pallet, item, params, block) { queries.push({ pallet, item, params, block }); return state[item] } }
}

test('registration challenge binds network, anchor, hotkey and coldkey in runtime order', () => {
  const prefix = registrationPrefix(513, hash(9), 'hot', 'cold', decode)
  assert.equal(prefix.length, 123)
  assert.equal(new TextDecoder().decode(prefix.slice(0, 25)), 'subtensor-pow-register-v1')
  assert.deepEqual(Array.from(prefix.slice(25, 27)), [1, 2])
  assert.deepEqual(prefix.slice(59, 91), decode('hot'))
  assert.deepEqual(prefix.slice(91), decode('cold'))
  assert.equal(effectiveDifficulty(2n, 7n, 3n), 7n)
  assert.equal(effectiveDifficulty(10n, 1n, 4n), 4n)
  assert.throws(() => registrationPrefix(0, hash(1), 'hot', 'cold', decode), /non-root/)
})

test('registration mining returns a semantic intent with pinned and rechecked difficulty', async () => {
  const reader = source()
  const intent = await mineRegistration(reader, 1, 'hot', 'cold', miner, { workers: 1, backend: 'cpu' })
  assert.equal(intent.op, 'pow_register')
  assert.equal(intent.args.work_block, 100)
  assert.equal(intent.args.nonce, 7n)
  assert.equal(intent.args.work_hex, '00'.repeat(32))
  assert.ok(reader.queries.every(query => query.block === hash(100)))
  assert.equal(reader.queries.filter(query => query.item === 'Difficulty').length, 2)
})

test('registration mining discards proofs five blocks old before rebuilding the challenge', async () => {
  const reader = source([100, 105, 105, 105])
  const intent = await mineRegistration(reader, 1, 'hot', 'cold', miner, { workers: 1 })
  assert.equal(intent.args.work_block, 105)
  assert.equal(reader.queries.filter(query => query.item === 'NetworkPowRegistrationAllowed').length, 2)
})

test('registration mining rejects invalid miner proofs, registered hotkeys and cancellation', async () => {
  await assert.rejects(mineRegistration(source(), 1, 'hot', 'cold', { ...miner, async mine() { return { nonce: 7n, work: new Uint8Array(32).fill(1) } } }, { workers: 1 }), /invalid registration proof/)
  const registered = source(); registered.state.Uids = 0
  await assert.rejects(mineRegistration(registered, 1, 'hot', 'cold', miner), /already registered/)
  const controller = new AbortController(); controller.abort(new Error('stop'))
  await assert.rejects(mineRegistration(source(), 1, 'hot', 'cold', miner, { signal: controller.signal }), /stop/)
})
