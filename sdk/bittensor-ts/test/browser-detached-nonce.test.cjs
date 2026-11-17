'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const browser = require('../dist/browser')
const { Client } = require('../dist/browser-client')
const alice = sdk.Keypair.fromUri('//Alice')
const signer = { address: alice.ss58Address, publicKey: alice.publicKey, sign() { throw new Error('must not sign during preparation') } }

test.before(async () => {
  await browser.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default
    return module
  })
})

// PROTECTED_REGRESSION: detached browser preparations cannot silently reuse a nonce reserved for another transaction; changes require explicit human approval.
test('browser detached preparation requires an explicit nonce before chain I/O', async t => {
  const client = new Client('local'); t.after(() => client.close())
  let prepared = 0
  client.prepareRaw = async (_call, _signer, options) => { prepared++; return { nonce: options.nonce ?? 7 } }
  await assert.rejects(client.prepareCall({}, signer, { allowRawCall: true }), /explicit nonce/)
  await assert.rejects(client.prepareCall({}, signer, { allowRawCall: true, nonce: null }), /explicit nonce/)
  assert.equal(prepared, 0)
  assert.equal((await client.prepareCall({}, signer, { allowRawCall: true, nonce: 7 })).nonce, 7)
  assert.equal(prepared, 1)
})

// PROTECTED_REGRESSION: explicitly reserved browser signing nonces must remain distinct from later automatic submissions; changes require explicit human approval.
test('browser detached plans use reserved nonces and automatic submission advances past them', async t => {
  const client = new Client('local'); t.after(() => client.close())
  client.rpc = async method => { assert.equal(method, 'system_accountNextIndex'); return 7 }
  client.prepareRaw = async (_call, _signer, options) => ({ nonce: options.nonce })
  const firstNonce = await client.reserveNonces(alice.ss58Address), secondNonce = await client.reserveNonces(alice.ss58Address)
  const first = await client.prepareCall({}, signer, { allowRawCall: true, nonce: firstNonce })
  const second = await client.prepareCall({}, signer, { allowRawCall: true, nonce: secondNonce })
  client.signPrepared = async unsigned => ({ bytes: new Uint8Array([unsigned.nonce]) })
  client.broadcastExtrinsic = async bytes => ({ status: 'submitted', extrinsicHash: 'hash', events: [], nonce: bytes[0] })
  client.blockHash = async () => 'block'
  const submitted = await client.submit({}, signer, { allowRawCall: true })
  assert.deepEqual([first.nonce, second.nonce, submitted.nonce], [7, 8, 9])
})
