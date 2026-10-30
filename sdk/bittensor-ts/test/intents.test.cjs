const test = require('node:test')
const assert = require('node:assert/strict')
const { buildIntent, listIntentTools, checkIntentPolicy, intentFromJSON, isPreparedIntent, encodeIntentCall } = require('../dist/intents.js')
const { Balance } = require('../dist/balance.js')
const { hyperparameterToRaw, OWNER_HYPERPARAMETERS } = require('../dist/intent-hyperparameters.js')
const { defaultWeights, ONE } = require('../dist/emission-weights.js')
const { IntentCall } = require('../dist/transaction.js')

const context = { coldkey: 'coldkey', hotkey: 'hotkey', hotkeyPublicKey: new Uint8Array(32), feePayer: 'coldkey' }
function reader(overrides = {}) {
  return {
    async query(pallet, item, params) {
      switch (item) {
        case 'Account': return { data: { free: 100_000_000_000n } }
        case 'StakingHotkeys': return ['hotkey']
        case 'BasketClaimed': return -1n
        case 'MinerCollateral': return { locked: 1n }
        case 'Delegates': return 0
        case 'RootStakeUnlockInterval': return 0
        case 'RootClaimableThreshold': return 500000n << 32n
        case 'SubnetOwner': return 'coldkey'
        case 'LastRateLimitedBlock': return 0
        case 'AlphaValues': return [1000, 50000]
        case 'Uids': return 0
        case 'CommitRevealWeightsEnabled': return false
        case 'WeightsSetRateLimit': return 0
        case 'LastUpdate': return [0, 0]
        case 'MinAllowedWeights': return 0
        case 'MaxWeightsLimit': return 65535
        case 'SubnetEpochConsensus': return 'Yuma'
        case 'Tempo': return 360
        case 'RevealPeriodEpochs': return 1
        case 'LastEpochBlock': return 0
        case 'PendingEpochAt': return 0
        case 'SubnetEpochIndex': return 0
        case 'BlocksSinceLastStep': return 1
        default: throw new Error(`unexpected query ${pallet}.${item}`)
      }
    },
    async runtimeCall(api, method) {
      switch (method) {
        case 'current_alpha_price': return 2_000_000_000n
        case 'get_stake_info_for_hotkey_coldkey_netuid': return { stake: 10_000_000_000n }
        case 'get_stake_info_for_coldkey': return [{ hotkey: 'hotkey', netuid: 0, stake: 10n }]
        case 'get_validator_basket': return [[1, 10_000_000_000n, 20_000_000_000n]]
        case 'get_basket_claim_preview': return { accrued_tao: 10_000_000n, redeemable_tao: 10_000_000n, forfeited_tao_est: 0n, rows: 1, rows_to_sell: 1, dust_rows: 0, swept: 0 }
        case 'get_root_basket_claim_previews': return [{ hotkey: 'hotkey', accrued_tao: 10_000_000n, redeemable_tao: 10_000_000n, forfeited_tao_est: 0n, rows: 1, rows_to_sell: 1, dust_rows: 0, swept: 0 }]
        case 'get_stake_availability_for_coldkeys': return { coldkey: { 1: { available: 5_000_000_000n } } }
        case 'get_subnet_emission_weights': return { weights: defaultWeights(), root_baseline_share: 1n, created_via_lease: false }
        default: throw new Error(`unexpected runtime call ${api}.${method}`)
      }
    },
    constant: async (_, name) => name === 'SubtensorPalletId' ? new TextEncoder().encode('subtensr') : 1000n,
    queryMap: async () => [],
    countStorageKeys: async (_, item) => item === 'AlphaV2' ? 1 : 0,
    blockNumber: async () => 100,
    blockHash: async () => '0x' + '00'.repeat(32),
    blockTime: async () => 12,
    estimateFee: async () => 1000n,
    estimateWeight: async () => ({ ref_time: 1000n, proof_size: 0n }),
    decodeAddress: address => Uint8Array.from({ length: 32 }, () => address === 'coldkey' ? 0 : address === 'other' ? 2 : 1),
    encodeAddress: () => 'escrow',
    multisigAccount: () => ({ address: 'multisig', publicKey: new Uint8Array(32) }),
    hashCall: async () => new Uint8Array(32),
    evmMirror: () => 'mirror',
    encryptWeightsV2: async () => ({ commit: new Uint8Array([1, 2, 3]), revealRound: 123n }),
    ...overrides,
  }
}

