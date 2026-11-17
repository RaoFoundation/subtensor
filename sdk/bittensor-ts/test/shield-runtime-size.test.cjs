'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { WorkflowClient } = require('../dist/node-workflows')
const { encodeIntentCall } = require('../dist/intents')
const { shieldedCiphertextSize, validateShieldedCiphertextSize } = require('../dist/shield-size')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const runtime = new sdk.Runtime(metadata, 419, 1, 42), key = sdk.Keypair.fromUri('//Alice')
const block = '0x' + '11'.repeat(32), encryptionKey = Buffer.alloc(1184, 0x42)
const weights = { pallet: 'SubtensorModule', call: 'set_weights', params: { netuid: 1, dests: Array.from({ length: 2048 }, (_, i) => i), weights: Array(2048).fill(1), version_key: 0n } }
const tx = nonce => ({ era: { period: 8, current: 99 }, nonce, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32), metadataHashEnabled: false })
const compose = call => encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))

// PROTECTED_REGRESSION: validators must submit complete supported weight vectors through Shield; changes require explicit human approval.
test('Node broadcasts a real signed and encrypted 2048-destination weight call', async t => {
  const client = new sdk.Client('local'), signed = [], carriers = [], broadcasts = []
  t.after(() => client.close())
  Object.assign(client, {
    requireNativeClient: async () => ({}), runtimeAt: async () => runtime,
    blockHash: async () => block, query: async () => encryptionKey,
    signExtrinsic: async (call, signer, options) => {
      const data = call instanceof Uint8Array ? call : await compose({ pallet: call.pallet, call: call.function ?? call.call, params: call.params })
      const params = tx(options.nonce), payload = runtime.signaturePayload(data, params)
      const signature = key.sign(payload)
      assert.equal(key.verify(payload, signature), true)
      const result = runtime.encodeSignedExtrinsic(data, key.publicKey, signature, 1, params)
      signed.push(result)
      return result
    },
    composeCall: async (pallet, call, params) => { carriers.push(params.ciphertext); return runtime.composeCall(pallet, call, params) },
    submitSigned: async bytes => { broadcasts.push(bytes); return { status: 'submitted', message: 'Submitted', extrinsicHash: '0xcarrier', events: [] } },
  })
  const result = await new WorkflowClient(client).submitRawShielded(await compose(weights), key, { allowRawCall: true, nonce: 0, waitForInclusion: false })
  assert.equal(carriers[0].length, 9460)
  assert.equal(carriers[0].length, signed[0].bytes.length + 1146)
  assert.equal(broadcasts.length, 1)
  assert.equal(result.innerExtrinsicHash, '0x' + Buffer.from(signed[0].hash).toString('hex'))
})

test('shared Shield size policy accepts both weight tiers through32768 and rejects overflow', async () => {
  for (const size of [8192, 8193, 9460, 32768]) {
    assert.equal(validateShieldedCiphertextSize(size), size)
    assert.equal(shieldedCiphertextSize(size - 1146), size)
  }
  assert.throws(() => validateShieldedCiphertextSize(32769), /32768/)
  assert.throws(() => shieldedCiphertextSize(32769 - 1146), /32768/)
  assert.throws(() => shieldedCiphertextSize(-1), /nonnegative/)
})

test('browser quotes the real carrier tier and nonce width without signing', async t => {
  const browser = await import('../dist/browser.mjs')
  browser.configureBrowserWasm(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports); instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default; return module
  })
  await browser.initBrowser()
  const client = new browser.Client('local'), carriers = [], quotes = []
  t.after(() => client.close())
  Object.assign(client, {
    blockHash: async () => block, blockNumber: async () => 100, genesisHash: async () => block,
    runtimeEntry: async () => ({ runtime, metadata, version: { specName: 'test' }, properties: {} }),
    compose: async call => {
      if (call.pallet === 'MevShield') carriers.push(call.params.ciphertext.length)
      return compose(call)
    },
    intentReader: async () => ({ query: async () => key.ss58Address, countStorageKeys: async () => 1 }),
    rpc: async (method, params) => {
      assert.equal(method, 'payment_queryInfo')
      const bytes = Buffer.from(params[0].slice(2), 'hex'), decoded = runtime.decodeExtrinsic(bytes)
      quotes.push(decoded)
      return { partialFee: String(bytes.length) }
    },
  })
  const signer = { address: key.ss58Address, publicKey: key.publicKey, cryptoType: 1, signBytes: async () => assert.fail('fee preview must not sign') }
  for (const [call, nonce] of [[{ pallet: 'System', call: 'remark', params: { remark: Buffer.from('small') } }, 0], [weights, 0], [weights, 63]]) {
    carriers.length = 0; quotes.length = 0
    const result = await client.estimateShieldedFees(call, signer, { nonce, metadataHash: null }, block)
    const bytes = await compose(call), params = tx(nonce + 1)
    const actual = runtime.encodeSignedExtrinsic(bytes, key.publicKey, key.sign(runtime.signaturePayload(bytes, params)), 1, params)
    const encrypted = sdk.sealMevShieldTransaction(encryptionKey, actual.bytes)
    assert.deepEqual(carriers, [encrypted.length])
    assert.deepEqual(quotes.map(quote => Number(quote.nonce)), [nonce + 1, nonce])
    assert.equal(encrypted.length > 8192, call === weights)
    assert.equal(result.innerFee.rao, BigInt(actual.bytes.length))
    assert.ok(result.carrierFee.rao > BigInt(encrypted.length))
  }
})
