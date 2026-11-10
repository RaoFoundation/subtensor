'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const emission = require('../dist/emission-weights')
const { buildIntent } = require('../dist/intents')
const { subnetEmissionWeights } = require('../dist/reads/subnets')
const { createNodeIntentReader } = require('../dist/node-workflows')
const { IDEMPOTENT_RPC_METHODS } = require('../dist/transport')
const BLOCK = '0x' + '11'.repeat(32)
const gamma = emission.parseShare('10%')
const owner = emission.parseShare('18%'), validator = emission.parseShare('36%')
const weights = [0n, 0n, owner, validator, emission.ONE - gamma - owner - validator]

test('emission normalization and validation preserve the Gamma allocation with exact integer rounding', () => {
  assert.deepEqual(emission.validate(emission.defaultWeights()), [])
  assert.deepEqual(emission.renormalize(weights, { owner }, 0n), emission.renormalize(weights, { owner }))
  assert.deepEqual(emission.validate(weights, { gamma_share: gamma }), [])
  assert.match(emission.validate(weights).map(row => row.error).join(), /EmissionWeightsDoNotSumToOne/)
  const fixed = emission.parseShare('20%'), adjusted = emission.renormalize(weights, { owner: fixed }, gamma)
  assert.equal(adjusted[2], fixed)
  assert.equal(adjusted.reduce((sum, value) => sum + value, gamma), emission.ONE)
  assert.deepEqual(emission.validate(adjusted, { gamma_share: gamma }), [])
  assert.deepEqual(emission.renormalize(weights, Object.fromEntries(weights.map((value, index) => [index, value])), gamma), weights)
  assert.throws(() => emission.renormalize(weights, { owner: emission.ONE }, gamma), /exceed 100%/)
  assert.throws(() => emission.renormalize(weights, {}, -1n), /outside u64/)
  assert.equal(emission.gammaEmissionShare([[7, 9007199254740993n], [8, '2']]), 9007199254740995n)
  for (const rows of [false, [[7]], [[7, 9007199254740992]], [[7, -1n]], [[7, emission.ONE], [8, 1n]]]) assert.throws(() => emission.gammaEmissionShare(rows))
})

function emissionReader(gammaRows = [[7, gamma]], current = weights) {
  return {
    blockHash: async () => BLOCK, blockNumber: async () => 100,
    runtimeCall: async (api, method) => {
      assert.equal(api, 'SubnetInfoRuntimeApi'); assert.equal(method, 'get_subnet_emission_weights')
      return { netuid: 2, weights: current, root_baseline_share: 1n, root_share: 1n, created_via_lease: false }
    },
    query: async (_pallet, item) => {
      if (item === 'SubnetGammaEmission') return gammaRows
      if (item === 'SubnetOwner') return 'cold'
      if (item === 'LastRateLimitedBlock') return 0
      throw new Error(`unexpected query ${item}`)
    },
  }
}
test('emission intent sparse and renormalized calls retain existing Gamma shares', async () => {
  for (const renormalize of [false, true]) {
    const plan = await buildIntent('set_subnet_emission_weights', { netuid: 2, slots: ['owner'], values: [owner], renormalize }).prepare(emissionReader(), { coldkey: 'cold' })
    assert.deepEqual(plan.violations, [])
    assert.equal(plan.preview.gamma_share, gamma)
    assert.equal(plan.preview.after.reduce((sum, value) => sum + value, gamma), emission.ONE)
    assert.equal(plan.call.call, 'sudo_set_subnet_emission_weights')
    assert.deepEqual(plan.call.params.uids, renormalize ? [0, 1, 2, 3, 4] : [2])
    assert.deepEqual(plan.call.params.values, renormalize ? plan.preview.after : [owner])
  }
  const invalid = await buildIntent('set_subnet_emission_weights', { netuid: 2, slots: ['owner'], values: [owner + 1n] }).prepare(emissionReader(), { coldkey: 'cold' })
  assert.match(invalid.violations.join(), /EmissionWeightsDoNotSumToOne/)
  const legacy = await buildIntent('set_subnet_emission_weights', { netuid: 2, slots: ['owner'], values: ['20%'], renormalize: true }).prepare(emissionReader([], emission.defaultWeights()), { coldkey: 'cold' })
  assert.deepEqual(legacy.violations, [])
  assert.equal(legacy.preview.gamma_share, 0n)
  assert.equal(legacy.preview.after.reduce((sum, value) => sum + value), emission.ONE)
  const unavailable = emissionReader()
  unavailable.query = async () => { throw new Error('RPC unavailable') }
  await assert.rejects(buildIntent('set_subnet_emission_weights', { netuid: 2, slots: ['owner'], values: [owner] }).prepare(unavailable, { coldkey: 'cold' }), /RPC unavailable/)
})

