'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const { SigningKey, getBytes } = require('ethers')
const sdk = require('../dist')
const { gammaUnits } = require('../dist/gamma')

const endpoint = process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944'
const localnet = { skip: process.env.BT_TS_LOCALNET === '1' ? false : 'set BT_TS_LOCALNET=1 for isolated release feature tests', timeout: 600_000 }
const wait = { waitForFinalization: true, timeoutMs: 120_000 }

test('release features execute against an isolated development runtime', localnet, async t => {
  assert.ok(['localhost', '127.0.0.1', '[::1]'].includes(new URL(endpoint).hostname), 'development signers require a loopback localnet')
  const client = await new sdk.Client(endpoint).connect(), alice = sdk.Keypair.fromUri('//Alice')
  t.after(() => client.close())
  // The suite changes development feature flags. A familiar account balance is
  // insufficient: require this chain's actual sudo key to be the development key.
  assert.equal(await client.query('Sudo', 'Key'), alice.ss58Address, 'release integration tests require the development sudo key')
  const raw = async (pallet, call, params, signer = alice) => {
    const result = await client.submit([pallet, call, params], signer, { ...wait, allowRawCall: true })
    assert.equal(result.success, true, result.message)
    return result
  }
  const execute = async (name, args, signer = alice) => {
    const result = await client.execute(sdk.buildIntent(name, args), signer, { ...wait, mevShield: false })
    assert.equal(result.success, true, result.message)
    return result
  }
  const sudo = async (pallet, call, params) => raw('Sudo', 'sudo', { call: await client.composeCall(pallet, call, params) })

  await t.test('external ECDSA signs and submits the exact large offline payload', async () => {
    const key = new SigningKey('0x' + '11'.repeat(32)), publicKey = Buffer.from(getBytes(key.compressedPublicKey))
    const signer = { publicKey, address: sdk.ss58FromEcdsaPublic(publicKey), cryptoType: 2 }
    const sign = payload => {
      const s = key.sign(sdk.blake2_256(payload))
      return Buffer.concat([Buffer.from(getBytes(s.r)), Buffer.from(getBytes(s.s)), Buffer.from([s.yParity])])
    }
    await execute('transfer', { dest_ss58: signer.address, amount_tao: '1' })
    const nonce = await client.peekNextIndex(signer.address)
    const prepared = await client.prepareCall(['System', 'remark', { remark: Buffer.alloc(1024, 0x42) }], signer, { nonce, allowRawCall: true })
    assert.equal(prepared.payload.length, 32, 'large signing payload uses the runtime hash rule')
    assert.deepEqual(prepared.publicKey, sdk.blake2_256(publicKey))
    const imported = await client.importPreparedCall(JSON.parse(JSON.stringify(client.exportPreparedCall(prepared))), { allowRawCall: true })
    await assert.rejects(client.submitSignature(imported, sign(Buffer.from('wrong message')), wait), /does not verify/)
    const signature = sign(prepared.payload)
    assert.equal(sdk.verifySignature(prepared.payload, signature, signer.address, 2), true)
    const result = await client.submitSignature(imported, Buffer.concat([Buffer.from([2]), signature]), wait)
    assert.equal(result.success, true, result.message)
    assert.equal(result.finalized, true)
    assert.equal(await client.peekNextIndex(signer.address), nonce + 1)
  })

  await t.test('Gamma services, quotes, mint batches, partial spends and moves use runtime state', async () => {
    const ownerHotkey = sdk.Keypair.fromUri('//PR2853//release-owner')
    const destination = sdk.Keypair.fromUri('//PR2853//release-gamma-destination')
    await sudo('AdminUtils', 'sudo_set_start_call_delay', { delay: 0 })
    await sudo('AdminUtils', 'sudo_set_gamma_enabled', { enabled: true })
    await sudo('AdminUtils', 'sudo_set_tao_usd_price_oracle', { oracle: alice.ss58Address })
    await execute('register_subnet', { hotkey_ss58: ownerHotkey.ss58Address })
    const owners = await client.queryMap('SubtensorModule', 'SubnetOwnerHotkey')
    const created = owners.find(([, hotkey]) => String(hotkey) === ownerHotkey.ss58Address)
    assert.ok(created, 'registration created the owned subnet')
    const netuid = Number(Array.isArray(created[0]) ? created[0][0] : created[0])
    assert.ok(netuid > 0)
    await execute('start_call', { netuid })
    await sudo('AdminUtils', 'sudo_set_subnet_gamma_enabled', { netuid, enabled: true })
    await raw('SubtensorModule', 'feed_tao_usd_price', { price: 400_000_000_000n })
    await execute('set_service', { netuid, hotkey_ss58: ownerHotkey.ss58Address, url: 'https://example.test/release-service', spender_ss58: alice.ss58Address })
    const services = await client.read('services', { netuid })
    assert.equal(services[ownerHotkey.ss58Address].url, 'https://example.test/release-service')
    assert.equal(services[ownerHotkey.ss58Address].owner, true)
    const quote = await client.read('quote_mint_gamma', { netuid, amount_tao: '0.01' })
    assert.ok(gammaUnits(quote.gamma) >= 100_000_000n)
    const minted = await execute('mint_gamma', { netuid, hotkey_ss58: ownerHotkey.ss58Address, amount_tao: '0.01', service_hotkey_ss58: ownerHotkey.ss58Address, daily_spend_cap_gamma: '0.05' })
    assert.equal(minted.data.minted.spender_ss58, alice.ss58Address)
    assert.equal(minted.data.mints[0].netuid, netuid)
    const deposit = await client.read('gamma_deposit', { netuid, hotkey_ss58: ownerHotkey.ss58Address })
    assert.equal(deposit.balance_gamma, minted.data.minted.gamma)
    assert.equal(deposit.daily_spend_cap_gamma, '0.05')

    const topup = await client.composeCall('SubtensorModule', 'mint_gamma', { netuid, hotkey: ownerHotkey.ss58Address, amount: 10_000_000n, min_amount_out: 0, spender: null, daily_spend_cap: null })
    const batch = await raw('Utility', 'batch_all', { calls: [topup, topup] })
    assert.equal(batch.data.mints.length, 2)
    assert.ok(batch.data.mints.every(row => row.netuid === netuid))
    const afterBatch = await client.read('gamma_deposit', { netuid, hotkey_ss58: ownerHotkey.ss58Address })
    assert.equal(gammaUnits(afterBatch.balance_gamma), gammaUnits(deposit.balance_gamma) + batch.data.mints.reduce((sum, row) => sum + gammaUnits(row.gamma), 0n))

    const reference = '0x' + 'ab'.repeat(32)
    const spent = await execute('spend_gamma', { netuid, spends: [
      { hotkey_ss58: ownerHotkey.ss58Address, amount_gamma: '0.02', reference },
      { hotkey_ss58: ownerHotkey.ss58Address, amount_gamma: '1', reference },
    ] })
    assert.equal(spent.data.spent.length, 1)
    assert.equal(spent.data.spent[0].amount_gamma, '0.02')
    assert.equal(spent.data.spent[0].netuid, netuid)
    assert.equal(spent.data.skipped.length, 1)
    assert.equal(spent.data.skipped[0].reason, 'GammaDepositDailySpendCapExceeded')
    const afterSpend = await client.read('gamma_deposit', { netuid, hotkey_ss58: ownerHotkey.ss58Address })
    assert.equal(gammaUnits(afterSpend.balance_gamma), gammaUnits(afterBatch.balance_gamma) - 20_000_000n)
    await execute('associate_hotkey', { hotkey_ss58: destination.ss58Address })
    await execute('move_gamma', { netuid, origin_hotkey_ss58: ownerHotkey.ss58Address, destination_hotkey_ss58: destination.ss58Address, amount_gamma: 'all' })
    assert.equal(await client.read('gamma_deposit', { netuid, hotkey_ss58: ownerHotkey.ss58Address }), null)
    const moved = await client.read('gamma_deposit', { netuid, hotkey_ss58: destination.ss58Address })
    assert.equal(moved.balance_gamma, afterSpend.balance_gamma)
  })

  // PROTECTED_REGRESSION: Raw signer envelopes must not block registered wallets from transferring actual funds; changes require explicit human approval.
  await t.test('public-only hashed Ed25519 registration enables a real protected transfer', async () => {
    await sudo('AdminUtils', 'sudo_set_hashed_accounts_enabled', { enabled: true })
    const key = sdk.Keypair.fromSeed(Buffer.alloc(32, 83), sdk.WALLET_TYPES['hashed-ed25519'])
    const deadline = Date.now() + 60_000
    while ((await client.query('MevShield', 'NextKey')) == null) {
      assert.ok(Date.now() < deadline, 'development authority did not publish a Shield key')
      await new Promise(resolve => setTimeout(resolve, 500))
    }
    const deposit = await client.registrationDepositForTypes([key.cryptoType])
    const registered = await client.registerWallet(key.publicOnly(), { payer: alice, maxRegistrationDepositRao: deposit.rao, timeoutMs: 180_000 })
    assert.equal(registered.result.success, true, registered.result.message)
    assert.ok(registered.finalizedBlockHash)
    assert.deepEqual(registered.accounts, [key.ss58Address])
    assert.ok(await client.query('HashedAccounts', 'Accounts', [key.ss58Address]))
    await execute('transfer', { dest_ss58: key.ss58Address, amount_tao: '1' })
    const recipient = sdk.Keypair.fromUri('//PR2853//release-protected-recipient').ss58Address
    const before = await client.balances.get(recipient)
    await execute('transfer', { dest_ss58: recipient, amount_tao: '0.01' }, key)
    assert.equal((await client.balances.get(recipient)).rao - before.rao, 10_000_000n)
  })
})
