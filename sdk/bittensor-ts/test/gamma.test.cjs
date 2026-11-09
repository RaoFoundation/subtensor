const test = require('node:test')
const assert = require('node:assert/strict')
const { Balance } = require('../dist/balance')
const { buildIntent, checkIntentPolicy, intentFromJSON } = require('../dist/intents')
const { gammaAmount, gammaUnits, gammaFromUnits, minimumGammaOut, decodeGammaQuote, GammaQuoteError, gammaReceiptData } = require('../dist/gamma')
const { dispatchRead } = require('../dist/reads')

const BLOCK = '0x' + '12'.repeat(32)
const MAX = (1n << 64n) - 1n
const ZERO_REFERENCE = '0x' + '00'.repeat(32)
const context = { coldkey: 'cold', hotkey: 'hot-a' }
const deposit = (extra = {}) => ({ coldkey: 'cold', spender: 'spender', balance: 10_000_000_000n, daily_spend_cap: null, ...extra })
const accountId = address => Uint8Array.from({ length: 32 }, () => ({ cold: 1, 'hot-a': 2, 'hot-a-prefix': 2, 'hot-b': 3, spender: 4, service: 5, other: 6, zero: 0 }[address] ?? 7))
function reader({ queries = {}, runtime = {}, ...overrides } = {}) {
  const calls = []
  return {
    calls, blockHash: async () => BLOCK, decodeAddress: accountId,
    query: async (pallet, item, params = [], block) => {
      calls.push(['query', item, params, block])
      if (Object.hasOwn(queries, item)) return typeof queries[item] === 'function' ? queries[item](params) : queries[item]
      return { GammaDeposits: null, Owner: 'cold', ColdkeyGammaHotkeys: [], Services: { spender: 'spender' }, NetworksAdded: true, SubnetOwner: 'cold', SubnetOwnerHotkey: 'hot-a', Uids: 0, ValidatorPermit: [true] }[item]
    },
    runtimeCall: async (api, method, params = [], block) => {
      calls.push(['runtime', method, params, block])
      if (Object.hasOwn(runtime, method)) return typeof runtime[method] === 'function' ? runtime[method](params) : runtime[method]
      if (method.startsWith('sim_mint_gamma')) return { Ok: [5_000_000_000n, 2_500_000_000n, 10_000_000_000n] }
      if (method === 'get_stake_info_for_hotkey_coldkey_netuid') return { stake: 10_000_000_000n }
      if (method === 'get_gamma_deposit') return { netuid: 2, hotkey: params[1], deposit: deposit({ spender: 'cold' }), spendable: 10_000_000_000n }
      throw new Error(`unexpected runtime call ${api}.${method}`)
    },
    ...overrides,
  }
}
const prepare = (op, args, r = reader()) => buildIntent(op, { netuid: 2, ...args }).prepare(r, context)

test('Gamma decimal units remain exact across u64, exponent, and subunit boundaries', () => {
  assert.equal(gammaAmount(' +001.234567890 '), '1.23456789')
  assert.equal(gammaUnits('1e-9'), 1n)
  assert.equal(gammaUnits('.5'), 500_000_000n)
  assert.equal(gammaUnits('18446744073.709551615'), MAX)
  assert.equal(gammaFromUnits(MAX), '18446744073.709551615')
  assert.equal(gammaAmount('ALL', true), 'all')
  for (const invalid of ['', '.', '+', 'e2', '1e-10', '18446744073.709551616', '-1', NaN, Infinity, 'all', {}, 1n]) assert.throws(() => gammaAmount(invalid))
  assert.throws(() => gammaAmount(Balance.fromTao('1')), /not Balance/)
  assert.throws(() => gammaFromUnits(9007199254740992), /exact integer/)
  assert.equal(minimumGammaOut(MAX, .05), MAX * 95n / 100n)
  assert.equal(minimumGammaOut(MAX, 1e-9), MAX * 999999999n / 1000000000n)
  assert.equal(minimumGammaOut(1n, .05), 0n)
  for (const tolerance of [-.1, 1, Infinity]) assert.throws(() => minimumGammaOut(1n, tolerance))
})

