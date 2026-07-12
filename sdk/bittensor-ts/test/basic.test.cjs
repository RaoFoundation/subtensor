'use strict'

const assert = require('node:assert/strict')
const fs = require('node:fs')
const { createRequire } = require('node:module')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')
const { pathToFileURL } = require('node:url')

const core = require('../dist/index.js')

function submittedExtrinsicHash(extrinsicHex) {
  return `0x${core.blake2_256(Buffer.from(String(extrinsicHex).slice(2), 'hex')).toString('hex')}`
}

function goldenMetadataBytes() {
  const raw = Buffer.from(goldenMetadataResponseHex().slice(2), 'hex')
  const metadata = core.decodeOptionalOpaqueMetadata(raw)
  assert.ok(metadata)
  return metadata
}

function goldenMetadataResponseHex() {
  const golden = JSON.parse(
    fs.readFileSync(
      path.join(__dirname, '..', '..', 'python', 'tests', 'fixtures', 'golden.json'),
      'utf8',
    ),
  )
  return golden.metadata.v15_hex
}

function goldenLegacyMetadataHex() {
  const golden = JSON.parse(
    fs.readFileSync(
      path.join(__dirname, '..', '..', 'python', 'tests', 'fixtures', 'golden.json'),
      'utf8',
    ),
  )
  return golden.metadata.v14_hex
}

function ledgerProofVector() {
  const vector = JSON.parse(
    fs.readFileSync(
      path.join(__dirname, '..', '..', 'bittensor-core', 'fixtures', 'ledger_proof_vector.json'),
      'utf8',
    ),
  )
  return {
    specVersion: vector.spec_version,
    callData: Buffer.from(vector.call_data_hex, 'hex'),
    includedInExtrinsic: Buffer.from(vector.included_in_extrinsic_hex, 'hex'),
    includedInSignedData: Buffer.from(vector.included_in_signed_data_hex, 'hex'),
  }
}

function fakeSigningRuntime(overrides = {}) {
  const captures = {}
  const runtime = {
    specVersion: 419,
    transactionVersion: 1,
    extrinsicVersion: 4,
    ss58Format: 42,
    metadataBytes: Buffer.alloc(0),
    signaturePayloadParts(params) {
      captures.partsParams = params
      return {
        includedInExtrinsic: Buffer.from([1]),
        includedInSignedData: Buffer.from([2]),
      }
    },
    signaturePayload(_callData, params) {
      captures.payloadParams = params
      return Buffer.from([9, 9, 9])
    },
    signerPayload(address, callData, params) {
      captures.signerPayload = { address, callData: Buffer.from(callData), params }
      return {
        address,
        blockHash: `0x${Buffer.from(params.eraBlockHash).toString('hex')}`,
        blockNumber: '0x00000000',
        era: '0x00',
        genesisHash: `0x${Buffer.from(params.genesisHash).toString('hex')}`,
        method: `0x${Buffer.from(callData).toString('hex')}`,
        nonce: '0x30',
        signedExtensions: runtime.signedExtensionIdentifiers(),
        specVersion: '0xa3010000',
        tip: '0x00',
        transactionVersion: '0x01000000',
        version: runtime.extrinsicVersion,
        assetId: params.tipAssetId == null ? null : '0x010000',
        metadataHash:
          params.metadataHash == null ? undefined : `0x${Buffer.from(params.metadataHash).toString('hex')}`,
        mode: params.metadataHash == null ? 0 : 1,
      }
    },
    signedExtensionIdentifiers() {
      return ['CheckNonce']
    },
    encodeSignedExtrinsic(callData, publicKey, signature, signatureVersion, params) {
      captures.encoded = { callData, publicKey, signature, signatureVersion, params }
      return {
        bytes: Buffer.concat([Buffer.from([signatureVersion]), signature.subarray(0, 2)]),
        hash: Buffer.alloc(32, 7),
      }
    },
    composeCall(pallet, fn, params) {
      captures.composeCall = { pallet, fn, params }
      return Buffer.from([pallet.length, fn.length])
    },
    ...overrides,
  }
  return { runtime, captures }
}

function fakeSigningClient(runtime, callData) {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const finalizedHash = `0x${'42'.repeat(32)}`
  client.runtimeAt = async () => runtime
  client.callData = async () => Buffer.from(callData)
  client.finalizedHead = async () => finalizedHash
  client.blockNumber = async () => 64
  client.genesisHash = async () => `0x${'41'.repeat(32)}`
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      client.lastNonceAddress = params[0]
      return 12
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    throw new Error(`unexpected RPC ${method}`)
  }
  client.transport.request = (method, params = [], options = {}) => client.rpc(method, params, options)
  return client
}

function fakeRuntimeCacheClient(options = {}) {
  const metadataV15Hex = options.metadataV15Hex ?? goldenMetadataResponseHex()
  const legacyMetadataHex = options.legacyMetadataHex ?? goldenLegacyMetadataHex()
  const calls = {
    metadata: [],
    metadataAtVersion: [],
    version: [],
    properties: [],
  }
  let headVersion = {
    specVersion: 419,
    transactionVersion: 1,
  }
  const blockVersions = new Map()
  const client = new core.Client('local', {
    endpoint: 'http://127.0.0.1:9944',
    headRuntimeTtlMs: options.headRuntimeTtlMs ?? 1_000,
    historicalRuntimeCacheSize: options.historicalRuntimeCacheSize ?? 2,
  })

  client.rpc = async (method, params = []) => {
    const blockHash = params[0] ?? null
    if (method === 'state_getRuntimeVersion') {
      calls.version.push(blockHash)
      const version = blockHash == null ? headVersion : blockVersions.get(blockHash)
      if (version == null) throw new Error(`missing version for ${blockHash}`)
      return {
        specName: 'node-subtensor',
        specVersion: version.specVersion,
        transactionVersion: version.transactionVersion,
      }
    }
    if (method === 'state_call' && params[0] === 'Metadata_metadata_at_version') {
      calls.metadataAtVersion.push({ versionHex: params[1], blockHash: params[2] ?? null })
      if (options.metadataAtVersionError != null) throw options.metadataAtVersionError
      return metadataV15Hex
    }
    if (method === 'state_getMetadata') {
      calls.metadata.push(blockHash)
      return legacyMetadataHex
    }
    if (method === 'system_properties') {
      calls.properties.push(blockHash)
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    throw new Error(`unexpected RPC ${method}`)
  }

  return {
    client,
    calls,
    setHeadVersion(specVersion, transactionVersion = 1) {
      headVersion = { specVersion, transactionVersion }
    },
    setBlockVersion(blockHash, specVersion, transactionVersion = 1) {
      blockVersions.set(blockHash, { specVersion, transactionVersion })
    },
  }
}

function waitFor(predicate, label = 'condition') {
  return new Promise((resolve, reject) => {
    let attempts = 0
    const tick = () => {
      if (predicate()) {
        resolve()
        return
      }
      attempts += 1
      if (attempts > 200) {
        reject(new Error(`timed out waiting for ${label}`))
        return
      }
      setTimeout(tick, 5)
    }
    tick()
  })
}

function installFakeWebSocket() {
  const original = globalThis.WebSocket
  class FakeWebSocket {
    static sockets = []
    static onSend = () => undefined

    readyState = 0
    sent = []
    listeners = new Map()

    constructor(url) {
      this.url = url
      FakeWebSocket.sockets.push(this)
      queueMicrotask(() => this.open())
    }

    addEventListener(type, listener) {
      const listeners = this.listeners.get(type) ?? []
      listeners.push(listener)
      this.listeners.set(type, listeners)
    }

    send(data) {
      const message = JSON.parse(String(data))
      this.sent.push(message)
      FakeWebSocket.onSend(this, message)
    }

    close() {
      if (this.readyState === 3) return
      this.readyState = 3
      this.emit('close', {})
    }

    open() {
      if (this.readyState !== 0) return
      this.readyState = 1
      this.emit('open', {})
    }

    serverMessage(message) {
      this.emit('message', { data: JSON.stringify(message) })
    }

    emit(type, event) {
      for (const listener of this.listeners.get(type) ?? []) listener(event)
    }
  }
  globalThis.WebSocket = FakeWebSocket
  return {
    FakeWebSocket,
    restore() {
      globalThis.WebSocket = original
    },
  }
}

test('package exposes a WASM browser subset without the Node native addon', () => {
  const root = path.join(__dirname, '..')
  const packageJson = JSON.parse(
    fs.readFileSync(path.join(root, 'package.json'), 'utf8'),
  )

  assert.equal(Object.prototype.hasOwnProperty.call(packageJson, 'browser'), false)
  assert.equal(Object.prototype.hasOwnProperty.call(packageJson.exports['.'], 'browser'), false)
  assert.equal(packageJson.exports['.'].node.import, './dist/index.mjs')
  assert.equal(packageJson.exports['./browser'].import, './dist/browser.mjs')
  assert.equal(Object.prototype.hasOwnProperty.call(packageJson.exports['./browser'], 'require'), false)
  assert.equal(packageJson.exports['./native'].node.import, './native.cjs')

  const browserSource = fs.readFileSync(path.join(root, 'dist', 'browser.js'), 'utf8')
  assert.equal(browserSource.includes('./native'), false)
  assert.equal(browserSource.includes('node:buffer'), false)
  assert.equal(browserSource.includes('.node'), false)
})

test('browser package subpath is ESM-only for package consumers', async (t) => {
  const packageRoot = path.join(__dirname, '..')
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-sdk-package-'))
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }))
  const scope = path.join(temp, 'node_modules', '@bittensor')
  fs.mkdirSync(scope, { recursive: true })
  fs.symlinkSync(packageRoot, path.join(scope, 'sdk'), 'dir')

  const consumerRequire = createRequire(path.join(temp, 'consumer.cjs'))
  assert.throws(
    () => consumerRequire('@bittensor/sdk/browser'),
    (error) => error.code === 'ERR_PACKAGE_PATH_NOT_EXPORTED' || error.code === 'ERR_REQUIRE_ESM',
  )

  const consumer = path.join(temp, 'consumer.mjs')
  fs.writeFileSync(
    consumer,
    [
      "import * as browser from '@bittensor/sdk/browser'",
      "export const ok = typeof browser.initBrowser === 'function' && typeof browser.Keypair === 'function'",
      '',
    ].join('\n'),
  )
  const imported = await import(`${pathToFileURL(consumer).href}?${Date.now()}`)
  assert.equal(imported.ok, true)
})

