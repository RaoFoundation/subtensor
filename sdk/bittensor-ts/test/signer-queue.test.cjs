const test = require('node:test')
const assert = require('node:assert/strict')
const { Keypair, ss58FromPublic } = require('../dist/keys.js')
const { runSignerQueue, signerQueueOptions, bindSignerIdentity } = require('../dist/signer-queue.js')
const alice = Keypair.fromUri('//Alice'), bob = Keypair.fromUri('//Bob')

test('ordinary submissions wait for the complete two-stage operation across address formats', async () => {
  const client = {}, order = []
  let release, started
  const gate = new Promise(resolve => { release = resolve })
  const entered = new Promise(resolve => { started = resolve })
  const shield = runSignerQueue(client, alice.ss58Address, async permit => {
    order.push('inner')
    await runSignerQueue(client, alice.ss58Address, async () => { order.push('carrier') }, signerQueueOptions({}, permit))
    started()
    await gate
    order.push('inner-receipt')
  })
  await entered
  const normal = runSignerQueue(client, ss58FromPublic(alice.publicKey, 0), async () => { order.push('ordinary') })
  await Promise.resolve()
  assert.deepEqual(order, ['inner', 'carrier'])
  release()
  await Promise.all([shield, normal])
  assert.deepEqual(order, ['inner', 'carrier', 'inner-receipt', 'ordinary'])
})

test('failed operations release the queue and different signers are independent', async () => {
  const client = {}
  let release
  const gate = new Promise(resolve => { release = resolve })
  const blocked = runSignerQueue(client, alice.ss58Address, async () => { await gate; throw new Error('signing canceled') })
  const handled = assert.rejects(blocked, /signing canceled/)
  assert.equal(await runSignerQueue(client, bob.ss58Address, async () => 'bob'), 'bob')
  const later = runSignerQueue(client, alice.ss58Address, async () => 'alice')
  release()
  await handled
  assert.equal(await later, 'alice')
})

test('bound signer preserves method context and rejects account or metadata-mode changes', async () => {
  let account = alice
  const original = { marker: 42, cryptoType: 1, getAccount: () => account, sign(payload) { assert.equal(this.marker, 42); return account.sign(payload) } }
  const identity = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, requiresMetadataProof: false }
  const bound = bindSignerIdentity(original, identity)
  assert.equal((await bound.getAccount({})).address, alice.ss58Address)
  assert.equal(bound.sign(Buffer.from('payload')).length, 64)
  account = bob
  await assert.rejects(bound.getAccount({}), /account changed/)
  account = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, requiresMetadataProof: true }
  await assert.rejects(bound.getAccount({}), /account changed/)
})