test('Gamma input validation rejects contradictory terms, malformed spends, and URLs before RPC', () => {
  assert.throws(() => buildIntent('mint_gamma', { netuid: 2, hotkey_ss58: 'hot-a', amount_tao: '1', spender_ss58: 'spender', service_hotkey_ss58: 'service' }), /not both/)
  assert.throws(() => buildIntent('set_gamma_deposit_spender', { netuid: 2, hotkey_ss58: 'hot-a' }), /exactly one/)
  assert.throws(() => buildIntent('set_gamma_deposit_spender', { netuid: 2, hotkey_ss58: 'hot-a', default_spender: true, spender_ss58: 'spender' }), /exactly one/)
  assert.throws(() => buildIntent('set_gamma_deposit_daily_spend_cap', { netuid: 2, hotkey_ss58: 'hot-a', daily_spend_cap_gamma: '0', remove_cap: true }), /exactly one/)
  assert.throws(() => buildIntent('move_gamma', { netuid: 2, origin_hotkey_ss58: 'hot-a', destination_hotkey_ss58: 'hot-b', amount_gamma: '0' }), /above 0/)
  for (const spends of [[], Array(513).fill({ hotkey_ss58: 'hot-a', amount_gamma: '1' }), [{ hotkey_ss58: 'hot-a', amount_gamma: '1', extra: true }], [{ hotkey_ss58: 'hot-a', amount_gamma: 'all' }], [{ hotkey_ss58: 'hot-a', amount_gamma: '1', reference: '0x01' }]]) assert.throws(() => buildIntent('spend_gamma', { netuid: 2, spends }))
  for (const url of ['ftp://example.test', '/relative', 'https://' + 'x'.repeat(250)]) assert.throws(() => buildIntent('set_service', { netuid: 2, url }))
  assert.throws(() => buildIntent('set_service', { netuid: 2, url: 'https://example.test/' + 'é'.repeat(119) }), /UTF-8/)
})

test('TAO mint uses a block-pinned quote, exact slippage, service terms, and bounded policy', async () => {
  const r = reader({ runtime: { sim_mint_gamma: ([id, amount]) => { assert.equal(id, 2); assert.equal(amount, 9007199254740993n); return { Ok: [1n, 9007199254740993n, MAX] } } } })
  const args = { hotkey_ss58: 'hot-a', amount_tao: '9007199.254740993', service_hotkey_ss58: 'service', daily_spend_cap_gamma: '0', rate_tolerance: .05 }
  const plan = await prepare('mint_gamma', args, r)
  assert.deepEqual(plan.call.params, { hotkey: 'hot-a', amount: 9007199254740993n, netuid: 2, min_amount_out: MAX * 95n / 100n, spender: 'spender', daily_spend_cap: 0n })
  assert.equal(plan.spendRao, 9007199254740993n)
  assert.deepEqual(plan.spendByAccount, [{ address: 'cold', amountRao: 9007199254740993n }])
  assert.equal(plan.signerRole, 'coldkey')
  assert.equal(plan.preview.gamma, gammaFromUnits(MAX))
  assert.equal(plan.preview.minimum_gamma, gammaFromUnits(MAX * 95n / 100n))
  assert.equal(plan.preview.daily_spend_cap_gamma, '0')
  assert.deepEqual(plan.violations, [])
  assert.ok(r.calls.every(call => call[3] === BLOCK))
  assert.match(checkIntentPolicy(plan, { maxSpendRao: 1n }).join(), /spend/)
  const intent = buildIntent('mint_gamma', { netuid: 2, ...args })
  assert.deepEqual(intentFromJSON(intent.toJSON()).toJSON(), intent.toJSON())
  assert.throws(() => buildIntent('mint_gamma', { netuid: 2, hotkey_ss58: 'hot-a', amount_tao: Balance.fromAlpha('1', 2) }), /netuid 0/)
})