const transfer = { op: 'transfer', dest_ss58: 'dest', amount_tao: '1.25' }
const order = { V1: { signer:'coldkey',hotkey:'hotkey',fee_recipient:'other',netuid:1,order_type:'LimitBuy',amount:1n,limit_price:1n,expiry:1000n,fee_rate:0,relayer:null,max_slippage:null,chain_id:1n,partial_fills_enabled:false } }
function sample(tool) {
  const args = {}
  for (const name of tool.input_schema.required ?? []) {
    const schema = tool.input_schema.properties[name]
    args[name] = tool.money_fields.includes(name) ? '1.25' : schema.type === 'integer' ? 1 : schema.type === 'boolean' ? true : schema.type === 'array' ? [1] : schema.type === 'object' ? {} : 'value'
  }
  for (const name of Object.keys(args)) if (name.includes('hotkey')) args[name] = name.startsWith('dest') || name.startsWith('new') ? 'other' : 'hotkey'
  for (const name of Object.keys(args)) if (name.includes('coldkey') || name === 'delegate_ss58' || name === 'spawner_ss58') args[name] = 'other'
  if ('dest_netuid' in args) args.dest_netuid = 2
  if ('ip' in args) args.ip = '::1'
  if ('evm_key' in args) args.evm_key = '0x' + '11'.repeat(20)
  if ('evm_address' in args) args.evm_address = '0x' + '11'.repeat(20)
  if ('signature' in args) args.signature = '0x' + '11'.repeat(65)
  if ('certificate' in args) args.certificate = '0x0011'
  if ('work_hex' in args) args.work_hex = '0x' + '11'.repeat(32)
  if ('other_signatories' in args) args.other_signatories = ['other']
  if ('threshold' in args && !tool.money_fields.includes('threshold')) args.threshold = 2
  if ('call' in args) args.call = transfer
  if ('timepoint' in args) args.timepoint = { height: 1, index: 0 }
  if ('take' in args) args.take = '0.1'
  switch (tool.name) {
    case 'authorize_order': case 'cancel_order': args.order = order; args.nonce = 1; break
    case 'execute_orders': case 'execute_batched_orders': args.orders = [{order:{Authorized:{nonce:1n,terms:order}},signature:'AccountApproved',partial_fill:null}]; args.should_fail = true; break
    case 'create_crowdloan': args.target_ss58 = 'target'; break
    case 'execute_proxy_announced': args.real_ss58 = 'real'; args.inner_op = 'transfer'; args.inner_args = { dest_ss58: 'dest', amount_tao: '1' }; break
    case 'batch': args.intents = [transfer]; break
    case 'set_children': args.children = [['0.5', 'hotkey']]; break
    case 'set_hyperparameter': args.name = 'tempo'; args.value = 360; break
    case 'set_subnet_emission_weights': args.slots = ['owner']; args.values = ['0.18']; args.renormalize = true; break
    case 'swap_basket_many': args.legs = [{ origin_netuid: 1, dest_netuid: 2, amount: '1' }]; break
    case 'set_weights': case 'commit_weights': case 'reveal_weights': args.uids = [0, 1]; args.weights = [0.5, 1]; if (tool.name === 'reveal_weights') args.salt = [1, 2]; break
  }
  return args
}

for (const tool of listIntentTools()) {
  test(`Python semantic catalog operation ${tool.name} prepares a call`, async () => {
    const intent = buildIntent(tool.name, sample(tool))
    const plan = await intent.prepare(reader(), context)
    assert.equal(isPreparedIntent(plan), true)
    assert.ok(plan.call.pallet)
    assert.ok(plan.call.call)
    assert.ok(Object.isFrozen(plan))
    assert.equal(plan.violations.length, 0, plan.violations.join('; '))
    assert.deepEqual(intentFromJSON(intent.toJSON()).toJSON(), intent.toJSON())
    const encoded = await encodeIntentCall(plan.call, async (_, __, params) => {
      for (const value of Object.values(params)) if (value && typeof value === 'object' && value.pallet) assert.fail('nested descriptors must be encoded before the outer call')
      return new Uint8Array([1])
    })
    assert.deepEqual(encoded, new Uint8Array([1]))
    // Every semantic leaf must be admitted by the fixed native registry. This
    // catches stale argument names and targets independently of the JS facade.
    function validateLeaves(call) {
      const nested = [call.params.call, ...(call.params.calls ?? []), call.semanticCall].filter(value => value && typeof value === 'object' && value.pallet)
      if (nested.length) nested.forEach(validateLeaves)
      else if (call.pallet !== 'Crowdloan' || call.call !== 'create') assert.doesNotThrow(() => IntentCall.operation(`${call.pallet}.${call.call}`, call.params))
    }
    validateLeaves(plan.call)
  })
}