test('browser Runtime exposes WASM codec, call, storage, and extrinsic helpers', async () => {
  const browser = require('../dist/browser.js')

  class FakeRuntime {
    constructor(metadataBytes, specVersion, transactionVersion, ss58Format) {
      this.metadataBytes = metadataBytes
      this.specVersion = specVersion
      this.transactionVersion = transactionVersion
      this.ss58Format = ss58Format
      this.isV15 = true
      this.extrinsicVersion = 4
    }

    decode(typeString, data, strict = true) {
      return { typeString, first: data[0], strict }
    }

    batchDecode(typeStrings, data) {
      return data.map((item, index) => ({ typeString: typeStrings[index], first: item[0] }))
    }

    encode(_typeString, value) {
      return Uint8Array.of(value.value)
    }

    typeIdOf(name) {
      return name === 'Known' ? 7 : undefined
    }

    typeNameOf(id) {
      return id === 7 ? 'Known' : undefined
    }

    registryJson() {
      return '{"ok":true}'
    }

    composeCall(pallet, fn) {
      return Uint8Array.of(pallet.length, fn.length)
    }

    decodeCall(data) {
      return { len: data.length }
    }

    storageEntry(pallet, storageFunction) {
      return {
        pallet,
        name: storageFunction,
        prefix: pallet,
        modifier: 'Default',
        valueType: 'scale_info::1',
        paramTypes: ['scale_info::0'],
        paramHashers: ['Blake2_128Concat'],
        defaultBytes: Uint8Array.of(9),
      }
    }

    storagePrefix() {
      return Uint8Array.of(1)
    }

    storageKey(_pallet, _storageFunction, params) {
      return Uint8Array.of(params.length)
    }

    storageKeyBatch(_pallet, _storageFunction, paramsList) {
      return paramsList.map((params) => Uint8Array.of(params.length))
    }

    decodeStorageKeyParams(_pallet, _storageFunction, key, fixed = 0) {
      return [fixed, key[0]]
    }

    decodeMapPairs(_pallet, _storageFunction, rawKeys, rawValues) {
      return rawKeys.map((key, index) => [key[0], rawValues[index][0]])
    }

    decodeMapChanges(_pallet, _storageFunction, changes) {
      return changes.map(([key, value]) => [key, value])
    }

    constant() {
      return 55
    }

    moduleError() {
      return ['BadOrigin', ['doc']]
    }

    signedExtensionIdentifiers() {
      return ['CheckNonce']
    }

    encodeEra(era) {
      return Uint8Array.of(era.period)
    }

    signaturePayloadParts() {
      return [Uint8Array.of(1), Uint8Array.of(2)]
    }

    signaturePayload(callData, _era, nonce) {
      return Uint8Array.of(callData[0], Number(nonce))
    }

    encodeSignedExtrinsic(callData, _publicKey, _signature, signatureVersion) {
      return [Uint8Array.of(callData[0], signatureVersion), Uint8Array.of(3, 4)]
    }

    decodeExtrinsic(data, strict = true) {
      return { len: data.length, strict }
    }

    runtimeApiMap() {
      return { Api: { method: { name: 'method', inputs: [], output: 'scale_info::0', docs: [] } } }
    }

    metadataIr() {
      return { specVersion: 1, pallets: [], runtimeApis: [] }
    }
  }

  await browser.initBrowser(async () => ({
    Runtime: FakeRuntime,
    eraBirth: (period, current) => Number(current) - (Number(current) % Number(period)),
    multisigAccountId: (signatories, threshold) => [
      Uint8Array.of(threshold),
      signatories.slice().reverse(),
    ],
    getEncryptedCommitV2: (
      uids,
      weights,
      versionKey,
      lastEpochBlock,
      pendingEpochAt,
      subnetEpochIndex,
      tempo,
      blocksSinceLastStep,
      currentBlock,
      subnetRevealPeriodEpochs,
      blockTime,
      hotkey,
    ) => [
      Uint8Array.of(
        uids[0],
        weights[0],
        Number(versionKey),
        Number(lastEpochBlock),
        Number(pendingEpochAt),
        Number(subnetEpochIndex),
        tempo,
        Number(blocksSinceLastStep),
        Number(currentBlock),
        Number(subnetRevealPeriodEpochs),
        blockTime,
        hotkey[0],
      ),
      777,
    ],
  }))

  const runtime = new browser.Runtime(Uint8Array.of(1), 10, 20, 42)
  assert.equal(runtime.specVersion, 10)
  assert.equal(runtime.transactionVersion, 20)
  assert.equal(runtime.ss58Format, 42)
  assert.equal(runtime.isV15, true)
  assert.equal(runtime.extrinsicVersion, 4)
  assert.deepEqual(runtime.decode('u8', Uint8Array.of(5)), { typeString: 'u8', first: 5, strict: true })
  assert.deepEqual(runtime.decodeBatch(['u8'], [Uint8Array.of(6)]), [{ typeString: 'u8', first: 6 }])
  assert.deepEqual(runtime.encode('u8', { value: 7 }), Uint8Array.of(7))
  assert.equal(runtime.typeIdOf('Known'), 7)
  assert.equal(runtime.typeNameOf(7), 'Known')
  assert.equal(runtime.registryJson(), '{"ok":true}')
  assert.deepEqual(runtime.composeCall('System', 'remark', {}), Uint8Array.of(6, 6))
  assert.deepEqual(runtime.decodeCall(Uint8Array.of(1, 2, 3)), { len: 3 })
  assert.deepEqual(runtime.storageEntry('System', 'Account').defaultBytes, Uint8Array.of(9))
  assert.deepEqual(runtime.storagePrefix('System', 'Account'), Uint8Array.of(1))
  assert.deepEqual(runtime.storageKey('System', 'Account', ['Alice']), Uint8Array.of(1))
  assert.deepEqual(runtime.storageKeyBatch('System', 'Account', [['Alice'], ['Alice', 'Bob']]), [
    Uint8Array.of(1),
    Uint8Array.of(2),
  ])
  assert.deepEqual(runtime.decodeStorageKeyParams('System', 'Account', Uint8Array.of(8), 1), [1, 8])
  assert.deepEqual(
    runtime.decodeMapPairs('System', 'Account', [Uint8Array.of(1)], [Uint8Array.of(2)]),
    [{ key: 1, value: 2 }],
  )
  assert.deepEqual(
    runtime.decodeMapChanges('System', 'Account', [{ key: '0x01', value: '0x02' }]),
    [{ key: '0x01', value: '0x02' }],
  )
  assert.equal(runtime.constant('Balances', 'ExistentialDeposit'), 55)
  assert.deepEqual(runtime.moduleError(0, 0), { name: 'BadOrigin', docs: ['doc'] })
  assert.deepEqual(runtime.signedExtensionIdentifiers(), ['CheckNonce'])
  assert.deepEqual(runtime.encodeEra({ period: 64, current: 128 }), Uint8Array.of(64))
  assert.deepEqual(
    runtime.signaturePayloadParts({
      era: '00',
      nonce: 2,
      genesisHash: Uint8Array.of(0),
      eraBlockHash: Uint8Array.of(0),
    }),
    { includedInExtrinsic: Uint8Array.of(1), includedInSignedData: Uint8Array.of(2) },
  )
  assert.deepEqual(
    runtime.signaturePayload(Uint8Array.of(9), {
      era: '00',
      nonce: 2,
      genesisHash: Uint8Array.of(0),
      eraBlockHash: Uint8Array.of(0),
    }),
    Uint8Array.of(9, 2),
  )
  assert.deepEqual(
    runtime.encodeSignedExtrinsic(Uint8Array.of(9), Uint8Array.of(1), Uint8Array.of(2), 1, {
      era: '00',
      nonce: 2,
    }),
    { bytes: Uint8Array.of(9, 1), hash: Uint8Array.of(3, 4) },
  )
  assert.deepEqual(runtime.decodeExtrinsic(Uint8Array.of(1, 2), false), { len: 2, strict: false })
  assert.deepEqual(runtime.runtimeApis(), runtime.runtimeApiMap())
  assert.deepEqual(runtime.metadataIr(), { specVersion: 1, pallets: [], runtimeApis: [] })
  assert.equal(browser.eraBirth(64, 130), 128)
  assert.deepEqual(
    browser.multisigAccountId([Uint8Array.of(1), Uint8Array.of(2)], 2),
    { accountId: Uint8Array.of(2), sortedSignatories: [Uint8Array.of(2), Uint8Array.of(1)] },
  )
  assert.deepEqual(
    browser.generateCommitV2(
      [1],
      [2],
      3,
      {
        lastEpochBlock: 4,
        pendingEpochAt: 5,
        subnetEpochIndex: 6,
        tempo: 7,
        blocksSinceLastStep: 8,
        currentBlock: 9,
      },
      10,
      11,
      Uint8Array.of(12),
    ),
    [Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12), 777],
  )
})

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

test('Python-compatible bittensor_core names are exported', () => {
  const alice = core.Keypair.create_from_uri('//Alice')
  const bob = core.Keypair.from_uri('//Bob')
  const message = Buffer.from('python parity')
  const signature = alice.sign(message)

  assert.equal(core.__core_version__, core.BINDING_VERSION)
  assert.equal(alice.crypto_type, alice.cryptoType)
  assert.deepEqual(alice.public_key, alice.publicKey)
  assert.equal(alice.ss58_address, alice.ss58Address)
  assert.equal(alice.ss58_format, alice.ss58Format)
  assert.deepEqual(core.ss58_decode(alice.ss58_address), alice.publicKey)
  assert.equal(core.decode_ss58, core.ss58_decode)
  assert.equal(core.ss58_encode(alice.public_key, 42), alice.ss58_address)
  assert.equal(core.encode_ss58, core.ss58_encode)
  assert.equal(
    core.verify_signature(message, signature, alice.ss58_address, core.CRYPTO_SR25519),
    true,
  )

  const publicOnly = new core.Keypair(alice.ss58_address)
  assert.throws(() => alice.serialize(), /public-only keypairs/)
  const publicKeyfile = JSON.parse(
    core.serialized_keypair_to_keyfile_data(publicOnly).toString('utf8'),
  )
  assert.equal(publicKeyfile.ss58Address, alice.ss58_address)
  assert.deepEqual(
    JSON.parse(core.serializePublicKeypair(publicOnly).toString('utf8')),
    JSON.parse(publicOnly.serialize().toString('utf8')),
  )
  const privatePlaintext = core.dangerouslySerializePrivateKeypair(alice)
  assert.equal(core.deserializeKeypair(privatePlaintext).ss58_address, alice.ss58_address)
  assert.equal(core.keyfile_data_is_encrypted(Buffer.from('plain')), false)
  assert.deepEqual(core.mlkem_kdf_id(), core.MLKEM_KDF_ID)
  assert.equal(typeof core.metadata_digest, 'function')
  assert.equal(typeof core.generate_extrinsic_proof, 'function')
  assert.equal(typeof core.get_encrypted_commitment, 'function')
  assert.equal(typeof core.get_signature_for_round, 'function')
  assert.equal(core.era_birth(64n, 70n), core.eraBirth(64n, 70n))

  const camelMultisig = core.multisigAccountId([alice.public_key, bob.public_key], 2)
  const [snakeAccountId, snakeSorted] = core.multisig_account_id(
    [alice.public_key, bob.public_key],
    2,
  )
  assert.deepEqual(snakeAccountId, camelMultisig.accountId)
  assert.deepEqual(snakeSorted, camelMultisig.sortedSignatories)
  assert.equal(core.rustCore.keys.ss58_decode, core.ss58_decode)
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

  // sr25519 uses a randomized nonce, so two independently created signatures
  // for the same payload are both valid but are not required to be identical.
  assert.equal(alice.verify(payload, raw, alice.publicKey), true)
  assert.equal(alice.verify(payload, typed, alice.publicKey), true)
  const srSignatureLabelledEd25519 = Buffer.concat([
    Buffer.from([core.CRYPTO_ED25519]),
    raw,
  ])
  assert.equal(alice.verify(payload, srSignatureLabelledEd25519), false)

  const aliceEd25519 = core.Keypair.fromUri('//Alice', core.CRYPTO_ED25519)
  const edRaw = aliceEd25519.sign(payload)
  const edSignatureLabelledSr25519 = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    edRaw,
  ])
  assert.equal(aliceEd25519.verify(payload, edSignatureLabelledSr25519), false)
})

test('mnemonics and secret URIs never appear in public keypair metadata', () => {
  const mnemonic =
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
  const fromMnemonic = core.Keypair.fromMnemonic(mnemonic)

  assert.equal(fromMnemonic.meta.suri, undefined)
  assert.equal(Object.prototype.hasOwnProperty.call(fromMnemonic, 'sourceUri'), false)
  assert.equal(Object.prototype.hasOwnProperty.call(fromMnemonic, 'metadata'), false)

  fromMnemonic.setMeta({ name: 'safe', suri: mnemonic })
  assert.equal(fromMnemonic.meta.name, 'safe')
  assert.equal(fromMnemonic.meta.suri, undefined)

  const metadataView = fromMnemonic.meta
  metadataView.suri = mnemonic
  assert.equal(fromMnemonic.meta.suri, undefined)

  const secretUri = `${mnemonic}//review-secret`
  const fromUri = core.Keypair.fromUri(secretUri)
  assert.equal(fromUri.meta.suri, undefined)
  assert.equal(Object.prototype.hasOwnProperty.call(fromUri, 'sourceUri'), false)

  const derived = fromUri.derive('//child')
  assert.equal(derived.meta.suri, undefined)
})

test('private key bytes are not exported to JavaScript', async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-keyfile-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const alice = core.Keypair.fromUri('//Alice')
  assert.equal(Object.prototype.hasOwnProperty.call(alice, 'privateKey'), false)
  assert.equal('privateKey' in alice, false)
  assert.equal(
    Object.prototype.hasOwnProperty.call(core.native.NativeKeypair.prototype, 'privateKey'),
    false,
  )
  assert.throws(() => alice.serialize(), /public-only keypairs/)
  assert.throws(() => core.serializeKeypair(alice), /public-only keypairs/)
  const rawNativeSerialize = core.native.serializeKeypair(
    core.native.keypairFromUri('//Alice', core.CRYPTO_SR25519),
  )
  assert.equal(rawNativeSerialize instanceof Error, true)
  assert.match(rawNativeSerialize.message, /plaintext private key serialization is disabled/)
  assert.equal(
    core.deserializeKeypair(core.dangerouslySerializePrivateKeypair(alice)).ss58Address,
    alice.ss58Address,
  )

  const publicOnly = new core.Keypair(alice.ss58Address)
  const publicKeyfile = JSON.parse(publicOnly.serialize().toString('utf8'))
  assert.equal(publicKeyfile.privateKey, undefined)

  await assert.rejects(
    () => alice.writeKeyfile(path.join(root, 'plaintext-default')),
    /plaintext private keyfile writes are disabled/,
  )
  await alice.writeKeyfile(path.join(root, 'plaintext-allowed'), { allowPlaintext: true })

  const encrypted = await alice.toKeyfileData('review-password')
  assert.equal(core.keyfileDataIsEncrypted(encrypted), true)
  await assert.rejects(
    () => alice.toKeyfileData(''),
    /keyfile password must not be empty/,
  )
  await assert.rejects(
    () => alice.writeKeyfile(path.join(root, 'empty-password'), { password: '' }),
    /keyfile password must not be empty/,
  )
})

test('fallible Runtime construction uses the native factory', () => {
  assert.throws(
    () => new core.Runtime(Buffer.from([0, 1, 2, 3]), 1, 1),
    (error) => error instanceof core.CodecError,
  )
})

