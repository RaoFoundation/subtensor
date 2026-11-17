'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const { SigningKey, getBytes } = require('ethers')
const sdk = require('../dist')
const { WorkflowClient } = require('../dist/node-workflows')
const { IntentCall } = require('../dist/transaction')
const { bindSignerIdentity } = require('../dist/signer-queue')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)

function key(byte = '11') {
  const secret = new SigningKey('0x' + byte.repeat(32))
  const publicKey = Buffer.from(getBytes(secret.compressedPublicKey))
  const accountId = sdk.accountIdFromEcdsaPublic(publicKey)
  return { publicKey, accountId, address: sdk.ss58FromPublic(accountId), cryptoType: 2,
    sign(payload) {
      const signature = secret.sign(sdk.blake2_256(payload))
      return Buffer.concat([Buffer.from(getBytes(signature.r)), Buffer.from(getBytes(signature.s)), Buffer.from([signature.yParity])])
    } }
}

// PROTECTED_REGRESSION: supported external ECDSA wallets must be able to execute ordinary transfers; changes require explicit human approval.
test('Node workflow accepts compressed ECDSA public keys and signs a valid transfer', async t => {
  const external = key(), client = new sdk.Client('local'), signed = [], identities = []
  t.after(() => client.close())
  const original = IntentCall.fromDescriptor
  IntentCall.fromDescriptor = (_client, descriptor) => IntentCall.operation(`${descriptor.pallet}.${descriptor.call}`, descriptor.params)
  t.after(() => { IntentCall.fromDescriptor = original })
  client.runtimeAt = async () => runtime
  client.blockHash = async () => '0x' + '11'.repeat(32)
  client.blockNumber = async () => 1
  client.query = async () => ({ data: { free: 100_000_000_000n } })
  client.requireNativeClient = async () => ({
    accountNextIndex: async () => 0,
    externalSigningPlanForIntent: async (_call, address, publicKey, cryptoType) => {
      assert.equal(address, external.address)
      assert.equal(cryptoType, 2)
      assert.deepEqual(Buffer.from(publicKey), external.accountId)
      identities.push(publicKey)
      return { feeRao: 1n }
    },
  })
  client.submit = async (call, signer) => {
    const resolved = await client.resolveSigner(signer, runtime)
    const callData = runtime.composeCall(call.pallet, call.function, call.params)
    const tx = { era: '00', nonce: 0, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32) }
    const payload = runtime.signaturePayload(callData, tx)
    const signature = await resolved.signer.sign(payload)
    assert.equal(sdk.verifySignature(payload, signature, external.address, 2), true)
    const encoded = runtime.encodeSignedExtrinsic(callData, resolved.publicKey, signature, 2, tx)
    signed.push(runtime.decodeExtrinsic(encoded.bytes))
    return { status: 'submitted', extrinsicHash: '0x' + encoded.hash.toString('hex'), message: 'Submitted', events: [] }
  }
  const signer = { getAccount: async () => external, sign: payload => external.sign(payload) }
  const result = await new WorkflowClient(client).execute({ op: 'transfer', dest_ss58: sdk.Keypair.fromUri('//Bob').ss58Address, amount_tao: '1' }, signer, { mevShield: false })
  assert.equal(result.status, 'submitted')
  assert.equal(identities.length, 1)
  assert.equal(signed.length, 1)
  assert.equal(signed[0].address, external.address)
  assert.ok(signed[0].signature.Ecdsa)
})

test('ECDSA workflow identity accepts AccountId aliases but rejects account changes', async () => {
  const external = key(), other = key('22')
  let current = external
  const signer = { getAccount: async () => current }
  const identity = { ...external, requiresMetadataProof: false }
  const bound = bindSignerIdentity(signer, identity)
  for (const publicKey of [external.publicKey, external.accountId]) {
    current = { ...external, address: sdk.ss58FromPublic(external.accountId, 0), publicKey }
    const account = await bound.getAccount({})
    assert.deepEqual(Buffer.from(account.publicKey), external.accountId)
  }
  current = other
  await assert.rejects(bound.getAccount({}), /account changed/)
  assert.throws(() => bindSignerIdentity(signer, { ...identity, publicKey: other.publicKey }), /differs from its address/)
  assert.throws(() => bindSignerIdentity(signer, { ...identity, cryptoType: 1 }), /differs from its address/)
})
