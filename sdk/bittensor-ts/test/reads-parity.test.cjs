const test = require('node:test')
const assert = require('node:assert/strict')
const { dispatchRead, READ_CATALOG, READ_HANDLERS } = require('../dist/reads')
const { Balance } = require('../dist/balance')
const emission = require('../dist/emission-weights')
const { fetchMetagraph, fetchCommitments } = require('../dist/metagraph')
const { projectLock, blocksUntilConviction } = require('../dist/reads/locks')

function view({ queries = {}, maps = {}, runtime = {}, constants = {}, block = 100, seconds = 12 } = {}) {
  const calls = []
  const find = (table, descriptor, params = []) => {
    const key = descriptor.join('.')
    calls.push([key, params])
    const value = table[key]
    if (value instanceof Error) throw value
    return typeof value === 'function' ? value(params) : value
  }
  const pinned = {
    block, calls, at: async () => pinned,
    query: async (d, p) => find(queries, d, p),
    queryMap: async (d, p) => find(maps, d, p) ?? [],
    queryBatch: async (d, sets) => Promise.all(sets.map(p => find(queries, d, p))),
    runtime: async (d, p) => find(runtime, d, p),
    constant: async d => find(constants, d),
    balance: (amount, netuid = 0, symbol) => Balance.fromRao(amount, netuid, symbol),
    timestamp: async () => new Date('2026-10-01T12:00:00Z'),
    blockTime: async () => seconds,
    isFastBlocks: async () => seconds < 1,
    blockInfo: async n => ({ number: n, hash: '0x123', timestamp: new Date(0), extrinsics: [{ call: { call_module: 'System', call_function: 'remark' } }], explorerUrl: 'https://example.test' }),
  }
  return pinned
}

test('105 Python read contracts have concrete typed handlers and validate parameters', async () => {
  assert.equal(READ_CATALOG.length, 105)
  assert.deepEqual(READ_CATALOG.map(s => s.name).sort(), Object.keys(READ_HANDLERS).sort())
  await assert.rejects(dispatchRead(view(), 'does_not_exist'), /unknown read/)
  await assert.rejects(dispatchRead(view(), 'stake', {}), /missing parameter/)
  await assert.rejects(dispatchRead(view(), 'burn', { netuid: -1 }), /safe integer/)
  await assert.rejects(dispatchRead(view(), 'burn', { netuid: 2, typo: 1 }), /unknown parameter/)
})

test('all query dependencies execute on the single snapshot acquired by dispatch', async () => {
  const pinned = view({ queries: { 'SubtensorModule.ColdkeySwapAnnouncements': [120, '0xabc'], 'SubtensorModule.ColdkeySwapDisputes': 0 } })
  let snapshots = 0
  const head = { at: async () => { snapshots++; return pinned }, query: () => { throw new Error('unpinned read') } }
  const result = await dispatchRead(head, 'coldkey_swap_announcement', { coldkey_ss58: 'cold' })
  assert.equal(snapshots, 1)
  assert.equal(result.recovery_locked, true)
  assert.equal(result.execute_block, 120)
  assert.equal(result.disputed, false)
})

test('lifecycle reads retain dissolving subnets and per-subnet emission state', async () => {
  const v = view({ maps: { 'SubtensorModule.SubnetState': [[2, 'Started'], [4, { Dissolving: null }]], 'SubtensorModule.Tempo': [[2, 360]], 'SubtensorModule.Burn': [[2, 9007199254740995n]], 'SubtensorModule.SubnetworkN': [[2, 42]], 'SubtensorModule.SubnetEmissionEnabled': [[2, false]] } })
  const rows = await dispatchRead(v, 'subnets')
  assert.deepEqual(rows.map(r => [r.netuid, r.state, r.pool_emission_enabled]), [[2, 'started', false], [4, 'dissolving', true]])
  assert.equal(rows[0].burn.rao, 9007199254740995n)
})

test('subnet start schedule uses the current delay instead of its initial default', async () => {
  const v = view({
    block: 150,
    queries: { 'SubtensorModule.NetworkRegisteredAt': 100, 'SubtensorModule.StartCallDelay': 80 },
    constants: { 'SubtensorModule.InitialStartCallDelay': 20 },
  })
  assert.deepEqual(await dispatchRead(v, 'subnet_start_schedule', { netuid: 2 }), {
    netuid: 2, registered_at: 100, start_call_delay: 80, earliest_start_block: 180, current_block: 150,
  })
})