test('Runtime signer payload encodes signed extension fields through metadata', () => {
  const runtime = new core.Runtime(goldenMetadataBytes(), 419, 1, 42)
  const genesisHash = Buffer.alloc(32, 0)
  const eraBlockHash = Buffer.alloc(32, 0x42)
  const metadataHash = Buffer.alloc(32, 3)
  const payload = runtime.signerPayload('5F', Buffer.from([5, 6, 7]), {
    era: { period: 64, current: 70 },
    nonce: 12,
    tip: 1,
    tipAssetId: null,
    genesisHash,
    eraBlockHash,
    metadataHash,
  })

  assert.equal(payload.address, '5F')
  assert.equal(payload.method, '0x050607')
  assert.equal(payload.blockHash, `0x${eraBlockHash.toString('hex')}`)
  assert.equal(payload.blockNumber, '0x46000000')
  assert.equal(payload.nonce, '0x30')
  assert.equal(payload.tip, '0x04')
  assert.equal(payload.assetId == null, true)
  assert.equal(payload.metadataHash, `0x${metadataHash.toString('hex')}`)
  assert.equal(payload.mode, 1)
  assert.ok(payload.signedExtensions.includes('ChargeTransactionPayment'))

  const noAsset = runtime.signerPayload('5F', Buffer.from([5, 6, 7]), {
    era: '00',
    nonce: 12,
    tip: 1,
    tipAssetId: null,
    genesisHash,
    eraBlockHash,
    metadataHash: null,
  })
  assert.equal(noAsset.assetId == null, true)
  assert.equal(noAsset.metadataHash == null, true)
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

test('dynamic boundary preserves large bigint, bytes, and non-string map keys', () => {
  const bigintKey = 2n ** 80n
  const input = new Map([
    [bigintKey, Buffer.from([0, 1, 254, 255])],
    ['nested', { value: 2n ** 200n, wideSafeNumber: Number.MAX_SAFE_INTEGER }],
  ])
  const output = core.wireRoundtrip(input)
  assert.ok(output instanceof Map)
  assert.deepEqual(output.get(bigintKey), Buffer.from([0, 1, 254, 255]))
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

test('MEV shield high-level helper prefixes ciphertext with key hash', () => {
  const publicKey = Buffer.alloc(1184, 0x42)
  const ciphertext = core.sealMevShieldTransaction(publicKey, Buffer.from('payload'))

  assert.deepEqual(ciphertext.subarray(0, 16), core.twox_128(publicKey))
  assert.equal(ciphertext.readUInt16LE(16), 1088)
})

test('browser MEV shield wrapper keeps high-level prefix and low-level option', async () => {
  const browser = await import(`${pathToFileURL(path.join(__dirname, '..', 'dist', 'browser.mjs')).href}?shield=${Date.now()}`)
  const calls = []
  browser.configureBrowserWasm(async () => ({
    default: async () => undefined,
    encryptMlkem768(publicKey, plaintext, includeKeyHash = false) {
      calls.push({
        publicKey: Array.from(publicKey),
        plaintext: Array.from(plaintext),
        includeKeyHash,
      })
      return Uint8Array.of(includeKeyHash ? 1 : 0, publicKey[0] ?? 0, plaintext[0] ?? 0)
    },
  }))
  await browser.initBrowser()

  assert.deepEqual(
    Array.from(browser.sealMevShieldTransaction(Uint8Array.of(7), Uint8Array.of(8))),
    [1, 7, 8],
  )
  assert.equal(calls.at(-1).includeKeyHash, true)
  assert.deepEqual(
    Array.from(browser.encryptMlkem768(Uint8Array.of(7), Uint8Array.of(8), false)),
    [0, 7, 8],
  )
  assert.equal(calls.at(-1).includeKeyHash, false)
})

test('browser Keypair.verify rejects wrong typed signature scheme before native verify', async () => {
  const browser = await import(`${pathToFileURL(path.join(__dirname, '..', 'dist', 'browser.mjs')).href}?verify=${Date.now()}`)
  let verifyCalls = 0
  class FakeKeypair {
    constructor(ss58Address, publicKey, cryptoType = browser.CRYPTO_SR25519, ss58Format = 42) {
      this.ss58Address = ss58Address ?? '5Fake'
      this.publicKey = publicKey ?? new Uint8Array(32)
      this.cryptoType = cryptoType
      this.ss58Format = ss58Format
      this.kind = cryptoType === browser.CRYPTO_ED25519 ? 'Ed25519' : 'Sr25519'
    }

    sign() {
      return new Uint8Array(64)
    }

    verify() {
      verifyCalls += 1
      return true
    }

    derive() {
      return this
    }
  }
  browser.configureBrowserWasm(async () => ({
    default: async () => undefined,
    Keypair: FakeKeypair,
  }))
  await browser.initBrowser()
  const keypair = new browser.Keypair('5Fake', new Uint8Array(32), browser.CRYPTO_SR25519, 42)
  const wrong = new Uint8Array(65)
  wrong[0] = browser.CRYPTO_ED25519
  const right = new Uint8Array(65)
  right[0] = browser.CRYPTO_SR25519

  assert.equal(keypair.verify(Uint8Array.of(1), wrong), false)
  assert.equal(verifyCalls, 0)
  assert.equal(keypair.verify(Uint8Array.of(1), right), true)
  assert.equal(verifyCalls, 1)
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
  assert.equal(esm.Client, core.Client)
  assert.equal(esm.generateKeyringPair, core.generateKeyringPair)
  assert.equal(esm.bytesToHex, core.bytesToHex)

  const root = path.join(__dirname, '..')
  const source = fs.readFileSync(path.join(root, 'dist', 'index.mjs'), 'utf8')
  assert.equal(source.includes("import * as sdk from './index.js'"), true)
  assert.equal(source.includes("import sdk from './index.js'"), false)
})

test('exact codec::Value descriptors preserve every Rust enum variant', () => {
  const descriptor = core.coreValueDict([
    { key: core.coreValueString('int'), value: core.coreValueInt(-(2n ** 127n)) },
    { key: core.coreValueString('uint'), value: core.coreValueUint(2n ** 128n - 1n) },
    {
      key: core.coreValueString('u256'),
      value: core.coreValueU256Le(Buffer.alloc(32, 0xff)),
    },
    {
      key: core.coreValueString('containers'),
      value: core.coreValueTuple([
        core.coreValueList([core.coreValueBytes(Buffer.from([1, 2, 3]))]),
        core.coreValueString('text'),
      ]),
    },
  ])

  const normalized = core.normalizeCoreValue(descriptor)
  assert.equal(normalized.kind, 'dict')
  assert.equal(normalized.entries[0].value.kind, 'int')
  assert.equal(normalized.entries[1].value.kind, 'uint')
  assert.equal(normalized.entries[2].value.kind, 'u256')
  assert.equal(normalized.entries[3].value.kind, 'tuple')
  assert.equal(
    core.u256LeToDecimal(Buffer.alloc(32, 0xff)),
    '115792089237316195423570985008687907853269984665640564039457584007913129639935',
  )
})

test('public Cursor and TypeSpec APIs are forwarded to Rust', () => {
  const encoded = core.encodeCompact(16384n)
  const cursor = new core.ScaleCursor(Buffer.concat([Buffer.from([9]), encoded]), true)
  assert.equal(cursor.byte(), 9)
  assert.equal(cursor.decodeCompactU128(), 16384n)
  assert.equal(cursor.remaining, 0)

  assert.deepEqual(core.typeSpec.array(core.typeSpec.primitive('u8'), 32), {
    kind: 'array',
    inner: { kind: 'primitive', name: 'u8' },
    length: 32,
  })
  assert.equal(core.primitiveFromName('String'), 'str')
  assert.equal(core.convertTypeString('Vec<u8>'), 'Bytes')
})

test('epoch errors are available as exact Rust variants without throwing', () => {
  const state = {
    lastEpochBlock: 0n,
    pendingEpochAt: 0n,
    subnetEpochIndex: 0n,
    tempo: 0,
    blocksSinceLastStep: 0n,
    currentBlock: 0n,
  }
  assert.deepEqual(core.predictFirstRevealBlockResult(state, 1n), {
    ok: false,
    block: null,
    error: 'TempoIsZero',
  })
})

test('module-shaped export mirrors the public Rust crate', () => {
  assert.equal(core.rustCore.keys.Keypair, core.Keypair)
  assert.equal(core.rustCore.codec.decode.Cursor, core.ScaleCursor)
  assert.equal(core.rustCore.codec.batch.PARALLEL_THRESHOLD, core.PARALLEL_THRESHOLD)
  assert.equal(core.rustCore.client.Client, core.Client)
  assert.equal(core.rustCore.client.storage.System.Events[0], 'System')
  assert.equal(core.rustCore.signers.ledger.LedgerSigner, core.LedgerSigner)
  assert.equal(core.rustCore.mlkem.MLKEM_NONCE_LEN, 24)
  assert.equal(core.rustCore.timelock.constants.GENESIS_TIME, core.GENESIS_TIME)
  assert.deepEqual(core.rustCore.timelock.epoch_schedule.EpochScheduleError, [
    'BoundExceeded',
    'TempoIsZero',
  ])
})

test('chain client surface is exported without Polkadot.js glue', () => {
  assert.equal(typeof core.Client, 'function')
  assert.equal(typeof core.Client.prototype.watchSigned, 'function')
  assert.equal(Object.prototype.hasOwnProperty.call(core, 'NativeChainClient'), false)
  assert.equal(Object.prototype.hasOwnProperty.call(core, 'RustWallet'), false)
  assert.equal(Object.prototype.hasOwnProperty.call(core, 'Executor'), false)
  assert.equal(core.Subtensor, core.SubtensorClient)
  assert.equal(typeof core.subtensor, 'function')
  assert.equal(typeof core.Wallet, 'function')
  assert.equal(typeof core.Balance.fromTao, 'function')
  assert.deepEqual(core.storage.SubtensorModule.NetworksAdded, [
    'SubtensorModule',
    'NetworksAdded',
  ])
  assert.deepEqual(core.runtimeApi.SubnetInfoRuntimeApi.get_metagraph, [
    'SubnetInfoRuntimeApi',
    'get_metagraph',
  ])
  assert.deepEqual(core.runtimeApi.SubnetInfoRuntimeApi.get_subnet_hyperparams_v3, [
    'SubnetInfoRuntimeApi',
    'get_subnet_hyperparams_v3',
  ])
  assert.equal(
    Object.prototype.hasOwnProperty.call(core.runtimeApi.SubnetInfoRuntimeApi, 'get_subnet_hyperparams'),
    false,
  )
  assert.deepEqual(core.calls.subtensor.rootRegister('5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY'), [
    'SubtensorModule',
    'root_register',
    { hotkey: '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY' },
  ])
  assert.deepEqual(core.calls.SubtensorModule.root_register('5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY'), [
    'SubtensorModule',
    'root_register',
    { hotkey: '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY' },
  ])
})

test('declared Node runtime supports the default WebSocket client path', () => {
  assert.equal(typeof globalThis.WebSocket, 'function')
})

test('Client subnet hyperparameters read uses the v3 runtime API', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const calls = []
  client.runtime = async (method, params, block) => {
    calls.push({ method, params, block })
    return { ok: true }
  }

  assert.deepEqual(await client.subnets.hyperparameters(7, '0xabc'), { ok: true })
  assert.deepEqual(await client.getSubnetHyperparameters(8), { ok: true })
  assert.deepEqual(calls, [
    {
      method: ['SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3'],
      params: [7],
      block: '0xabc',
    },
    {
      method: ['SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3'],
      params: [8],
      block: undefined,
    },
  ])
})

test('Balance numeric getters throw before losing precision', () => {
  const small = core.Balance.fromTao('1.25')
  assert.equal(small.amount, 1.25)
  assert.equal(small.tao, 1.25)
  assert.equal(small.amountString, '1.25')
  assert.equal(small.taoString, '1.25')

  const alpha = core.Balance.fromRao('123000000000', 7, 'ALPHA')
  assert.equal(alpha.alphaString, '123')
  assert.equal(alpha.toString(), '123 ALPHA')

  const unsafe = core.Balance.fromRao((BigInt(Number.MAX_SAFE_INTEGER) + 1n).toString())
  assert.equal(unsafe.amountString, '9007199.254740992')
  assert.throws(() => unsafe.amount, /safe integer precision/)
  assert.throws(() => unsafe.tao, /safe integer precision/)
})

test('transaction amounts require explicit units', () => {
  assert.equal(core.Balance.fromRao('1').rao, 1n)
  assert.equal(core.Balance.fromTao('1.0').rao, 1_000_000_000n)
  assert.throws(
    () => core.Balance.fromTao('0.1234567891'),
    /more than 9 decimal places/,
  )
  assert.throws(
    () => core.Balance.fromRao('1.0'),
    /rao must be an integer/,
  )

  assert.deepEqual(core.calls.balances.transferKeepAlive('5F', 1n), [
    'Balances',
    'transfer_keep_alive',
    { dest: '5F', value: 1n },
  ])
  assert.deepEqual(core.calls.balances.transferKeepAlive('5F', core.taoAmount('1.0')), [
    'Balances',
    'transfer_keep_alive',
    { dest: '5F', value: 1_000_000_000n },
  ])
  assert.deepEqual(core.calls.balances.transferKeepAlive('5F', core.raoAmount('2')), [
    'Balances',
    'transfer_keep_alive',
    { dest: '5F', value: 2n },
  ])
  assert.deepEqual(core.calls.subtensor.removeStake('5F', 8, core.alphaAmount('1.0', 8)), [
    'SubtensorModule',
    'remove_stake',
    { hotkey: '5F', netuid: 8, amount_unstaked: 1_000_000_000n },
  ])
  assert.deepEqual(core.calls.balances.transferKeepAlive('5F', core.Balance.fromTao('0.5')), [
    'Balances',
    'transfer_keep_alive',
    { dest: '5F', value: 500_000_000n },
  ])
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', -1n),
    /transfer amount must be non-negative/,
  )
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', core.raoAmount('-1')),
    /transfer amount must be non-negative/,
  )
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', core.Balance.fromAlpha('1', 7)),
    /must be a TAO balance/,
  )
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', core.alphaAmount('1', 7)),
    /must be a TAO amount/,
  )
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', '1'),
    /transfer amount must be/,
  )
  assert.throws(
    () => core.calls.balances.transferKeepAlive('5F', 1),
    /transfer amount must be/,
  )
  assert.equal(core.assetIdValue('1'), 1n)
  assert.equal(core.assetIdValue(1), 1n)
  assert.equal(core.assetIdValue(1n), 1n)
  assert.throws(
    () => core.assetIdValue('-1'),
    /must be non-negative/,
  )
  assert.throws(
    () => core.assetIdValue('1.0'),
    /must be an integer/,
  )
  assert.throws(
    () => core.assetIdValue(core.taoAmount('1')),
    /must be a bigint/,
  )
  assert.throws(
    () => core.calls.subtensor.removeStake('5F', 8, core.Balance.fromAlpha('1', 7)),
    /subnet-8 alpha/,
  )
  assert.throws(
    () => core.calls.subtensor.removeStake('5F', 8, core.taoAmount('1')),
    /must be subnet-8 alpha, not TAO/,
  )
  assert.throws(
    () => core.calls.subtensor.removeStake('5F', 8, core.alphaAmount('1', 7)),
    /subnet-8 alpha, not subnet-7 alpha/,
  )
  assert.deepEqual(core.calls.subtensor.removeStake('5F', 8, core.Balance.fromAlpha('1', 8)), [
    'SubtensorModule',
    'remove_stake',
    { hotkey: '5F', netuid: 8, amount_unstaked: 1_000_000_000n },
  ])
  assert.deepEqual(core.calls.subtensor.serveAxon(1, '2001:db8::1', 30333), [
    'SubtensorModule',
    'serve_axon',
    {
      netuid: 1,
      version: 0,
      ip: 0x20010db8000000000000000000000001n,
      port: 30333,
      ip_type: 6,
      protocol: 4,
      placeholder1: 0,
      placeholder2: 0,
    },
  ])
  assert.deepEqual(core.calls.subtensor.serveAxon(1, 0x1_0000_0000n, 30333), [
    'SubtensorModule',
    'serve_axon',
    {
      netuid: 1,
      version: 0,
      ip: 0x1_0000_0000,
      port: 30333,
      ip_type: 6,
      protocol: 4,
      placeholder1: 0,
      placeholder2: 0,
    },
  ])
  assert.deepEqual(core.calls.subtensor.register(
    1,
    '9007199254740993',
    '9007199254740995',
    Buffer.from([1]),
    '5F',
    '5G',
  ), [
    'SubtensorModule',
    'register',
    {
      netuid: 1,
      block_number: 9007199254740993n,
      nonce: 9007199254740995n,
      work: Buffer.from([1]),
      hotkey: '5F',
      coldkey: '5G',
    },
  ])
  assert.throws(
    () => core.calls.subtensor.setWeights(1, [], [], Number.MAX_SAFE_INTEGER + 1),
    /versionKey must be a safe integer/,
  )
  assert.throws(
    () => core.calls.SubtensorModule.register(1, Number.MAX_SAFE_INTEGER + 1, 1, Buffer.alloc(0), '5F', '5G'),
    /blockNumber must be a safe integer/,
  )
  assert.throws(
    () => core.calls.subtensor.register(1, 1, Number.MAX_SAFE_INTEGER + 1, Buffer.alloc(0), '5F', '5G'),
    /nonce must be a safe integer/,
  )
  assert.throws(
    () => core.calls.subtensor.revealWeights(1, [], [], [], '0x10'),
    /versionKey must be an integer string/,
  )
  assert.throws(
    () => core.calls.subtensor.serveAxon(1, '2001:db8::1', 30333, 0, 4),
    /ipType does not match/,
  )
  assert.throws(
    () => core.calls.subtensor.serveAxon(1, '192.0.2.1', 30333, 0, 6),
    /ipType does not match/,
  )
  assert.throws(
    () => core.calls.subtensor.serveAxon(1, 0x1_0000_0000n, 30333, 0, 4),
    /IPv4 address must fit in u32/,
  )
  assert.throws(
    () => core.calls.subtensor.serveAxon(1, '1:2:3:4:5:6:7::8', 30333),
    /invalid IPv6 address/,
  )
})

