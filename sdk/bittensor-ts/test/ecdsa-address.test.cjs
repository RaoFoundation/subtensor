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

test('actual browser WASM verifies externally signed ECDSA payloads and rejects another payload or account', async () => {
  const wasm = await browser.initBrowser(loadBrowserWasm)
  const { SigningKey, getBytes } = require('ethers')
  // Public test scalar 1 produces the independent account vector above.
  const externalKey = new SigningKey('0x' + '0'.repeat(63) + '1')
  assert.deepEqual(Buffer.from(getBytes(externalKey.compressedPublicKey)), publicKey)
  const payload = Buffer.from('real WASM external ECDSA verification')
  const signed = externalKey.sign(browser.blake2_256(payload))
  const signature = Uint8Array.from([...getBytes(signed.r), ...getBytes(signed.s), signed.yParity])
  const address = browser.ss58FromPublic(accountId)
  assert.equal(signature.length, 65)
  assert.equal(browser.verifySignature(payload, signature, address, browser.CRYPTO_ECDSA), true)
  assert.equal(browser.verifySignature(Buffer.from('different payload'), signature, address, browser.CRYPTO_ECDSA), false)
  assert.equal(browser.verifySignature(payload, signature, browser.ss58FromPublic(new Uint8Array(32).fill(9)), browser.CRYPTO_ECDSA), false)
  assert.equal(wasm.CryptoType.Ecdsa, 2)
  // This release accepts external ECDSA signers, not internal ECDSA key generation.
  // The WASM enum must not interpret scheme 2 as the omitted/default Sr25519 value.
  assert.throws(() => wasm.Keypair.fromSeed(new Uint8Array(32).fill(1), 2), /unsupported signing scheme/i)
  assert.throws(() => browser.Keypair.fromSeed(new Uint8Array(32).fill(1), 2), /unsupported crypto type/i)
})

test('Node and browser key constructors reject invalid numeric schemes before foreign enum coercion', async () => {
  await browser.initBrowser(loadBrowserWasm)
  const seed = new Uint8Array(32).fill(1)
  const mnemonic = 'bottom drive obey lake curtain smoke basket hold race lonely fit walk'
  for (const sdk of [native, browser]) for (const scheme of [2, 3, 5, -1, 1.5, 256, NaN, Infinity]) {
    for (const construct of [
      () => new sdk.Keypair(sdk.ss58FromPublic(accountId), accountId, scheme),
      () => sdk.Keypair.fromSeed(seed, scheme),
      () => sdk.Keypair.fromMnemonic(mnemonic, scheme),
      () => sdk.Keypair.fromUri('//Alice', scheme),
      () => sdk.Keypair.fromPrivateKey('0x' + '11'.repeat(64), scheme),
      () => sdk.Keypair.generateWithMnemonic(scheme),
    ]) assert.throws(construct, /unsupported crypto type/i)
  }
  assert.throws(() => browser.verifySignature(seed, new Uint8Array(64), browser.ss58FromPublic(accountId), 3), /unsupported signer crypto type/i)
})