test('stake mint clamps the quote while retaining requested/all dispatch amounts and unbounded spend', async () => {
  for (const amount_alpha of ['20', 'all']) {
    const r = reader({ runtime: { sim_mint_gamma_with_stake: ([id, amount]) => { assert.equal(id, 2); assert.equal(amount, 10_000_000_000n); return { Ok: [amount, 5n, 1000n] } } } })
    const plan = await prepare('mint_gamma_with_stake', { hotkey_ss58: 'hot-a', origin_hotkey_ss58: 'hot-b', amount_alpha }, r)
    assert.equal(plan.call.params.alpha_amount, amount_alpha === 'all' ? MAX : 20_000_000_000n)
    assert.equal(plan.call.params.destination_hotkey, 'hot-a')
    assert.equal(plan.spendRao, 'unbounded')
    assert.match(checkIntentPolicy(plan, { maxSpendRao: MAX }).join(), /unbounded/)
    assert.equal(plan.warnings.length, 1)
    assert.deepEqual(plan.violations, [])
  }
  await assert.rejects(prepare('mint_gamma_with_stake', { hotkey_ss58: 'hot-a', origin_hotkey_ss58: 'hot-b', amount_alpha: '1' }, reader({ runtime: { get_stake_info_for_hotkey_coldkey_netuid: null } })), /no stake/)
})

test('mint enforces existing ownership and immutable terms, deposit count, and checked balance', async () => {
  const args = { hotkey_ss58: 'hot-a', amount_tao: '1', spender_ss58: 'other', daily_spend_cap_gamma: '2' }
  const existing = deposit({ coldkey: 'other', balance: MAX, daily_spend_cap: { cap: 1n } })
  const blocked = await prepare('mint_gamma', args, reader({ queries: { GammaDeposits: existing } }))
  assert.match(blocked.violations.join(';'), /another coldkey/)
  assert.match(blocked.violations.join(';'), /set its spender/)
  assert.match(blocked.violations.join(';'), /set its cap/)
  assert.match(blocked.violations.join(';'), /exceed u64/)
  const full = await prepare('mint_gamma', { hotkey_ss58: 'hot-a', amount_tao: '1' }, reader({ queries: { Owner: 'zero', ColdkeyGammaHotkeys: Array(64).fill('hot-b') } }))
  assert.match(full.violations.join(';'), /associate/)
  assert.match(full.violations.join(';'), /64/)
  const keeps = await prepare('mint_gamma', { hotkey_ss58: 'hot-a', amount_tao: '1' }, reader({ queries: { GammaDeposits: deposit({ daily_spend_cap: { cap: 0n } }) } }))
  assert.equal(keeps.preview.spender_ss58, 'spender')
  assert.equal(keeps.preview.daily_spend_cap_gamma, '0')
  assert.equal(keeps.call.params.spender, null)
  assert.equal(keeps.call.params.daily_spend_cap, null)
})

test('quote failures preserve dispatch errors and resolve against the selected runtime metadata', async () => {
  assert.throws(() => decodeGammaQuote({ Err: 'BadOrigin' }), error => error instanceof GammaQuoteError && error.chainError.name === 'BadOrigin')
  assert.throws(() => decodeGammaQuote({ Ok: [1n, 2n] }), /invalid/)
  assert.throws(() => decodeGammaQuote({ Ok: [1n, 2n, MAX + 1n] }), /u64/)
  const failure = { Err: { Module: { index: 7, error: '0xc0000000' } } }
  const r = reader({ runtime: { sim_mint_gamma: failure }, moduleError: async (pallet, index, block) => {
    assert.deepEqual([pallet, index, block], [7, 192, BLOCK])
    return { name: 'SubtensorModule.GammaDisabled', docs: ['Gamma is disabled.'] }
  } })
  await assert.rejects(prepare('mint_gamma', { hotkey_ss58: 'hot-a', amount_tao: '1' }, r), error => error instanceof GammaQuoteError && error.chainError.name === 'GammaDisabled' && error.chainError.code === 'disabled')
  assert.throws(() => decodeGammaQuote(failure), error => error.chainError.name !== 'GammaDisabled' && error.chainError.code === 'unknown')
})

