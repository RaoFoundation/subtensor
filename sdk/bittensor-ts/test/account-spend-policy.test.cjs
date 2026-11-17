'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { policyEvaluate } = require('../dist/policy')
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const reader = { query: async () => null }
const cases = [
  ['root_register', { hotkey_ss58: bob.ss58Address }],
  ['swap_hotkey', { hotkey_ss58: alice.ss58Address, new_hotkey_ss58: bob.ss58Address }],
  ['swap_hotkey', { hotkey_ss58: alice.ss58Address, new_hotkey_ss58: bob.ss58Address, netuid: 1 }],
  ['announce_coldkey_swap', { new_coldkey_ss58: bob.ss58Address }],
  ['swap_coldkey_announced', { new_coldkey_ss58: bob.ss58Address }],
]

// PROTECTED_REGRESSION: burns and full coldkey migrations cannot bypass semantic or native TAO spend caps; changes require explicit human approval.
test('account burns and migrations reject zero and finite caps before signing', async () => {
  for (const [op, args] of cases) {
    const intent = sdk.buildIntent(op, args)
    const prepared = await intent.prepare(reader, { coldkey: alice.ss58Address })
    const native = sdk.IntentCall.operation(`${prepared.call.pallet}.${prepared.call.call}`, prepared.call.params)
    assert.equal(prepared.spendRao, 'unbounded', op)
    for (const maxSpendRao of [0n, (1n << 128n) - 1n]) {
      assert.match(sdk.checkIntentPolicy(prepared, { maxSpendRao, maxFeeRao: 0n }, 0n).join(';'), /unbounded spend/, op)
      assert.match(new sdk.Policy({ maxSpendRao, maxFeeRao: 0n }).check(native, 0n).join(';'), /unbounded spend/, op)
    }
    const batch = await sdk.buildIntent('batch', { intents: [intent] }).prepare(reader, { coldkey: alice.ss58Address })
    assert.match(sdk.checkIntentPolicy(batch, { maxSpendRao: 0n }).join(';'), /unbounded spend/, op)
    assert.deepEqual(sdk.checkIntentPolicy(prepared, { maxFeeRao: 0n }, 0n), [], op)
    assert.deepEqual(new sdk.Policy({ maxFeeRao: 0n }).check(native, 0n), [], op)
  }
  assert.match(new sdk.Policy({ maxSpendRao: 0n }).check(sdk.IntentCall.rootRegister(bob.ss58Address), 0n).join(';'), /unbounded spend/)
})

// PROTECTED_REGRESSION: browser WASM must enforce the same TAO burn spend cap as native policy; changes require explicit human approval.
test('native and browser WASM agree on operation burns and migration spend', async () => {
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  for (const [op, args] of cases) {
    const input = JSON.stringify({ op, args })
    const native = JSON.parse(policyEvaluate('transaction.intent_metadata', input))
    const browser = JSON.parse(binding.policyEvaluate('transaction.intent_metadata', input))
    assert.equal(native.value.spend_rao, 'unbounded', op)
    assert.deepEqual(browser, native, op)
    const policy = JSON.stringify({ max_spend_rao: '0', spend_rao: browser.value.spend_rao })
    assert.deepEqual(JSON.parse(binding.policyEvaluate('transaction.intent_policy', policy)).value.violations, [{ code: 'unbounded', cap: '0' }])
  }
})
