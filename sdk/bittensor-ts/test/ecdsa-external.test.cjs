'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { SigningKey, getBytes } = require('ethers')
const core = require('../dist')
const { normalizeSignatureBytes } = require('../dist/signatures')
const { serializeUnsignedTransaction, restoreUnsignedTransaction } = require('../dist/unsigned-transaction')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = core.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const genesis = '0x' + '11'.repeat(32)
const crypto = { blake2_256: core.blake2_256, publicKeyFromSs58: core.publicKeyFromSs58 }

function externalKey(byte = '11') {
  const key = new SigningKey('0x' + byte.repeat(32))
  const publicKey = Buffer.from(getBytes(key.compressedPublicKey))
  const accountId = core.accountIdFromEcdsaPublic(publicKey)
  return {
    publicKey, accountId, address: core.ss58FromPublic(accountId), cryptoType: 2,
    sign(payload) {
      const signature = key.sign(core.blake2_256(payload))
      return Buffer.concat([Buffer.from(getBytes(signature.r)), Buffer.from(getBytes(signature.s)), Buffer.from([signature.yParity])])
    },
  }
}

function fixture() {
  const runtime = new core.Runtime(metadata, 419, 1, 42), signer = externalKey()
  const callData = runtime.composeCall('System', 'remark', { remark: Buffer.from('external ECDSA') })
  const txParams = { era: '00', nonce: 7, tip: 13n, genesisHash: Buffer.from(genesis.slice(2), 'hex'), eraBlockHash: Buffer.from(genesis.slice(2), 'hex') }
  const state = { runtime, address: signer.address, publicKey: signer.accountId, cryptoType: 2, callData, payload: runtime.signaturePayload(callData, txParams), ...runtime.signaturePayloadParts(txParams), txParams }
  return { runtime, signer, state, serialized: serializeUnsignedTransaction(state, crypto, metadata) }
}

function assertEcdsaExtrinsic(runtime, bytes, signer, signature) {
  const decoded = runtime.decodeExtrinsic(bytes)
  assert.equal(decoded.address, signer.address)
  assert.equal(decoded.signature.Ecdsa, '0x' + Buffer.from(signature).toString('hex'))
  assert.equal(decoded.nonce, 7)
}

test('ECDSA normalization retains raw 65-byte signatures beginning with every legacy variant byte', () => {
  assert.equal(core.CRYPTO_ECDSA, 2)
  for (const first of [0, 1, 2]) {
    const raw = Buffer.alloc(65, 0x42)
    raw[0] = first
    raw[64] = 1
    const normalized = normalizeSignatureBytes(raw, 2)
    assert.deepEqual(Buffer.from(normalized), raw)
    assert.notEqual(normalized, raw, 'normalization must not share mutable signer-owned bytes')
    assert.deepEqual(Buffer.from(normalizeSignatureBytes(Buffer.concat([Buffer.from([2]), raw]), 2)), raw)
  }
  for (const prefix of [0, 1]) assert.throws(() => normalizeSignatureBytes(Buffer.alloc(66, prefix), 2), /scheme differs/)
  for (const size of [0, 64, 67]) assert.throws(() => normalizeSignatureBytes(Buffer.alloc(size), 2), /65 bytes/)
  for (const scheme of [0, 1]) {
    const raw = Buffer.alloc(64, 9)
    assert.deepEqual(Buffer.from(normalizeSignatureBytes(Buffer.concat([Buffer.from([scheme]), raw]), scheme)), raw)
  }
})

test('external ECDSA recovery binds the message and compressed-public-key AccountId', () => {
  const signer = externalKey(), other = externalKey('22'), message = Buffer.from('message identity')
  const signature = signer.sign(message)
  assert.deepEqual(signer.accountId, core.blake2_256(signer.publicKey))
  assert.equal(core.ss58FromEcdsaPublic(signer.publicKey), signer.address)
  assert.equal(core.verifySignature(message, signature, signer.address, 2), true)
  assert.equal(core.verifySignature(Buffer.from('different message'), signature, signer.address, 2), false)
  assert.equal(core.verifySignature(message, signature, other.address, 2), false)
  assert.throws(() => core.verifySignature(message, signature, signer.address, 1), /invalid sr25519 signature length/)
  assert.equal(core.verifySignature(message, Buffer.alloc(65), signer.address, 2), false)
})