test('descriptor schema validation reports metadata drift', () => {
  const runtime = {
    pallet(name) {
      if (name === 'Balances') return { storage: [{ name: 'Account' }], constants: [] }
      return null
    },
    constantInfo() {
      return null
    },
    runtimeApis() {
      return {}
    },
    metadataIr() {
      return {
        pallets: [{
          name: 'Balances',
          calls: [
            { name: 'transfer_keep_alive' },
            { name: 'transfer_allow_death', args: ['dest', 'value'], argTypeIds: [174, 999] },
          ],
        }],
      }
    },
  }
  const issues = core.validateDescriptorSchema(runtime)
  assert.ok(issues.some((issue) => issue.path === 'storage.Balances.TotalIssuance'))
  assert.ok(issues.some((issue) => issue.path === 'runtimeApi.StakeInfoRuntimeApi.get_stake_fee'))
  assert.ok(issues.some((issue) => issue.path === 'calls.balances.transferKeepAlive' && /argument count/.test(issue.message)))
  assert.ok(issues.some((issue) => issue.path === 'calls.balances.transferAllowDeath' && /type drifted/.test(issue.message)))
})

test('descriptor schema validation accepts metadata-local type ID shifts', () => {
  const runtime = {
    pallet(name) {
      if (name === 'Balances') return { storage: Object.values(core.storage.Balances).map(([, item]) => ({ name: item })) }
      if (name === 'SubtensorModule') return { storage: Object.values(core.storage.SubtensorModule).map(([, item]) => ({ name: item })) }
      if (name === 'System') return { storage: Object.values(core.storage.System).map(([, item]) => ({ name: item })) }
      if (name === 'Timestamp') return { storage: Object.values(core.storage.Timestamp).map(([, item]) => ({ name: item })) }
      if (name === 'Multisig') return { storage: Object.values(core.storage.Multisig).map(([, item]) => ({ name: item })) }
      if (name === 'Proxy') return { storage: Object.values(core.storage.Proxy).map(([, item]) => ({ name: item })) }
      return null
    },
    constantInfo() {
      return {}
    },
    runtimeApis() {
      return Object.fromEntries(
        Object.entries(core.runtimeApi).map(([api, methods]) => [
          api,
          Object.fromEntries(Object.keys(methods).map((method) => [method, { inputs: [], inputDetails: [], outputTypeId: 0 }])),
        ]),
      )
    },
    metadataIr() {
      return {
        pallets: [
          {
            name: 'Balances',
            calls: [
              { name: 'transfer_keep_alive', args: ['dest', 'value'], argTypeIds: [176, 178] },
              { name: 'transfer_allow_death', args: ['dest', 'value'], argTypeIds: [176, 178] },
            ],
          },
          {
            name: 'SubtensorModule',
            calls: [
              { name: 'add_stake', args: ['hotkey', 'netuid', 'amount_staked'], argTypeIds: [0, 40, 6] },
              { name: 'burned_register', args: ['netuid', 'hotkey'], argTypeIds: [40, 0] },
              { name: 'commit_weights', args: ['netuid', 'commit_hash'], argTypeIds: [40, 13] },
              { name: 'move_stake', args: ['origin_hotkey', 'destination_hotkey', 'origin_netuid', 'destination_netuid', 'alpha_amount'], argTypeIds: [0, 0, 40, 40, 6] },
              { name: 'register', args: ['netuid', 'block_number', 'nonce', 'work', 'hotkey', 'coldkey'], argTypeIds: [40, 6, 6, 14, 0, 0] },
              { name: 'register_network', args: ['hotkey'], argTypeIds: [0] },
              { name: 'remove_stake', args: ['hotkey', 'netuid', 'amount_unstaked'], argTypeIds: [0, 40, 6] },
              { name: 'reveal_weights', args: ['netuid', 'uids', 'values', 'salt', 'version_key'], argTypeIds: [40, 209, 209, 209, 6] },
              { name: 'root_register', args: ['hotkey'], argTypeIds: [0] },
              { name: 'serve_axon', args: ['netuid', 'version', 'ip', 'port', 'ip_type', 'protocol', 'placeholder1', 'placeholder2'], argTypeIds: [40, 4, 8, 40, 2, 2, 2, 2] },
              { name: 'serve_prometheus', args: ['netuid', 'version', 'ip', 'port', 'ip_type'], argTypeIds: [40, 4, 8, 40, 2] },
              { name: 'set_children', args: ['hotkey', 'netuid', 'children'], argTypeIds: [0, 40, 44] },
              { name: 'set_weights', args: ['netuid', 'dests', 'weights', 'version_key'], argTypeIds: [40, 209, 209, 6] },
              { name: 'start_call', args: ['netuid'], argTypeIds: [40] },
              { name: 'transfer_stake', args: ['destination_coldkey', 'hotkey', 'origin_netuid', 'destination_netuid', 'alpha_amount'], argTypeIds: [0, 0, 40, 40, 6] },
              { name: 'unstake_all', args: ['hotkey'], argTypeIds: [0] },
            ],
          },
        ],
      }
    },
    typeNameOf(typeId) {
      return {
        0: 'AccountId32',
        2: 'u8',
        4: 'u32',
        6: 'u64',
        8: 'u128',
        13: 'H256',
        14: 'Vec<u8>',
        40: 'u16',
        44: 'Vec<(u64, AccountId32)>',
        176: 'MultiAddress<AccountId32, ()>',
        178: 'Compact<u64>',
        209: 'Vec<u16>',
      }[typeId] ?? null
    },
  }
  const issues = core.validateDescriptorSchema(runtime)
  assert.deepEqual(issues, [])
})

test('Rust decoder unwraps Metadata_metadata_at_version responses strictly', () => {
  const response = Buffer.from(goldenMetadataResponseHex().slice(2), 'hex')
  assert.deepEqual(core.decodeOptionalOpaqueMetadata(response), goldenMetadataBytes())
  assert.equal(core.decodeOptionalOpaqueMetadata(Buffer.from([0])), null)
  assert.throws(
    () => core.decodeOptionalOpaqueMetadata(Buffer.from([2])),
    /invalid Metadata_metadata_at_version response/,
  )
  assert.throws(
    () => core.decodeOptionalOpaqueMetadata(Buffer.concat([response, Buffer.from([0])])),
    /trailing bytes/,
  )
})

test('JsonRpcTransport restores websocket subscriptions after reconnect', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  let nextSubscription = 1
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'chain_subscribeNewHeads') {
      const subscription = `sub-${nextSubscription++}`
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: subscription }))
      return
    }
    if (message.method === 'chain_unsubscribeNewHeads') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: true }))
    }
  }

  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const subscription = await transport.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  const iterator = subscription[Symbol.asyncIterator]()
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1', result: { number: 1 } },
  })
  assert.deepEqual(await iterator.next(), { done: false, value: { number: 1 } })

  FakeWebSocket.sockets[0].close()
  await waitFor(
    () => FakeWebSocket.sockets.length === 2 &&
      FakeWebSocket.sockets[1].sent.some((message) => message.method === 'chain_subscribeNewHeads'),
    'resubscribe after reconnect',
  )
  FakeWebSocket.sockets[1].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-2', result: { number: 2 } },
  })
  assert.deepEqual(await iterator.next(), { done: false, value: { number: 2 } })

  await subscription.unsubscribe()
})

test('JsonRpcTransport routes subscriptions only to WebSocket fallbacks', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  let nextSubscription = 1
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'chain_subscribeNewHeads') {
      const subscription = `sub-${nextSubscription++}`
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: subscription }))
      return
    }
    if (message.method === 'chain_unsubscribeNewHeads') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: true }))
    }
  }

  const httpPrimary = new core.JsonRpcTransport('http://node-a', ['ws://node-b'], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const initialSubscription = await httpPrimary.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  assert.equal(FakeWebSocket.sockets.at(-1).url, 'ws://node-b')
  await initialSubscription.unsubscribe()

  const mixedFallbacks = new core.JsonRpcTransport('ws://node-a', ['http://node-b', 'ws://node-c'], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const subscription = await mixedFallbacks.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  mixedFallbacks.endpointIndex = 1
  FakeWebSocket.sockets.at(-1).close()
  await waitFor(
    () => FakeWebSocket.sockets.some((socket) =>
      socket.url === 'ws://node-c' &&
        socket.sent.some((message) => message.method === 'chain_subscribeNewHeads'),
    ),
    'resubscribe on websocket fallback',
  )
  await subscription.unsubscribe()
})

test('JsonRpcTransport rejects old pending websocket requests on endpoint rotation', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  FakeWebSocket.onSend = () => undefined
  const transport = new core.JsonRpcTransport('ws://node-a', ['ws://node-b'], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
    retryBackoffMs: 1,
    maxRetryBackoffMs: 1,
  })

  const oldPending = transport.request('state_getStorage', [], {
    timeoutMs: 1_000,
    maxRetries: 0,
  })
  await waitFor(() => FakeWebSocket.sockets[0]?.sent.length === 1, 'first pending request')
  const rotating = transport.request('state_getMetadata', [], {
    timeoutMs: 5,
    maxRetries: 1,
    retryBackoffMs: 1,
    maxRetryBackoffMs: 1,
  })
  rotating.catch(() => undefined)

  await assert.rejects(oldPending, /endpoint rotated from ws:\/\/node-a/)
})

test('JsonRpcTransport rejects pending requests on malformed websocket JSON', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })

  const pending = transport.request('state_getMetadata')
  await waitFor(() => FakeWebSocket.sockets[0]?.sent.length === 1, 'websocket request send')
  FakeWebSocket.sockets[0].emit('message', { data: '{not-json' })

  await assert.rejects(pending, /invalid JSON-RPC message/)
})

test('JsonRpcTransport validates WebSocket JSON-RPC response envelopes', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })

  const missingResult = transport.request('state_getMetadata')
  await waitFor(() => FakeWebSocket.sockets[0]?.sent.length === 1, 'websocket request send')
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    id: FakeWebSocket.sockets[0].sent[0].id,
  })
  await assert.rejects(missingResult, /exactly one of result or error/)

  const malformedError = transport.request('state_getMetadata')
  await waitFor(() => FakeWebSocket.sockets[0]?.sent.length === 2, 'second websocket request send')
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    id: FakeWebSocket.sockets[0].sent[1].id,
    error: { message: 'missing code' },
  })
  await assert.rejects(malformedError, /invalid JSON-RPC error response/)
})

test('JsonRpcTransport validates HTTP JSON-RPC envelopes and response size', async (t) => {
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })

  let mode = 'id'
  globalThis.fetch = async (_url, init) => {
    const request = JSON.parse(String(init.body))
    let body
    if (mode === 'id') {
      body = JSON.stringify({ jsonrpc: '2.0', id: request.id + 1, result: '0x00' })
    } else if (mode === 'both') {
      body = JSON.stringify({
        jsonrpc: '2.0',
        id: request.id,
        result: '0x00',
        error: { message: 'also bad' },
      })
    } else {
      body = JSON.stringify({ jsonrpc: '2.0', id: request.id, result: 'x'.repeat(128) })
    }
    return new Response(body, {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  }

  const transport = new core.JsonRpcTransport('http://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
    maxMessageBytes: 256,
  })
  await assert.rejects(
    () => transport.request('state_getMetadata'),
    /id did not match/,
  )

  mode = 'both'
  await assert.rejects(
    () => transport.request('state_getMetadata'),
    /exactly one of result or error/,
  )

  mode = 'large'
  const cappedTransport = new core.JsonRpcTransport('http://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
    maxMessageBytes: 32,
  })
  await assert.rejects(
    () => cappedTransport.request('state_getMetadata'),
    /exceeded size limit/,
  )
})

test('JsonRpcTransport caps subscription notification queues', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'chain_subscribeNewHeads') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'sub-1' }))
    }
  }
  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
    maxSubscriptionQueue: 1,
  })
  const subscription = await transport.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  const iterator = subscription[Symbol.asyncIterator]()
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1', result: { number: 1 } },
  })
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1', result: { number: 2 } },
  })

  assert.deepEqual(await iterator.next(), { done: false, value: { number: 1 } })
  await assert.rejects(() => iterator.next(), /subscription notification queue exceeded limit/)
})