test('legacy lifecycle fallback is limited to absent metadata; network errors surface', async () => {
  const v = view({ queries: { 'SubtensorModule.SubnetState': new Error('storage item SubnetState not found'), 'SubtensorModule.NetworksAdded': true, 'SubtensorModule.FirstEmissionBlockNumber': 0 } })
  assert.equal(await dispatchRead(v, 'subnet_state', { netuid: 2 }), 'started')
  const broken = view({ queries: { 'SubtensorModule.SubnetState': new Error('connection timed out') } })
  await assert.rejects(dispatchRead(broken, 'subnet_state', { netuid: 2 }), /timed out/)
})

test('stake valuation keeps exact root and alpha rao past JS integer precision', async () => {
  const amount = 9007199254740997n
  const v = view({ maps: { 'SubtensorModule.SubnetState': [[0, 'Started'], [2, 'Started']] }, runtime: {
    'StakeInfoRuntimeApi.get_stake_info_for_coldkeys': [['cold', [{ hotkey: 'hot', coldkey: 'cold', netuid: 0, stake: amount, is_registered: true }, { hotkey: 'hot', coldkey: 'cold', netuid: 2, stake: amount, is_registered: true }]]],
    'SwapRuntimeApi.current_alpha_price_all': [{ netuid: 2, price: 1234567890n }],
  } })
  const result = await dispatchRead(v, 'stake_value_for_coldkeys', { coldkey_ss58s: ['cold', 'empty'] })
  assert.equal(result.cold.stake_value.rao, amount + amount * 1234567890n / 1000000000n)
  assert.equal(result.cold.positions[1].stake.netuid, 2)
  assert.equal(result.cold.positions[1].subnet_state, 'started')
  assert.equal(result.empty.stake_value.rao, 0n)
})

test('stake availability sorts and deduplicates IDs and preserves locked denomination', async () => {
  const v = view({ runtime: { 'StakeInfoRuntimeApi.get_stake_availability_for_coldkeys': ([keys, ids]) => {
    assert.deepEqual(ids, [0, 2]); return { cold: { 2: { total: 10n, locked: 6n, available: 4n } } }
  } } })
  const result = await dispatchRead(v, 'stake_availability_for_coldkey', { coldkey_ss58: 'cold', netuids: [2, 0, 2] })
  assert.deepEqual(result.map(r => [r.netuid, r.locked.rao]), [[0, 0n], [2, 6n]])
  assert.equal(result[1].available.netuid, 2)
})

test('swap quote converts decimal user amounts to exact rao and tags every side', async () => {
  const v = view({ runtime: { 'SwapRuntimeApi.sim_swap_tao_for_alpha': ([netuid, amount]) => { assert.equal(netuid, 2); assert.equal(amount, 1234567891n); return { tao_amount: amount, alpha_amount: 99n, tao_fee: 2n, alpha_fee: 3n, tao_slippage: 4n, alpha_slippage: 5n } } } })
  const result = await dispatchRead(v, 'quote_stake', { netuid: 2, amount_tao: '1.234567891' })
  assert.equal(result.alpha_fee.netuid, 2)
  assert.equal(result.tao_slippage.rao, 4n)
})

test('epoch timing uses chain next-start API and detected block seconds', async () => {
  const v = view({ block: 100, seconds: .25, queries: { 'SubtensorModule.Tempo': 360, 'SubtensorModule.LastEpochBlock': 95, 'SubtensorModule.BlocksSinceLastStep': 5, 'SubtensorModule.PendingEpochAt': 110, 'SubtensorModule.SubnetEpochIndex': 8 }, runtime: { 'SubnetInfoRuntimeApi.get_next_epoch_start_block': 110 } })
  const result = await dispatchRead(v, 'epoch_status', { netuid: 2 })
  assert.equal(result.blocks_remaining, 10)
  assert.equal(result.seconds_remaining, 2.5)
  assert.equal(result.pending_epoch_at, 110)
})

