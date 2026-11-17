'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const browser = require('../dist/browser.js')
const { runSignerQueue } = require('../dist/signer-queue.js')
const { extrinsicSignerAccountId } = require('../dist/extrinsic-signer.js')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const runtime = new sdk.Runtime(metadata, 419, 1, 42)
const recipient = sdk.Keypair.fromUri('//Bob')
const call = runtime.composeCall('Balances', 'transfer_allow_death', { dest: { Id: recipient.publicKey }, value: 10_000_000n })
const params = { era: '00', nonce: 7, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32) }
const extra = runtime.signaturePayloadParts(params).includedInExtrinsic
const profiles = [[4, 1, 106], [6, 3, 106], [7, 4, 5367], [8, 5, 5367], [10, 6, 1669], [9, 7, 1669], [12, 8, 3179], [11, 9, 3179]]
const tick = () => new Promise(resolve => setImmediate(resolve))

let browserRuntime
async function loadBrowser() {
  if (browserRuntime) return browserRuntime
  browser.configureBrowserWasm(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }
    delete module.default
    return module
  })
  await browser.initBrowser()
  browserRuntime = new browser.Runtime(metadata, 419, 1, 42)
  return browserRuntime
}

function compact(value) {
  if (value < 64) return Buffer.from([value << 2])
  assert(value < 16384)
  const bytes = Buffer.alloc(2)
  bytes.writeUInt16LE((value << 2) | 1)
  return bytes
}

// The fixture predates protected-wallet feature advertisement, but has the real
// v4 MultiAddress codec. Construct the runtime's exact wire layout from genuine
// authorization proofs; both native and WASM decode the complete transfer.
function encodeAddress(address, signature = Buffer.alloc(64, 0x4d), signatureVariant = 0) {
  const body = Buffer.concat([Buffer.from([0x84]), address, Buffer.from([signatureVariant]), signature, extra, call])
  return Buffer.concat([compact(body.length), body])
}
function rawExtrinsic(envelope, signature, signatureVariant) {
  return encodeAddress(Buffer.concat([Buffer.from([2]), compact(envelope.length), envelope]), signature, signatureVariant)
}
function protectedTransfer(crypto, profile) {
  const key = sdk.Keypair.fromSeed(Buffer.alloc(32, crypto + 20), crypto).atGeneration(sdk.isRotatingCrypto(crypto) ? 3n : 0n)
  const payload = runtime.signaturePayload(call, params), proof = key.signAuthorization(payload)
  assert(sdk.verifyAuthorization(proof, payload, key.authorizationDescriptor(), key.authorizationGeneration(), key.authorizationCurrentCommitment()))
  const envelope = profile < 4 ? Buffer.concat([key.publicKey, Buffer.from([1, profile]), proof.subarray(0, 72)]) : proof
  const bytes = rawExtrinsic(envelope, profile < 4 ? proof.subarray(72) : undefined, profile === 1 ? 1 : 0)
  return { key, envelope, bytes }
}
function assertTransfer(decoded, envelope) {
  assert.deepEqual(Object.keys(decoded.address), ['Raw'])
  assert.equal(decoded.address.Raw, '0x' + envelope.toString('hex'))
  assert.equal(decoded.nonce, 7)
  assert.equal(decoded.call.call_module, 'Balances')
  assert.equal(decoded.call.call_function, 'transfer_allow_death')
  assert.equal(BigInt(decoded.call.call_args.find(arg => arg.name === 'value').value), 10_000_000n)
  assert.equal(decoded.call.call_args.find(arg => arg.name === 'dest').value, recipient.ss58Address)
}

async function clients(t) {
  const node = new sdk.Client('local'), web = new browser.Client('local'), sent = { node: [], web: [] }
  t.after(async () => { await node.close(); await web.close() })
  node.runtimeAt = async () => runtime
  node.requireNativeClient = async () => ({ submitEncoded: async bytes => {
    sent.node.push(Buffer.from(bytes))
    return { success: true, message: 'Submitted', extrinsicHash: '0x' + sdk.blake2_256(bytes).toString('hex'), events: [] }
  } })
  const wasmRuntime = await loadBrowser()
  web.currentRuntime = async () => wasmRuntime
  web.broadcastExtrinsic = async bytes => {
    sent.web.push(Buffer.from(bytes))
    return { status: 'submitted', extrinsicHash: '0x' + sdk.blake2_256(bytes).toString('hex'), events: [] }
  }
  return { node, web, sent, wasmRuntime }
}