test('JsonRpcTransport validates subscription notification envelopes', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'chain_subscribeNewHeads') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'sub-1' }))
    }
  }
  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const subscription = await transport.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  const iterator = subscription[Symbol.asyncIterator]()
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1' },
  })

  await assert.rejects(() => iterator.next(), /missing result/)
})

test('JsonRpcTransport does not resubmit submit-and-watch subscriptions after reconnect', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  let submissions = 0
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'author_submitAndWatchExtrinsic') {
      submissions += 1
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'watch-1' }))
    }
  }

  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const subscription = await transport.subscribe(
    'author_submitAndWatchExtrinsic',
    ['0x01'],
    'author_unwatchExtrinsic',
    { resubscribe: false },
  )
  const iterator = subscription[Symbol.asyncIterator]()
  const pending = iterator.next()

  FakeWebSocket.sockets[0].close()

  await assert.rejects(pending, /connection closed/)
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(submissions, 1)
  assert.equal(FakeWebSocket.sockets.length, 1)
})

test('JsonRpcTransport bounds retries and supports request cancellation', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  let requests = 0
  FakeWebSocket.onSend = () => {
    requests += 1
  }
  const transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 5,
    maxRequestRetries: 1,
    retryBackoffMs: 1,
    maxRetryBackoffMs: 1,
  })

  await assert.rejects(
    () => transport.request('state_getMetadata'),
    (error) => error.name === 'RequestTimeoutError',
  )
  assert.equal(requests, 2)

  const controller = new AbortController()
  const pending = transport.request('state_getMetadata', [], {
    signal: controller.signal,
    timeoutMs: 1_000,
    maxRetries: 0,
  })
  controller.abort()
  await assert.rejects(
    pending,
    (error) => error.name === 'RequestAbortedError',
  )
})

test('JsonRpcTransport only retries idempotent RPC methods by default', async (t) => {
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })
  let requests = 0
  globalThis.fetch = async () => {
    requests += 1
    throw new Error('connection dropped')
  }
  const transport = new core.JsonRpcTransport('http://node-a', [], false, {
    requestTimeoutMs: 5,
    maxRequestRetries: 2,
    retryBackoffMs: 1,
    maxRetryBackoffMs: 1,
  })

  await assert.rejects(
    () => transport.request('engine_createBlock'),
    /connection dropped/,
  )
  assert.equal(requests, 1)

  requests = 0
  await assert.rejects(
    () => transport.request('state_getMetadata'),
    /connection dropped/,
  )
  assert.equal(requests, 3)

  requests = 0
  await assert.rejects(
    () => transport.request('engine_createBlock', [], { maxRetries: 2 }),
    /connection dropped/,
  )
  assert.equal(requests, 3)
})

test('JsonRpcTransport can disable retryForever for transaction submissions', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  let requests = 0
  FakeWebSocket.onSend = () => {
    requests += 1
  }
  const transport = new core.JsonRpcTransport('ws://node-a', ['ws://node-b'], true, {
    requestTimeoutMs: 5,
    retryBackoffMs: 1,
    maxRetryBackoffMs: 1,
  })

  await assert.rejects(
    () => transport.request('author_submitExtrinsic', ['0x00'], {
      timeoutMs: 5,
      maxRetries: 0,
      retryForever: false,
    }),
    (error) => error.name === 'RequestTimeoutError',
  )
  assert.equal(requests, 1)
  assert.equal(FakeWebSocket.sockets.length, 1)
})

test('Client accepts an injected WebSocket factory when no global WebSocket exists', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  restore()
  const original = globalThis.WebSocket
  globalThis.WebSocket = undefined
  t.after(() => {
    globalThis.WebSocket = original
  })

  const urls = []
  FakeWebSocket.onSend = (socket, message) => {
    queueMicrotask(() => socket.serverMessage({
      jsonrpc: '2.0',
      id: message.id,
      result: '0x1234',
    }))
  }

  const client = new core.Client('local', {
    endpoint: 'ws://node-a',
    webSocketFactory(url) {
      urls.push(url)
      return new FakeWebSocket(url)
    },
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })

  assert.equal(await client.rpc('state_getMetadata'), '0x1234')
  assert.deepEqual(urls, ['ws://node-a'])
})

test('Client passes maxSubscriptionQueue into its transport', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  const genesis = `0x${'12'.repeat(32)}`
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'chain_getBlockHash') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: genesis }))
      return
    }
    if (message.method === 'chain_subscribeNewHeads') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'sub-1' }))
    }
  }
  const client = new core.Client('local', {
    endpoint: 'ws://node-a',
    expectedGenesisHash: genesis,
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
    maxSubscriptionQueue: 1,
  })
  const subscription = await client.transport.subscribe(
    'chain_subscribeNewHeads',
    [],
    'chain_unsubscribeNewHeads',
  )
  const iterator = subscription[Symbol.asyncIterator]()
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1', result: { number: 1 } },
  })
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'chain_subscription',
    params: { subscription: 'sub-1', result: { number: 2 } },
  })

  assert.deepEqual(await iterator.next(), { done: false, value: { number: 1 } })
  await assert.rejects(() => iterator.next(), /subscription notification queue exceeded limit/)
})

test('Client autoConnect exposes a handled readiness promise', async (t) => {
  const originalRuntimeAt = core.Client.prototype.runtimeAt
  t.after(() => {
    core.Client.prototype.runtimeAt = originalRuntimeAt
  })
  core.Client.prototype.runtimeAt = async () => {
    throw new Error('startup failed')
  }

  const client = new core.Client('local', {
    endpoint: 'http://127.0.0.1:9944',
    autoConnect: true,
  })

  assert.ok(client.ready instanceof Promise)
  await assert.rejects(client.ready, /startup failed/)
})

test('Client validates fallback endpoint genesis before use', async (t) => {
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })
  const primaryGenesis = `0x${'aa'.repeat(32)}`
  const fallbackGenesis = `0x${'bb'.repeat(32)}`
  globalThis.fetch = async (url, init) => {
    const request = JSON.parse(String(init.body))
    const endpoint = String(url)
    if (request.method === 'chain_getBlockHash') {
      return {
        ok: true,
        json: async () => ({
          jsonrpc: '2.0',
          id: request.id,
          result: endpoint.includes('fallback') ? fallbackGenesis : primaryGenesis,
        }),
      }
    }
    if (endpoint.includes('primary') && request.method === 'state_getMetadata') {
      throw new Error('primary unavailable')
    }
    return {
      ok: true,
      json: async () => ({ jsonrpc: '2.0', id: request.id, result: '0x00' }),
    }
  }

  const client = new core.Client('local', {
    endpoint: 'http://primary',
    expectedGenesisHash: primaryGenesis,
    fallbackEndpoints: ['http://fallback'],
    maxRequestRetries: 1,
    requestTimeoutMs: 100,
    retryBackoffMs: 0,
    maxRetryBackoffMs: 0,
  })

  await assert.rejects(
    () => client.rpc('state_getMetadata'),
    (error) => error.name === 'EndpointValidationError' &&
      /does not match expected genesis/.test(error.message),
  )
})

test('Client fallback can validate from expected genesis when primary is unavailable', async (t) => {
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })
  const expectedGenesis = `0x${'cc'.repeat(32)}`
  globalThis.fetch = async (url, init) => {
    const endpoint = String(url)
    if (endpoint.includes('primary')) throw new Error('primary unavailable')
    const request = JSON.parse(String(init.body))
    if (request.method === 'chain_getBlockHash') {
      return {
        ok: true,
        json: async () => ({ jsonrpc: '2.0', id: request.id, result: expectedGenesis }),
      }
    }
    return {
      ok: true,
      json: async () => ({ jsonrpc: '2.0', id: request.id, result: '0x1234' }),
    }
  }

  const client = new core.Client('local', {
    endpoint: 'http://primary',
    expectedGenesisHash: expectedGenesis,
    fallbackEndpoints: ['http://fallback'],
    maxRequestRetries: 1,
    requestTimeoutMs: 100,
    retryBackoffMs: 0,
    maxRetryBackoffMs: 0,
  })

  assert.equal(await client.rpc('state_getMetadata'), '0x1234')
})

test('Client rotates past a wrong-genesis primary to a valid fallback', async (t) => {
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })
  const expectedGenesis = `0x${'dd'.repeat(32)}`
  const wrongGenesis = `0x${'ee'.repeat(32)}`
  globalThis.fetch = async (url, init) => {
    const request = JSON.parse(String(init.body))
    const endpoint = String(url)
    if (request.method === 'chain_getBlockHash') {
      return {
        ok: true,
        json: async () => ({
          jsonrpc: '2.0',
          id: request.id,
          result: endpoint.includes('primary') ? wrongGenesis : expectedGenesis,
        }),
      }
    }
    return {
      ok: true,
      json: async () => ({ jsonrpc: '2.0', id: request.id, result: '0x1234' }),
    }
  }

  const client = new core.Client('local', {
    endpoint: 'http://primary',
    expectedGenesisHash: expectedGenesis,
    fallbackEndpoints: ['http://fallback'],
    maxRequestRetries: 1,
    requestTimeoutMs: 100,
    retryBackoffMs: 0,
    maxRetryBackoffMs: 0,
  })

  assert.equal(await client.rpc('state_getMetadata'), '0x1234')
})

test('Client requires expected genesis for custom fallback endpoints', () => {
  assert.throws(
    () => new core.Client('local', {
      endpoint: 'http://primary',
      fallbackEndpoints: ['http://fallback'],
    }),
    /expectedGenesisHash/,
  )
})

test('Client expires head runtime metadata and invalidates it on runtime upgrade', async () => {
  const { client, calls, setHeadVersion } = fakeRuntimeCacheClient({
    headRuntimeTtlMs: 1_000,
  })

  const first = await client.runtimeAt()
  const cached = await client.runtimeAt()
  assert.equal(cached, first)
  assert.equal(calls.version.length, 1)
  assert.deepEqual(calls.metadataAtVersion, [{ versionHex: '0x0f000000', blockHash: null }])
  assert.equal(calls.metadata.length, 0)

  client.headRuntimeCache.expiresAtMs = 0
  const sameVersion = await client.runtimeAt()
  assert.equal(sameVersion, first)
  assert.equal(calls.version.length, 2)
  assert.deepEqual(calls.metadataAtVersion, [{ versionHex: '0x0f000000', blockHash: null }])
  assert.equal(calls.metadata.length, 0)

  setHeadVersion(420, 2)
  client.headRuntimeCache.expiresAtMs = 0
  const upgraded = await client.runtimeAt()
  assert.notEqual(upgraded, first)
  assert.equal(upgraded.specVersion, 420)
  assert.equal(upgraded.transactionVersion, 2)
  assert.equal(calls.version.length, 3)
  assert.deepEqual(calls.metadataAtVersion, [
    { versionHex: '0x0f000000', blockHash: null },
    { versionHex: '0x0f000000', blockHash: null },
  ])
  assert.equal(calls.metadata.length, 0)
})

test('Client falls back to legacy metadata when V15 metadata runtime API is unavailable', async () => {
  const { client, calls } = fakeRuntimeCacheClient({
    metadataAtVersionError: new Error('Execution failed: Exported method Metadata_metadata_at_version is not found'),
  })

  await client.runtimeAt()
  assert.deepEqual(calls.metadataAtVersion, [{ versionHex: '0x0f000000', blockHash: null }])
  assert.deepEqual(calls.metadata, [null])
})

test('Client caches historical runtimes by block hash with LRU eviction', async () => {
  const { client, calls, setBlockVersion } = fakeRuntimeCacheClient({
    historicalRuntimeCacheSize: 2,
  })
  const blockA = `0x${'aa'.repeat(32)}`
  const blockB = `0x${'bb'.repeat(32)}`
  const blockC = `0x${'cc'.repeat(32)}`
  setBlockVersion(blockA, 419, 1)
  setBlockVersion(blockB, 420, 1)
  setBlockVersion(blockC, 421, 1)

  const runtimeA = await client.runtimeAt(blockA)
  assert.equal(await client.runtimeAt(blockA), runtimeA)
  assert.deepEqual(calls.version, [blockA])
  assert.deepEqual(calls.metadataAtVersion, [{ versionHex: '0x0f000000', blockHash: blockA }])
  assert.equal(calls.metadata.length, 0)

  await client.runtimeAt(blockB)
  await client.runtimeAt(blockC)
  assert.equal(client.historicalRuntimeCache.has(blockA), false)
  assert.equal(client.historicalRuntimeCache.has(blockB), true)
  assert.equal(client.historicalRuntimeCache.has(blockC), true)

  assert.equal(await client.runtimeAt(blockA), runtimeA)
  assert.deepEqual(calls.version, [blockA, blockB, blockC, blockA])
  assert.deepEqual(calls.metadataAtVersion, [
    { versionHex: '0x0f000000', blockHash: blockA },
    { versionHex: '0x0f000000', blockHash: blockB },
    { versionHex: '0x0f000000', blockHash: blockC },
  ])
  assert.equal(calls.metadata.length, 0)
})

test('Client queryBatch decodes metadata defaults for missing storage values', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const blockHash = `0x${'33'.repeat(32)}`
  const keys = [Buffer.from([1]), Buffer.from([2])]
  const runtime = {
    storageKeyBatch() {
      return keys
    },
    storageEntry() {
      return {
        pallet: 'Example',
        name: 'Value',
        prefix: 'Example',
        modifier: 'Default',
        valueType: 'u8',
        valueTypeId: 0,
        paramTypes: [],
        paramTypeIds: [],
        paramHashers: [],
        defaultBytes: Buffer.from([9]),
      }
    },
    decode(_type, bytes) {
      return bytes[0]
    },
  }
  client.finalizedHead = async () => blockHash
  client.runtimeAt = async (block) => {
    assert.equal(block, blockHash)
    return runtime
  }
  client.rpc = async (method, params = []) => {
    assert.equal(method, 'state_queryStorageAt')
    assert.deepEqual(params, [['0x01', '0x02'], blockHash])
    return [{ changes: [['0x01', '0x05']] }]
  }

  assert.deepEqual(await client.queryBatch('Example', 'Value', [[], []]), [5, 9])
})

