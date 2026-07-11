'use strict'

const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const core = require('../dist/index.js')

test('package exposes a WASM browser subset without the Node native addon', () => {
  const root = path.join(__dirname, '..')
  const packageJson = JSON.parse(
    fs.readFileSync(path.join(root, 'package.json'), 'utf8'),
  )

  assert.equal(packageJson.browser, './dist/browser.mjs')
  assert.equal(packageJson.exports['.'].browser.import, './dist/browser.mjs')
  assert.equal(packageJson.exports['./browser'].import, './dist/browser.mjs')
  assert.equal(packageJson.exports['./native'].node.import, './native.cjs')

  const browserSource = fs.readFileSync(path.join(root, 'dist', 'browser.js'), 'utf8')
  assert.equal(browserSource.includes('./native'), false)
  assert.equal(browserSource.includes('node:buffer'), false)
  assert.equal(browserSource.includes('.node'), false)
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

test('private key bytes are not exported to JavaScript', () => {
  const alice = core.Keypair.fromUri('//Alice')
  assert.equal(Object.prototype.hasOwnProperty.call(alice, 'privateKey'), false)
  assert.equal('privateKey' in alice, false)
  assert.equal(
    Object.prototype.hasOwnProperty.call(core.native.NativeKeypair.prototype, 'privateKey'),
    false,
  )
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
