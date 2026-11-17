'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const vectors = require('../../bittensor-core/tests/fixtures/policy-financial.json')
const native = require('../native.cjs')
const policy = require('../dist/policy.js')
const gamma = require('../dist/gamma.js')
const emission = require('../dist/emission-weights.js')
const orders = require('../dist/limit-orders.js')

test('financial policy vectors agree through actual native and WASM bindings', async () => {
  const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
  bindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  for (const binding of [native, bindings]) for (const { operation, input, expected } of vectors) {
    const response = JSON.parse(binding.policyEvaluate(operation, JSON.stringify(input)))
    assert.equal(response.error, undefined, `${operation}: ${JSON.stringify(response.error)}`)
    assert.deepEqual(response.value, expected, operation)
  }
})

test('financial wrappers retain exact bigint values and caller-owned order objects', () => {
  assert.equal(gamma.gammaUnits('18446744073.709551615'), 18446744073709551615n)
  assert.equal(emission.parseShare('50%'), 9223372036854775807n)
  assert.throws(() => gamma.gammaFromUnits(Number.MAX_SAFE_INTEGER + 1), /exact integer/)
  assert.throws(() => policy.evaluatePolicy('financial.intent.free_amount', { free: 500001n, existential_deposit: 1n }), /fee headroom/)
  const economic = { signer: 'owner', hotkey: 'hotkey', fee_recipient: 'fee', netuid: 7, amount: 9007199254740993n }
  const original = { V2: economic }
  assert.equal(orders.orderTerms(original), economic)
  const wrapped = orders.authorizedOrder(9007199254740993n, original)
  assert.equal(wrapped.Authorized.nonce, 9007199254740993n)
  assert.equal(wrapped.Authorized.terms.V2.amount, 9007199254740993n)
  assert.notEqual(wrapped.Authorized.terms.V2, economic)
})
