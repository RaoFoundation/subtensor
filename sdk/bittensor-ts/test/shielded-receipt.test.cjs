'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { Balance } = require('../dist/balance.js')
const { followShieldedResult } = require('../dist/shielded-receipt.js')
const { BrowserChainClient } = require('../dist/client.js')
const { blake2_256 } = require('../dist/crypto.js')
const outer = { status: 'inBlock', success: true, message: 'Success', extrinsicHash: 'outer', blockNumber: 100, events: [], fee: Balance.fromRao(5), data: { custom: true } }

test('shield receipts return actual inner failure and aggregate actual fees', async () => {
  const result = await followShieldedResult(outer, 'inner', 8, {
    rpc: async method => method === 'chain_getHeader' ? { number: 108 } : 'block',
    findExtrinsic: async () => ({ ...outer, status: 'failed', success: false, message: 'BadOrigin', extrinsicHash: 'inner', fee: Balance.fromRao(7) }),
  })
  assert.equal(result.success, false)
  assert.equal(result.extrinsicHash, 'inner')
  assert.equal(result.carrierExtrinsicHash, 'outer')
  assert.equal(result.fee.rao, 12n)
  assert.equal(result.data.carrier_only, false)
  assert.equal(result.data.custom, true)
})
test('shield finality scans only finalized heights and absent inner fails at expiry', async () => {
  const seen = []
  const result = await followShieldedResult(outer, 'inner', 8, {
    rpc: async (method, params) => {
      seen.push([method, params])
      if (method === 'chain_getFinalizedHead') return 'final'
      if (method === 'chain_getHeader') { assert.deepEqual(params, ['final']); return { number: 108 } }
      return `block-${params[0]}`
    },
    findExtrinsic: async (hash, _block, finalized) => { assert.equal(finalized, true); return hash === 'outer' ? outer : null },
  }, { waitForFinalization: true })
  assert.equal(result.success, false)
  assert.match(result.message, /era expired/)
  assert.equal(result.chainError.code, 'expired')
  assert.equal(seen.filter(([method]) => method === 'chain_getBlockHash').length, 10)
  assert.equal(result.data.inner_expiry_finalized, true)
})
test('shield followup aborts a stalled adapter and does not scan submission-only or failed carriers', async () => {
  await assert.rejects(followShieldedResult(outer, 'inner', 8, { rpc: () => new Promise(() => {}), findExtrinsic: async () => null }, { timeoutMs: 5 }), /timed out/)
  const result = await followShieldedResult({ ...outer, success: undefined, status: 'submitted', blockNumber: undefined }, 'inner', 8, { rpc: async () => { throw Error('not called') }, findExtrinsic: async () => null })
  assert.equal(result.status, 'submitted')
  assert.equal(result.data.carrier_only, true)
})
test('Node receipt decodes upgrade events with parent metadata and surfaces nested failure', async () => {
  const client = new BrowserChainClient('local', { autoConnect: false })
  const extrinsic = Buffer.from([1, 2, 3]), hash = '0x' + blake2_256(extrinsic).toString('hex'), decodedAt = []
  client.runtimeAt = async block => { decodedAt.push(block); return { storageEntry: () => ({ valueType: 0 }), storageKey: () => Buffer.alloc(0), decode: () => [
    { extrinsic_idx: 0, module_id: 'System', event_id: 'ExtrinsicSuccess', attributes: {} },
    { extrinsic_idx: 0, module_id: 'Utility', event_id: 'BatchInterrupted', attributes: { index: 0, error: 'BadOrigin' } },
  ] } }
  client.rpc = async method => method === 'chain_getBlock' ? { block: { header: { number: 10, parentHash: 'parent' }, extrinsics: ['0x010203'] } } : '0x00'
  const result = await client.findExtrinsic(hash, 'upgrade-block')
  assert.equal(result.success, false)
  assert.match(result.message, /BadOrigin/)
  assert.equal(result.chainError.code, 'not_authorized')
  assert.deepEqual(decodedAt, ['parent'])
  await client.close()
})


test('best-head expiry rescans finality and never releases a reorged carrier', async () => {
  for (const carrierPresent of [false, true]) {
    const checks = []
    let finalityRequested = false
    const result = await followShieldedResult(outer, 'inner', 8, {
      rpc: async (method, params) => {
        if (method === 'chain_getFinalizedHead') { finalityRequested = true; return 'final' }
        if (method === 'chain_getHeader') return { number: 108 }
        return `block-${params[0]}`
      },
      findExtrinsic: async (hash, _block, finalized) => { checks.push([hash, finalized]); return hash === 'outer' && carrierPresent ? outer : null },
    })
    assert.equal(finalityRequested, true)
    assert.equal(checks.filter(([hash, finalized]) => hash === 'inner' && finalized).length, 9)
    assert.equal(result.data.inner_expiry_finalized === true, carrierPresent)
    if (!carrierPresent) { assert.equal(result.status, 'unknown'); assert.equal(result.data.submission_uncertain, true) }
  }
})

test('expiry on a best fork cannot hide an inner inclusion in finalized history', async () => {
  const result = await followShieldedResult(outer, 'inner', 8, {
    rpc: async (method, params) => method === 'chain_getHeader' ? { number: 108 } : method === 'chain_getFinalizedHead' ? 'final' : `block-${params[0]}`,
    findExtrinsic: async (hash, _block, finalized) => hash === 'inner' && finalized ? { ...outer, finalized: true, extrinsicHash: 'inner' } : null,
  })
  assert.equal(result.extrinsicHash, 'inner')
  assert.equal(result.success, true)
  assert.equal(result.data.carrier_only, false)
  assert.equal(result.data.inner_expiry_finalized, undefined)
})
