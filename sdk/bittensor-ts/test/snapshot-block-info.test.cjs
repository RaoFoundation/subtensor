const test = require('node:test')
const assert = require('node:assert/strict')
const { Client, BrowserChainClient } = require('../dist/client')
const hash = byte => '0x' + byte.repeat(32)
const original = hash('11'), replacement = hash('22'), other = hash('33')
const originalParent = hash('aa'), replacementParent = hash('bb')

function forkedClient(ClientClass, t) {
  const client = new ClientClass('local'), storageReads = [], metadataReads = [], blockReads = []
  let canonical = original
  const blocks = new Map([
    [original, { header: { number: '0x2a', parentHash: originalParent }, extrinsics: ['0x0101'] }],
    [replacement, { header: { number: '0x2a', parentHash: replacementParent }, extrinsics: ['0x0202'] }],
    [other, { header: { number: '0x2b', parentHash: replacement }, extrinsics: ['0x0303'] }],
  ])
  client.blockHash = async number => number === 43 ? other : canonical
  client.finalizedHead = async () => canonical
  client.blockNumber = async () => 42
  client.runtimeAt = async at => { metadataReads.push(at); return { decodeExtrinsic: bytes => ({ bytes: [...bytes], runtime: at }) } }
  client.rpc = async (method, params) => {
    assert.equal(method, 'chain_getBlock'); blockReads.push(params[0])
    return blocks.has(params[0]) ? { block: blocks.get(params[0]) } : null
  }
  client.query = async (item, params, at) => {
    storageReads.push(at)
    if (item[0] === 'Timestamp') return at === original ? 1000n : 2000n
    return at === original ? 'original storage' : 'replacement storage'
  }
  t.after(() => client.close())
  return { client, storageReads, metadataReads, blockReads, reorg: () => { canonical = replacement } }
}

for (const [name, ClientClass] of [['Node', Client], ['legacy client', BrowserChainClient]]) {
  test(`${name} snapshot blockInfo remains on the captured fork after a reorg`, async t => {
    const f = forkedClient(ClientClass, t), snapshot = await f.client.at(42)
    f.reorg()
    assert.equal(await snapshot.query(['System', 'Account']), 'original storage')
    for (const info of [await snapshot.blockInfo(), await snapshot.block_info(null)]) {
      assert.equal(info.number, 42); assert.equal(info.hash, original)
      assert.deepEqual(info.extrinsics, ['0x0101'])
      assert.deepEqual(info.decodedExtrinsics, [{ bytes: [1, 1], runtime: originalParent }])
      assert.equal(info.timestamp.getTime(), 1000)
    }
    assert.ok(f.storageReads.every(at => at === original))
    assert.deepEqual(f.metadataReads, [originalParent, originalParent])
    assert.deepEqual(f.blockReads, [original, original])
  })

  test(`${name} blockInfo accepts explicit hashes and preserves numeric overrides`, async t => {
    const f = forkedClient(ClientClass, t), snapshot = await f.client.at(42)
    f.reorg()
    assert.equal((await f.client.blockInfo(original)).hash, original)
    assert.equal((await f.client.block_info(original)).hash, original)
    assert.equal((await snapshot.blockInfo(original)).hash, original)
    assert.equal((await snapshot.block_info(42)).hash, replacement)
    assert.equal((await snapshot.blockInfo(43)).hash, other)
    assert.equal((await f.client.blockInfo()).hash, replacement)
    assert.equal(await snapshot.blockInfo(hash('ff')), null)
  })
}

test('browser snapshots and direct blockInfo calls use explicit hashes', async t => {
  const { Client: BrowserClient } = await import('../dist/browser.mjs')
  const f = forkedClient(BrowserClient, t), snapshot = await f.client.at(42)
  f.reorg()
  assert.equal(await snapshot.query(['System', 'Account']), 'original storage')
  const info = await snapshot.blockInfo()
  assert.equal(info.hash, original)
  assert.equal(info.timestamp.getTime(), 1000)
  assert.deepEqual(info.decodedExtrinsics, [{ bytes: [1, 1], runtime: originalParent }])
  assert.equal((await snapshot.blockInfo(replacement)).hash, replacement)
  assert.equal((await snapshot.blockInfo(43)).hash, other)
})
