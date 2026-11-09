"use strict"
const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const native = require('../dist/index.js')
const browser = require('../dist/browser.js')

async function loadBrowserWasm() {
  const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(
    fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')),
    { './bittensor_core_wasm_bg.js': bindings },
  )
  bindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  const module = { ...bindings }
  delete module.default
  return module
}

const publicKey = Buffer.from('0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798', 'hex')
// Independent BLAKE2b-256 vector for the compressed secp256k1 generator point.
const accountId = Buffer.from('2975f1d28b92b6e84499b83b0797ef5235553eeb7edaa0cea243c1128c2fe737', 'hex')

test('native and browser ECDSA address helpers preserve the chain account mapping', async () => {
  browser.configureBrowserWasm(loadBrowserWasm)
  await browser.initBrowser()
  for (const sdk of [native, browser]) {
    assert.deepEqual(Buffer.from(sdk.accountIdFromEcdsaPublic(publicKey)), accountId)
    assert.equal(sdk.ss58FromEcdsaPublic(publicKey), sdk.ss58FromEcdsaPublic(publicKey, 42))
    for (const prefix of [0, 42, 4096]) {
      const address = sdk.ss58FromEcdsaPublic(publicKey, prefix)
      assert.equal(address, native.ss58FromPublic(accountId, prefix))
      assert.deepEqual(Buffer.from(sdk.publicKeyFromSs58(address)), accountId)
    }
    for (const invalid of [publicKey.subarray(1), Buffer.alloc(33, 4), Buffer.alloc(65, 4)]) {
      assert.throws(() => sdk.accountIdFromEcdsaPublic(invalid), /ecdsa public key/)
      assert.throws(() => sdk.ss58FromEcdsaPublic(invalid), /ecdsa public key/)
    }
  }
})