test('extension ECDSA adapters preserve raw signatures and strip only their own 66-byte prefix', async () => {
  const signer = externalKey(), requests = []
  for (const first of [0, 1, 2]) for (const prefixed of [false, true]) {
    const raw = Buffer.alloc(65, 8); raw[0] = first
    const response = prefixed ? Buffer.concat([Buffer.from([2]), raw]) : raw
    const adapter = new core.ExtensionSigner({ address: signer.address, type: 'ecdsa', genesisHash: genesis }, {
      accounts: { get: async () => [] }, signer: {
        signRaw: async request => { requests.push(request); return { signature: '0x' + response.toString('hex') } },
        signPayload: async () => ({ signature: '0x' + response.toString('hex') }),
      },
    })
    assert.equal(adapter.cryptoType, 2)
    assert.deepEqual(Buffer.from(await adapter.sign(Buffer.from([9]))), raw)
    assert.deepEqual(requests.at(-1), { address: signer.address, data: '0x09', type: 'bytes' })
    assert.equal((await adapter.signPayload({ address: signer.address, genesisHash: genesis })).signature, '0x' + response.toString('hex'))
    assert.throws(() => adapter.signPayload({ address: externalKey('22').address, genesisHash: genesis }), /address mismatch/)
    assert.throws(() => adapter.signPayload({ address: signer.address, genesisHash: '0x' + '22'.repeat(32) }), /another chain/)
  }
})

test('ECDSA unsigned JSON round-trips its 32-byte AccountId and exact signing payload', () => {
  const f = fixture(), serialized = JSON.parse(JSON.stringify(f.serialized))
  assert.equal(serialized.cryptoType, 2)
  assert.equal(serialized.publicKey, '0x' + f.signer.accountId.toString('hex'))
  const restored = restoreUnsignedTransaction(serialized, f.runtime, crypto, metadata, genesis)
  assert.deepEqual(Buffer.from(restored.payload), f.state.payload)
  const signature = f.signer.sign(restored.payload)
  const signed = f.runtime.encodeSignedExtrinsic(restored.callData, restored.publicKey, signature, 2, restored.txParams)
  assertEcdsaExtrinsic(f.runtime, signed.bytes, f.signer, signature)
  assert.throws(() => restoreUnsignedTransaction({ ...serialized, publicKey: '0x' + f.signer.publicKey.toString('hex') }, f.runtime, crypto, metadata, genesis), /32 bytes/)
  assert.throws(() => restoreUnsignedTransaction({ ...serialized, address: externalKey('22').address }, f.runtime, crypto, metadata, genesis), /address and public key differ/)
})

test('large ECDSA payloads apply the runtime payload hash before the ECDSA message hash', () => {
  const f = fixture()
  const call = f.runtime.composeCall('System', 'remark', { remark: Buffer.alloc(1024, 0x42) })
  const parts = f.runtime.signaturePayloadParts(f.state.txParams)
  const complete = Buffer.concat([call, parts.includedInExtrinsic, parts.includedInSignedData])
  const payload = f.runtime.signaturePayload(call, f.state.txParams)
  assert.ok(complete.length > 256)
  assert.equal(payload.length, 32)
  assert.deepEqual(payload, core.blake2_256(complete))
  assert.equal(core.verifySignature(payload, f.signer.sign(payload), f.signer.address, 2), true)
  assert.equal(core.verifySignature(payload, f.signer.sign(complete), f.signer.address, 2), false)
})

test('Node detached ECDSA imports verify raw and prefixed signatures before encoding', async t => {
  const f = fixture(), client = new core.Client('local'), submitted = []
  t.after(() => client.close())
  client.runtimeAt = async () => f.runtime
  client.genesisHash = async () => genesis
  client.submitSigned = async signed => { submitted.push(signed.bytes); return { status: 'submitted', message: 'Submitted', events: [], extrinsicHash: '0x1234' } }
  const imported = await client.importPreparedCall(JSON.parse(JSON.stringify(f.serialized)), { allowRawCall: true })
  assert.deepEqual(client.exportPreparedCall(imported), f.serialized)
  await assert.rejects(client.submitSignature(imported, externalKey('22').sign(imported.payload)), /does not verify/)
  await assert.rejects(client.submitSignature(imported, f.signer.sign(Buffer.from('another message'))), /does not verify/)
  assert.equal(submitted.length, 0)
  const signature = f.signer.sign(imported.payload)
  await client.submitSignature(imported, signature)
  await client.submitSignature(imported, { signature: '0x02' + signature.toString('hex') })
  assert.equal(submitted.length, 2)
  for (const bytes of submitted) assertEcdsaExtrinsic(f.runtime, bytes, f.signer, signature)
})

