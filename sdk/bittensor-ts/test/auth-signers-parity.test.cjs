'use strict'
const assert = require('node:assert/strict')
const test = require('node:test')
const { Keypair, publicKeyFromSs58, ss58FromPublic, verifySignature } = require('../dist/keys.js')
const { blake2_256 } = require('../dist/crypto.js')
const { createHttpAuth, buildAuthPayload, InMemoryNonceStore } = require('../dist/http-auth.js')
const { VaultSigner, ExtensionSigner, uosMultipartFrames, vaultTransactionFrames } = require('../dist/external-signers.js')
const crypto = { publicKeyFromSs58, ss58FromPublic, blake2_256, verify: verifySignature }
const nonceNs = 1752076800000000000n, body = Buffer.from('{"prompt": "hello"}')
const sender = Keypair.fromUri('//Alice'), receiver = Keypair.fromUri('//Bob')
const request = { method: 'POST', path: '/generate?stream=false', body, receiverSs58: receiver.ss58Address, nonceNs }

test('HTTP authentication matches Python normative payload and deterministic ed25519 signature vectors', async () => {
  const payload = await buildAuthPayload({ ...request, scheme: 'sr25519', senderSs58: sender.ss58Address })
  assert.equal(new TextDecoder().decode(payload), [
    'btauth/1', 'sr25519', 'POST', '/generate?stream=false',
    '341c57448e531310fbbe83f44cea2a5e838bd9e8a6b82b269f01d0dbbc23c3cc',
    '1752076800000000000', '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY',
    '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty',
  ].join('\n'))
  const auth = createHttpAuth(crypto), ed = Keypair.fromUri('//Alice', 0)
  const headers = await auth.sign(ed, request)
  assert.equal(headers['X-Bittensor-Signature'], '0x1a6b631988bf1f7ea093ab9d5acf61f05c316d0b0e428b24b487b75a5f6ee0e1a56a26918fdf871b6cf09437e6588023a4ce423cf622621b2a7feee17612f704')
  assert.equal(headers['X-Bittensor-Crypto'], 'ed25519')
  const options = { method: request.method, path: request.path, selfHotkeySs58: receiver.ss58Address, nowNs: nonceNs + 1000000n }
  assert.equal((await auth.verify(headers, body, options)).hotkeySs58, ed.ss58Address)
  await assert.rejects(auth.verify(headers, body, options), { name: 'ReplayedRequest' })
  await assert.rejects(auth.verify(headers, Buffer.from('different'), { ...options, nonceStore: new InMemoryNonceStore() }), { name: 'BadSignature' })
  await assert.rejects(auth.verify(headers, body, { ...options, selfHotkeySs58: sender.ss58Address }), { name: 'WrongReceiver' })
})

test('UOS proof and legacy framing matches Python byte layout and preserves multipart indexes', () => {
  const unsigned = { address: sender.ss58Address, publicKey: sender.publicKey, cryptoType: 1, callData: new Uint8Array([1, 2]), payload: new Uint8Array([1, 2, 3, 4]), genesisHash: '0x' + '11'.repeat(32), includedInExtrinsic: new Uint8Array([3]), includedInSignedData: new Uint8Array([4]), signerPayload: {} }
  const legacy = vaultTransactionFrames(unsigned)
  const expected = Buffer.concat([Buffer.from([0, 0, 1, 0, 0, 0x53, 1, 2]), sender.publicKey, Buffer.from([8, 1, 2, 3, 4]), Buffer.alloc(32, 0x11)])
  assert.deepEqual(Buffer.from(legacy.frames[0]), expected)
  const proof = vaultTransactionFrames(unsigned, new Uint8Array([9]))
  assert.equal(proof.frames[0][7], 6)
  assert.deepEqual(Array.from(proof.frames[0].slice(40, 46)), [9, 8, 1, 2, 3, 4])
  const frames = uosMultipartFrames(new Uint8Array(2049))
  assert.equal(frames.length, 3)
  assert.deepEqual(Array.from(frames[2].slice(0, 5)), [0, 0, 3, 0, 2])
})

test('Vault verifies raw and typed signatures and rejects wrong-scheme or stale scans', async () => {
  const unsigned = { address: sender.ss58Address, publicKey: sender.publicKey, cryptoType: 1, callData: new Uint8Array([1]), payload: new Uint8Array([1, 2]), genesisHash: '0x' + '11'.repeat(32), includedInExtrinsic: new Uint8Array([3]), includedInSignedData: new Uint8Array([4]), signerPayload: {} }
  const raw = sender.sign(unsigned.payload)
  for (const signature of [raw, Buffer.concat([Buffer.from([1]), raw])]) {
    const vault = new VaultSigner(sender.ss58Address, sender.publicKey, async () => signature, 1, crypto)
    assert.deepEqual(await vault.signUnsignedExtrinsic(unsigned), new Uint8Array(signature))
  }
  const wrong = new VaultSigner(sender.ss58Address, sender.publicKey, async () => Buffer.concat([Buffer.from([0]), raw]), 1, crypto)
  await assert.rejects(wrong.signUnsignedExtrinsic(unsigned), /invalid Vault signature/)
  const unverified = new VaultSigner(sender.ss58Address, sender.publicKey, async () => raw)
  await assert.rejects(unverified.signUnsignedExtrinsic(unsigned), /verification backend/)
})

test('extension message signing normalizes typed signatures and checks account chain binding', async () => {
  const signature = sender.sign(body)
  const extension = { accounts: { async get() { return [] } }, signer: {
    async signRaw() { return { signature: '0x01' + signature.toString('hex') } },
    async signPayload() { return { signature: '0x01' + signature.toString('hex') } },
  } }
  const signer = new ExtensionSigner({ address: sender.ss58Address, type: 'sr25519', genesisHash: '0x11' }, extension)
  assert.deepEqual(await signer.sign(body), new Uint8Array(signature))
  assert.throws(() => signer.signPayload({ address: receiver.ss58Address, genesisHash: '0x11' }), /address mismatch/)
  assert.throws(() => signer.signPayload({ address: sender.ss58Address, genesisHash: '0x22' }), /another chain/)
})