test('dynamic baskets and optional root claims cannot bypass a subnet allowlist', async () => {
  for (const op of ['stake_into_basket', 'claim_root_with_hotkey']) {
    const plan = await buildIntent(op, { hotkey_ss58: 'hotkey', ...(op === 'stake_into_basket' ? { amount_tao: '1' } : {}) }).prepare(reader(), context)
    assert.equal(plan.affectsAllSubnets, true)
    assert.match(checkIntentPolicy(plan, { allowedNetuids: [1] }).join(), /all subnets/)
  }
  const explicit = await buildIntent('claim_root', { subnets: [1, 7] }).prepare(reader(), context)
  assert.deepEqual(explicit.netuids, [])
  assert.match(checkIntentPolicy(explicit, { allowedNetuids: [1] }).join(), /all subnets/)
})

test('exact money and default slippage survive preparation without floating point loss', async () => {
  const amount = '9007199.254740993'
  const plan = await buildIntent('add_stake', { hotkey_ss58: 'hotkey', netuid: 1, amount_tao: amount }).prepare(reader(), context)
  assert.equal(plan.call.call, 'add_stake_limit')
  assert.equal(plan.call.params.amount_staked, 9007199254740993n)
  assert.equal(plan.call.params.limit_price, 2100000000n)
  assert.equal(plan.call.params.allow_partial, false)
  assert.equal(plan.spendRao, 9007199254740993n)
  assert.throws(() => buildIntent('add_stake', { hotkey_ss58: 'hotkey', netuid: 1, amount_tao: Balance.fromAlpha('1', 1) }), /netuid 0/)
})

test('proportion rounding and scientific-notation slippage preserve the requested value', async () => {
  for (const take of [0.1, '0.1', '1e-1', '10%']) {
    const plan = await buildIntent('set_take', { take }).prepare(reader(), context)
    assert.equal(plan.call.params.take, 6554)
  }
  const raw = await buildIntent('set_take', { take: ' +6553 ' }).prepare(reader(), context)
  assert.equal(raw.call.params.take, 6553)
  const tiny = await buildIntent('remove_stake', { hotkey_ss58: 'hotkey', netuid: 1, amount_alpha: '1', rate_tolerance: 1e-10 }).prepare(reader(), context)
  assert.equal(tiny.call.params.limit_price, 1999999999n)
})

test('draining all remains unbounded after state resolution', async () => {
  const plan = await buildIntent('add_stake', { hotkey_ss58: 'hotkey', netuid: 1, amount_tao: 'all' }).prepare(reader(), context)
  assert.equal(plan.call.params.amount_staked, 99_999_499_000n)
  assert.equal(plan.spendRao, 'unbounded')
  assert.match(checkIntentPolicy(plan, { maxSpendRao: 999999999999n }).join(), /unbounded/)
})

test('root claim admission blocks the 130-unit envelope and failed state cannot sign', async () => {
  const overweight = reader({ countStorageKeys: async (_, item) => item === 'AlphaV2' ? 130 : 0 })
  const plan = await buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(overweight, context)
  assert.match(plan.violations.join(), /130-unit/)
  await assert.rejects(buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(reader({ countStorageKeys: async () => { throw new Error('storage keys unavailable') } }), context), /storage keys unavailable/)
  await assert.rejects(buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(reader({ countStorageKeys: async () => -1 }), context), /count/)
})

