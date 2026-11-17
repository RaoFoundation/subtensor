'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const core = require('../dist')

const blockHash = '0x' + '11'.repeat(32)
const parentHash = '0x' + '22'.repeat(32)
const reference = '0x' + 'ab'.repeat(32)
const skipped = index => ({
  extrinsic_idx: index,
  module_id: 'SubtensorModule', event_id: 'GammaSpendSkipped',
  attributes: { netuid: 2, hotkey: 'hotkey', amount: 7n, reference, reason: { Module: { index: 7, error: [195, 0, 0, 0] } } },
})

test('Node native receipts resolve Gamma skip reasons with the execution parent and retain native data', async t => {
  const client = new core.Client('local'), requests = [], runtimes = []
  t.after(() => client.close())
  client.runtimeAt = async at => {
    runtimes.push(at)
    return { decodeExtrinsic: () => ({}), moduleError: (pallet, error) => {
      assert.equal(at, parentHash)
      assert.deepEqual([pallet, error], [7, 195])
      return { name: 'GammaDailySpendCapExceeded', docs: [] }
    } }
  }
  client.rpc = async (method, params) => {
    requests.push([method, params])
    assert.equal(method, 'chain_getHeader')
    assert.deepEqual(params, [blockHash])
    return { number: '0x2a', parentHash }
  }
  let outcome = { success: true, message: 'Success', extrinsicHash: '0x1234', blockHash, blockNumber: 42n, extrinsicIndex: 0, events: [skipped(0), skipped(0)], data: { retained: true } }
  client.requireNativeClient = async () => ({ submitEncoded: async () => outcome })
  const result = await client.submitSigned(Buffer.from([1]))
  assert.equal(result.success, true)
  assert.equal(result.data.retained, true)
  assert.deepEqual(result.data.spent, [])
  assert.equal(result.data.skipped.length, 2)
  assert.deepEqual(result.data.skipped[0], { netuid: 2, hotkey_ss58: 'hotkey', amount_gamma: '0.000000007', reference, reason: 'GammaDailySpendCapExceeded' })
  assert.equal(requests.length, 1)
  assert.deepEqual(runtimes, [undefined, parentHash])

  outcome = { ...outcome, events: [], data: { retained: true } }
  const ordinary = await client.submitSigned(Buffer.from([1]))
  assert.deepEqual(ordinary.data, { retained: true })
  assert.equal(requests.length, 1, 'ordinary receipts need no additional metadata RPC')
})

test('JS inclusion and shielded lookup enrich only the matching extrinsic Gamma events', async t => {
  const client = new core.BrowserChainClient('local'), extrinsic = Buffer.from([1, 2, 3])
  t.after(() => client.close())
  const hash = '0x' + core.blake2_256(extrinsic).toString('hex')
  const events = [skipped(0), { extrinsic_idx: 0, module_id: 'System', event_id: 'ExtrinsicSuccess' }, skipped(1)]
  client.runtimeAt = async at => {
    assert.equal(at, parentHash)
    return {
      storageEntry: () => ({ valueType: 'events', defaultBytes: Buffer.alloc(0) }),
      storageKey: () => Buffer.from([7]), decode: () => events,
      moduleError: () => ({ name: 'GammaInsufficientBalance', docs: [] }),
    }
  }
  client.rpc = async method => {
    if (method === 'chain_getBlock') return { block: { header: { number: '0x2a', parentHash }, extrinsics: ['0x' + extrinsic.toString('hex')] } }
    if (method === 'state_getStorage') return '0x01'
    throw new Error(method)
  }
  const result = await client.findExtrinsic(hash, blockHash, true)
  assert.equal(result.success, true)
  assert.equal(result.status, 'finalized')
  assert.equal(result.data.skipped.length, 1)
  assert.equal(result.data.skipped[0].reason, 'GammaInsufficientBalance')
})
