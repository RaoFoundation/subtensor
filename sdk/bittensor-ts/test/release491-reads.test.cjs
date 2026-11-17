const test = require('node:test')
const assert = require('node:assert/strict')
const { dispatchRead } = require('../dist/reads')
const { accountReads } = require('../dist/reads/accounts')
const { subnetConvictions } = require('../dist/reads/locks')
const { subnetEmissionWeights } = require('../dist/reads/subnets')
const { Balance } = require('../dist/balance')
const { defaultWeights, ONE } = require('../dist/emission-weights')

function snapshot({ queries = {}, maps = {}, runtime = {}, absent = [], metadataError } = {}) {
  const calls = []
  const get = (table, kind, item, params = []) => {
    const name = item.join('.')
    calls.push([kind, name, params])
    assert(!absent.includes(name), `queried storage absent from pinned metadata: ${name}`)
    const value = table[name]
    if (value instanceof Error) throw value
    return typeof value === 'function' ? value(params) : value ?? null
  }
  const pinned = {
    block: 100, calls,
    at: async () => pinned,
    hasStorage: async item => {
      calls.push(['metadata', item.join('.')])
      if (metadataError) throw metadataError
      return !absent.includes(item.join('.'))
    },
    query: async (item, params) => get(queries, 'query', item, params),
    queryMap: async (item, params) => get(maps, 'map', item, params) ?? [],
    runtime: async (item, params) => get(runtime, 'runtime', item, params),
    balance: (amount, netuid = 0) => Balance.fromRao(amount, netuid),
  }
  const head = { at: async () => pinned, query: () => assert.fail('unpinned query'), hasStorage: () => assert.fail('unpinned metadata') }
  return { pinned, head, calls }
}

for (const [backend, module] of [['Node', '../dist/client'], ['browser', '../dist/browser-client']]) {
  test(`${backend} snapshot storage checks use pinned metadata and propagate metadata failures`, async () => {
    const { Snapshot } = require(module)
    const hash = '0x' + '34'.repeat(32), calls = []
    const storage = ['ColdkeySwapV2PolicySnapshots', 'HotkeyLock']
    const client = { runtimeAt: async block => {
      calls.push(block)
      return {
        pallet: name => name === 'SubtensorModule' ? { storage: storage.map(name => ({ name })) } : null,
        metadataIr: () => ({ pallets: [{ name: 'SubtensorModule', storage }] }),
      }
    } }
    const context = { client, blockHash: hash, hash }
    assert.equal(await Snapshot.prototype.hasStorage.call(context, ['SubtensorModule', 'ColdkeySwapV2PolicySnapshots']), true)
    assert.equal(await Snapshot.prototype.hasStorage.call(context, ['SubtensorModule', 'OwnerLock']), false)
    assert.equal(await Snapshot.prototype.hasStorage.call(context, ['MissingPallet', 'OwnerLock']), false)
    assert.deepEqual(calls, [hash, hash, hash])
    const failure = new Error('runtime metadata unavailable')
    client.runtimeAt = async () => { throw failure }
    await assert.rejects(Snapshot.prototype.hasStorage.call(context, ['SubtensorModule', 'OwnerLock']), error => error === failure)
  })
}

test('proxy announcements use the delegate key and preserve exact reserved TAO', async () => {
  const hash = '0x' + '12'.repeat(32), amount = 9007199254740997n
  const { head, calls } = snapshot({ queries: {
    'Proxy.Announcements': [[{ real: 'owner', call_hash: hash, height: 91 }], amount],
  } })
  const result = await dispatchRead(head, 'proxy_announcements', { delegate_ss58: { address: 'delegate' } })
  assert.deepEqual(result.announcements, [{ real: 'owner', call_hash: hash, height: 91 }])
  assert.equal(result.deposit.rao, amount)
  assert.equal(result.deposit.netuid, 0)
  assert.deepEqual(calls, [['query', 'Proxy.Announcements', ['delegate']]])
  const empty = await dispatchRead(snapshot().head, 'proxy_announcements', { delegate_ss58: 'delegate' })
  assert.deepEqual(empty.announcements, [])
  assert.equal(empty.deposit.rao, 0n)
})