test('hyperparameter flattening preserves u64 fixed bits and signed i32 integer values', async () => {
  const v = view({ runtime: { 'SubnetInfoRuntimeApi.get_subnet_hyperparams_v3': [{ name: 'alpha_sigmoid_steepness', value: { I32F32: { bits: -3n << 32n } } }, { name: 'burn_increase_mult', value: { U64F64: { bits: 2n << 64n } } }] } })
  assert.deepEqual(await dispatchRead(v, 'subnet_hyperparameters', { netuid: 2 }), { alpha_sigmoid_steepness: -3n, burn_increase_mult: 2n << 64n })
})

test('root claim threshold reads the configured root map entry', async () => {
  const threshold = 9007199254740997n
  const v = view({ queries: {
    'SubtensorModule.RootClaimableThreshold': params => {
      assert.deepEqual(params, [0])
      return { bits: threshold << 32n }
    },
  } })
  const result = await dispatchRead(v, 'root_claim_threshold')
  assert.equal(result.rao, threshold)
  assert.equal(result.netuid, 0)
  assert.deepEqual(v.calls, [['SubtensorModule.RootClaimableThreshold', [0]]])
})

test('root basket paging joins canonical pricing and rejects a nonadvancing cursor', async () => {
  const v = view({ queries: { 'SubtensorModule.BasketRate': { bits: 1n << 31n } }, runtime: {
    'BetaBasketRuntimeApi.get_all_validator_baskets': [{ hotkey: 'a', nav_tao: 20n, shares: 10n, deposited_tao: 5n, redeemed_tao: 1n }],
    'BetaBasketRuntimeApi.get_all_beta_pricing': ([cursor]) => cursor == null ? { pricing: [], next: 'a' } : { pricing: [{ hotkey: 'a', stake_price: { bits: 2n << 64n } }], next: null },
  } })
  const result = await dispatchRead(v, 'root_baskets')
  assert.equal(result[0].basket_rate, .5)
  assert.equal(result[0].chain_pricing.stake_price, 2)
  assert.equal(result[0].lifetime_return, 4.2)
  const bad = view({ runtime: { 'BetaBasketRuntimeApi.get_all_validator_baskets': [{ hotkey: 'a' }], 'BetaBasketRuntimeApi.get_all_beta_pricing': { pricing: [], next: 'same' } } })
  await assert.rejects(dispatchRead(bad, 'root_baskets'), /pagination did not advance/)
})

test('basket turnover budget clamps available and computes refill in bigint', async () => {
  const v = view({ runtime: { 'BetaBasketRuntimeApi.get_basket_trading_status': { enabled: true, frozen: false, tao_available: 100n, budget_tao: 90n, refill_blocks: 7 } } })
  const result = await dispatchRead(v, 'basket_trading_status', { hotkey_ss58: 'a' })
  assert.equal(result.remaining_tao.rao, 90n)
  assert.equal(result.used_tao.rao, 0n)
  assert.equal(result.refill_per_block_tao.rao, 12n)
})

test('collateral reports exact headroom and needed work and survives deregistration', async () => {
  const v = view({ queries: { 'SubtensorModule.Owner': 'cold', 'SubtensorModule.MinerCollateral': { locked: 9007199254740997n, min_locked: 7n, earned: 2n, drain_ratio: { bits: 1n << 63n } } } })
  const result = await dispatchRead(v, 'miner_collateral', { netuid: 2, hotkey_ss58: 'hot' })
  assert.equal(result.coldkey, 'cold')
  assert.equal(result.headroom_alpha.rao, 9007199254740990n)
  assert.equal(result.releasable_work_alpha.rao, 18014398509481980n)
})

test('conviction projection honors strict >18% ownership gate and owner locks', () => {
  assert.equal(blocksUntilConviction([[18, 18, true, true]], 100, 100, 100), null)
  assert.equal(blocksUntilConviction([[19, 19, true, true]], 100, 100, 100), 0)
  assert.ok(blocksUntilConviction([[30, 0, true, false]], 100, 100, 100) > 0)
  const [mass, conviction] = projectLock(100, 0, 100, 100, 100, false, true)
  assert.equal(mass, conviction)
})

