const test = require('node:test')
const assert = require('node:assert/strict')
const { mkdtemp, readFile, writeFile, rm } = require('node:fs/promises')
const { join } = require('node:path')
const { tmpdir } = require('node:os')
const { SdkPreferences, MemoryJsonStore } = require('../dist/preferences.js')
const { FileJsonStore } = require('../dist/node-preferences.js')
const { transferHistory } = require('../dist/history.js')
const { FilePruningJournal } = require('../dist/node-pruning.js')
const { pruneNullSubnet } = require('../dist/pruning.js')

test('preferences serialize writes, validate multisigs and preserve explicit false', async () => {
  const prefs = new SdkPreferences(new MemoryJsonStore())
  await Promise.all([prefs.set('network', 'local'), prefs.set('mev_shield', 'false')])
  assert.deepEqual(await prefs.settings(), { network: 'local', mev_shield: false })
  await Promise.all([prefs.addresses.put({ name: 'alice', address: 'one' }), prefs.addresses.put({ name: '__proto__', address: 'two' })])
  assert.equal(await prefs.resolveAddress('__proto__'), 'two')
  assert.equal((await prefs.addresses.list()).length, 2)
  assert.throws(() => prefs.multisigs.put({ name: 'bad', threshold: 2, signatories: ['same', 'same'] }), /invalid multisig/)
  await prefs.saveLabels('token_symbols', 'local', { 3: 'abc' })
  assert.deepEqual(await prefs.labels('token_symbols', 'local'), { values: { 3: 'abc' }, fresh: true })
})

test('queued preference changes snapshot their inputs and validate loaded books', async () => {
  const store = new MemoryJsonStore(), prefs = new SdkPreferences(store)
  const entry = { name: 'alice', address: 'intended' }
  const put = prefs.addresses.put(entry)
  entry.address = 'mutated'
  await put
  assert.equal(await prefs.resolveAddress('alice'), 'intended')
  const fields = { address: 'updated' }, patch = prefs.addresses.patch('alice', fields)
  fields.address = 'mutated-again'
  await patch
  assert.equal(await prefs.resolveAddress('alice'), 'updated')
  await store.write('addresses', [{ name: 'broken', address: 123 }])
  await assert.rejects(prefs.addresses.list(), /requires an address/)
  assert.throws(() => prefs.multisigs.put({ name: 'bad', threshold: 1, signatories: [null] }), /invalid multisig/)
})

test('filesystem preferences preserve corrupt data and pruning journal reserves exclusively', async () => {
  const path = await mkdtemp(join(tmpdir(), 'ts-preferences-'))
  try {
    const store = new FileJsonStore(path), prefs = new SdkPreferences(store)
    await prefs.set('network', 'local')
    assert.match(await readFile(store.path('config'), 'utf8'), /local/)
    await writeFile(store.path('config'), '{broken')
    await assert.rejects(() => prefs.set('network', 'finney'), SyntaxError)
    assert.equal(await readFile(store.path('config'), 'utf8'), '{broken')
    const journal = new FilePruningJournal(join(path, 'pruning.json'))
    await journal.reserve({ netuid: 1, target: 256, operation: 'trim', state: {} })
    await assert.rejects(() => journal.reserve({}), /EEXIST/)
    assert.equal((await journal.pending()).target, 256)
    await journal.clear()
    assert.equal(await journal.pending(), undefined)
  } finally { await rm(path, { recursive: true, force: true }) }
})

test('history preserves exact amounts, caps paging and rejects foreign next URLs', async () => {
  const row = { from_coldkey: 'alice', to_coldkey: 'bob', from_netuid: 3, to_netuid: 3, amount: '9007199254740993123', block_number: 10, extrinsic_idx: 1, timestamp: 'date', success: true }
  let n = 0
  const fetcher = async () => new Response(JSON.stringify({ results: [row], next: ++n < 2 ? 'http://index.test/history?page=2' : null }))
  const records = await transferHistory('alice', { endpoint: 'https://index.test/history', limit: 2, fetch: fetcher })
  assert.equal(records.length, 2)
  assert.equal(records[0].amount.rao, 9007199254740993123n)
  assert.equal(records[0].amount.netuid, 3)
  assert.equal(records[0].direction, 'out')
  await assert.rejects(() => transferHistory('alice', { endpoint: 'https://index.test/history', fetch: async () => new Response(JSON.stringify({ results: [], next: 'https://elsewhere.test/' })) }), /another origin/)
})