// PROTECTED_REGRESSION: real Raw protected signers must submit transfers on Node and browser using the permanent account nonce queue, instead of failing Buffer conversion; changes require explicit human approval.
test('all protected profiles submit real-codec transfers behind their permanent account queue', async t => {
  const { node, web, sent, wasmRuntime } = await clients(t)
  for (const [crypto, profile, length] of profiles) {
    const { key, envelope, bytes } = protectedTransfer(crypto, profile)
    assert.equal(envelope.length, length)
    assert.equal(envelope[33], profile)
    assertTransfer(runtime.decodeExtrinsic(bytes), envelope)
    assertTransfer(wasmRuntime.decodeExtrinsic(bytes), envelope)
    for (const [name, hold, submit] of [
      ['node', action => runSignerQueue(node, key.ss58Address, action), () => node.submitSigned(bytes)],
      ['web', action => web.serial({ address: key.ss58Address }, action), () => web.submitExtrinsic(bytes)],
    ]) {
      let release, enter, settled = false
      const entered = new Promise(resolve => { enter = resolve })
      const held = hold(async () => { enter(); await new Promise(resolve => { release = resolve }) })
      await entered
      const before = sent[name].length
      const submitted = submit().then(result => ({ result }), error => ({ error })).finally(() => { settled = true })
      try {
        await tick()
        assert.equal(settled, false, `${name} profile ${profile} must wait for its account queue`)
        assert.equal(sent[name].length, before)
      } finally { release() }
      await held
      const outcome = await submitted
      assert.ifError(outcome.error)
      assert.equal(outcome.result.status, 'submitted')
      assert.equal(sent[name].length, before + 1)
      assert.deepEqual(sent[name].at(-1), bytes)
    }
  }
})

test('protected submission rejects a mismatched Node signer hint and malformed Raw envelopes before broadcasting', async t => {
  const { node, web, sent } = await clients(t)
  const { key, envelope, bytes } = protectedTransfer(6, 3)
  await assert.rejects(node.submitSigned(bytes, recipient.ss58Address), /differs from signerAddress/)
  const invalidVersion = Buffer.from(envelope); invalidVersion[32] = 2
  const invalidProfile = Buffer.from(envelope); invalidProfile[33] = 2
  const malformed = [envelope.subarray(0, 105), Buffer.concat([envelope, Buffer.from([0])]), invalidVersion, invalidProfile, key.publicKey]
  for (const raw of malformed) {
    const encoded = rawExtrinsic(raw)
    await assert.rejects(node.submitSigned(encoded, key.ss58Address), /unsupported protected extrinsic signer envelope/)
    await assert.rejects(web.submitExtrinsic(encoded), /unsupported protected extrinsic signer envelope/)
  }
  assert.deepEqual(sent, { node: [], web: [] })
  // Equivalent SS58 networks represent one account and remain valid hints.
  await node.submitSigned(bytes, sdk.ss58FromPublic(key.publicKey, 0))
  assert.deepEqual(sent.node, [bytes])
})

test('signer parsing rejects address aliases and ambiguous representations instead of assigning another account queue', () => {
  const account = recipient.publicKey, decode = sdk.publicKeyFromSs58
  for (const address of [account, Array.from(account), recipient.ss58Address, { Id: account }, { Address32: account }]) {
    assert.deepEqual(Buffer.from(extrinsicSignerAccountId(address, decode)), account)
  }
  for (const address of [{ Index: 7 }, { Address20: Buffer.alloc(20) }, { Id: account, Raw: account }, { Id: Array(32).fill(256) }, { Id: '0xzz' }, { Raw: '0x0' }, { Id: account.subarray(1) }]) {
    assert.throws(() => extrinsicSignerAccountId(address, decode), /extrinsic signer/)
  }
})