test('moving all credits frees an origin slot before opening the destination and preserves its policy', async () => {
  const r = reader({ queries: { GammaDeposits: ([, hotkey]) => hotkey === 'hot-a' ? deposit() : null, ColdkeyGammaHotkeys: Array(64).fill('hot-a') } })
  const all = await prepare('move_gamma', { origin_hotkey_ss58: 'hot-a', destination_hotkey_ss58: 'hot-b', amount_gamma: 'all' }, r)
  assert.equal(all.call.params.amount, MAX)
  assert.deepEqual(all.violations, [])
  assert.deepEqual(all.preview, { moved_gamma: '10', closes_origin: true })
  assert.equal(all.spendRao, 'unbounded')
  const partial = await prepare('move_gamma', { origin_hotkey_ss58: 'hot-a', destination_hotkey_ss58: 'hot-b', amount_gamma: '1' }, r)
  assert.match(partial.violations.join(';'), /64/)
  await assert.rejects(prepare('move_gamma', { origin_hotkey_ss58: 'hot-a', destination_hotkey_ss58: 'hot-a-prefix', amount_gamma: '1' }), /same/)
  const changed = await prepare('move_gamma', { origin_hotkey_ss58: 'hot-a', destination_hotkey_ss58: 'hot-b', amount_gamma: '1' }, reader({ queries: { GammaDeposits: ([, hotkey]) => deposit(hotkey === 'hot-b' ? { spender: 'other', daily_spend_cap: { cap: 0n } } : {}) } }))
  assert.match(changed.warnings.join(';'), /other/)
  assert.match(changed.warnings.join(';'), /destination deposit daily spend cap/)
})

test('deposit administration distinguishes paused caps, no cap, default spender, and nonempty close', async () => {
  const r = reader({ queries: { GammaDeposits: deposit() } })
  const paused = await prepare('set_gamma_deposit_daily_spend_cap', { hotkey_ss58: 'hot-a', daily_spend_cap_gamma: '0' }, r)
  assert.equal(paused.call.params.daily_spend_cap, 0n)
  const removed = await prepare('set_gamma_deposit_daily_spend_cap', { hotkey_ss58: 'hot-a', remove_cap: true }, r)
  assert.equal(removed.call.params.daily_spend_cap, null)
  assert.equal(removed.spendRao, null)
  const defaultSpender = await prepare('set_gamma_deposit_spender', { hotkey_ss58: 'hot-a', default_spender: true }, r)
  assert.equal(defaultSpender.call.params.spender, null)
  assert.equal(defaultSpender.spendRao, 'unbounded')
  const service = await prepare('set_gamma_deposit_spender', { hotkey_ss58: 'hot-a', service_hotkey_ss58: 'service' }, r)
  assert.equal(service.call.params.spender, 'spender')
  await assert.rejects(prepare('set_gamma_deposit_spender', { hotkey_ss58: 'hot-a', service_hotkey_ss58: 'service' }, reader({ queries: { Services: null } })), /publishes no service/)
  const nonempty = await prepare('close_gamma_deposit', { hotkey_ss58: 'hot-a' }, r)
  assert.match(nonempty.violations.join(';'), /holds 10 gamma/)
  const empty = await prepare('close_gamma_deposit', { hotkey_ss58: 'hot-a' }, reader({ queries: { GammaDeposits: deposit({ balance: 0n }) } }))
  assert.deepEqual(empty.violations, [])
})

test('spends simulate ordered balance and cap consumption across alternate SS58 prefixes', async () => {
  const r = reader({ runtime: { get_gamma_deposit: ([, hotkey]) => hotkey === 'other' ? null : { deposit: deposit({ spender: 'cold' }), spendable: 6_000_000_000n } } })
  const spends = [
    { hotkey_ss58: 'hot-a', amount_gamma: '4', reference: 'AB'.repeat(32) },
    { hotkey_ss58: 'hot-a-prefix', amount_gamma: '3' },
    { hotkey_ss58: 'hot-a', amount_gamma: '2' },
    { hotkey_ss58: 'hot-a', amount_gamma: '5' },
    { hotkey_ss58: 'hot-a', amount_gamma: '0' },
    { hotkey_ss58: 'other', amount_gamma: '1' },
  ]
  const plan = await prepare('spend_gamma', { spends }, r)
  assert.equal(plan.preview.spent_gamma_units, 6_000_000_000n)
  assert.deepEqual(plan.preview.skipped.map(row => row.reason), ['GammaDepositDailySpendCapExceeded', 'InsufficientGammaBalance', 'AmountTooLow', 'GammaDepositDoesNotExist'])
  assert.equal(plan.call.params.spends[0][2], '0x' + 'ab'.repeat(32))
  assert.equal(plan.call.params.spends[1][2], ZERO_REFERENCE)
  assert.equal(r.calls.filter(call => call[1] === 'get_gamma_deposit').length, 2)
  assert.equal(plan.spendRao, null)
  assert.deepEqual(checkIntentPolicy(plan, { maxSpendRao: 0n, allowedNetuids: [2] }), [])
  const denied = await prepare('spend_gamma', { spends: [spends[0]] }, reader({ runtime: { get_gamma_deposit: { deposit: deposit(), spendable: 10_000_000_000n } } }))
  assert.equal(denied.preview.skipped[0].reason, 'NotGammaDepositSpender')
  assert.match(denied.warnings[0], /nothing would be spent/)
  const missingSubnet = await prepare('spend_gamma', { spends: [spends[0]] }, reader({ queries: { NetworksAdded: false } }))
  assert.match(missingSubnet.violations.join(';'), /subnet 2 does not exist/)
})