test('Client query and runtimeCall pin default reads to one finalized block', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const blockHash = `0x${'34'.repeat(32)}`
  const calls = []
  const runtime = {
    storageKey() {
      return Buffer.from([3])
    },
    storageEntry() {
      return {
        pallet: 'Example',
        name: 'Value',
        prefix: 'Example',
        modifier: 'Default',
        valueType: 'u8',
        valueTypeId: 0,
        paramTypes: [],
        paramTypeIds: [],
        paramHashers: [],
        defaultBytes: Buffer.from([0]),
      }
    },
    decode(_type, bytes) {
      return bytes[0]
    },
    runtimeApis() {
      return {
        ExampleApi: {
          thing: {
            inputDetails: [],
            outputTypeId: 7,
            outputType: 'u8',
          },
        },
      }
    },
    encodeRuntimeApiInput() {
      return Buffer.from([9, 9])
    },
    decodeTypeId(typeId, bytes) {
      assert.equal(typeId, 7)
      return bytes[0]
    },
  }
  client.finalizedHead = async () => blockHash
  client.runtimeAt = async (block) => {
    assert.equal(block, blockHash)
    return runtime
  }
  client.rpc = async (method, params = []) => {
    calls.push({ method, params })
    if (method === 'state_getStorage') return '0x05'
    if (method === 'state_call') return '0x07'
    throw new Error(`unexpected RPC ${method}`)
  }

  assert.equal(await client.query('Example', 'Value'), 5)
  assert.equal(await client.runtimeCall('ExampleApi', 'thing'), 7)
  assert.deepEqual(calls, [
    { method: 'state_getStorage', params: ['0x03', blockHash] },
    { method: 'state_call', params: ['ExampleApi_thing', '0x0909', blockHash] },
  ])
})

test('Client queryMap pins reads and rejects pagination without progress', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const blockHash = `0x${'44'.repeat(32)}`
  const calls = []
  const runtime = {
    storageKey() {
      return Buffer.from([0])
    },
    storageEntry() {
      return { valueType: 'u8' }
    },
    decodeStorageKeyParams(_pallet, _name, key) {
      return [key[0]]
    },
    decode(_type, bytes) {
      return bytes[0]
    },
  }
  client.finalizedHead = async () => blockHash
  client.runtimeAt = async (block) => {
    assert.equal(block, blockHash)
    return runtime
  }
  client.transport.request = async (method, params = []) => {
    calls.push({ method, params })
    if (method === 'state_getKeysPaged') return ['0x01']
    if (method === 'state_queryStorageAt') return [{ changes: [['0x01', '0x05']] }]
    throw new Error(`unexpected ${method}`)
  }

  await assert.rejects(() => client.queryMap('Example', 'Map'), /pagination did not advance/)
  assert.ok(calls.every((call) => call.params.at(-1) === blockHash))
})

test('Client signs extrinsics with extension-style signRaw signers', async () => {
  const callData = Buffer.from([5, 6, 7])
  const { runtime, captures } = fakeSigningRuntime()
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 6)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 9),
  ])
  let request
  const signer = {
    address,
    publicKey,
    signRaw(req) {
      request = req
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const signed = await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true })

  assert.equal(signed.signerAddress, address)
  assert.equal(signed.nonce, 12)
  assert.equal(client.lastNonceAddress, address)
  assert.equal(request.address, address)
  assert.equal(request.type, 'bytes')
  assert.equal(request.data, '0x090909')
  assert.equal(request.metadataProof, undefined)
  assert.deepEqual(captures.encoded.callData, callData)
  assert.deepEqual(captures.encoded.publicKey, publicKey)
  assert.deepEqual(captures.encoded.signature, Buffer.alloc(64, 9))
  assert.equal(captures.encoded.signatureVersion, core.CRYPTO_SR25519)
  assert.equal(captures.encoded.params.metadataHashEnabled, false)
  assert.equal(await client.accountNextIndex(address), 12)
  assert.equal(await client.accountNextIndex(address), 12)
  await assert.rejects(
    () => client.signExtrinsic(callData, signer, { period: null, allowRawCall: true, tip: core.Balance.fromAlpha('1', 7) }),
    /tip must be a TAO balance/,
  )
  await assert.rejects(
    () => client.signExtrinsic(callData, signer, { period: null, allowRawCall: true, tipAssetId: core.taoAmount('1') }),
    /tipAssetId must be a bigint/,
  )
  await assert.rejects(
    () => client.signExtrinsic(callData, signer, { period: null, allowRawCall: true, tipAssetId: -1 }),
    /tipAssetId must be non-negative/,
  )
  await assert.rejects(
    () => client.signExtrinsic(callData, { ...signer, publicKey: Buffer.alloc(32, 7) }, { period: null, allowRawCall: true }),
    /publicKey does not match/,
  )
})

test('Client rejects raw call shapes unless callers opt in', async () => {
  const { runtime } = fakeSigningRuntime()
  const callData = Buffer.from([5, 6, 7])
  const client = fakeSigningClient(runtime, callData)
  const signer = core.Keypair.fromUri('//Alice')

  await assert.rejects(
    () => client.signExtrinsic(callData, signer, { period: null }),
    /opaque call bytes require explicit raw-call permission/,
  )
  await assert.rejects(
    () => client.signExtrinsic(['System', 'remark', { remark: Buffer.from('hello') }], signer, { period: null }),
    /raw metadata calls require explicit raw-call permission/,
  )
  await assert.rejects(
    () => client.signExtrinsic(callData, signer, {
      period: null,
      policy: new core.Policy({ allowRawCalls: true, maxSpendRao: 1n }),
    }),
    /opaque call bytes cannot prove fee, spend, or subnet policy/,
  )
})

test('Client accepts raw metadata calls only with explicit raw permission', async () => {
  const { runtime, captures } = fakeSigningRuntime()
  const client = fakeSigningClient(runtime, Buffer.alloc(0))
  const signer = core.Keypair.fromUri('//Alice')

  const signed = await client.signExtrinsic(
    ['System', 'remark', { remark: Buffer.from('hello') }],
    signer,
    { period: null, allowRawCall: true },
  )

  assert.ok(signed.bytes.length > 0)
  assert.equal(captures.composeCall.pallet, 'System')
  assert.equal(captures.composeCall.fn, 'remark')
  assert.deepEqual(captures.composeCall.params.remark, Buffer.from('hello'))
})

test('Client callData composes trusted Rust intent calls', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const captures = {}
  client.composeCall = async (pallet, fn, params, block) => {
    captures.composeCall = { pallet, fn, params, block }
    return Buffer.from([1, 2, 3])
  }

  const bytes = await client.callData(
    core.IntentCall.transfer('5F3sa2TJAWMqDhXG6jhV4N8ko9SxwGy8TpaNS1repo5EYjQX', 7n),
    99,
  )

  assert.deepEqual(bytes, Buffer.from([1, 2, 3]))
  assert.equal(captures.composeCall.pallet, 'Balances')
  assert.equal(captures.composeCall.fn, 'transfer_keep_alive')
  assert.equal(captures.composeCall.params.dest, '5F3sa2TJAWMqDhXG6jhV4N8ko9SxwGy8TpaNS1repo5EYjQX')
  assert.equal(captures.composeCall.params.value, 7)
  assert.equal(captures.composeCall.block, 99)
})

test('Client enables metadata hash by default for software signers when supported', async () => {
  const callData = Buffer.from([5, 6, 7])
  const metadataBytes = goldenMetadataBytes()
  const { runtime, captures } = fakeSigningRuntime({
    metadataBytes,
    signedExtensionIdentifiers() {
      return ['CheckNonce', 'CheckMetadataHash']
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 6)
  const address = core.ss58FromPublic(publicKey, 42)
  let request
  const signer = {
    address,
    publicKey,
    signRaw(req) {
      request = req
      return { signature: `0x${Buffer.alloc(64, 9).toString('hex')}` }
    },
  }

  await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true })

  const expectedMetadataHash = core.metadataDigest(metadataBytes, {
    specVersion: runtime.specVersion,
    specName: 'node-subtensor',
    base58Prefix: 42,
    decimals: 9,
    tokenSymbol: 'TAO',
  })
  assert.equal(captures.encoded.params.metadataHashEnabled, true)
  assert.deepEqual(captures.payloadParams.metadataHash, expectedMetadataHash)
  assert.equal(request.metadataHash, `0x${expectedMetadataHash.toString('hex')}`)
  await assert.rejects(
    () => client.signExtrinsic(callData, signer, { period: null, allowRawCall: true, metadataHash: null }),
    /metadataHash cannot be disabled/,
  )

  const explicitMetadataHash = Buffer.alloc(32, 3)
  await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true, metadataHash: explicitMetadataHash })
  assert.equal(captures.encoded.params.metadataHashEnabled, true)
  assert.deepEqual(captures.payloadParams.metadataHash, explicitMetadataHash)
  assert.equal(request.metadataHash, `0x${explicitMetadataHash.toString('hex')}`)
})

test('Client passes structured payloads to extension signPayload signers', async () => {
  const callData = Buffer.from([5, 6, 7])
  const { runtime, captures } = fakeSigningRuntime({
    extrinsicVersion: 5,
    signedExtensionIdentifiers() {
      return ['CheckNonce', 'ChargeAssetTxPayment', 'CheckMetadataHash']
    },
    signerPayload(address, callData, params) {
      captures.signerPayload = { address, callData: Buffer.from(callData), params }
      return {
        address,
        blockHash: '0x4242424242424242424242424242424242424242424242424242424242424242',
        blockNumber: '0x2a000000',
        era: '0x2500',
        genesisHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
        method: `0x${Buffer.from(callData).toString('hex')}`,
        nonce: '0x3000',
        signedExtensions: ['CheckNonce', 'ChargeAssetTxPayment', 'CheckMetadataHash'],
        specVersion: '0xa3010000',
        tip: '0x0400',
        transactionVersion: '0x01000000',
        version: 5,
        assetId: '0x010000',
        metadataHash: '0x0303030303030303030303030303030303030303030303030303030303030303',
        mode: 1,
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 6)
  const address = core.ss58FromPublic(publicKey, 42)
  let payload
  let rawCalls = 0
  const signer = {
    address,
    publicKey,
    signPayload(value) {
      payload = value
      return { signature: `0x${Buffer.alloc(64, 9).toString('hex')}` }
    },
    signRaw() {
      rawCalls += 1
      throw new Error('signRaw should not be used when signPayload exists')
    },
  }

  await client.signExtrinsic(callData, signer, {
    allowRawCall: true,
    period: 64,
    tip: core.raoAmount(1),
    tipAssetId: 0n,
    metadataHash: Buffer.alloc(32, 3),
  })

  assert.equal(payload.address, address)
  assert.equal(payload.method, '0x050607')
  assert.equal(payload.version, 5)
  assert.equal(payload.assetId, '0x010000')
  assert.equal(payload.nonce, '0x3000')
  assert.equal(payload.tip, '0x0400')
  assert.equal(payload.mode, 1)
  assert.deepEqual(payload.signedExtensions, ['CheckNonce', 'ChargeAssetTxPayment', 'CheckMetadataHash'])
  assert.equal(rawCalls, 0)
  assert.deepEqual(captures.signerPayload.callData, callData)
  assert.equal(captures.signerPayload.params.tipAssetId, 0n)
  assert.equal(captures.encoded.params.metadataHashEnabled, true)
})

test('Client rejects invalid chain nonce values', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  client.rpc = async () => 'NaN'
  await assert.rejects(() => client.accountNextIndex('5F'), /invalid nonce/)
})

test('Client rejects mismatched submit hashes and keeps local hash authoritative', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  client.signedExtrinsicNonceTracking = async () => ({})
  client.transport.request = async (method) => {
    assert.equal(method, 'author_submitExtrinsic')
    return `0x${'00'.repeat(32)}`
  }
  await assert.rejects(() => client.submitSigned(Buffer.from([1, 2])), /returned hash/)
})

test('Client reconciles managed nonce before rejecting mismatched submit hash', async () => {
  const callData = Buffer.from([7, 5, 3])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 22)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 4),
  ])
  const nonceReads = []
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      nonceReads.push(params[0])
      return 30
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return `0x${'ff'.repeat(32)}`
    if (method === 'author_pendingExtrinsics') return []
    if (method === 'chain_getHeader') return { number: '0x0' }
    if (method === 'chain_getBlockHash') return `0x${'02'.repeat(32)}`
    if (method === 'chain_getBlock') return { block: { extrinsics: [] } }
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await assert.rejects(
    () => client.submit(callData, signer, { period: null, allowRawCall: true }),
    /returned hash/,
  )
  await assert.rejects(
    () => client.submit(callData, signer, { period: null, allowRawCall: true }),
    /nonce 30 .* ambiguous/,
  )
  assert.deepEqual(capturedNonces, [30])
  assert.deepEqual(nonceReads, [address, address, address])
})

test('Client estimateFee peeks the chain nonce without reserving it', async () => {
  const callData = Buffer.from([9, 8, 7])
  const { runtime, captures } = fakeSigningRuntime({
    runtimeApis() {
      return {
        TransactionPaymentApi: {
          query_info: {
            inputDetails: [
              { name: 'uxt', typeId: 10, type: 'Extrinsic' },
              { name: 'len', typeId: 11, type: 'u32' },
            ],
            outputTypeId: 1,
          },
        },
      }
    },
    encodeRuntimeApiInput(api, method, params) {
      assert.equal(api, 'TransactionPaymentApi')
      assert.equal(method, 'query_info')
      assert.equal(params.length, 2)
      const [uxt, len] = params
      const encodedLen = Buffer.alloc(4)
      encodedLen.writeUInt32LE(len, 0)
      return Buffer.concat([Buffer.from(uxt), encodedLen])
    },
    decodeTypeId() {
      return { partial_fee: 123n }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 8)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 4),
  ])
  const nonceReads = []
  let stateCallParams
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      nonceReads.push(params[0])
      return 7
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'state_call') {
      stateCallParams = params
      return '0x00'
    }
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const fee = await client.estimateFee(callData, signer, { allowRawCall: true })
  const firstRealNonce = await client.accountNextIndex(address)
  const secondRealNonce = await client.accountNextIndex(address)

  assert.equal(fee.rao, 123n)
  assert.equal(captures.payloadParams.nonce, 7)
  assert.equal(firstRealNonce, 7)
  assert.equal(secondRealNonce, 7)
  assert.deepEqual(nonceReads, [address, address, address])
  assert.equal(stateCallParams[0], 'TransactionPaymentApi_query_info')
  assert.equal(stateCallParams[2], `0x${'42'.repeat(32)}`)
})