test('subnet emission read exposes Gamma parts in its rule inputs at the pinned view', async () => {
  const queries = [], view = {
    block: 100, at: async () => view,
    runtime: async () => ({ netuid: 2, weights, root_baseline_share: 1n, root_share: 1n, owner, validator, miner: weights[4] }),
    query: async (item, params) => { queries.push([item, params]); return item[1] === 'SubnetGammaEmission' ? [[7, gamma]] : 0n },
  }
  const result = await subnetEmissionWeights(view, 2)
  assert.equal(result.gamma_share, gamma)
  assert.equal(result.rules().gamma_share, gamma)
  assert.deepEqual(emission.validate(result.weights, result.rules()), [])
  assert(queries.some(([item, params]) => item[1] === 'SubnetGammaEmission' && params[0] === 2))
})

const addressId = address => Uint8Array.from({ length: 32 }, () => ({ cold: 1, 'cold-prefix': 1, other: 2, zero: 0, hot: 3, origin: 4 }[address]))
function gammaReader({ owner = 'zero', present = false, origin = false, existing = null } = {}) {
  const reads = []
  return {
    reads, blockHash: async () => BLOCK, decodeAddress: addressId,
    storageExists: async (pallet, item, params, block) => { reads.push(['exists', pallet, item, params, block]); return present },
    query: async (pallet, item, params, block) => {
      reads.push(['query', pallet, item, params, block])
      if (item === 'Owner') return owner
      if (item === 'GammaDeposits') return origin && params[1] === 'origin' ? { coldkey: 'cold', spender: 'cold', balance: 1_000_000_000n } : existing
      if (item === 'ColdkeyGammaHotkeys') return []
      throw new Error(`unexpected query ${item}`)
    },
    runtimeCall: async (_api, method, _params, block) => {
      assert.equal(block, BLOCK)
      if (method === 'get_stake_info_for_hotkey_coldkey_netuid') return { stake: 1_000_000_000n }
      assert.match(method, /^sim_mint_gamma/)
      return { Ok: [1_000_000_000n, 1_000_000_000n, 1_000_000_000n] }
    },
  }
}
test('Gamma mint and move distinguish absent Owner from stored zero and compare account identity', async () => {
  for (const op of ['mint_gamma', 'mint_gamma_with_stake', 'move_gamma']) {
    const args = op === 'mint_gamma' ? { hotkey_ss58: 'hot', amount_tao: '1' }
      : op === 'mint_gamma_with_stake' ? { hotkey_ss58: 'hot', origin_hotkey_ss58: 'origin', amount_alpha: '1' }
        : { origin_hotkey_ss58: 'origin', destination_hotkey_ss58: 'hot', amount_gamma: 'all' }
    for (const [owner, present, coldkey, allowed] of [['cold', true, 'cold-prefix', true], ['other', true, 'cold', false], ['zero', false, 'cold', true], ['zero', true, 'cold', false]]) {
      const r = gammaReader({ owner, present, origin: true })
      const plan = await buildIntent(op, { netuid: 2, ...args }).prepare(r, { coldkey })
      assert.equal(plan.violations.length === 0, allowed, `${op}: ${owner}, ${present}`)
      if (!allowed) assert.match(plan.violations.join(), /another coldkey/)
      assert(r.reads.some(row => row[0] === 'exists' && row[2] === 'Owner'))
      assert(r.reads.every(row => row[4] === BLOCK))
    }
  }
  const zeroOwner = await buildIntent('mint_gamma', { netuid: 2, hotkey_ss58: 'hot', amount_tao: '1' }).prepare(gammaReader({ owner: 'zero', present: true }), { coldkey: 'zero' })
  assert.deepEqual(zeroOwner.violations, [])
})

test('Gamma existing deposit ownership remains authoritative and failed presence reads fail closed', async () => {
  const intent = buildIntent('mint_gamma', { netuid: 2, hotkey_ss58: 'hot', amount_tao: '1' })
  const r = gammaReader({ owner: 'cold', present: true, existing: { coldkey: 'other', spender: 'other', balance: 1n } })
  const plan = await intent.prepare(r, { coldkey: 'cold' })
  assert.match(plan.violations.join(), /deposit.*another coldkey/)
  assert.equal(r.reads.some(row => row[0] === 'exists'), false)
  const failed = gammaReader(); failed.storageExists = async () => { throw new Error('presence RPC unavailable') }
  await assert.rejects(intent.prepare(failed, { coldkey: 'cold' }), /presence RPC unavailable/)
  const legacy = gammaReader(); delete legacy.storageExists
  assert.match((await intent.prepare(legacy, { coldkey: 'cold' })).violations.join(), /cannot verify.*unassociated/)
})

