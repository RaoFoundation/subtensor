'use strict'

const assert = require('node:assert/strict')
const test = require('node:test')

const core = require('../dist/index.js')

test('sr25519 keypair forwards signing and SS58 work to Rust', () => {
  const alice = core.Keypair.fromUri('//Alice')
  assert.equal(
    alice.ss58Address,
    '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY',
  )
  const message = Buffer.from('hello')
  const signature = alice.sign(message)
  assert.equal(signature.length, 64)
  assert.equal(alice.verify(message, signature), true)
  assert.deepEqual(core.publicKeyFromSs58(alice.ss58Address), alice.publicKey)
})

test('Rust keypair is compatible with Polkadot.js and Moonwall signer expectations', () => {
  const alice = core.createKeyringPairFromUri('//Alice')
  assert.equal(alice.address, alice.ss58Address)
  assert.equal(alice.type, 'sr25519')
  assert.deepEqual(alice.addressRaw, alice.publicKey)

  const payload = Buffer.from('polkadot-compatible')
  const raw = alice.sign(payload)
  const typed = alice.sign(payload, { withType: true })
  assert.equal(raw.length, 64)
  assert.equal(typed.length, 65)
  assert.equal(typed[0], core.CRYPTO_SR25519)
  assert.deepEqual(typed.subarray(1), raw)
  assert.equal(alice.verify(payload, typed, alice.publicKey), true)
})

test('fallible Runtime construction uses the native factory', () => {
  assert.throws(
    () => new core.Runtime(Buffer.from([0, 1, 2, 3]), 1, 1),
    (error) => error instanceof core.CodecError,
  )
})

test('compact codec is the Rust implementation', () => {
  const values = [0n, 63n, 64n, 16383n, 16384n, 2n ** 64n, 2n ** 127n]
  for (const value of values) {
    const encoded = core.encodeCompact(value)
    const decoded = core.decodeCompactU128(encoded)
    assert.equal(decoded.value, value)
    assert.equal(decoded.remaining, 0)
  }
})

test('dynamic boundary preserves bigint, bytes, and non-string map keys', () => {
  const input = new Map([
    [7n, Buffer.from([0, 1, 254, 255])],
    ['nested', { value: 2n ** 200n, wideSafeNumber: Number.MAX_SAFE_INTEGER }],
  ])
  const output = core.wireRoundtrip(input)
  assert.ok(output instanceof Map)
  assert.deepEqual(output.get(7n), Buffer.from([0, 1, 254, 255]))
  assert.equal(output.get('nested').value, 2n ** 200n)
  assert.equal(output.get('nested').wideSafeNumber, Number.MAX_SAFE_INTEGER)
})

test('public constants and low-level hashes are exposed', () => {
  assert.equal(core.CRYPTO_ED25519, 0)
  assert.equal(core.CRYPTO_SR25519, 1)
  assert.equal(core.MLKEM_NONCE_LENGTH, 24)
  assert.equal(
    core.twox_128(Buffer.from('System')).toString('hex'),
    '26aa394eea5630e07c48ae0c9558cef7',
  )
  assert.equal(core.blake2_256(Buffer.from('System')).length, 32)
  assert.ok(core.PARALLEL_DECODE_THRESHOLD > 0)
})

test('epoch schedule functions stay in Rust', () => {
  const state = {
    lastEpochBlock: 0n,
    pendingEpochAt: 0n,
    subnetEpochIndex: 0n,
    tempo: 100,
    blocksSinceLastStep: 0n,
    currentBlock: 0n,
  }
  const advanced = core.advanceBlocks(state, 1n, 10n)
  assert.equal(typeof advanced.currentBlock, 'bigint')
  assert.equal(advanced.currentBlock, 10n)
})

test('ESM consumers receive the same named exports', async () => {
  const esm = await import('../dist/index.mjs')
  assert.equal(esm.BINDING_VERSION, core.BINDING_VERSION)
  assert.equal(esm.Keypair, core.Keypair)
})
