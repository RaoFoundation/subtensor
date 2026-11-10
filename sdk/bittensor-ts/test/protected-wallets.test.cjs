const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const native = require('../native.cjs')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
for (const [name, crypto] of Object.entries(sdk.WALLET_TYPES).filter(([,type]) => sdk.isProtectedCrypto(type))) {
  test(`protected ${name}: private seed cannot pass public exports or keyfile guards`, async t => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-protected-export-'))
    t.after(() => fs.rm(root, { recursive: true, force: true }))
    const key = sdk.Keypair.fromMnemonic(sdk.generateMnemonic(24), crypto)
    const active = key.atGeneration(sdk.isRotatingCrypto(crypto) ? 3n : 0n)
    for (const privateKey of [key, active, sdk.Keypair.deserialize(key.dangerouslySerializePrivateKeypair())]) {
      assert.notEqual(privateKey.kind, 'PublicOnly')
      for (const exportPublic of [() => privateKey.serializePublic(), () => privateKey.serialize(), () => sdk.serializePublicKeypair(privateKey)]) {
        assert.throws(exportPublic, /public-only|plaintext private key serialization/)
      }
      const nativeResult = native.serializeKeypair(privateKey.nativeHandle())
      assert(nativeResult instanceof Error)
      assert.match(nativeResult.message, /plaintext private key serialization/)
      await assert.rejects(privateKey.toKeyfileData(), /plaintext private key serialization/)
    }
    const publicKey = key.publicOnly()
    for (const watch of [publicKey, sdk.Keypair.fromDescriptor(key.authorizationDescriptor()), sdk.Keypair.deserialize(publicKey.serializePublic())]) {
      assert.equal(watch.kind, 'PublicOnly')
      const json = JSON.parse(watch.serializePublic())
      for (const secret of ['privateKey', 'secretSeed', 'secretPhrase']) assert.equal(Object.hasOwn(json, secret), false)
      assert.deepEqual(watch.authorizationDescriptor(), key.authorizationDescriptor())
      assert.throws(() => watch.signAuthorization(Buffer.from('watch-only')))
    }
    const privatePath = path.join(root, 'private'), publicPath = path.join(root, 'public')
    await assert.rejects(key.writeKeyfile(privatePath), /plaintext private keyfile/)
    await assert.rejects(sdk.writeKeypairPairKeyfile(key, privatePath, publicKey, publicPath), /plaintext private keyfile/)
    await assert.rejects(sdk.writeKeypairPairKeyfile(key, privatePath, key, publicPath, { allowPlaintext: true }), /public keyfile pair member/)
    assert.deepEqual(await fs.readdir(root), [])
    await sdk.writeKeypairPairKeyfile(key, privatePath, publicKey, publicPath, { password: 'regression-only' })
    assert.equal(sdk.keyfileDataIsEncrypted(await fs.readFile(privatePath)), true)
    assert.notEqual((await sdk.Keypair.fromKeyfile(privatePath, 'regression-only')).kind, 'PublicOnly')
    assert.equal((await sdk.Keypair.fromKeyfile(publicPath)).kind, 'PublicOnly')
  })
}
test('Polkadot compatibility accepts classical keys and rejects protected keys', () => {
  const key = sdk.createKeyringPairFromUri('//Alice')
  assert.equal(sdk.asPolkadotKeypair(key), key)
  assert.equal(sdk.asPolkadotKeypair(key.derive('//compat')).type, 'sr25519')
  assert.throws(() => key.setMeta({ type: 'hashed-sr25519' }), /must match/)
  assert.throws(() => sdk.asPolkadotKeypair(sdk.Keypair.fromSeed(Buffer.alloc(32, 17), sdk.WALLET_TYPES['hashed-sr25519'])), /requires an sr25519 or ed25519/)
})
for (const [name, crypto] of Object.entries(sdk.WALLET_TYPES)) {
  test(`release wallet ${name}: recovery, public view and authorization`, () => {
    const seed = Buffer.alloc(32, 17), key = sdk.Keypair.fromSeed(seed, crypto)
    assert.equal(key.type, name)
    assert.equal(key.publicOnly().ss58Address, key.ss58Address)
    if (!sdk.isProtectedCrypto(crypto)) return
    const descriptor = key.authorizationDescriptor(), generation = sdk.isRotatingCrypto(crypto) ? 3n : 0n
    const restored = sdk.Keypair.fromDescriptor(descriptor), active = key.atGeneration(generation)
    assert.equal(restored.ss58Address, key.ss58Address)
    assert.equal(active.ss58Address, key.ss58Address)
    assert.equal(active.authorizationGeneration(), generation)
    const payload = Buffer.alloc(400, 42), proof = active.signAuthorization(payload)
    assert(sdk.verifyAuthorization(proof, payload, descriptor, generation, active.authorizationCurrentCommitment()))
    assert.equal(sdk.verifyAuthorization(proof, Buffer.alloc(400, 43), descriptor, generation, active.authorizationCurrentCommitment()), false)
    assert.equal(key.dummyAuthorization().length, proof.length)
    assert.equal(sdk.verifyAuthorization(key.dummyAuthorization(), payload, descriptor, generation, active.authorizationCurrentCommitment()), false)
    assert.throws(() => restored.signAuthorization(payload))
    if (sdk.isRotatingCrypto(crypto)) {
      assert.deepEqual(sdk.decodeReceivingAddress(key.receivingAddress()), descriptor)
      assert.equal(sdk.registrationCall(key).call, 'register')
      assert.equal(sdk.rotationCall(key).pallet, 'System')
    } else {
      assert.equal(key.receivingAddress(), key.ss58Address)
      assert.throws(() => sdk.registrationCall(key), /fixed/)
      assert.throws(() => key.atGeneration(1n))
    }
  })
}
test('safe multisig derivation is distinct and rejects duplicate members', () => {
  const keys = [sdk.Keypair.fromSeed(Buffer.alloc(32,1)).publicKey, sdk.Keypair.fromSeed(Buffer.alloc(32,2)).publicKey]
  assert.notDeepEqual(sdk.multisigAccountId(keys,2).accountId,sdk.multisigAccountId(keys,2,true).accountId)
  assert.throws(() => sdk.multisigAccountId([keys[0],keys[0]],2,true),/duplicate/)
})
