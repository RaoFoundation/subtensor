'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const { policyEvaluate, evaluatePolicy, evaluatePolicyPreservingValues } = require('../dist/policy')
const corpus = require('../../bittensor-core/tests/fixtures/sdk-policy.json')

test('Node and browser WASM execute the shared Rust policy corpus exactly', async () => {
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  for (const vector of corpus) {
    const encoded = JSON.stringify(vector.input)
    const key = Object.hasOwn(vector, 'error') ? 'error' : 'value'
    const expected = { version: 1, [key]: vector[key] }
    assert.deepEqual(JSON.parse(policyEvaluate(vector.operation, encoded)), expected, 'Node: ' + vector.name)
    assert.deepEqual(JSON.parse(binding.policyEvaluate(vector.operation, encoded)), expected, 'WASM: ' + vector.name)
  }
})

test('policy bridge preserves exact amount and byte input representations', () => {
  assert.equal(evaluatePolicy('validation.amount.units', { left: 9007199254740993n, right: '9007199254740993', mode: 'same' }), true)
  const data = Buffer.from('04010100000000000000', 'hex')
  assert.deepEqual(evaluatePolicy('validation.timelock.envelope', { data }), { inner: '0x01', round: '1' })
  assert.deepEqual(evaluatePolicy('validation.timelock.envelope', { data: new Uint8Array(data) }), { inner: '0x01', round: '1' })
  assert.throws(() => evaluatePolicy('validation.weights.clip', { weights: [Infinity], limit: 1 }), /finite/)
})

test('structural policy preserves enum keys that resemble opaque value tokens', () => {
  const pallet = '__bittensor_policy_value__0'
  const result = evaluatePolicyPreservingValues('protected.decoded_call', {
    call: { [pallet]: { transfer: { value: 9007199254740993n } } },
  })
  assert.equal(result.pallet, pallet)
  assert.equal(result.params.value, 9007199254740993n)
})