test('root claim admission includes claim-only swap credits, retired rows and queued flush work', async () => {
  const claims = await buildIntent('claim_root').prepare(reader({ queryMap: async () => Array.from({ length: 257 }, (_, index) => [`credit${index}`, null]) }), context)
  assert.match(claims.violations.join(), /selection scans/)
  const retired = await buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(reader({ countStorageKeys: async (_, item) => item === 'Alpha' ? 129 : item === 'AlphaV2' ? 1 : 0 }), context)
  assert.match(retired.violations.join(), /130-unit/)
  const flush = await buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(reader({ countStorageKeys: async (_, item) => item === 'PendingBasketDeposits' ? 513 : 0 }), context)
  assert.match(flush.violations.join(), /512-row/)
})

test('root claim reserves use the actual sponsored fee account while retaining signer identity', async () => {
  const base = reader()
  const view = reader({
    resolveFeePayer: async () => 'sponsor',
    query: async (pallet, item, params) => item === 'Account' ? { data: { free: params[0] === 'sponsor' ? 100000n : 0n } } : base.query(pallet, item, params),
  })
  const plan = await buildIntent('claim_root_with_hotkey', { hotkey_ss58: 'hotkey' }).prepare(view, { ...context, feePayer: 'delegate' })
  assert.equal(plan.feePayer, 'delegate')
  assert.equal(plan.requiredFreeAccount, 'sponsor')
  assert.equal(plan.availableFreeRao, 100000n)
  assert.deepEqual(plan.violations, [])
})

test('claim and full root unstake use the post-claim u64 cap atomically', async () => {
  const plan = await buildIntent('remove_stake', { hotkey_ss58: 'hotkey', netuid: 0, amount_alpha: 'all', claim: true }).prepare(reader(), context)
  assert.equal(plan.call.call, 'batch_all')
  assert.equal(plan.call.params.calls[0].call, 'claim_root_with_hotkey')
  assert.equal(plan.call.params.calls[1].call, 'remove_stake')
  assert.equal(plan.call.params.calls[1].params.amount_unstaked, (1n << 64n) - 1n)
})

test('batch and multisig preserve bounded spend, subnet scope, and mandatory shielding', async () => {
  const nested = { op: 'add_collateral', netuid: 7, hotkey_ss58: 'hotkey', amount_alpha: '1' }
  const batch = await buildIntent('batch', { intents: [nested] }).prepare(reader(), context)
  assert.equal(batch.mevShieldRequired, true)
  assert.equal(batch.spendRao, 'unbounded')
  assert.deepEqual(batch.netuids, [7])
  assert.match(checkIntentPolicy(batch, { allowedNetuids: [8] }).join(), /netuid 7/)
  const multisig = await buildIntent('multisig_execute', { threshold: 2, other_signatories: ['other'], call: nested }).prepare(reader(), context)
  assert.equal(multisig.mevShieldRequired, true)
  assert.equal(multisig.spendRao, 'unbounded')
  assert.deepEqual(multisig.netuids, [7])
  assert.throws(() => checkIntentPolicy({ ...batch }, {}), /SDK-prepared/)
})

test('batch preflight combines multisig opening deposits against one signer balance', async () => {
  const base = reader()
  const view = reader({ query: async (pallet, item, params) => item === 'Account' ? { data: { free: 5000n } } : base.query(pallet, item, params) })
  const approval = { op: 'multisig_execute', threshold: 2, other_signatories: ['other'], call: transfer }
  const single = await intentFromJSON(approval).prepare(view, context)
  assert.equal(single.violations.length, 0)
  const combined = await buildIntent('batch', { intents: [approval, { ...approval, call: { ...transfer, amount_tao: '2' } }] }).prepare(view, context)
  assert.equal(combined.requiredFreeRao, 8000n)
  assert.equal(combined.availableFreeRao, 5000n)
  assert.match(combined.violations.join(), /combined reserves/)
})

