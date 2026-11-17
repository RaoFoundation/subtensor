'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { MemoryJsonStore } = require('../dist/preferences')
const { ProxyAnnouncementStore } = require('../dist/announcement-store')
const genesis = '0x' + '11'.repeat(32), other = '0x' + '22'.repeat(32), callHash = '0x' + '33'.repeat(32)
const input = extra => ({ controller_wallet: 'alice', delegate_ss58: 'controller', real_ss58: 'pure', call_hash: callHash, genesis_hash: genesis, inner_op: 'transfer', inner_args: { dest_ss58: 'recipient', amount_tao: '1' }, delay_blocks: 10, announced_at_block: 100, call_data: '0x1234', runtime_spec_version: 491, call_semantics: { value: 9007199254740993n }, ...extra })
function fixture() { const store = new MemoryJsonStore(); return { store, journal: new ProxyAnnouncementStore(store, value => value.replace('-alias', ''), () => 123) } }

test('announcement records preserve exact JSON-safe bytes and are isolated by chain', async () => {
  const { journal, store } = fixture(), row = await journal.add(input())
  assert.equal(row.call_semantics.value, '9007199254740993')
  assert.equal(row.executable_at_block, 110)
  const before = await store.read('proxy_announcements')
  assert.equal(await journal.update('alice', row.index, { status: 'executed' }, { genesisHash: other }), undefined)
  assert.equal(await journal.remove('alice', row.index, { genesisHash: other }), false)
  assert.equal(await journal.moveWallet('alice', 'bob', { genesisHash: other }), 0)
  assert.deepEqual(await store.read('proxy_announcements'), before)
  const copy = await journal.getByIndex('alice', row.index, { genesisHash: genesis })
  copy.inner_args.amount_tao = '999'
  assert.equal((await journal.getByIndex('alice', row.index, { genesisHash: genesis })).inner_args.amount_tao, '1')
  assert.deepEqual(await journal.listForWallet('alice'), [])
})

test('discovery retains indices, recovers plaintext and distinguishes same-block duplicate commitments', async () => {
  const { journal } = fixture()
  const live = input({ inner_op: '', inner_args: {}, announcement_ordinal: 0 })
  const discovered = await journal.upsertOnChain(live)
  assert.equal(discovered.plaintext_missing, true)
  const attached = await journal.add(input())
  assert.equal(attached.index, discovered.index)
  const duplicate = await journal.upsertOnChain({ ...live, announcement_ordinal: 1 })
  assert.notEqual(duplicate.index, attached.index)
  assert.equal(duplicate.call_data, '0x1234')
  await journal.markStatus('alice', attached.index, 'executed', { genesisHash: genesis })
  const reannounced = await journal.upsertOnChain({ ...live, announced_at_block: 200 })
  assert.notEqual(reannounced.index, attached.index)
  assert.equal(reannounced.inner_op, 'transfer')
  assert.equal(await journal.executedCommitmentIndex(reannounced), attached.index)
  await journal.remove('alice', duplicate.index, { genesisHash: genesis })
  assert.equal((await journal.getByIndex('alice', reannounced.index, { genesisHash: genesis })).announced_at_block, 200)
})

test('provisional approval reuses its displayed index when the commitment lands', async () => {
  const { journal } = fixture(), row = await journal.reserve(input({ announced_at_block: 90 }))
  await journal.markStatus('alice', row.index, 'awaiting_approval', { genesisHash: genesis })
  const live = await journal.upsertOnChain(input({ announced_at_block: 102, delegate_ss58: 'controller-alias' }))
  assert.equal(live.index, row.index)
  assert.equal(live.status, 'pending')
  assert.equal(live.executable_at_block, 112)
  assert.equal(live.call_data, row.call_data)
})

test('atomic reservation rejects duplicates across journal instances before another submission', async () => {
  const { journal, store } = fixture(), second = new ProxyAnnouncementStore(store)
  const attempts = await Promise.allSettled(Array.from({ length: 8 }, (_, i) => (i % 2 ? second : journal).reserve(input())))
  assert.equal(attempts.filter(result => result.status === 'fulfilled').length, 1)
  assert.equal((await journal.all()).length, 1)
  assert(attempts.filter(result => result.status === 'rejected').every(result => /identical announcement/.test(result.reason.message)))
})

test('legacy binding is explicit and conflict failures preserve both histories', async () => {
  const { journal, store } = fixture()
  await journal.add(input({ genesis_hash: undefined }))
  assert.deepEqual(await journal.listForWallet('alice', { genesisHash: genesis }), [])
  await journal.add(input())
  const before = await store.read('proxy_announcements')
  await assert.rejects(journal.bindLegacy('alice', genesis, 'pure'), /both stored copies/)
  assert.deepEqual(await store.read('proxy_announcements'), before)
  assert.equal(await journal.bindLegacy('alice', other, 'pure-alias'), 1)
  assert.equal((await journal.listForWallet('alice', { genesisHash: other }))[0].call_data, '0x1234')
})

test('corrupt persistent journals fail closed and active rebinding preserves completed history', async () => {
  const { journal, store } = fixture(), done = await journal.add(input())
  await journal.markStatus('alice', done.index, 'executed', { genesisHash: genesis })
  const pending = await journal.add(input({ announced_at_block: 200 }))
  assert.equal(await journal.moveWallet('alice', 'bob', { genesisHash: genesis }), 1)
  assert.equal((await journal.getByIndex('alice', done.index, { genesisHash: genesis })).status, 'executed')
  assert.equal((await journal.getByIndex('bob', pending.index, { genesisHash: genesis })).status, 'pending')
  await store.write('proxy_announcements', { version: 1, announcements: [{ index: 'bad' }] })
  await assert.rejects(journal.all(), /index/)
  await assert.rejects(journal.reserve(input()), /index/)
})

test('action claims atomically serialize duplicate identities and recheck executed history', async () => {
  const { journal, store } = fixture(), second = new ProxyAnnouncementStore(store), scope = { genesisHash: genesis }
  const old = await journal.add(input()), young = await journal.add(input({ announced_at_block: 110 }))
  const attempts = await Promise.allSettled([journal.claimAction('alice', old.index, 'execute', scope), second.claimAction('alice', young.index, 'execute', scope)])
  assert.equal(attempts.filter(result => result.status === 'fulfilled').length, 1)
  const winner = attempts.find(result => result.status === 'fulfilled').value, loser = winner.index === old.index ? young : old
  await journal.update('alice', winner.index, { pending_action: null, status: 'executed' }, scope)
  await assert.rejects(second.claimAction('alice', loser.index, 'execute', scope), /already executed/)
  assert.equal((await second.claimAction('alice', loser.index, 'execute', scope, true)).pending_action.kind, 'execute')
  const stale = await journal.upsertOnChain(input({ announced_at_block: winner.announced_at_block }))
  assert.equal(stale.status, 'executed')
  assert.equal(await journal.executedCommitmentIndex(loser), winner.index)
})

test('a historical action read cannot overwrite a subsequent reservation', async () => {
  const { journal } = fixture(), scope = { genesisHash: genesis }, row = await journal.add(input())
  const first = await journal.claimAction('alice', row.index, 'execute', scope)
  await journal.update('alice', row.index, { pending_action: { kind: 'execute', state: 'submission_unknown', receipt: { extrinsicHash: callHash } } }, scope)
  const current = await journal.settleAction('alice', row.index, first.pending_action, { pending_action: null, status: 'failed' }, scope)
  assert.equal(current.status, 'pending')
  assert.equal(current.pending_action.receipt.extrinsicHash, callHash)
})