test('subnet conviction subtracts protocol and burned alpha and does not aggregate takeover votes', async () => {
  const v = view({ queries: { 'SubtensorModule.SubnetAlphaOut': 1000n, 'SubtensorModule.SubnetProtocolAlpha': 100n, 'AlphaAssets.AlphaBurned': 100n, 'SubtensorModule.NetworkRegisteredAt': 10 }, maps: { 'SubtensorModule.HotkeyLock': [['a', { locked_mass: 100n, conviction: { bits: 100n << 64n }, last_update: 100 }], ['b', { locked_mass: 100n, conviction: { bits: 100n << 64n }, last_update: 100 }]] } })
  const result = await dispatchRead(v, 'subnet_convictions', { netuid: 2 })
  assert.equal(result.eligible_alpha.rao, 800n)
  assert.equal(result.threshold_alpha.rao, 144n)
  assert.equal(result.total_conviction_alpha.rao, 200n)
  assert.equal(result.leader_blocks_to_threshold, null)
  assert.equal(result.ownership_changeable_at_block, 2629810)
})

test('commitments expose sealed, failed, and revealed-only history at a pinned block', async () => {
  const v = view({ seconds: .25, maps: {
    'Commitments.CommitmentOf': [['sealed', { block: 90, info: { fields: [{ TimelockEncrypted: { reveal_round: 2 } }] } }], ['failed', { block: 91, info: { fields: [{ TimelockRevealFailed: { reveal_round: 2 } }] } }]],
    'Commitments.RevealedCommitments': [['revealed', [['0x086869', 95]]]],
    'SubtensorModule.Uids': [['sealed', 2]],
  } })
  const records = await fetchCommitments(v, 2)
  assert.deepEqual(Object.keys(records), ['revealed', 'failed', 'sealed'])
  assert.equal(records.sealed.duration, 2.5)
  assert.equal(records.sealed.uid, 2)
  assert.equal(records.sealed.status, 'sealed')
  assert.equal(records.failed.status, 'failed')
  assert.equal(records.failed.is_revealed, false)
  assert.equal(records.revealed.value, 'hi')
  assert.equal(records.revealed.status, 'revealed')
})

test('metagraph uses the graph token symbol, normalizes scores and distinguishes unsupported collateral', async () => {
  const graph = { netuid: 2, symbol: [...new TextEncoder().encode('र')], name: [65], block: 100, tempo: 360, hotkeys: ['hot'], coldkeys: ['cold'], rank: [32767], emission: [9007199254740997n], tao_stake: [12n], alpha_in: 2n, tao_in: 3n, axons: [{ ip: 1n, ip_type: 6, port: 80 }] }
  const v = view({ runtime: { 'SubnetInfoRuntimeApi.get_metagraph': graph } })
  const result = await fetchMetagraph(v, 2, { commitments: false })
  assert.equal(result.symbol, 'र')
  assert.equal(result.neuron(0).emission.symbol, 'र')
  assert.equal(result.neuron(0).emission.rao, 9007199254740997n)
  assert.equal(result.neuron(0).collateral_locked, null)
  assert.equal(result.neuron(0).rank, 32767 / 65535)
  assert.equal(result.neuron(0).axon, '[::1]:80')
  assert.equal(result.price, 1.5)
  assert.equal(result.raw, graph)
})

test('weights use mechanism index and row normalization while bonds use u16 fractions', async () => {
  const v = view({ maps: { 'SubtensorModule.Weights': ([id]) => { assert.equal(id, 4098); return [[4, [[1, 2], [2, 6]]]] }, 'SubtensorModule.Bonds': [[4, [[1, 32767]]]] } })
  assert.deepEqual(await dispatchRead(v, 'weights', { netuid: 2, mechid: 1 }), { 4: { 1: .25, 2: .75 } })
  assert.deepEqual(await dispatchRead(v, 'bonds', { netuid: 2 }), { 4: { 1: 32767 / 65535 } })
})

test('emission shares parse exact decimal percentages and preserve root floor to one part', () => {
  assert.equal(emission.parseShare('50%'), emission.ONE / 2n)
  assert.equal(emission.parseShare('0.50'), emission.ONE / 2n)
  assert.equal(emission.parseShare('1'), 1n)
  assert.equal(emission.parseShare('100%'), emission.ONE)
  const current = emission.defaultWeights()
  assert.deepEqual(emission.validate(current), [])
  const changed = emission.renormalize(current, { owner: emission.parseShare('20%') })
  assert.equal(changed.reduce((a, b) => a + b), emission.ONE)
  assert.equal(emission.minValidatorParts(123n), 124n)
  assert.ok(emission.validate(current, { min_validator: current[3] + 1n }).some(v => v.error === 'ValidatorEmissionTooLowToPreserveRoot'))
  assert.throws(() => emission.parseShare('101%'), /between zero and one/)
})

