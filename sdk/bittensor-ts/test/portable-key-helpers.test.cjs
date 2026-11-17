'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const node = require('../dist/index.js')
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

test.before(async () => { await browser.initBrowser(loadBrowserWasm) })

test('native and WASM strict verification rejects the legacy wrapper retry for ordinary and hashed keys', () => {
  const payload = Buffer.from('btauth/1\nPOST\n/submit?task=1\nbody\n123\nsender\n-')
  const wrapped = Buffer.concat([Buffer.from('<Bytes>'), payload, Buffer.from('</Bytes>')])
  for (const crypto of [0, 1, 4, 6]) {
    let key = node.Keypair.fromSeed(Buffer.alloc(32, 7), crypto)
    if (crypto === 4 || crypto === 6) key = key.atGeneration(1n)
    const signature = key.sign(payload), wrappedSignature = key.sign(wrapped)
    const other = node.Keypair.fromSeed(Buffer.alloc(32, 8), crypto)
    assert.equal(node.verifySignature(payload, wrappedSignature, key.ss58Address, crypto), true)
    for (const sdk of [node, browser]) {
      assert.equal(sdk.verifyStrict(payload, signature, key.ss58Address, crypto), true)
      assert.equal(sdk.verifyStrict('0x' + payload.toString('hex'), signature, key.ss58Address, crypto), true)
      assert.equal(sdk.verifyStrict(Buffer.from('other message'), signature, key.ss58Address, crypto), false)
      assert.equal(sdk.verifyStrict(payload, signature, other.ss58Address, crypto), false)
      assert.equal(sdk.verifyStrict(payload, wrappedSignature, key.ss58Address, crypto), false)
      assert.equal(sdk.verifyStrict(wrapped, wrappedSignature, key.ss58Address, crypto), true)
      if (crypto < 2) assert.throws(() => sdk.verifyStrict(payload, signature.subarray(1), key.ss58Address, crypto), /signature length/)
    }
  }
})

test('strict verification retains external ECDSA account recovery and error boundaries', () => {
  const { SigningKey, getBytes } = require('ethers')
  const key = new SigningKey('0x' + '01'.repeat(32))
  const payload = Buffer.from('strict external ECDSA payload')
  const wrapped = Buffer.concat([Buffer.from('<Bytes>'), payload, Buffer.from('</Bytes>')])
  const sign = bytes => {
    const signature = key.sign(node.blake2_256(bytes))
    return Uint8Array.from([...getBytes(signature.r), ...getBytes(signature.s), signature.yParity])
  }
  const address = node.ss58FromEcdsaPublic(getBytes(key.compressedPublicKey))
  for (const sdk of [node, browser]) {
    assert.equal(sdk.verifyStrict(payload, sign(payload), address, 2), true)
    assert.equal(sdk.verifySignature(payload, sign(wrapped), address, 2), true)
    assert.equal(sdk.verifyStrict(payload, sign(wrapped), address, 2), false)
    assert.throws(() => sdk.verifyStrict(payload, new Uint8Array(64), address, 2), /ecdsa signature length/)
    assert.throws(() => sdk.verifyStrict(payload, new Uint8Array(64), 'bad SS58', 1), /invalid ss58/)
    assert.throws(() => sdk.verifyStrict(payload, new Uint8Array(64), address, 255), /unknown crypto type/)
    for (const invalid of [-1, 1.5, 256, NaN, Infinity]) {
      assert.throws(() => sdk.verifyStrict(payload, new Uint8Array(64), address, invalid), /cryptoType must be a u8/)
    }
  }
})

test('pure password helpers match wallet encodings and Unicode, NUL, and empty password values', () => {
  for (const [name, password, expected] of [
    ['BT_PW_TEST', 'native-test', 'LDUrOSE6eTE2JzY='],
    ['WALLET_α', 'pässwörd\0\n', 'J4LoPzYjnHjDM0FG'],
    ['A', '', ''],
    ['A', 'a\0\n', 'IEFL'],
  ]) for (const sdk of [node, browser]) {
    assert.equal(sdk.encodePasswordForEnvironment(name, password), expected)
    assert.equal(sdk.decodePasswordFromEnvironment(name, expected), password)
    assert.equal(sdk.decodePasswordFromEnvironment(name, ` \n${expected}\t `), password)
  }
})

test('pure password helpers preserve decoder errors and leave process environment untouched', () => {
  const name = `BT_PW_PORTABLE_HELPERS_${process.pid}`
  const original = process.env[name]
  try {
    process.env[name] = 'existing-value'
    for (const sdk of [node, browser]) {
      const encoded = sdk.encodePasswordForEnvironment(name, 'password covering multiple decoder blocks')
      assert.equal(process.env[name], 'existing-value')
      assert.equal(sdk.decodePasswordFromEnvironment(name, encoded), 'password covering multiple decoder blocks')
      assert.equal(process.env[name], 'existing-value')
      assert.throws(() => sdk.encodePasswordForEnvironment('', 'password'), /env var name must not be empty/)
      assert.throws(() => sdk.decodePasswordFromEnvironment('', ''), /env var name must not be empty/)
      assert.throws(() => sdk.decodePasswordFromEnvironment('A', '!!!!'), /invalid base64/)
      assert.throws(() => sdk.decodePasswordFromEnvironment(name, encoded + '!!!!'), /invalid base64/)
      assert.throws(() => sdk.decodePasswordFromEnvironment('A', 'vg=='), /corrupt UTF-8/)
      const nonVariableName = 'not=an\0os-variable'
      assert.equal(sdk.decodePasswordFromEnvironment(nonVariableName, sdk.encodePasswordForEnvironment(nonVariableName, 'secret')), 'secret')
    }
  } finally {
    if (original === undefined) delete process.env[name]
    else process.env[name] = original
  }
})

test('Node and browser export the same pure helper aliases', () => {
  for (const sdk of [node, browser]) {
    assert.equal(sdk.verify_strict, sdk.verifyStrict)
    assert.equal(sdk.encode_password_for_environment, sdk.encodePasswordForEnvironment)
    assert.equal(sdk.decode_password_from_environment, sdk.decodePasswordFromEnvironment)
  }
})