test('Gamma emission whitelist read exposes the numeric flag', async () => {
  for (const [stored, expected] of [[true, 1], [false, 0]]) {
    const { head, calls } = snapshot({ queries: { 'SubtensorModule.SubnetGammaEmissionWhitelisted': stored } })
    assert.equal(await dispatchRead(head, 'subnet_gamma_emission_whitelisted', { netuid: 2 }), expected)
    assert.deepEqual(calls, [['query', 'SubtensorModule.SubnetGammaEmissionWhitelisted', [2]]])
  }
})

for (const policy of [false, true, null]) {
  test(`coldkey swap reads pinned policy ${policy} without losing false or legacy null`, async () => {
    const { head, calls } = snapshot({ queries: {
      'SubtensorModule.ColdkeySwapAnnouncements': [140, '0xabc'],
      'SubtensorModule.ColdkeySwapDisputes': 0,
      'SubtensorModule.ColdkeySwapV2PolicySnapshots': policy,
    } })
    const result = await accountReads.coldkey_swap_announcement(head, { coldkey_ss58: 'source' })
    assert.deepEqual(result, { execute_block: 140, new_coldkey_hash: '0xabc', import_source_policies: policy, disputed: false, dispute_block: null, recovery_locked: true })
    assert(calls.some(([kind, name]) => kind === 'metadata' && name.endsWith('ColdkeySwapV2PolicySnapshots')))
    assert.equal(calls.filter(([kind]) => kind === 'query').length, 3)
  })
}

test('coldkey swap legacy fallback requires metadata absence and retains recovery-only state', async () => {
  const { head, calls } = snapshot({ absent: ['SubtensorModule.ColdkeySwapV2PolicySnapshots'], queries: { 'SubtensorModule.ColdkeySwapDisputes': 0 } })
  assert.deepEqual(await accountReads.coldkey_swap_announcement(head, { coldkey_ss58: 'source' }), {
    execute_block: null, new_coldkey_hash: null, import_source_policies: null, disputed: false, dispute_block: null, recovery_locked: true,
  })
  assert(!calls.some(([kind, name]) => kind === 'query' && name.endsWith('ColdkeySwapV2PolicySnapshots')))
  assert.equal(await accountReads.coldkey_swap_announcement(snapshot().head, { coldkey_ss58: 'source' }), null)
})

test('coldkey swap does not disguise metadata, RPC or malformed-policy errors as legacy state', async () => {
  const failure = new Error('storage unavailable after RPC connection failed')
  for (const options of [{ metadataError: failure }, { queries: { 'SubtensorModule.ColdkeySwapV2PolicySnapshots': failure } }]) {
    await assert.rejects(accountReads.coldkey_swap_announcement(snapshot(options).head, { coldkey_ss58: 'source' }), error => error === failure)
  }
  const malformed = snapshot({ queries: { 'SubtensorModule.ColdkeySwapV2PolicySnapshots': 'false' } })
  await assert.rejects(accountReads.coldkey_swap_announcement(malformed.head, { coldkey_ss58: 'source' }), /invalid coldkey swap policy/)
})

const lock = mass => ({ locked_mass: BigInt(mass), conviction: { bits: 0n }, last_update: 100 })
const ownerMaps = ['SubtensorModule.OwnerLock', 'SubtensorModule.DecayingOwnerLock']
function conviction(options = {}) {
  return snapshot({ ...options, queries: { 'SubtensorModule.SubnetOwnerHotkey': 'owner-hotkey', 'SubtensorModule.SubnetAlphaOut': 5000n, ...options.queries } })
}