test('fee payer helper sees wrapper leaves, allows repeated calls, and rejects actual cycles', () => {
  const { chargesColdkeyFee, iterCallLeaves } = require('../dist/fee-filters')
  const hotkeyCall = ['SubtensorModule', 'set_mechanism_weights', {}]
  const batch = ['Utility', 'batch_all', { calls: [hotkeyCall, hotkeyCall] }]
  assert.equal(chargesColdkeyFee(['Proxy', 'proxy', { call: batch }]), true)
  assert.equal([...iterCallLeaves(batch)].length, 2)
  assert.equal(chargesColdkeyFee(['Balances', 'transfer_keep_alive', {}]), false)
  const cycle = ['Utility', 'batch', { calls: [] }]
  cycle[2].calls.push(cycle)
  assert.throws(() => [...iterCallLeaves(cycle)], /cyclic/)
})

test('read address coercion uses public wallet views without touching private keys', async () => {
  const wallet = {
    coldkeypub: Promise.resolve({ ss58Address: 'cold' }),
    get coldkey() { throw new Error('must not unlock private key') },
  }
  const v = view({ queries: { 'System.Account': ([address]) => { assert.equal(address, 'cold'); return { data: { free: 9n } } } } })
  assert.equal((await dispatchRead(v, 'balance', { coldkey_ss58: wallet })).rao, 9n)
})

test('Node and snapshot namespaces expose snake and camel methods at the original hash', async () => {
  const { BrowserChainClient, Snapshot } = require('../dist/client')
  const client = new BrowserChainClient('ws://127.0.0.1:9944', { autoConnect: false })
  const pinnedHash = '0x' + 'ab'.repeat(32)
  client.finalizedHead = async () => pinnedHash
  client.blockNumber = async hash => { assert.equal(hash, pinnedHash); return 100 }
  client.blockHash = async () => { throw new Error('must not re-resolve pinned hash') }
  client.query = async (descriptor, params, hash) => { assert.equal(hash, pinnedHash); assert.deepEqual(descriptor, ['SubtensorModule', 'Owner']); return 'owner' }
  const snapshot = await client.at()
  assert.equal(snapshot instanceof Snapshot, true)
  assert.equal(await snapshot.at(), snapshot)
  assert.equal(await client.neurons.hotkeyOwner('hot'), 'owner')
  assert.equal(await snapshot.neurons.hotkey_owner('hot'), 'owner')
  assert.equal(client.reads().length, 105)
  await client.close()
})

test('generated catalogs expose every metadata category and exact call parameter names', () => {
  const descriptors = require('../dist/descriptors')
  const count = catalog => Object.values(catalog).reduce((sum, value) => sum + Object.keys(value).length, 0)
  assert.equal(count(descriptors.storage), 417)
  assert.deepEqual(descriptors.storage.LimitOrders.ApprovedOrders, ['LimitOrders', 'ApprovedOrders'])
  assert.equal(descriptors.storage.LimitOrders.OrderNonces, undefined)
  assert.equal(count(descriptors.constants), 142)
  assert.equal(count(descriptors.runtimeApi), 125)
  assert.equal(count(descriptors.calls), 334)
  assert.deepEqual(descriptors.calls.SubtensorModule.announce_coldkey_swap('0x1234'), ['SubtensorModule', 'announce_coldkey_swap', { new_coldkey_hash: '0x1234' }])
})