test('Client serializes concurrent initial nonce reservations during submit', async () => {
  const callData = Buffer.from([8, 8, 8])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 11)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 5),
  ])
  let reads = 0
  let releaseRead
  const readGate = new Promise((resolve) => {
    releaseRead = resolve
  })
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      assert.equal(params[0], address)
      reads += 1
      await readGate
      return 14
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const first = client.submit(callData, signer, { period: null, allowRawCall: true })
  const second = client.submit(callData, signer, { period: null, allowRawCall: true })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(reads, 1)
  releaseRead()

  await Promise.all([first, second])
  assert.equal(reads, 1)
  assert.deepEqual(capturedNonces.sort((a, b) => a - b), [14, 15])
})

test('Client releases only the failed reserved nonce', async () => {
  const callData = Buffer.from([3, 2, 1])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 12)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 2),
  ])
  let releaseSign
  let signStarted
  const signGate = new Promise((resolve) => {
    releaseSign = resolve
  })
  const started = new Promise((resolve) => {
    signStarted = resolve
  })
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') return 12
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const failingSigner = {
    address,
    publicKey,
    async signRaw() {
      signStarted()
      await signGate
      throw new Error('signer declined')
    },
  }
  const succeedingSigner = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const signing = client.submit(callData, failingSigner, { period: null, allowRawCall: true })
  await started
  const independentlySubmitted = await client.submit(callData, succeedingSigner, { period: null, allowRawCall: true })
  releaseSign()
  await assert.rejects(signing, /signer declined/)
  await independentlySubmitted
  await client.submit(callData, succeedingSigner, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [12, 13, 12])
})

test('Client quarantines an ambiguous submit nonce even when a fallback node reports it absent', async () => {
  const callData = Buffer.from([4, 5, 6])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 13)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 8),
  ])
  const nonceReads = []
  let submitAttempts = 0
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      nonceReads.push(params[0])
      return 20
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') {
      submitAttempts += 1
      if (submitAttempts === 1) throw new core.JsonRpcError('lost response')
      return submittedExtrinsicHash(params[0])
    }
    if (method === 'author_pendingExtrinsics') return []
    if (method === 'chain_getHeader') return { number: '0x0' }
    if (method === 'chain_getBlockHash') return `0x${'01'.repeat(32)}`
    if (method === 'chain_getBlock') return { block: { extrinsics: [] } }
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await assert.rejects(() => client.submit(callData, signer, { period: null, allowRawCall: true }), /lost response/)
  await assert.rejects(
    () => client.submit(callData, signer, { period: null, allowRawCall: true }),
    /nonce 20 .* ambiguous/,
  )
  assert.deepEqual(capturedNonces, [20])
  assert.deepEqual(nonceReads, [address, address, address])
})

test('Client invalidates nonce state after unknown ambiguous submission reconciliation', async () => {
  const callData = Buffer.from([4, 5, 8])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 16)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 9),
  ])
  let submitAttempts = 0
  let networkRestored = false
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      if (submitAttempts === 0) return 50
      if (!networkRestored) throw new Error('network still unavailable')
      return 55
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') {
      submitAttempts += 1
      if (submitAttempts === 1) {
        throw new core.JsonRpcError('lost response')
      }
      return submittedExtrinsicHash(params[0])
    }
    if (method === 'author_pendingExtrinsics') throw new Error('network still unavailable')
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await assert.rejects(() => client.submit(callData, signer, { period: null, allowRawCall: true }), /lost response/)
  networkRestored = true
  await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [50, 55])
})

test('Client protects an ambiguous submit nonce when the extrinsic is still pending', async () => {
  const callData = Buffer.from([4, 5, 7])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 15)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 3),
  ])
  const nonceReads = []
  let submitAttempts = 0
  let submittedHex
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      nonceReads.push(params[0])
      return 40
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') {
      submitAttempts += 1
      submittedHex = params[0]
      if (submitAttempts === 1) throw new core.JsonRpcError('lost response')
      return submittedExtrinsicHash(params[0])
    }
    if (method === 'author_pendingExtrinsics') return [submittedHex]
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await assert.rejects(() => client.submit(callData, signer, { period: null, allowRawCall: true }), /lost response/)
  await client.submit(callData, signer, { period: null, allowRawCall: true })
  assert.deepEqual(capturedNonces, [40, 41])
  assert.deepEqual(nonceReads, [address, address])
})

test('Client submit without inclusion reports pool submission, not execution success', async () => {
  const callData = Buffer.from([7, 7, 7])
  const { runtime } = fakeSigningRuntime()
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 14)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 6),
  ])
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') return 30
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const result = await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.equal(result.status, 'submitted')
  assert.equal(result.success, undefined)
  assert.equal(result.message, 'Submitted')
  assert.equal(client.nonceAccounts.get(address).statuses.size, 0)
})

test('Client treats string and object fatal watch statuses as failures', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const subscriptionFor = (status) => ({
    async *[Symbol.asyncIterator]() {
      yield status
    },
    async unsubscribe() {},
  })

  await assert.rejects(
    () => client.resolveWatchedExtrinsic(subscriptionFor('dropped'), `0x${'01'.repeat(32)}`, false),
    /Extrinsic dropped/,
  )
  await assert.rejects(
    () => client.resolveWatchedExtrinsic(subscriptionFor({ invalid: '0xdead' }), `0x${'02'.repeat(32)}`, false),
    /Extrinsic invalid/,
  )
})

test('Client continues watching after retracted extrinsic status', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const blockHash = `0x${'33'.repeat(32)}`
  const extrinsicHash = `0x${'44'.repeat(32)}`
  const subscription = {
    async *[Symbol.asyncIterator]() {
      yield { retracted: `0x${'22'.repeat(32)}` }
      yield { finalized: blockHash }
    },
    async unsubscribe() {},
  }
  client.resolveInclusion = async (hash, block, finalized) => ({
    status: 'finalized',
    success: true,
    message: 'Success',
    extrinsicHash: hash,
    blockHash: block,
    finalized,
    events: [],
  })

  const result = await client.resolveWatchedExtrinsic(subscription, extrinsicHash, true)

  assert.equal(result.status, 'finalized')
  assert.equal(result.extrinsicHash, extrinsicHash)
  assert.equal(result.blockHash, blockHash)
  assert.equal(result.finalized, true)
})

test('Client reports included extrinsics with missing dispatch outcome as unknown', async () => {
  const client = new core.Client('local', { endpoint: 'http://127.0.0.1:9944' })
  const extrinsic = '0x0102'
  const extrinsicHash = `0x${core.blake2_256(Buffer.from([1, 2])).toString('hex')}`
  client.rpc = async (method) => {
    if (method === 'chain_getBlock') {
      return { block: { header: { number: '0x2a' }, extrinsics: [extrinsic] } }
    }
    throw new Error(`unexpected RPC ${method}`)
  }
  client.query = async () => []

  const result = await client.resolveInclusion(extrinsicHash, `0x${'12'.repeat(32)}`, false)

  assert.equal(result.status, 'unknown')
  assert.equal(result.success, undefined)
  assert.equal(result.message, 'Included, but no dispatch outcome event was found')
  assert.equal(result.extrinsicIndex, 0)
  assert.equal(result.extrinsicId, '42-0000')
})

test('Client records detached submitSigned nonces before the next managed submit', async () => {
  const callData = Buffer.from([2, 4, 6])
  const capturedNonces = []
  const submitMaxRetries = []
  const submitRetryForever = []
  const publicKey = Buffer.alloc(32, 18)
  const address = core.ss58FromPublic(publicKey, 42)
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
    decodeExtrinsic(data) {
      return { address, nonce: data[1] }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 4),
  ])
  let nonceReads = 0
  client.rpc = async (method, params = [], options = {}) => {
    if (method === 'system_accountNextIndex') {
      assert.equal(params[0], address)
      nonceReads += 1
      return nonceReads === 1 ? 70 : 71
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') {
      submitMaxRetries.push(options.maxRetries)
      submitRetryForever.push(options.retryForever)
      return submittedExtrinsicHash(params[0])
    }
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await client.submit(callData, signer, { period: null, allowRawCall: true })
  const detached = await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true })
  assert.equal(detached.nonce, 71)
  await client.submitSigned(detached)
  await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [70, 71, 72])
  assert.deepEqual(submitMaxRetries, [0, 0, 0])
  assert.deepEqual(submitRetryForever, [false, false, false])
  assert.equal(nonceReads, 2)
})

test('Client decodes detached signed nonce instead of trusting mutable public fields', async () => {
  const callData = Buffer.from([2, 4, 5])
  const capturedNonces = []
  const publicKey = Buffer.alloc(32, 20)
  const address = core.ss58FromPublic(publicKey, 42)
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
    decodeExtrinsic(data) {
      return { address, nonce: data[1] }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 6),
  ])
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      assert.equal(params[0], address)
      return 70
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const detached = await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true })
  detached.signerAddress = core.ss58FromPublic(Buffer.alloc(32, 99), 42)
  detached.nonce = 999
  await client.submitSigned(detached)
  await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [70, 71])
})

test('Client invalidates nonce state for opaque externally signed submissions', async () => {
  const callData = Buffer.from([2, 4, 7])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  const publicKey = Buffer.alloc(32, 21)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 2),
  ])
  let nonceReads = 0
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      assert.equal(params[0], address)
      nonceReads += 1
      return nonceReads === 1 ? 90 : 100
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await client.submit(callData, signer, { period: null, allowRawCall: true })
  await client.submitSigned(Buffer.from([9, 9, 9]), address)
  await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [90, 100])
  assert.equal(nonceReads, 2)
})

test('Client records detached watchSigned nonces before the next managed submit', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'author_submitAndWatchExtrinsic') {
      assert.equal(message.params.length, 1)
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'watch-detached' }))
      return
    }
    if (message.method === 'author_unwatchExtrinsic') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: true }))
      return
    }
    if (message.method === 'author_submitExtrinsic') {
      queueMicrotask(() => socket.serverMessage({
        jsonrpc: '2.0',
        id: message.id,
        result: submittedExtrinsicHash(message.params[0]),
      }))
    }
  }

  const callData = Buffer.from([2, 4, 8])
  const capturedNonces = []
  const publicKey = Buffer.alloc(32, 19)
  const address = core.ss58FromPublic(publicKey, 42)
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
    decodeExtrinsic(data) {
      return { address, nonce: data[1] }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  client.transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  client.resolveInclusion = async (extrinsicHash, blockHash) => ({
    status: 'inBlock',
    success: true,
    message: 'Success',
    extrinsicHash,
    blockHash,
    events: [],
  })
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 5),
  ])
  let nonceReads = 0
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') {
      assert.equal(params[0], address)
      nonceReads += 1
      return 80
    }
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  const detached = await client.signExtrinsic(callData, signer, { period: null, allowRawCall: true })
  const watcher = await client.watchSigned(detached, { timeoutMs: 100 })
  await waitFor(
    () => FakeWebSocket.sockets[0].sent.some((message) => message.method === 'author_submitAndWatchExtrinsic'),
    'detached submit-and-watch request',
  )
  FakeWebSocket.sockets[0].serverMessage({
    jsonrpc: '2.0',
    method: 'author_extrinsicUpdate',
    params: { subscription: 'watch-detached', result: { inBlock: `0x${'44'.repeat(32)}` } },
  })
  await watcher.result
  await client.submit(callData, signer, { period: null, allowRawCall: true })

  assert.deepEqual(capturedNonces, [80, 81])
  assert.equal(nonceReads, 2)
})

test('Client submission watches support timeout and reconcile managed nonces', async (t) => {
  const { FakeWebSocket, restore } = installFakeWebSocket()
  t.after(restore)
  FakeWebSocket.onSend = (socket, message) => {
    if (message.method === 'author_submitAndWatchExtrinsic') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: 'watch-1' }))
      return
    }
    if (message.method === 'author_unwatchExtrinsic') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: true }))
      return
    }
    if (message.method === 'author_submitExtrinsic') {
      queueMicrotask(() => socket.serverMessage({ jsonrpc: '2.0', id: message.id, result: `0x${'cc'.repeat(32)}` }))
    }
  }

  const callData = Buffer.from([6, 6, 6])
  const capturedNonces = []
  const { runtime } = fakeSigningRuntime({
    signaturePayload(_callData, params) {
      capturedNonces.push(params.nonce)
      return Buffer.from([Number(params.nonce)])
    },
    encodeSignedExtrinsic(_callData, _publicKey, _signature, signatureVersion, params) {
      return {
        bytes: Buffer.from([signatureVersion, Number(params.nonce)]),
        hash: Buffer.alloc(32, Number(params.nonce)),
      }
    },
  })
  const client = fakeSigningClient(runtime, callData)
  client.transport = new core.JsonRpcTransport('ws://node-a', [], false, {
    requestTimeoutMs: 100,
    maxRequestRetries: 0,
  })
  const publicKey = Buffer.alloc(32, 17)
  const address = core.ss58FromPublic(publicKey, 42)
  const typedSignature = Buffer.concat([
    Buffer.from([core.CRYPTO_SR25519]),
    Buffer.alloc(64, 1),
  ])
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') return 60
    if (method === 'state_getRuntimeVersion') {
      return {
        specName: 'node-subtensor',
        specVersion: runtime.specVersion,
        transactionVersion: runtime.transactionVersion,
      }
    }
    if (method === 'system_properties') {
      return { ss58Format: 42, tokenDecimals: [9], tokenSymbol: ['TAO'] }
    }
    if (method === 'author_pendingExtrinsics') return []
    if (method === 'chain_getHeader') return { number: '0x0' }
    if (method === 'chain_getBlockHash') return `0x${'02'.repeat(32)}`
    if (method === 'chain_getBlock') return { block: { extrinsics: [] } }
    if (method === 'author_submitExtrinsic') return submittedExtrinsicHash(params[0])
    throw new Error(`unexpected RPC ${method}`)
  }
  const signer = {
    address,
    publicKey,
    signRaw() {
      return { signature: `0x${typedSignature.toString('hex')}` }
    },
  }

  await assert.rejects(
    () => client.submit(callData, signer, { period: null, allowRawCall: true, waitForInclusion: true, timeoutMs: 5 }),
    (error) => error.name === 'RequestTimeoutError',
  )
  await assert.rejects(
    () => client.submit(callData, signer, { period: null, allowRawCall: true }),
    /nonce 60 .* ambiguous/,
  )
  assert.deepEqual(capturedNonces, [60])
  assert.equal(
    FakeWebSocket.sockets[0].sent.some((message) => message.method === 'author_unwatchExtrinsic'),
    true,
  )
})