test('one-bucket layout gives the owner hotkey immediate conviction for both lock modes', async () => {
  const { head, calls } = conviction({ absent: ownerMaps, maps: {
    'SubtensorModule.HotkeyLock': [[['owner-hotkey'], lock(1000)], ['challenger', lock(500)]],
    'SubtensorModule.DecayingHotkeyLock': [['owner-hotkey', lock(200)]],
  } })
  const result = await subnetConvictions(head, 1)
  const [owner, challenger] = result.hotkeys
  assert.equal(owner.hotkey, 'owner-hotkey')
  assert.equal(owner.is_owner, true)
  assert.equal(owner.locked_alpha.rao, 1200n)
  assert.equal(owner.conviction_alpha.rao, 1200n)
  assert.equal(owner.blocks_to_threshold, 0)
  assert.equal(challenger.conviction_alpha.rao, 0n)
  assert.equal(result.total_locked_alpha.rao, 1700n)
  assert(!calls.some(([kind, name]) => kind === 'query' && ownerMaps.includes(name)))
})

for (const [label, stored, absent, expectedMass, expectedConviction] of [
  ['both owner maps', { [ownerMaps[0]]: lock(1000), [ownerMaps[1]]: lock(200) }, [], 1700n, 1200n],
  ['only perpetual owner map', { [ownerMaps[0]]: lock(1000) }, [ownerMaps[1]], 1500n, 1000n],
  ['present empty owner maps', {}, [], 500n, 0n],
]) {
  test(`legacy conviction layout preserves ${label} without counting hotkey rows as owner buckets`, async () => {
    const { head } = conviction({ absent, queries: stored, maps: { 'SubtensorModule.HotkeyLock': [['owner-hotkey', lock(500)]] } })
    const [owner] = (await subnetConvictions(head, 1)).hotkeys
    assert.equal(owner.is_owner, true)
    assert.equal(owner.locked_alpha.rao, expectedMass)
    assert.equal(owner.conviction_alpha.rao, expectedConviction)
  })
}

test('conviction layout does not reinterpret failed metadata or storage reads', async () => {
  const failure = new Error('OwnerLock unavailable: RPC timed out')
  for (const options of [{ metadataError: failure }, { queries: { [ownerMaps[0]]: failure } }]) {
    await assert.rejects(subnetConvictions(conviction(options).head, 1), error => error === failure)
  }
})

test('emission read retains totals and exposes Gamma targets and lease rules at one block', async () => {
  const gamma = ONE / 10n, weights = defaultWeights()
  weights[3] -= gamma
  const { head } = snapshot({ queries: {
    'SubtensorModule.SubnetGammaEmission': [[2, gamma]], 'SubtensorModule.SubnetUidToLeaseId': 0,
    'SubtensorModule.LastRateLimitedBlock': 90, 'SubtensorModule.SubnetTAO': 100n,
  }, maps: {
    'SubtensorModule.SubnetGammaEmissionWhitelisted': [[[4], true], [2, true], [3, true], [5, false]],
    'SubtensorModule.SubnetGammaEnabled': [[2, true], [3, false], [5, true]],
  }, runtime: {
    'SubnetInfoRuntimeApi.get_subnet_emission_weights': { netuid: 1, weights, root_baseline_share: 1n, root_share: 1n, created_via_lease: false },
  } })
  const info = await subnetEmissionWeights(head, 1)
  assert.equal(info.gamma_share, gamma)
  assert.deepEqual(info.gamma_shares, { 2: gamma })
  assert.deepEqual(info.gamma_targets, { 2: true, 3: false, 4: false })
  assert.equal(info.leased, true)
  assert.equal(info.block, 100)
  assert.deepEqual(info.rules().gamma_targets, info.gamma_targets)
  assert.equal(info.rules().gamma_share, gamma)
  assert.equal(info.rules().leased, true)
  assert.equal(info.weights.reduce((sum, value) => sum + value, gamma), ONE)
  assert.equal(await subnetEmissionWeights({ at: () => assert.fail('root has no emission vector') }, 0), null)
})