test('Node and browser presence readers query the exact runtime key at the selected block', async t => {
  const golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const address = sdk.Keypair.fromUri('//Alice').ss58Address
  const expectedKey = '0x' + runtime.storageKey('SubtensorModule', 'Owner', [address]).toString('hex')
  const browser = require('../dist/browser.js')
  const node = new sdk.Client('local'), web = new browser.Client('local')
  t.after(async () => { await node.close(); await web.close() })
  for (const client of [node, web]) {
    let present = false
    const calls = []
    client.blockHash = async () => BLOCK; client.blockNumber = async () => 100
    client.runtimeAt = async block => { assert.equal(block, BLOCK); return runtime }
    client.at = async () => ({ hash: BLOCK, block: 100 })
    client.rpc = async (method, params) => { calls.push([method, params]); return present ? '0x' + '00'.repeat(32) : null }
    const reader = client === node ? await createNodeIntentReader(client, {}, BLOCK) : await client.intentReader(BLOCK)
    assert.equal(await reader.storageExists('SubtensorModule', 'Owner', [address], BLOCK), false)
    present = true
    assert.equal(await reader.storageExists('SubtensorModule', 'Owner', [address], BLOCK), true)
    assert.deepEqual(calls, Array.from({ length: 2 }, () => ['state_getStorageHash', [expectedKey, BLOCK]]))
  }
  assert(IDEMPOTENT_RPC_METHODS.has('state_getStorageHash'))
})

test('pinned Owner presence reads fall back from pruned primary to a validated archive in Node and browser', async t => {
  const originalFetch = globalThis.fetch
  t.after(() => { globalThis.fetch = originalFetch })
  const golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const address = sdk.Keypair.fromUri('//Alice').ss58Address
  const key = '0x' + runtime.storageKey('SubtensorModule', 'Owner', [address]).toString('hex')
  const genesis = '0x' + '22'.repeat(32), archive = 'http://archive.same-chain.invalid'
  const browser = require('../dist/browser.js')
  for (const kind of ['node', 'browser']) for (const sameChain of [true, false]) {
    const calls = [], primary = []
    globalThis.fetch = async (url, init) => {
      assert.equal(url, archive)
      const request = JSON.parse(init.body)
      calls.push([request.method, request.params])
      let result
      if (request.method === 'chain_getBlockHash') {
        assert.deepEqual(request.params, [0])
        result = sameChain ? genesis : '0x' + '33'.repeat(32)
      } else {
        assert.equal(request.method, 'state_getStorageHash')
        assert.deepEqual(request.params, [key, BLOCK])
        result = '0x' + '00'.repeat(32)
      }
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: request.id, result }), { headers: { 'content-type': 'application/json' } })
    }
    const Client = kind === 'node' ? sdk.Client : browser.Client
    const client = new Client('local', { archiveEndpoints: [archive], expectedGenesisHash: genesis })
    t.after(() => client.close())
    client.genesisHash = async () => genesis
    client.blockHash = async () => BLOCK; client.blockNumber = async () => 100
    client.runtimeAt = async block => { assert.equal(block, BLOCK); return runtime }
    client.at = async () => ({ hash: BLOCK, block: 100 })
    const pruned = async (method, params) => { primary.push([method, params]); throw new Error('State unavailable: state already discarded') }
    if (kind === 'node') client.requireNativeClient = async () => ({ rpcValue: pruned })
    else client.transport.request = pruned
    const reader = kind === 'node' ? await createNodeIntentReader(client, {}, BLOCK) : await client.intentReader(BLOCK)
    const request = reader.storageExists('SubtensorModule', 'Owner', [address], BLOCK)
    if (sameChain) assert.equal(await request, true)
    else await assert.rejects(request, /belongs to another chain/)
    assert.deepEqual(primary, [['state_getStorageHash', [key, BLOCK]]])
    assert.deepEqual(calls, sameChain
      ? [['chain_getBlockHash', [0]], ['state_getStorageHash', [key, BLOCK]]]
      : [['chain_getBlockHash', [0]]])
  }
})