test('nested spend receiving addresses require their finalized registration descriptor', async () => {
  const descriptor = new Uint8Array([1, 2, 3])
  const r = reader({ parseRecipient: () => ({ account: 'hot-a', descriptor }), finalizedHead: async () => 'finalized', queries: { Accounts: { descriptor: '0x010203' } } })
  const plan = await prepare('spend_gamma', { spends: [{ hotkey_ss58: 'tao_test', amount_gamma: '1' }] }, r)
  assert.equal(plan.call.params.spends[0][0], 'hot-a')
  assert.equal(r.calls.find(call => call[1] === 'Accounts')[3], 'finalized')
  const unregistered = reader({ parseRecipient: r.parseRecipient, finalizedHead: r.finalizedHead, queries: { Accounts: null } })
  await assert.rejects(prepare('spend_gamma', { spends: [{ hotkey_ss58: 'tao_test', amount_gamma: '1' }] }, unregistered), /not finalized/)
})

test('fixed-profile Gamma hotkeys require activation and profile support without registration', async () => {
  for (const profile of [5, 7, 9]) {
    const r = reader({ parseRecipient: () => ({ account: 'hot-a', descriptor: new Uint8Array([2, profile]) }), finalizedHead: async () => 'finalized', supportsPqProfile: async found => { assert.equal(found, profile); return true }, queries: { HashedAccountsEnabled: true, MlDsaAccounts: () => assert.fail('fixed profiles do not need a registry row') } })
    const plan = await prepare('spend_gamma', { spends: [{ hotkey_ss58: 'tao_test', amount_gamma: '1' }] }, r)
    assert.equal(plan.call.params.spends[0][0], 'hot-a')
    assert.equal(r.calls.find(call => call[1] === 'HashedAccountsEnabled')[3], 'finalized')
    const disabled = reader({ parseRecipient: r.parseRecipient, finalizedHead: r.finalizedHead, supportsPqProfile: r.supportsPqProfile, queries: { HashedAccountsEnabled: false } })
    await assert.rejects(prepare('spend_gamma', { spends: [{ hotkey_ss58: 'tao_test', amount_gamma: '1' }] }, disabled), /disabled/)
    const unsupported = reader({ parseRecipient: r.parseRecipient, finalizedHead: r.finalizedHead, supportsPqProfile: async () => false, queries: { HashedAccountsEnabled: true } })
    await assert.rejects(prepare('spend_gamma', { spends: [{ hotkey_ss58: 'tao_test', amount_gamma: '1' }] }, unsupported), /profile/)
  }
})

test('service publication enforces current ownership, subnet registration, and validator or owner permission', async () => {
  const url = 'https://example.test/é'
  const owner = await prepare('set_service', { url }, reader({ queries: { ValidatorPermit: [false] } }))
  assert.deepEqual(owner.violations, [])
  assert.equal(owner.call.params.spender, 'cold')
  assert.deepEqual(owner.call.params.url, new TextEncoder().encode(url))
  const validator = await prepare('set_service', { url, hotkey_ss58: 'hot-b', spender_ss58: 'spender' })
  assert.deepEqual(validator.violations, [])
  assert.equal(validator.call.params.spender, 'spender')
  const unregisteredOwner = await prepare('set_service', { url }, reader({ queries: { Uids: null } }))
  assert.match(unregisteredOwner.violations.join(';'), /not registered/)
  const denied = await prepare('set_service', { url, hotkey_ss58: 'hot-b' }, reader({ queries: { Owner: 'other', ValidatorPermit: [false], NetworksAdded: false } }))
  assert.equal(denied.violations.length, 3)
  const removal = await prepare('set_service', { url: '' }, reader({ queries: { Uids: null, ValidatorPermit: [] } }))
  assert.deepEqual(removal.violations, [])
  assert.equal(removal.call.params.url.length, 0)
})

