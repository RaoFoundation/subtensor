'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const sdk = require('../dist/index.js')
const endpoint = process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944'
const enabled = process.env.BT_TS_LOCALNET === '1'
const options = { skip: enabled ? false : 'set BT_TS_LOCALNET=1 for isolated localnet workflows', timeout: 240_000 }

test('semantic workflows enforce policy before signing and finalize real transfers and multisig', options, async () => {
  assert.ok(['localhost', '127.0.0.1', '[::1]'].includes(new URL(endpoint).hostname), 'development signers require a loopback localnet')
  const client = await new sdk.Client(endpoint).connect()
  const alice = sdk.Keypair.fromUri('//Alice')
  const bob = sdk.Keypair.fromUri('//Bob')
  const recipient = sdk.Keypair.fromUri('//PR2853//workflow-recipient').ss58Address
  const amount = sdk.Balance.fromTao('0.01')
  const transfer = sdk.buildIntent('transfer', { dest_ss58: recipient, amount_tao: amount })
  try {
    const snapshot = await client.at()
    const balance = await snapshot.read('balance', { coldkey_ss58: alice.ss58Address })
    assert.ok(balance.rao > amount.rao)
    const plan = await client.plan(transfer, alice, { mevShield: false, policy: { maxSpendRao: amount.rao, maxFeeRao: 100_000_000n } })
    assert.equal(plan.ok, true, plan.violations.join('; '))
    assert.equal(plan.intent.spendRao, amount.rao)
    assert.match(plan.blockHash, /^0x[0-9a-f]{64}$/i)
    assert.ok(plan.fee.rao > 0n)

    let signatures = 0
    const external = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: alice.cryptoType, sign: data => { signatures++; return alice.sign(data) } }
    await assert.rejects(client.execute(transfer, external, { mevShield: false, policy: { maxSpendRao: 0n } }), /policy|spend|rejected/i)
    assert.equal(signatures, 0, 'rejected policy must not request a signature')

    const before = await client.balances.get(recipient)
    const sent = await client.execute(transfer, external, { mevShield: false, waitForFinalization: true, timeoutMs: 90_000, policy: { maxSpendRao: amount.rao, maxFeeRao: 100_000_000n } })
    assert.equal(sent.success, true, sent.message)
    assert.equal(sent.finalized, true)
    assert.equal((await client.balances.get(recipient)).rao - before.rao, amount.rao)
    assert.equal((await snapshot.read('balance', { coldkey_ss58: alice.ss58Address })).rao, balance.rao, 'snapshot stays pinned across submission')

    const multisig = await client.multisig([alice.ss58Address, bob.ss58Address], 2)
    const funding = await client.execute(sdk.buildIntent('transfer', { dest_ss58: multisig.address, amount_tao: '1' }), alice, { mevShield: false, waitForFinalization: true })
    assert.equal(funding.success, true, funding.message)
    const call = transfer
    const first = await multisig.approve(call, alice, { mevShield: false, waitForFinalization: true })
    assert.equal(first.success, true, first.message)
    const balanceBeforeExecution = await client.balances.get(recipient)
    const second = await multisig.approve(call, bob, { mevShield: false, waitForFinalization: true })
    assert.equal(second.success, true, second.message)
    assert.equal((await client.balances.get(recipient)).rao - balanceBeforeExecution.rao, amount.rao)
  } finally { await client.close() }
})

test('MEV workflow returns the actual inner receipt and balance change', options, async () => {
  assert.ok(['localhost', '127.0.0.1', '[::1]'].includes(new URL(endpoint).hostname), 'development signers require a loopback localnet')
  const client = await new sdk.Client(endpoint).connect()
  const alice = sdk.Keypair.fromUri('//Alice')
  const recipient = sdk.Keypair.fromUri('//PR2853//shield-recipient').ss58Address
  try {
    const deadline = Date.now() + 60_000
    while ((await client.query('MevShield', 'NextKey')) == null) {
      assert.ok(Date.now() < deadline, 'local authority did not publish an MEV key')
      await new Promise(resolve => setTimeout(resolve, 500))
    }
    const before = await client.balances.get(recipient)
    const result = await client.execute(sdk.buildIntent('transfer', { dest_ss58: recipient, amount_tao: '0.01' }), alice, { mevShield: true, waitForFinalization: true, timeoutMs: 180_000, policy: { maxSpendRao: 10_000_000n, maxFeeRao: 100_000_000n } })
    assert.equal(result.success, true, result.message)
    assert.equal(result.data?.carrier_only, false)
    assert.notEqual(result.innerExtrinsicHash, result.carrierExtrinsicHash)
    assert.equal((await client.balances.get(recipient)).rao - before.rao, 10_000_000n)
    assert.equal(result.finalized, true)
  } finally { await client.close() }
})
