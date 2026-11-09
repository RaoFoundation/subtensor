'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { Client } = require('../dist/client')
const blockHash = '0x' + '11'.repeat(32), parentHash = '0x' + '22'.repeat(32)
const event = (module_id, event_id, attributes = {}) => ({ module_id, event_id, attributes })
const success = event('System', 'ExtrinsicSuccess')

function fixture(t, events, overrides = {}) {
  const client = new Client('local'), reads = []
  t.after(() => client.close())
  client.runtimeAt = async at => ({ decodeExtrinsic: () => ({}), moduleError: (pallet, error) => {
    assert.equal(at, parentHash)
    assert.deepEqual([pallet, error], [5, 2])
    return { name: 'Balances.InsufficientBalance', docs: ['Balance too low.'] }
  } })
  client.rpc = async (method, params) => { reads.push([method, params]); assert.equal(method, 'chain_getHeader'); assert.deepEqual(params, [blockHash]); return { number: '0x2a', parentHash } }
  client.requireNativeClient = async () => ({ submitEncoded: async () => ({ success: true, message: 'extrinsic succeeded', extrinsicHash: '0x1234', blockHash, blockNumber: 42n, extrinsicIndex: 0, feeRao: '7', events, data: { retained: true }, ...overrides }) })
  return { client, reads }
}

test('Node receipts report each nested wrapper failure despite System.ExtrinsicSuccess', async t => {
  for (const [pallet, name, field] of [['Sudo', 'Sudid', 'sudo_result'], ['Sudo', 'SudoAsDone', 'sudo_result'], ['Proxy', 'ProxyExecuted', 'result'], ['Multisig', 'MultisigExecuted', 'result'], ['Utility', 'BatchInterrupted', 'error'], ['Utility', 'ItemFailed', 'error']]) {
    const result = field === 'error' ? 'BadOrigin' : { Err: 'BadOrigin' }
    const events = [event(pallet, name, { [field]: { Ok: null } }), event(pallet, name, { [field]: result }), success]
    if (field === 'error') events.shift()
    const { client, reads } = fixture(t, events)
    const receipt = await client.submitSigned(Buffer.from([1]), undefined, { waitForFinalization: true })
    assert.equal(receipt.success, false, `${pallet}.${name}`)
    assert.equal(receipt.status, 'failed')
    assert.equal(receipt.chainError.name, 'BadOrigin')
    assert.match(receipt.message, /BadOrigin/)
    assert.equal(receipt.finalized, true)
    assert.equal(receipt.fee.rao, 7n)
    assert.deepEqual(receipt.events, events)
    assert.equal(receipt.data.retained, true)
    assert.deepEqual(reads, [])
  }
})

test('Node nested module errors use execution-parent metadata and survive metadata lookup failures', async t => {
  const events = [event('Sudo', 'Sudid', { sudo_result: { Err: { Module: { index: 5, error: '0x02000000' } } } }), success]
  const { client, reads } = fixture(t, events)
  const receipt = await client.submitSigned(Buffer.from([1]))
  assert.equal(receipt.success, false)
  assert.equal(receipt.chainError.name, 'InsufficientBalance')
  assert.equal(receipt.chainError.pallet, 'Balances')
  assert.match(receipt.message, /Balances\.InsufficientBalance/)
  assert.deepEqual(reads, [['chain_getHeader', [blockHash]]])
  client.rpc = async () => { throw new Error('archive unavailable') }
  const unavailable = await client.submitSigned(Buffer.from([1]))
  assert.equal(unavailable.success, false)
  assert.equal(unavailable.status, 'failed')
  assert.match(unavailable.message, /Unknown module error 5\/2/)
})

test('Node receipt normalization preserves uncertain submissions and existing native failures', async t => {
  const events = [event('Sudo', 'Sudid', { sudo_result: { Err: 'BadOrigin' } }), success]
  const uncertain = fixture(t, events, { success: false, message: 'submission unresolved', data: { submission_uncertain: true } })
  const pending = await uncertain.client.submitSigned(Buffer.from([1]))
  assert.equal(pending.status, 'unknown')
  assert.equal(pending.success, undefined)
  assert.equal(pending.message, 'submission unresolved')
  assert.equal(pending.chainError, undefined)
  assert.deepEqual(uncertain.reads, [])
  const error = { pallet: 'Balances', name: 'InsufficientBalance', docs: [], semanticCode: 'insufficient_balance' }
  for (const nativeSuccess of [false, true]) {
    const failed = fixture(t, events, { success: nativeSuccess, message: 'Balances.InsufficientBalance', error })
    const rejected = await failed.client.submitSigned(Buffer.from([1]))
    assert.equal(rejected.status, 'failed')
    assert.equal(rejected.success, false)
    assert.equal(rejected.message, 'Balances.InsufficientBalance')
    assert.deepEqual(rejected.error, error)
    assert.deepEqual(failed.reads, [])
  }
})

test('malformed or inconsistent receipt headers cannot resolve module errors with post-state metadata', async t => {
  const events = [event('Sudo', 'Sudid', { sudo_result: { Err: { Module: { index: 5, error: '0x02000000' } } } }), success]
  for (const header of [null, {}, { number: null }, { number: '' }, { number: -1 }, { number: 1.5 }, { number: Number.MAX_SAFE_INTEGER + 1 }, { number: '0x0', parentHash }, { number: '0x2b', parentHash }]) {
    const { client } = fixture(t, events)
    const metadataReads = []
    client.runtimeAt = async at => {
      if (at != null) metadataReads.push(at)
      return { decodeExtrinsic: () => ({}), moduleError: () => ({ name: 'WrongRuntime.WrongError', docs: [] }) }
    }
    client.rpc = async () => header
    const receipt = await client.submitSigned(Buffer.from([1]))
    assert.equal(receipt.success, false)
    assert.equal(receipt.status, 'failed')
    assert.match(receipt.message, /Unknown module error 5\/2/)
    assert.deepEqual(metadataReads, [], JSON.stringify(header))
  }
})

test('genesis receipt module errors resolve against the inclusion runtime', async t => {
  const events = [event('Sudo', 'Sudid', { sudo_result: { Err: { Module: { index: 5, error: '0x02000000' } } } }), success]
  const { client } = fixture(t, events, { blockNumber: 0n })
  const metadataReads = []
  client.runtimeAt = async at => {
    if (at != null) metadataReads.push(at)
    return { decodeExtrinsic: () => ({}), moduleError: (pallet, error) => {
      assert.deepEqual([at, pallet, error], [blockHash, 5, 2])
      return { name: 'Balances.InsufficientBalance', docs: ['Balance too low.'] }
    } }
  }
  client.rpc = async () => ({ number: '0x0', parentHash })
  const receipt = await client.submitSigned(Buffer.from([1]))
  assert.equal(receipt.success, false)
  assert.equal(receipt.status, 'failed')
  assert.equal(receipt.chainError.name, 'InsufficientBalance')
  assert.deepEqual(metadataReads, [blockHash])
})

test('successful wrappers and unexecuted approvals remain successful without metadata reads', async t => {
  for (const events of [[success], [event('Sudo', 'Sudid', { sudo_result: { Ok: null } }), success], [event('Multisig', 'MultisigApproval'), success]]) {
    const { client, reads } = fixture(t, events)
    const receipt = await client.submitSigned(Buffer.from([1]))
    assert.equal(receipt.success, true)
    assert.equal(receipt.status, 'inBlock')
    assert.equal(receipt.chainError, undefined)
    assert.deepEqual(reads, [])
  }
})
