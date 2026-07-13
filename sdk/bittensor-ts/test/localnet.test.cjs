'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')

const sdk = require('../dist/index.js')

const endpoint = process.env.BT_CHAIN_ENDPOINT ?? 'ws://127.0.0.1:9944'
const enabled = process.env.BT_TS_LOCALNET === '1' || process.env.BT_CHAIN_ENDPOINT != null
const timeout = Number(process.env.BT_TS_LOCALNET_TIMEOUT_MS ?? 120_000)
const localnet = {
  skip: enabled ? false : 'set BT_TS_LOCALNET=1 or BT_CHAIN_ENDPOINT to run localnet integration tests',
  timeout,
}

function assertHash(value, name = 'hash') {
  assert.match(String(value), /^0x[0-9a-fA-F]{64}$/, `${name} is a 32-byte hash`)
}

function assertBalance(value, name = 'balance') {
  assert.ok(value instanceof sdk.Balance, `${name} is a Balance`)
  assert.equal(typeof value.rao, 'bigint', `${name}.rao is bigint`)
}

async function closeAll(...clients) {
  await Promise.all(clients.map(async (client) => {
    if (client?.close == null) return
    await client.close().catch(() => undefined)
  }))
}

test('localnet covers live reads, metadata, namespaces, snapshots, and fallback RPC helpers', localnet, async () => {
  const alice = sdk.Keypair.fromUri('//Alice')
  const bob = sdk.Keypair.fromUri('//Bob')
  const node = new sdk.Client(endpoint, { autoConnect: false, requestTimeoutMs: 20_000 })
  const browser = new sdk.BrowserChainClient(endpoint, { autoConnect: false, requestTimeoutMs: 20_000 })
  const native = await sdk.NativeChainClient.connect(endpoint)

  try {
    await node.connect()
    await browser.connect()

    const head = await node.finalizedHead()
    assertHash(head, 'finalized head')
    const blockNumber = await node.blockNumber(head)
    assert.ok(blockNumber >= 0)
    assertHash(await node.blockHash(blockNumber), 'block hash')
    assertHash(await node.genesisHash(), 'genesis hash')

    const runtime = await node.runtimeAt()
    const historicalRuntime = await node.runtimeAt(head)
    assert.equal(historicalRuntime.specVersion, runtime.specVersion)
    assert.equal(runtime.typeNameOf(runtime.storageEntry('System', 'Account').valueTypeId) != null, true)
    assert.equal(runtime.runtimeApis().TransactionPaymentApi.query_info.name, 'query_info')

    const metadata = await node.stateCall('Metadata_metadata_at_version', '0x0f000000', head)
    assert.match(String(metadata), /^0x/)
    assert.equal(await node.decodeScale('u32', Buffer.from([42, 0, 0, 0]), head), 42)
    assert.ok((await node.composeCall('System', 'remark', { remark: Buffer.from('historical') }, head)).length > 0)
    assert.deepEqual(await node.validateDescriptorSchema(head), [])
    await node.assertDescriptorSchema(head)

    const now = await node.query(sdk.storage.Timestamp.Now)
    assert.ok(typeof now === 'number' || typeof now === 'bigint')
    assert.ok(await node.timestamp() instanceof Date)
    const blockInfo = await node.blockInfo(blockNumber)
    assert.equal(blockInfo?.number, blockNumber)
    assertHash(blockInfo?.hash, 'blockInfo hash')

    const account = await node.query(sdk.storage.System.Account, [alice.ss58Address])
    assert.ok(account?.data)
    const accountBatch = await node.queryBatch(sdk.storage.System.Account, [
      [alice.ss58Address],
      [bob.ss58Address],
    ])
    assert.equal(accountBatch.length, 2)
    const networkPage = await node.queryMap(
      sdk.storage.SubtensorModule.NetworksAdded,
      [],
      head,
      undefined,
      { pageSize: 1, maxResults: 3 },
    )
    assert.ok(networkPage.length >= 1)

    assertBalance(await node.balances.get(alice.ss58Address), 'alice balance')
    assertBalance((await node.balances.getMany([alice.ss58Address, bob.ss58Address]))[alice.ss58Address], 'alice batch balance')
    assertBalance(await node.balances.existentialDeposit(head), 'existential deposit')
    assertBalance(await node.getBalance(alice), 'getBalance alias')

    const subnets = await node.subnets.all()
    assert.ok(subnets.length >= 1)
    const netuid = subnets[0].netuid
    const subnet = await node.subnets.info(netuid)
    assert.equal(subnet.netuid, netuid)
    assert.equal(await node.subnets.exists(netuid), true)
    assertBalance(await node.subnets.burn(netuid), 'subnet burn')
    assert.equal(typeof await node.subnets.commitRevealEnabled(netuid), 'boolean')
    assert.ok(await node.subnets.hyperparameters(netuid))
    assert.ok(await node.subnets.metagraph(netuid))
    assert.ok(Array.isArray(await node.neurons.all(netuid)))
    assertBalance(await node.staking.get(alice.ss58Address, bob.ss58Address, netuid), 'stake balance')
    assert.ok(Array.isArray(await node.staking.positions(alice.ss58Address)))

    assertBalance(await node.read('balance', { coldkey_ss58: alice.ss58Address }), 'read balance')
    assert.ok(Array.isArray(await node.read('subnets')))
    assert.equal((await node.read('subnet', { netuid })).netuid, netuid)
    assertBalance(await node.read('burn', { netuid }), 'read burn')
    assert.equal(typeof await node.read('commit_reveal_enabled', { netuid }), 'boolean')
    assert.ok(await node.read('subnet_hyperparameters', { netuid }))
    assert.ok(await node.read('metagraph', { netuid }))
    assertBalance(
      await node.read('stake', {
        coldkey_ss58: alice.ss58Address,
        hotkey_ss58: bob.ss58Address,
        netuid,
      }),
      'read stake',
    )

    const snapshot = await node.at(blockNumber)
    assert.equal(snapshot.block, blockNumber)
    assertBalance(await snapshot.balances.get(alice.ss58Address), 'snapshot balance')
    assert.equal((await snapshot.subnets.info(netuid)).netuid, netuid)
    assert.ok(Array.isArray(await snapshot.neurons.all(netuid)))
    assert.ok(Array.isArray(await snapshot.staking.positions(alice.ss58Address)))

    assert.equal(native.ss58Format, 42)
    assertHash(`0x${native.genesisHash.toString('hex')}`, 'native genesis')
    assert.ok(native.readCatalog().length > 0)
    assert.equal(await native.blockNumber(head), blockNumber)
    assert.equal(await native.query('Timestamp', 'Now', [], head), now)
    assert.equal((await native.queryBatch('System', 'Account', [[alice.ss58Address]])).length, 1)
    assert.ok((await native.queryMap('SubtensorModule', 'NetworksAdded')).length >= 1)
    assert.ok((await native.runtimeCall('SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3', [netuid])))

    const browserAccount = await browser.query(sdk.storage.System.Account, [alice.ss58Address])
    assert.deepEqual(browserAccount.data.free, account.data.free)
    const waited = await browser.waitForBlock(blockNumber, { timeoutMs: 30_000 })
    assert.ok(waited.number >= blockNumber)
  } finally {
    await closeAll(node, browser)
  }
})

