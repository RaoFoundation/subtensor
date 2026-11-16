'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { followShieldedResult } = require('../dist/shielded-receipt')
const { SubmissionUncertainError, isDefiniteSubmissionRejection } = require('../dist/transport')
const { Balance } = require('../dist/balance')
const outer = { status: 'finalized', success: true, message: 'Carrier included', extrinsicHash: '0x' + '11'.repeat(32), blockHash: '0x' + '22'.repeat(32), blockNumber: 100, extrinsicIndex: 2, finalized: true, events: [{ carrier: true }], fee: Balance.fromRao(5n), data: { context: 'saved intent' } }
const innerHash = '0x' + '33'.repeat(32)

// PROTECTED_REGRESSION: interrupted Shield tracking must retain both hashes so an included payment is not retried blindly; changes require explicit human approval.
test('Shield tracking failures preserve structured uncertainty and both reconciliation hashes', async () => {
  for (const failure of ['rpc', 'receipt', 'timeout', 'abort']) {
    const controller = new AbortController()
    const cause = new Error(`${failure} interrupted tracking`)
    const adapter = {
      rpc: async method => {
        if (failure === 'rpc') throw cause
        if (failure === 'timeout') return new Promise(() => {})
        if (failure === 'abort') { controller.abort(cause); return new Promise(() => {}) }
        return method === 'chain_getHeader' ? { number: 100 } : outer.blockHash
      },
      findExtrinsic: async () => { throw cause },
    }
    await assert.rejects(followShieldedResult(outer, innerHash, 8, adapter, { timeoutMs: 20, signal: controller.signal }), error => {
      assert.equal(error.submissionUncertain, true)
      assert.ok(error instanceof SubmissionUncertainError)
      assert.equal(isDefiniteSubmissionRejection(error), false)
      assert.equal(error.innerExtrinsicHash, innerHash)
      assert.equal(error.carrierExtrinsicHash, outer.extrinsicHash)
      assert.equal(error.extrinsicHash, innerHash)
      assert.equal(error.outcome.status, 'unknown')
      assert.equal(error.outcome.success, undefined)
      assert.equal(error.outcome.extrinsicHash, innerHash)
      assert.equal(error.outcome.innerExtrinsicHash, innerHash)
      assert.equal(error.outcome.carrierExtrinsicHash, outer.extrinsicHash)
      assert.equal(error.outcome.data.submission_uncertain, true)
      assert.equal(error.outcome.data.inner_expiry_finalized, undefined)
      assert.equal(error.outcome.data.carrier_only, true)
      assert.equal(error.outcome.data.context, 'saved intent')
      assert.equal(error.outcome.blockHash, undefined, 'carrier inclusion must not be attributed to the inner transaction')
      assert.equal(error.outcome.finalized, undefined)
      assert.deepEqual(error.outcome.events, [])
      assert.deepEqual(error.carrierReceipt, outer)
      assert.equal(error.outcome.carrierFee.rao, 5n)
      assert.match(error.message, /reconcile/i)
      assert.ok(error.cause)
      if (failure === 'rpc' || failure === 'receipt') assert.equal(error.cause, cause)
      const serialized = JSON.stringify(error)
      assert.ok(serialized.includes(innerHash))
      assert.ok(serialized.includes(outer.extrinsicHash))
      return true
    })
  }
})

// PROTECTED_REGRESSION: reconcile the original Shield payment after interrupted tracking instead of retrying it; changes require explicit human approval.
test('an interrupted Shield operation can later be reconciled as included using its saved hash', async () => {
  let trackingError
  try {
    await followShieldedResult(outer, innerHash, 8, { rpc: async () => { throw Error('connection lost') }, findExtrinsic: async () => null })
  } catch (error) { trackingError = error }
  assert.equal(trackingError.submissionUncertain, true)
  const result = await followShieldedResult(trackingError.carrierReceipt, trackingError.innerExtrinsicHash, 8, {
    rpc: async method => method === 'chain_getHeader' ? { number: 101 } : 'later-block',
    findExtrinsic: async hash => ({ status: 'finalized', success: true, finalized: true, message: 'Payment succeeded', extrinsicHash: hash, blockHash: 'later-block', blockNumber: 101, events: [], fee: Balance.fromRao(7n) }),
  }, { waitForFinalization: true })
  assert.equal(result.success, true)
  assert.equal(result.extrinsicHash, innerHash)
  assert.equal(result.carrierExtrinsicHash, outer.extrinsicHash)
  assert.equal(result.data.submission_uncertain, undefined)
  assert.equal(result.fee.rao, 12n)
})