test('Node detached signing keeps the native plan authoritative and rejects forged or cross-client plans', async () => {
  const { Client } = require('../dist/client')
  const signer = require('../dist/keys').Keypair.fromUri('//Alice')
  const client = new Client('local'), other = new Client('local')
  let submitted = 0
  const native = { submitExternal: async (plan, signature) => { assert.equal(plan, handle); assert.equal(signature.length, 64); submitted++; return { success: true, message: 'submitted', extrinsicHash: '0xabc', events: [] } } }
  const handle = { publicKey: Buffer.alloc(32), cryptoType: 1, callData: Buffer.from([1]), payload: Buffer.from([2]), includedInExtrinsic: Buffer.from([3]), includedInSignedData: Buffer.from([4]), txParams: { era: '00', nonce: 7, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32) } }
  client.requireNativeClient = async () => native
  client.runtimeAt = async () => ({ ss58Format: 42 })
  client.nativeSigningPlan = async () => ({ native, runtime: {}, resolved: { ss58Address: signer.ss58Address, publicKey: signer.publicKey, cryptoType: 1 }, plan: handle })
  const plan = await client.prepareCall(Buffer.from([1]), { ss58Address: signer.ss58Address, publicKey: signer.publicKey, sign() { throw new Error('must not sign during prepare') } })
  assert.deepEqual(plan.payload, Buffer.from([2])); assert.equal(plan.txParams.nonce, 7)
  await assert.rejects(client.submitSignature({ ...plan }, Buffer.alloc(64)), /prepared by this client/)
  await assert.rejects(other.submitSignature(plan, Buffer.alloc(64)), /prepared by this client/)
  plan.callData[0] = 9
  assert.equal(handle.callData[0], 1)
  await client.submitSignature(plan, Buffer.alloc(64)); assert.equal(submitted, 1)
})

test('Node fee previews pass explicit nonce and cannot reserve the next transaction nonce', async () => {
  const { Client } = require('../dist/client')
  const client = new Client('local'), seen = []
  client.runtimeAt = async () => ({})
  client.resolveSigner = async () => ({ ss58Address: 'alice' })
  client.requireNativeClient = async () => ({ accountNextIndex: async () => 9 })
  client.nativeSigningPlan = async (call, signer, options) => { seen.push(options.nonce); return { plan: { feeRao: '12' } } }
  assert.equal((await client.estimateFee({}, {})).rao, 12n)
  assert.deepEqual(seen, [9])
})

test('epoch waits observe actual LastEpochBlock and timeout closes the subscription', async () => {
  const { BrowserChainClient } = require('../dist/client')
  const client = new BrowserChainClient('local'), read = []; let closed = 0
  client.at = async () => ({ query: async ([, name]) => name === 'Tempo' ? 10 : 100 })
  client.query = async (descriptor, params, block) => { read.push(block); return block === 102 ? 101 : 100 }
  client.transport.subscribe = async () => ({ async *[Symbol.asyncIterator]() { yield {number:'0x65'};yield {number:'0x66'} }, unsubscribe: async () => {closed++} })
  assert.deepEqual(await client.waitForEpoch(2), {netuid:2,block:101,previous:100})
  assert.deepEqual(read,[101,102]);assert.equal(closed,1)
  let end
  client.transport.subscribe = async () => ({ [Symbol.asyncIterator]() { return {next:()=>new Promise(resolve=>{end=resolve})} }, unsubscribe: async () => {closed++;end?.({done:true})} })
  await assert.rejects(client.waitForBlock(10,{timeoutMs:5}),/timed out/)
  assert.equal(closed,2)
})

test('legacy JS mortal signing anchors the parent of the best head, including at genesis', async () => {
  const { BrowserChainClient } = require('../dist/client')
  const client = new BrowserChainClient('local'), seen = []
  client.blockHash = async number => { seen.push(number); return number == null ? 'best' : `block-${number}` }
  client.finalizedHead = async () => { throw new Error('must not anchor short mortality to a lagging finalized head') }
  client.blockNumber = async hash => { assert.equal(hash, 'best'); return 100 }
  client.runtimeAt = async hash => { assert.equal(hash, 'best'); return {} }
  client.genesisHash = async () => 'genesis'
  const snapshot = await client.signingSnapshot()
  assert.deepEqual(await client.normalizeEra(8, snapshot), { era: { period: 8, current: 99 }, eraBlockHash: 'block-99' })
  assert.deepEqual(await client.normalizeEra(8, { ...snapshot, blockNumber: 0 }), { era: { period: 8, current: 0 }, eraBlockHash: 'block-0' })
  assert.deepEqual(seen, [undefined, 99, 0])
  await client.close()
})