test('mixed proxy and multisig reserves retain each dispatch account and independent fee sponsor', async () => {
  const base = reader(), funds = { coldkey: 3000n, real: 3000n, sponsor: 2000n, delegate: 0n }
  const view = reader({
    decodeAddress: address => Uint8Array.from({ length: 32 }, (_, index) => address.charCodeAt(index) || 0),
    resolveFeePayer: async () => 'sponsor',
    query: async (pallet, item, params) => item === 'Account' ? { data: { free: funds[params[0]] ?? 100000000000n } } : base.query(pallet, item, params),
  })
  const approval = { op: 'multisig_execute', threshold: 2, other_signatories: ['other'], call: transfer }
  const proxy = { op: 'execute_proxy_announced', real_ss58: 'real', delegate_ss58: 'delegate', inner_op: 'multisig_execute', inner_args: approval }
  const plan = await buildIntent('batch', { intents: [approval, proxy] }).prepare(view, { ...context, feePayer: 'delegate' })
  assert.deepEqual(Object.fromEntries(plan.requiredFreeByAccount.map(item => [item.address, item.requiredRao])), { coldkey: 3000n, sponsor: 2000n, real: 3000n })
  assert.equal(plan.requiredFreeRao, undefined)
  assert.deepEqual(plan.violations, [])
  funds.real = 2999n
  const short = await buildIntent('batch', { intents: [approval, proxy] }).prepare(view, { ...context, feePayer: 'delegate' })
  assert.ok(short.violations.some(reason => reason.includes('multisig dispatch account')))
})

test('a sponsored root claim and multisig deposit share only the sponsor fee constraint', async () => {
  const base = reader(), funds = { coldkey: 3000n, sponsor: 2000n, delegate: 0n }
  const view = reader({
    decodeAddress: address => Uint8Array.from({ length: 32 }, (_, index) => address.charCodeAt(index) || 0),
    resolveFeePayer: async () => 'sponsor',
    query: async (pallet, item, params) => item === 'Account' ? { data: { free: funds[params[0]] ?? 100000000000n } } : base.query(pallet, item, params),
  })
  const plan = await buildIntent('batch', { intents: [
    { op: 'claim_root_with_hotkey', hotkey_ss58: 'hotkey' },
    { op: 'multisig_execute', threshold: 2, other_signatories: ['other'], call: transfer },
  ] }).prepare(view, { ...context, feePayer: 'delegate' })
  assert.deepEqual(Object.fromEntries(plan.requiredFreeByAccount.map(item => [item.address, item.requiredRao])), { sponsor: 2000n, coldkey: 3000n })
  assert.deepEqual(plan.violations, [])
})

test('wrapped and mixed-batch spends retain each actual dispatch account', async () => {
  const multi = { op: 'multisig_execute', threshold: 2, other_signatories: ['other'], call: transfer }
  const announced = { op: 'execute_proxy_announced', real_ss58: 'real', delegate_ss58: 'delegate', inner_op: 'transfer', inner_args: { dest_ss58: 'dest', amount_tao: '2' } }
  const single = await intentFromJSON(multi).prepare(reader(), context)
  assert.equal(single.dispatchOrigin, 'multisig')
  assert.deepEqual(single.spendByAccount, [{ address: 'multisig', amountRao: 1250000000n }])
  const combined = await buildIntent('batch', { intents: [multi, announced, transfer] }).prepare(reader(), context)
  assert.deepEqual(combined.spendByAccount, [{ address: 'multisig', amountRao: 1250000000n }, { address: 'real', amountRao: 2000000000n }, { address: 'coldkey', amountRao: 1250000000n }])
  assert.equal(combined.spendRao, 4500000000n)
  const canceled = await buildIntent('multisig_cancel', { threshold: 2, other_signatories: ['other'], call: transfer, timepoint: { height: 1, index: 0 } }).prepare(reader(), context)
  assert.deepEqual(canceled.spendByAccount, [])
})

test('hash-only approval retains semantic descriptor for native hash verification', async () => {
  const plan = await buildIntent('multisig_approve', { threshold: 2, other_signatories: ['other'], call: transfer, timepoint: { height: 1, index: 0 } }).prepare(reader(), context)
  assert.equal(plan.call.call, 'approve_as_multi')
  assert.equal(plan.call.semanticCall.call, 'transfer_keep_alive')
  assert.equal(plan.spendRao, 1250000000n)
})

