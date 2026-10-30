'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const { IntentCall, Policy } = require('../dist/transaction.js')
const pow = require('../dist/pow.js')

function prefix() {
  return Buffer.concat([
    Buffer.from('subtensor-pow-register-v1'), Buffer.from([1, 0]),
    Buffer.alloc(32, 1), Buffer.alloc(32, 2), Buffer.alloc(32, 3),
  ])
}

test('native PoW agrees with independent seal vectors and preserves full u64 nonces', async () => {
  for (const [nonce, expected] of [
    [0n, '7cdaced691a70795a7656ad9b76ef2c16b961686a14a790bcb22eed6226ef103'],
    [1n, 'c2291b632879447b6fe0785b0f25b5151c85a7e07df23a7860cf029e8a03f466'],
    [0xffffffffffffffffn, '69f1aef50783893195365b4fb62a4745085c31fee525a0984bc20301ab377aaf'],
  ]) {
    assert.equal(pow.powSeal(prefix(), nonce).toString('hex'), expected)
    const solution = await pow.powMineCpu(prefix(), 1n, nonce, 1)
    assert.equal(solution.nonce, nonce)
    assert.equal(solution.work.toString('hex'), expected)
  }
})

test('native PoW rejects invalid challenges and unbounded or lossy batches', async () => {
  await assert.rejects(pow.powMineCpu(prefix(), 0n, 0n, 1), /difficulty/)
  await assert.rejects(pow.powMineCpu(prefix().subarray(1), 1n, 0n, 1), /prefix/)
  const root = prefix()
  root.fill(0, 25, 27)
  assert.throws(() => pow.powSeal(root, 0n), /root/)
  await assert.rejects(pow.powMineCpu(prefix(), 1n, 0n, pow.POW_MAX_BATCH + 1), /attempts/)
  await assert.rejects(pow.powMineCpu(prefix(), 1n, Number.MAX_SAFE_INTEGER + 1, 1), /safe integer/)
  assert.throws(() => pow.powSeal(prefix(), 1n << 64n), /u64/)
  await assert.rejects(pow.PowGpuMiner.create([1, 1]), /unique/)
})

test('moveStakeLimit preserves both subnet scopes and exact price/amount integers', () => {
  const intent = IntentCall.moveStakeLimit('origin', 1, 'destination', 7, (1n << 80n) + 3n, (1n << 90n) + 9n, true)
  assert.equal(intent.function, 'move_stake_limit')
  assert.equal(intent.params.alpha_amount, (1n << 80n) + 3n)
  assert.equal(intent.params.limit_price, (1n << 90n) + 9n)
  assert.equal(intent.params.allow_partial, true)
  assert.deepEqual(new Policy({ allowedNetuids: [1, 7], maxSpendRao: 0n }).check(intent), [])
  assert.notDeepEqual(new Policy({ allowedNetuids: [1] }).check(intent), [])
})

test('stake ownership transfer remains unbounded and policy floors cannot weaken it', () => {
  const intent = IntentCall.transferStakeAndHotkey('coldkey', 'origin', 'destination', 1, 7, 10n)
  assert.equal(intent.function, 'transfer_stake_and_hotkey')
  assert.equal(intent.params.destination_hotkey, 'destination')
  const bounded = new Policy({ maxSpendRao: (1n << 120n), allowedNetuids: [1, 7] })
  assert.notDeepEqual(bounded.check(intent), [])
  assert.notDeepEqual(bounded.check(intent.spend('none')), [])
  assert.notDeepEqual(bounded.check(intent.spend('bounded', 0n)), [])
  assert.notDeepEqual(new Policy({ allowedNetuids: [1] }).check(intent.touches([1])), [])
  assert.notDeepEqual(new Policy({ allowedNetuids: [1, 7] }).check(intent.affectsAllSubnets()), [])
  assert.throws(() => intent.spend('bounded'), /amountRao/)
})