test('history fails closed on malformed or unrelated rows and endless empty pagination', async () => {
  const row = { from_coldkey: 'alice', to_coldkey: 'bob', from_netuid: 0, to_netuid: 0, amount: '10', block_number: 10, extrinsic_idx: 1, timestamp: 'date', success: true }
  const get = value => transferHistory('alice', { endpoint: 'https://index.test/history', fetch: async () => new Response(JSON.stringify(value)) })
  for (const changed of [{ success: 'false' }, { from_coldkey: 'other' }, { from_netuid: null }, { block_number: -1 }, { extrinsic_idx: false }]) await assert.rejects(get({ results: [{ ...row, ...changed }] }), /invalid transfer|does not involve/)
  await assert.rejects(get({ results: [], next: '?page=2' }), /empty page/)
  const values = await transferHistory('alice', { endpoint: 'https://index.test/history', limit: 1, fetch: async () => new Response(JSON.stringify({ results: [row], next: 'https://unused.test/' })) })
  assert.equal(values.length, 1)
})

test('pruning keeps its recovery record after uncertain delivery', async () => {
  let record
  const journal = { pending: async () => record, reserve: async r => { record = r }, clear: async () => { record = undefined } }
  const data = { NetworksAdded: true, SubnetOwner: 'owner', SubnetworkN: 300, NullPruningTarget: null, MechanismCountCurrent: 1, MinAllowedUids: 1, MaxAllowedUids: 1000, SubnetEpochConsensus: 'Null', Tempo: 0, LastEpochBlock: 0, PendingEpochAt: 0, AdminFreezeWindow: 0, BlockHash: 'genesis' }
  const client = { at: async () => ({ block: 100, query: async ([, key]) => data[key] }), mapHasEntries: async () => false }
  await assert.rejects(() => pruneNullSubnet(client, { netuid: 1, owner: 'owner', journal, submit: async () => { throw new Error('transport lost') } }), /transport lost/)
  assert.equal(record.target, 256)
  await assert.rejects(() => pruneNullSubnet(client, { netuid: 1, owner: 'owner', journal, submit: async () => assert.fail('must not repeat') }), /unresolved pruning/)
})

test('pruning retries only a finalized administration-window race and bounds that retry', async () => {
  let record, attempts = 0
  const journal = { pending: async () => record, reserve: async r => { assert.equal(record, undefined); record = r }, clear: async () => { record = undefined } }
  const data = { NetworksAdded: true, SubnetOwner: 'owner', SubnetworkN: 300, NullPruningTarget: null, MechanismCountCurrent: 1, MinAllowedUids: 1, MaxAllowedUids: 1000, SubnetEpochConsensus: 'Null', Tempo: 0, LastEpochBlock: 0, PendingEpochAt: 0, AdminFreezeWindow: 0, BlockHash: 'genesis' }
  const client = { at: async () => ({ block: 100, query: async ([, key]) => data[key] }), mapHasEntries: async () => false }
  const options = { netuid: 1, owner: 'owner', journal, pollIntervalMs: 1, waitTimeoutMs: 100 }
  const failure = { success: false, finalized: true, blockHash: 'block', extrinsicIndex: 0, events: [], chainError: { name: 'AdminActionProhibitedDuringWeightsWindow' } }
  const result = await pruneNullSubnet(client, { ...options, submit: async () => {
    if (++attempts === 1) return failure
    data.SubnetworkN = 256
    return { ...failure, success: true, chainError: undefined, events: [{ module_id: 'SubtensorModule', event_id: 'NullUidsPruningProgress', attributes: { netuid: 1, target: 256, cleared_commits: 0 } }] }
  } })
  assert.equal(result.batches, 1)
  assert.equal(attempts, 2)
  assert.equal(record, undefined)
  data.SubnetworkN = 300
  await assert.rejects(pruneNullSubnet(client, { ...options, waitTimeoutMs: 0, submit: async () => failure }), /wait timed out/)
  assert.equal(record, undefined)
  await assert.rejects(pruneNullSubnet(client, { ...options, submit: async () => ({ ...failure, chainError: { name: 'TxRateLimitExceeded' } }) }), /stopping without retry/)
  data.SubnetworkN = null
  await assert.rejects(pruneNullSubnet(client, { ...options, submit: async () => assert.fail('must not submit malformed chain state') }), /invalid pruning population/)
})