test('Node preparation maps external compressed keys to AccountId before its native plan', async t => {
  const f = fixture(), client = new core.Client('local')
  t.after(() => client.close())
  client.runtimeAt = async () => f.runtime
  let plans = 0
  client.requireNativeClient = async () => ({ externalSigningPlan: async (call, address, accountId, scheme) => {
    plans++
    assert.deepEqual(call, f.state.callData)
    assert.equal(address, f.signer.address)
    assert.deepEqual(accountId, f.signer.accountId)
    assert.equal(scheme, 2)
    return { ...f.state, runtime: f.runtime.handle, nonce: 7, signerPayload: f.runtime.signerPayload(address, call, f.state.txParams) }
  } })
  const prepared = await client.prepareCall(f.state.callData, { ...f.signer, sign: () => { throw new Error('preparation must not sign') } }, { nonce: 7, period: null, allowRawCall: true })
  assert.equal(plans, 1)
  assert.equal(prepared.cryptoType, 2)
  assert.deepEqual(prepared.publicKey, f.signer.accountId)
  assert.deepEqual(client.exportPreparedCall(prepared), f.serialized)
  await assert.rejects(client.prepareCall(f.state.callData, { ...f.signer, publicKey: externalKey('22').publicKey }, { nonce: 7, allowRawCall: true }), /does not match/)
  assert.equal(plans, 1)
})

test('browser preparation, offline imports and extension encoding support ECDSA with verified identity', async t => {
  const f = fixture(), browser = await import('../dist/browser.mjs')
  browser.configureBrowserWasm(async () => ({
    default: async () => {}, ss58Decode: core.publicKeyFromSs58, ss58Encode: core.ss58FromPublic,
    accountIdFromEcdsaPublic: core.accountIdFromEcdsaPublic, blake2_256: core.blake2_256, verifySignature: core.verifySignature,
  }))
  await browser.initBrowser()
  const client = new browser.Client('local'), submitted = []
  t.after(() => client.close())
  client.blockHash = async () => genesis
  client.blockNumber = async () => 10
  client.genesisHash = async () => genesis
  client.compose = async () => f.state.callData
  client.runtimeEntry = async () => ({ runtime: f.runtime, metadata, version: { specName: 'test' }, properties: { ss58Format: 42 } })
  client.broadcastExtrinsic = async bytes => { submitted.push(bytes); return { status: 'submitted', events: [], extrinsicHash: '0x1234' } }
  const options = { allowRawCall: true, nonce: 7, tip: 13n, period: null, metadataHash: null }
  const prepared = await client.prepareCall({}, f.signer, options)
  assert.equal(prepared.cryptoType, 2)
  assert.deepEqual(Buffer.from(prepared.publicKey), f.signer.accountId)
  const serialized = await client.exportPreparedCall(prepared)
  assert.deepEqual(serialized, f.serialized)
  const imported = await client.importPreparedCall(JSON.parse(JSON.stringify(serialized)), { allowRawCall: true })
  const signature = f.signer.sign(prepared.payload)
  await assert.rejects(client.submitSignature(imported, externalKey('22').sign(prepared.payload)), /invalid transaction signature/)
  await assert.rejects(client.submitSignature(imported, f.signer.sign(Buffer.from('wrong payload'))), /invalid transaction signature/)
  assert.equal(submitted.length, 0)
  await client.submitSignature(imported, Buffer.concat([Buffer.from([2]), signature]))
  assertEcdsaExtrinsic(f.runtime, submitted[0], f.signer, signature)
  const extension = new browser.ExtensionSigner({ address: f.signer.address, type: 'ecdsa' }, {
    accounts: { get: async () => [] }, signer: { signPayload: async payload => {
      assert.equal(payload.address, f.signer.address)
      assert.equal(payload.method, '0x' + f.state.callData.toString('hex'))
      assert.equal(BigInt(payload.nonce), 7n)
      assert.equal(BigInt(payload.tip), 13n)
      return { signature: '0x02' + signature.toString('hex') }
    } },
  })
  const signed = await client.signExtrinsic({}, extension, options)
  assertEcdsaExtrinsic(f.runtime, signed.bytes, f.signer, signature)
  await assert.rejects(client.prepareCall({}, { ...f.signer, publicKey: externalKey('22').publicKey }, options), /identity changed/)
})