test('all 33 owner hyperparameters have exact human-unit conversion', async () => {
  assert.equal(OWNER_HYPERPARAMETERS.length, 33)
  assert.equal(hyperparameterToRaw('collateral_drain_ratio', '1.5'), 3n << 63n)
  assert.equal(hyperparameterToRaw('min_burn', '1.000000001'), 1000000001n)
  assert.equal(hyperparameterToRaw('bonds_penalty', '0.5'), 32768n)
  assert.throws(() => hyperparameterToRaw('activity_cutoff', 1), /owner-unsettable/)
  assert.throws(() => hyperparameterToRaw('owner_cut_enabled', true), /owner-unsettable/)
  assert.throws(() => hyperparameterToRaw('tempo', 359), /360/)
  const plan = await buildIntent('set_hyperparameter', { netuid: 1, name: 'alpha_low', value: '0.1' }).prepare(reader(), context)
  assert.deepEqual(plan.call.params, { netuid: 1, alpha_low: 6554n, alpha_high: 50000 })
})

test('emission weights enforce exact sum, baseline, and owner rate limit', async () => {
  const base = reader()
  const view = reader({ query: async (pallet, item, params) => item === 'LastRateLimitedBlock' ? 99 : base.query(pallet, item, params) })
  const plan = await buildIntent('set_subnet_emission_weights', { netuid: 1, slots: ['owner'], values: ['0.18'], renormalize: true }).prepare(view, context)
  assert.equal(plan.call.params.values.reduce((sum, value) => sum + value, 0n), ONE)
  assert.match(plan.violations.join(), /rate-limited/)
  const alias = reader({ query: async (pallet, item, params) => item === 'SubnetOwner' ? 'same-key-other-network' : base.query(pallet, item, params), decodeAddress: () => new Uint8Array(32) })
  const sameOwner = await buildIntent('set_subnet_emission_weights', { netuid: 1, slots: ['owner'], values: ['0.18'], renormalize: true }).prepare(alias, context)
  assert.equal(sameOwner.violations.length, 0)
})


test('integer strings and bigint JSON round trips produce encodable quantities', async () => {
  const core = require('../dist'), golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new core.Runtime(core.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const address = core.Keypair.fromUri('//Alice').ss58Address
  const proxy = buildIntent('add_proxy', { delegate_ss58: address, proxy_type: 'Any', delay: '1' })
  const prepared = await proxy.prepare(reader(), { coldkey: address })
  assert.equal(prepared.call.params.delay, 1n)
  await encodeIntentCall(prepared.call, (pallet, call, params) => runtime.composeCall(pallet, call, params))
  const large = buildIntent('associate_evm_key', { netuid: 1, evm_key: '0x' + '11'.repeat(20), block_number: 9007199254740993n, signature: '0x' + '11'.repeat(65) })
  assert.equal(intentFromJSON(large.toJSON()).args.block_number, 9007199254740993n)
  assert.throws(() => buildIntent('add_proxy', { delegate_ss58: address, proxy_type: 'Any', delay: '-1' }), /unsigned|nonnegative/)
})

test('nested multisig and announced proxy weights use the dispatch hotkey identity', async () => {
  for (const announced of [false, true]) {
    const seen = [], encrypted = [], base = reader()
    const wrappedReader = reader({
      query: async (pallet, item, params) => {
        if (item === 'Uids') seen.push(params[1])
        return item === 'CommitRevealWeightsEnabled' ? true : base.query(pallet, item, params)
      },
      encryptWeightsV2: async options => { encrypted.push(options.hotkey); return { commit: new Uint8Array([1]), revealRound: 123n } },
    })
    const nested = { op: 'batch', intents: [{ op: 'set_weights', netuid: 1, uids: [0, 1], weights: [0.5, 1] }] }
    const intent = announced
      ? buildIntent('execute_proxy_announced', { delegate_ss58: 'other', real_ss58: 'real', inner_op: 'batch', inner_args: { intents: nested.intents } })
      : buildIntent('multisig_execute', { threshold: 2, other_signatories: ['other'], call: nested })
    const prepared = await intent.prepare(wrappedReader, context)
    assert.deepEqual(seen, [announced ? 'real' : 'multisig'])
    assert.deepEqual(encrypted, [announced ? base.decodeAddress('real') : new Uint8Array(32)])
    assert.equal(prepared.dispatchOrigin, announced ? 'real' : 'multisig')
    assert.equal(prepared.feePayer, 'coldkey')
  }
})