function readView({ queries = {}, maps = {}, runtime = {}, block = 100, moduleError } = {}) {
  const calls = []
  const invoke = (kind, table, key, params) => { calls.push([kind, key[1], params]); const value = table[key[1]]; return typeof value === 'function' ? value(params) : value }
  const pinned = { calls, block, at: async () => pinned, balance: (amount, netuid = 0) => Balance.fromRao(amount, netuid), query: async (key, params) => invoke('query', queries, key, params), queryMap: async (key, params) => invoke('map', maps, key, params) ?? [], runtime: async (key, params) => invoke('runtime', runtime, key, params), moduleError }
  return pinned
}

test('Gamma reads preserve exact balances, caps, and currency-tagged runtime quotes on one snapshot', async () => {
  const info = { netuid: 2, hotkey: 'hot-a', deposit: deposit({ balance: MAX, daily_spend_cap: { cap: 0n } }), spendable: 0n }
  const pinned = readView({ runtime: { get_gamma_deposit: info, get_gamma_deposits_for_coldkey: [info], sim_mint_gamma: ([id, amount]) => { assert.deepEqual([id, amount], [2, 9007199254740993n]); return { Ok: [1n, amount, MAX] } }, sim_mint_gamma_with_stake: { Ok: [5n, 2n, 10n] } } })
  let pins = 0
  const head = { at: async () => { pins++; return pinned } }
  const one = await dispatchRead(head, 'gamma_deposit', { netuid: 2, hotkey_ss58: 'hot-a' })
  assert.deepEqual(one, { netuid: 2, hotkey: 'hot-a', coldkey: 'cold', spender: 'spender', balance_gamma: gammaFromUnits(MAX), daily_spend_cap_gamma: '0', spendable_gamma: '0' })
  assert.deepEqual(await dispatchRead(head, 'gamma_deposits', { coldkey_ss58: 'cold' }), [one])
  const quote = await dispatchRead(head, 'quote_mint_gamma', { netuid: 2, amount_tao: '9007199.254740993' })
  assert.equal(quote.alpha_burned.netuid, 2)
  assert.equal(quote.tao_value.netuid, 0)
  assert.equal(quote.tao_value.rao, 9007199254740993n)
  assert.equal(quote.gamma, gammaFromUnits(MAX))
  assert.equal((await dispatchRead(head, 'quote_mint_gamma_with_stake', { netuid: 2, amount_alpha: '1' })).alpha_burned.rao, 5n)
  assert.equal(pins, 4)
  assert.equal(await dispatchRead(readView(), 'gamma_deposit', { netuid: 2, hotkey_ss58: 'missing' }), null)
  await assert.rejects(dispatchRead(readView({ runtime: { sim_mint_gamma: { Err: 'BadOrigin' } } }), 'quote_mint_gamma', { netuid: 2, amount_tao: '1' }), /BadOrigin/)
})

test('TAO price projection uses ceiling per-block rise, immediate falls, saturation, and the 300-block freshness boundary', async () => {
  const price = (state, block = 100) => dispatchRead(readView({ block, queries: { TaoUsdPriceOracle: 'oracle', TaoUsdPrice: state } }), 'tao_usd_price')
  const rising = await price({ accepted: 100_000_000_001n, feed: 200_000_000_000n, fed_at: 90 })
  assert.equal(rising.accepted_usd_string, '100.066000011')
  assert.equal(rising.feed_usd_string, '200')
  assert.equal(rising.stale, false)
  assert.equal((await price({ accepted: 2n, feed: 3n, fed_at: 0 })).accepted_usd_string, '0.000000003')
  assert.equal((await price({ accepted: 10n, feed: 5n, fed_at: 0 })).accepted_usd_string, '0.000000005')
  assert.equal((await price({ accepted: 1n, feed: 10n, fed_at: 100 }, 400)).stale, false)
  assert.equal((await price({ accepted: 1n, feed: 10n, fed_at: 100 }, 401)).stale, true)
  assert.equal((await price({ accepted: 1n, feed: 10n, fed_at: 101 })).blocks_since_last_feed, 0)
  assert.equal((await price(null)).accepted_usd, null)
})