test('localnet covers signing, fee estimation, runtime payment APIs, and bounded submissions', localnet, async () => {
  const alice = sdk.Keypair.fromUri('//Alice')
  const bob = sdk.Keypair.fromUri('//Bob')
  const node = new sdk.Client(endpoint, { autoConnect: false, requestTimeoutMs: 20_000 })
  const browser = new sdk.BrowserChainClient(endpoint, { autoConnect: false, requestTimeoutMs: 20_000 })
  const native = await sdk.NativeChainClient.connect(endpoint)

  try {
    await node.connect()
    await browser.connect()

    const transfer = sdk.IntentCall.transfer(bob.ss58Address, 1n)
    const tuple = transfer.asCallTuple()
    assert.deepEqual(tuple.slice(0, 2), ['Balances', 'transfer_keep_alive'])
    assert.equal(sdk.isIntentCall(transfer), true)
    assert.equal(transfer.withSummary('one rao transfer').summary, 'one rao transfer')
    assert.equal(sdk.rawCall('System', 'remark', { remark: Buffer.from('raw') }).pallet, 'System')

    const allowPolicy = new sdk.Policy({ maxFeeRao: 10_000_000n, maxSpendRao: 1n })
    assert.deepEqual(allowPolicy.check(transfer, 1n), [])
    assert.equal(allowPolicy.hasOpaqueByteRestrictions(), true)
    assert.equal(allowPolicy.withRawCalls().allowRawCalls, true)
    assert.ok(new sdk.Policy({ maxSpendRao: 0n }).check(transfer, 1n).length > 0)

    for (const intent of [
      sdk.IntentCall.transferAllowDeath(bob.ss58Address, 1n),
      sdk.IntentCall.transferAll(bob.ss58Address, false),
      sdk.IntentCall.setWeights(0, [], [], 0n),
      sdk.IntentCall.addStake(bob.ss58Address, 0, 1n),
      sdk.IntentCall.addStakeLimit(bob.ss58Address, 0, 1n, 1n, false),
      sdk.IntentCall.removeStake(bob.ss58Address, 0, 1n),
      sdk.IntentCall.removeStakeLimit(bob.ss58Address, 0, 1n, 1n, false),
      sdk.IntentCall.burnedRegister(0, bob.ss58Address),
      sdk.IntentCall.rootRegister(bob.ss58Address),
      sdk.IntentCall.registerSubnet(bob.ss58Address),
      sdk.IntentCall.startCall(0),
      sdk.IntentCall.serveAxon(0, 2130706433, 30333),
      sdk.IntentCall.moveStake(bob.ss58Address, 0, alice.ss58Address, 0, 1n),
      sdk.IntentCall.swapStake(bob.ss58Address, 0, 0, 1n),
      sdk.IntentCall.transferStake(alice.ss58Address, bob.ss58Address, 0, 0, 1n),
      sdk.IntentCall.unstakeAll(bob.ss58Address),
      sdk.IntentCall.unstakeAllAlpha(bob.ss58Address),
      sdk.IntentCall.setHyperparameter(0, 'immunity_period', 99),
      sdk.IntentCall.setRootClaimType('Keep'),
      sdk.calls.balances.transferKeepAlive(bob.ss58Address, sdk.rao(1n)),
      sdk.calls.subtensor.servePrometheus(0, '127.0.0.1', 9090),
    ]) {
      assert.ok((await node.callData(intent, await node.finalizedHead())).length > 0)
    }

    const nativeFee = await node.estimateFee(transfer, alice)
    const browserFee = await browser.estimateFee(transfer, alice)
    assertBalance(nativeFee, 'native fee')
    assertBalance(browserFee, 'browser fee')
    assert.ok(nativeFee.rao > 0n)
    assert.ok(browserFee.rao > 0n)

    const nonce = await browser.peekNextIndex(alice.ss58Address)
    const signed = await browser.signExtrinsic(transfer, alice, { nonce, period: null })
    assert.equal(signed.signerAddress, alice.ss58Address)
    assert.equal(signed.nonce, nonce)
    assert.match(signed.hex, /^0x[0-9a-fA-F]+$/)
    assert.equal(signed.hash.length, 32)
    const paymentInfo = await browser.runtimeCall(
      sdk.runtimeApi.TransactionPaymentApi.query_info,
      [signed.bytes, signed.bytes.length],
    )
    assert.ok(BigInt(paymentInfo.partial_fee ?? paymentInfo.partialFee) > 0n)

    const nativeCallData = await native.composeIntent(transfer)
    assert.ok(nativeCallData.length > 0)
    assert.ok(await native.estimateFee(nativeCallData, alice) > 0n)
    const plan = await native.externalSigningPlanForIntent(
      transfer,
      alice.ss58Address,
      alice.publicKey,
      alice.cryptoType,
      false,
      allowPolicy,
    )
    assert.equal(plan.signerAddress, alice.ss58Address)
    assert.ok(plan.payload.length > 0)
    assert.ok(await native.estimateFeeExternal(plan) > 0n)
    const assembled = await native.assembleExternal(plan, alice.sign(plan.payload), alice.cryptoType)
    assert.ok(assembled.bytes.length > 0)
    assertHash(assembled.hash, 'assembled hash')

    const submitResult = await node.submit(transfer, alice, {
      waitForInclusion: true,
      policy: { maxFeeRao: 10_000_000n, maxSpendRao: 1n },
    })
    assert.match(submitResult.status, /^(inBlock|finalized)$/)
    assertHash(submitResult.extrinsicHash, 'node submit hash')

    const nextNonce = await browser.peekNextIndex(alice.ss58Address)
    const detached = await browser.signExtrinsic(transfer, alice, { nonce: nextNonce, period: null })
    const watched = await browser.watchSigned(detached, {
      signerAddress: alice.ss58Address,
      timeoutMs: 60_000,
    })
    assertHash(watched.extrinsicHash, 'watched hash')
    const watchedResult = await watched.result
    assert.match(watchedResult.status, /^(inBlock|finalized)$/)
    await watched.unsubscribe()
  } finally {
    await closeAll(node, browser)
  }
})