test('Client generates RFC-0078 proof for Ledger metadata-verifying signers', async () => {
  const vector = ledgerProofVector()
  const metadataBytes = goldenMetadataBytes()
  const captures = {}
  const { runtime } = fakeSigningRuntime({
    specVersion: vector.specVersion,
    metadataBytes,
    signaturePayloadParts(params) {
      captures.partsParams = params
      return {
        includedInExtrinsic: vector.includedInExtrinsic,
        includedInSignedData: vector.includedInSignedData,
      }
    },
    signaturePayload(_callData, params) {
      captures.payloadParams = params
      return Buffer.from([9, 9, 9])
    },
    encodeSignedExtrinsic(callData, publicKey, signature, signatureVersion, params) {
      captures.encoded = { callData, publicKey, signature, signatureVersion, params }
      return {
        bytes: Buffer.concat([Buffer.from([signatureVersion]), signature.subarray(0, 2)]),
        hash: Buffer.alloc(32, 7),
      }
    },
  })
  const client = fakeSigningClient(runtime, vector.callData)
  const publicKey = Buffer.alloc(32, 5)
  const address = core.ss58FromPublic(publicKey, 42)
  const ledgerCalls = {}
  const fakeDevice = {
    address(account, index, ss58Prefix, confirm) {
      ledgerCalls.address = { account, index, ss58Prefix, confirm }
      return { publicKey, ss58Address: address }
    },
    sign(account, index, payload, proof) {
      ledgerCalls.sign = {
        account,
        index,
        payload: Buffer.from(payload),
        proof: Buffer.from(proof),
      }
      return Buffer.concat([Buffer.from([core.CRYPTO_ED25519]), Buffer.alloc(64, 7)])
    },
  }
  const signer = new core.LedgerSigner(fakeDevice, {
    account: 1,
    index: 2,
    confirmAddress: true,
  })
  const chainInfo = {
    specVersion: vector.specVersion,
    specName: 'node-subtensor',
    base58Prefix: 42,
    decimals: 9,
    tokenSymbol: 'TAO',
  }

  const signed = await client.signExtrinsic(vector.callData, signer, { period: null, allowRawCall: true })
  const expectedMetadataHash = core.metadataDigest(metadataBytes, chainInfo)
  const expectedProof = core.generateExtrinsicProof(
    vector.callData,
    vector.includedInExtrinsic,
    vector.includedInSignedData,
    metadataBytes,
    chainInfo,
  )

  assert.equal(signed.signerAddress, address)
  assert.deepEqual(ledgerCalls.address, {
    account: 1,
    index: 2,
    ss58Prefix: 42,
    confirm: true,
  })
  assert.deepEqual(captures.partsParams.metadataHash, expectedMetadataHash)
  assert.deepEqual(ledgerCalls.sign.payload, Buffer.from([9, 9, 9]))
  assert.deepEqual(ledgerCalls.sign.proof, expectedProof)
  assert.deepEqual(captures.encoded.callData, vector.callData)
  assert.deepEqual(captures.encoded.signature, Buffer.alloc(64, 7))
  assert.equal(captures.encoded.signatureVersion, core.CRYPTO_ED25519)
  assert.equal(captures.encoded.params.metadataHashEnabled, true)
})

test('wallet helpers keep mnemonic and keyfile passwords separate', async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-wallet-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const keyfilePassword = 'review-keyfile-password'
  const mnemonicPassword = 'review-mnemonic-password'

  const created = new core.Wallet({ name: 'created', hotkey: 'default', path: root })
  const createdHotkey = await created.createNewHotkey({ keyfilePassword })
  assert.equal(createdHotkey.wallet, created)
  assert.equal(createdHotkey.mnemonic.split(/\s+/).length, 12)
  assert.deepEqual(createdHotkey.keypair.publicKey, (await created.getHotkey(keyfilePassword)).publicKey)
  assert.equal(
    core.keyfileDataIsEncrypted(fs.readFileSync(created.hotkeyFile.path)),
    true,
  )
  const generated = core.Wallet.generateHotkey()
  assert.equal(generated.mnemonic.split(/\s+/).length, 12)
  const generatedWallet = new core.Wallet({ name: 'generated', hotkey: 'default', path: root })
  await generatedWallet.setHotkey(generated.keypair, { keyfilePassword })
  assert.deepEqual(
    (await generatedWallet.getHotkey(keyfilePassword)).publicKey,
    generated.keypair.publicKey,
  )
  await assert.rejects(
    () => generatedWallet.setHotkey(core.Keypair.fromUri('//Bob')),
    /requires keyfilePassword or allowPlaintext/,
  )
  const plaintextDefault = new core.Wallet({ name: 'plaintext-default', hotkey: 'default', path: root })
  await assert.rejects(
    () => plaintextDefault.createNewHotkey(),
    /keyfilePassword or allowPlaintext/,
  )
  const plaintextAllowed = new core.Wallet({ name: 'plaintext-allowed', hotkey: 'default', path: root })
  const plaintextHotkey = await plaintextAllowed.createNewHotkey({ allowPlaintext: true })
  assert.equal(plaintextHotkey.mnemonic.split(/\s+/).length, 12)
  assert.equal(
    core.keyfileDataIsEncrypted(fs.readFileSync(plaintextAllowed.hotkeyFile.path)),
    false,
  )

  const mnemonic =
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
  const regenerated = new core.Wallet({ name: 'regenerated', hotkey: 'default', path: root })
  await regenerated.regenerateHotkey(mnemonic, { mnemonicPassword, keyfilePassword })
  assert.equal(
    core.keyfileDataIsEncrypted(fs.readFileSync(regenerated.hotkeyFile.path)),
    true,
  )
  assert.deepEqual(
    (await regenerated.getHotkey(keyfilePassword)).publicKey,
    core.Keypair.fromMnemonic(mnemonic, core.CRYPTO_SR25519, mnemonicPassword).publicKey,
  )
  assert.notDeepEqual(
    (await regenerated.getHotkey(keyfilePassword)).publicKey,
    core.Keypair.fromMnemonic(mnemonic, core.CRYPTO_SR25519).publicKey,
  )

  const cold = new core.Wallet({ name: 'regenerated-cold', hotkey: 'default', path: root })
  await cold.regenerateColdkey(mnemonic, { mnemonicPassword, keyfilePassword })
  assert.equal(
    core.keyfileDataIsEncrypted(fs.readFileSync(cold.coldkeyFile.path)),
    true,
  )
  assert.deepEqual(
    (await cold.getColdkey(keyfilePassword)).publicKey,
    core.Keypair.fromMnemonic(mnemonic, core.CRYPTO_SR25519, mnemonicPassword).publicKey,
  )
})

test('wallet names and hotkeys cannot escape the wallet root', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-wallet-paths-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))

  for (const name of ['', '.', '..', '../escape', 'nested/name', 'nested\\name', '/absolute']) {
    assert.throws(
      () => new core.Wallet({ name, hotkey: 'default', path: root }),
      /single path component/,
    )
  }
  for (const hotkey of ['', '.', '..', '../escape', 'nested/hotkey', 'nested\\hotkey', '/absolute']) {
    assert.throws(
      () => new core.Wallet({ name: 'default', hotkey, path: root }),
      /single path component/,
    )
  }

  const wallet = new core.Wallet({ name: 'contained', hotkey: 'hk', path: root })
  const resolvedRoot = path.resolve(root)
  assert.equal(wallet.path, resolvedRoot)
  assert.equal(
    path.relative(resolvedRoot, wallet.hotkeyFile.path).startsWith('..'),
    false,
  )
})

test('wallet keyfile writes are restrictive and reject symlink targets', async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-wallet-atomic-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const keypair = core.Keypair.fromUri('//Alice')
  const wallet = new core.Wallet({ name: 'atomic', hotkey: 'default', path: root })

  await wallet.setHotkey(keypair, { allowPlaintext: true })

  const hotkeyPath = wallet.hotkeyFile.path
  const hotkeyDir = path.dirname(hotkeyPath)
  assert.deepEqual((await wallet.hotkeyFile.getKeypair()).publicKey, keypair.publicKey)
  assert.deepEqual((await core.readKeypairKeyfile(hotkeyPath)).publicKey, keypair.publicKey)
  assert.equal(fs.lstatSync(hotkeyPath).isFile(), true)
  assert.equal(fs.statSync(hotkeyPath).mode & 0o777, 0o600)
  assert.equal(fs.statSync(hotkeyDir).mode & 0o777, 0o700)
  assert.deepEqual(
    fs.readdirSync(hotkeyDir).filter((name) => name.endsWith('.tmp')),
    [],
  )

  const target = path.join(root, 'outside-target')
  fs.writeFileSync(target, 'do not replace')
  const bob = core.Keypair.fromUri('//Bob')
  fs.rmSync(wallet.hotkeypubFile.path)
  fs.symlinkSync(target, wallet.hotkeypubFile.path)
  await assert.rejects(
    () => wallet.setHotkey(bob, { overwrite: true, allowPlaintext: true }),
    /symlink/,
  )
  assert.deepEqual((await wallet.hotkey).publicKey, keypair.publicKey)
  assert.deepEqual((await wallet.hotkeypub).publicKey, keypair.publicKey)
  fs.rmSync(wallet.hotkeypubFile.path)

  const linkedWallet = new core.Wallet({ name: 'atomic', hotkey: 'linked', path: root })
  fs.symlinkSync(target, linkedWallet.hotkeyFile.path)

  await assert.rejects(
    () => linkedWallet.setHotkey(keypair, { overwrite: true, allowPlaintext: true }),
    /symlink/,
  )
  await assert.rejects(
    () => core.readKeypairKeyfile(linkedWallet.hotkeyFile.path),
    /symlink/,
  )
  assert.equal(fs.readFileSync(target, 'utf8'), 'do not replace')
})

test('arbitrary StorageInfo helpers call Rust directly', () => {
  const entry = {
    pallet: 'System',
    name: 'Account',
    prefix: 'System',
    modifier: 'Default',
    valueType: 'scale_info::0',
    valueTypeId: 0,
    paramTypes: [],
    paramTypeIds: [],
    paramHashers: [],
    defaultBytes: Buffer.alloc(0),
  }
  assert.equal(
    core.storagePrefixFor(entry).toString('hex'),
    '26aa394eea5630e07c48ae0c9558cef7b99d880ec681799c0cf30e8886371da9',
  )
})

test('prototype-sensitive decoded keys never become object prototypes', () => {
  const dangerous = new Map([
    ['__proto__', { polluted: true }],
    // Safe integers intentionally normalize to JavaScript numbers at the Rust boundary.
    ['constructor', 7],
  ])
  const output = core.wireRoundtrip(dangerous)
  assert.ok(output instanceof Map)
  assert.equal(output.get('__proto__').polluted, true)
  assert.equal(output.get('constructor'), 7)
  assert.equal({}.polluted, undefined)
})

test('native keypair exposes the exact Rust backing variant', () => {
  assert.equal(core.Keypair.fromUri('//Alice').kind, 'Sr25519')
  assert.equal(new core.Keypair(core.Keypair.fromUri('//Alice').ss58Address).kind, 'PublicOnly')
})

test('raw native escape hatch includes the complete low-level bridge', () => {
  for (const name of [
    'coreValueDescriptorRoundtrip',
    'convertTypeString',
    'normalizeTypeSpec',
    'storagePrefixFor',
    'epochPredictFirstRevealBlockResult',
  ]) {
    assert.equal(typeof core.native[name], 'function', `${name} is exported`)
  }
  assert.equal(typeof core.native.NativeCursor.fromBytes, 'function')
  assert.equal(typeof core.native.NativeRuntime.fromMetadata, 'function')
})

test('password-protected mnemonic keypairs derive entirely through the native handle', () => {
  const mnemonic =
    'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
  const password = 'protected-derivation-password'
  const parent = core.Keypair.fromMnemonic(mnemonic, core.CRYPTO_SR25519, password)

  const child = parent.derive('//child')
  const expectedChild = core.Keypair.fromUri(`${mnemonic}//child///${password}`)
  assert.deepEqual(child.publicKey, expectedChild.publicKey)

  const grandchild = child.derive('//grandchild')
  const expectedGrandchild = core.Keypair.fromUri(
    `${mnemonic}//child//grandchild///${password}`,
  )
  assert.deepEqual(grandchild.publicKey, expectedGrandchild.publicKey)
  assert.equal(typeof core.native.NativeKeypair.prototype.derive, 'function')

  for (const pair of [parent, child, grandchild]) {
    assert.equal(pair.meta.suri, undefined)
    assert.equal(Object.prototype.hasOwnProperty.call(pair, 'sourceUri'), false)
    assert.equal(Object.prototype.hasOwnProperty.call(pair, 'derivationSource'), false)
  }
})

test('rustCore mirrors the Rust CoreError module and root re-export', () => {
  assert.equal(core.rustCore.CoreError, core.CoreError)
  assert.equal(core.rustCore.error.CoreError, core.CoreError)
  assert.equal(core.rustCore.error.KeyfileError, core.KeyfileError)
  assert.equal(core.rustCore.error.WrongPasswordError, core.WrongPasswordError)
  assert.equal(core.rustCore.error.NotInRuntimeError, core.NotInRuntimeError)
  assert.equal(core.rustCore.error.CodecError, core.CodecError)
  assert.equal(core.rustCore.error.CryptoError, core.CryptoError)
  assert.equal(core.rustCore.error.DeviceError, core.DeviceError)
})