test('service reads identify the current owner service and render terminal-active or invalid bytes as hex', async () => {
  const maps = { Services: [['hot-a', { url: 'https://example.test', spender: 'spender' }], ['hot-b', { url: '0x41', spender: 'spender' }], ['escape', { url: 'x\u001b[31m', spender: 'spender' }], ['format', { url: 'x\u200dy', spender: 'spender' }], ['bytes', { url: [0xff], spender: 'spender' }], ['utf8', { url: new TextEncoder().encode('https://example.test/é'), spender: 'spender' }]] }
  const queries = { SubnetOwnerHotkey: 'hot-a', SubnetOwner: 'cold', Owner: 'cold' }
  const result = await dispatchRead(readView({ queries, maps }), 'services', { netuid: 2 })
  assert.equal(result['hot-a'].owner, true)
  assert.equal(result['hot-b'].owner, false)
  assert.equal(result['hot-b'].url, '0x41')
  assert.equal(result.escape.url, '0x781b5b33316d')
  assert.equal(result.format.url, '0x78e2808d79')
  assert.equal(result.bytes.url, '0xff')
  assert.equal(result.utf8.url, 'https://example.test/é')
  const changed = await dispatchRead(readView({ queries: { ...queries, Owner: 'other' }, maps }), 'services', { netuid: 2 })
  assert.equal(changed['hot-a'].owner, false)
})

test('receipt data comes from actual Gamma events with execution metadata for skipped entries', async () => {
  const row = (event_id, attributes) => ({ module_id: 'SubtensorModule', event_id, attributes })
  const minted = row('GammaMinted', { netuid: 2, hotkey: 'hot-a', spender: 'spender', daily_spend_cap: 0n, alpha: 9007199254740993n, value: 10n, gamma: MAX })
  const spent = row('GammaSpent', { netuid: 2, hotkey: 'hot-a', amount: 1n, reference: new Uint8Array(32).fill(0xab) })
  const skipped = { event: { section: 'SubtensorModule', method: 'GammaSpendSkipped', attributes: { netuid: 3, hotkey: 'hot-b', amount: 2n, reference: ZERO_REFERENCE, reason: { Module: { index: 7, error: [195, 0, 0, 0] } } } } }
  const result = await gammaReceiptData([minted, spent, skipped, spent], async (pallet, index) => {
    assert.deepEqual([pallet, index], [7, 195])
    return { name: 'SubtensorModule.GammaDepositDoesNotExist', docs: ['The deposit does not exist.'] }
  })
  assert.deepEqual(result.minted, { hotkey_ss58: 'hot-a', spender_ss58: 'spender', daily_spend_cap_gamma: '0', alpha_burned: '9007199.254740993', tao_value: '0.00000001', gamma: gammaFromUnits(MAX) })
  assert.deepEqual(result.mints, [{ netuid: 2, ...result.minted }])
  assert.deepEqual(result.spent, Array(2).fill({ netuid: 2, hotkey_ss58: 'hot-a', amount_gamma: '0.000000001', reference: '0x' + 'ab'.repeat(32) }))
  assert.equal(result.skipped[0].netuid, 3)
  assert.equal(result.skipped[0].reason, 'GammaDepositDoesNotExist')
  assert.deepEqual(await gammaReceiptData([]), {})
  assert.deepEqual(await gammaReceiptData([{ module_id: 'Multisig', event_id: 'MultisigApproval', attributes: {} }]), {})
  const unknown = await gammaReceiptData([skipped], async () => { throw new Error('metadata unavailable') })
  assert.match(unknown.skipped[0].reason, /Unknown module error 7\/195/)
})

test('Gamma receipt batches preserve every mint and its subnet in execution order', async () => {
  const mint = (netuid, gamma) => ({ module_id: 'SubtensorModule', event_id: 'GammaMinted', attributes: { netuid, hotkey: 'same-hotkey', spender: 'spender', daily_spend_cap: null, alpha: 1n, value: 2n, gamma } })
  const result = await gammaReceiptData([mint(2, 3n), mint(3, 4n), mint(2, 5n)])
  assert.equal(result.minted.gamma, '0.000000003')
  assert.deepEqual(result.mints.map(({ netuid, gamma }) => [netuid, gamma]), [[2, '0.000000003'], [3, '0.000000004'], [2, '0.000000005']])
})
