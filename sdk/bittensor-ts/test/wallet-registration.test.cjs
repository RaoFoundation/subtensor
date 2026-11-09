'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
const { Keypair, publicKeyFromSs58 } = require('../dist/keys.js')
const { Wallet, listWallets } = require('../dist/wallet.js')
const { registerWallet, walletPublicKey, WalletRegistrationError, registrationDepositForTypes, estimateWalletRegistrationFee } = require('../dist/wallet-registration.js')
const { descriptorBytes } = require('../dist/protected-accounts.js')
const { isRotatingCrypto } = require('../dist/wallet-types.js')

function key(type, seed) { return Keypair.fromSeed(new Uint8Array(32).fill(seed), type) }
function fixture(types = [4, 6]) {
  const keys = types.map((type, i) => key(type, i + 1)), payer = key(1, 3)
  const wallet = { coldkeypub: keys[0].publicOnly(), hotkeypub: keys[1].publicOnly() }
  const records = new Map(), queries = [], submissions = [], timeline = []
  let reserve = 200n, free = 10_000n, frozen = 0n, enabled = true, shield = true, finalized = 0, failedAt = -1
  const reader = block => ({
    blockHash: async () => block, hasStorage: async () => true, supportsPqProfile: () => true,
    constant: async () => 1n,
    query: async (p, s, args, at) => {
      queries.push([p, s, at]);
      if (s === 'Accounts' || s === 'MlDsaAccounts') return records.get(args[0]) ?? null
      if (s === 'HashedAccountsEnabled') return enabled
      if (s === 'HashedAccountRegistrationDeposit') return reserve
      if (s === 'Account') return { data: { free, frozen } }
      if (s === 'NextKey') return shield ? new Uint8Array(1184) : null
      throw new Error(`unexpected ${p}.${s}`)
    },
  })
  const adapter = {
    publicKey: walletPublicKey, descriptorAccount: d => Keypair.fromDescriptor(d).ss58Address,
    decodeAddress: publicKeyFromSs58,
    finalizedHead: async () => { timeline.push(`finalized:${finalized}`); return `finalized:${finalized}` },
    blockHash: async () => 'head', reader: async block => reader(block), estimateFee: async () => 10n,
    submit: async (call, supplied, expected) => {
      assert.equal(expected.ss58Address, payer.ss58Address)
      timeline.push(`submit:${submissions.length}`); submissions.push(call)
      if (submissions.length === failedAt) throw new Error('connection lost before second registration')
      for (const child of call.call === 'batch_all' ? call.params.calls : [call]) {
        if (child.call === 'check_registration_deposit') { assert.ok(reserve <= child.params.max_deposit); continue }
        const value = child.params.descriptor
        const descriptor = descriptorBytes(value.Classical ?? value.MlDsa65 ?? value.Falcon ?? value)
        records.set(adapter.descriptorAccount(descriptor), { descriptor })
      }
      finalized++
      return { success: true, finalized: true, extrinsicHash: `receipt:${finalized}` }
    },
  }
  return { adapter, wallet, payer, keys, records, queries, submissions, timeline,
    reserve: value => { reserve = value }, free: value => { free = value }, frozen: value => { frozen = value },
    enabled: value => { enabled = value }, shield: value => { shield = value }, failAt: value => { failedAt = value },
  }
}

test('fixed and standard wallets need no registry, payer, fee, or chain request', async () => {
  for (const type of [0, 1, 8, 9, 11]) {
    const f = fixture([type, type]); f.adapter.finalizedHead = async () => assert.fail('must not contact chain')
    const result = await registerWallet(f.adapter, f.wallet)
    assert.equal(result.deposit.rao, 0n); assert.deepEqual(result.accounts, [])
    assert.equal((await registrationDepositForTypes(f.adapter, [type, type])).rao, 0n)
  }
})

test('classical rotating registration verifies finalized state between guarded Shield submissions', async () => {
  const f = fixture()
  let approved
  const result = await registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 400n, onRegistrationSponsorship: quote => { approved = quote; return true } })
  assert.equal(approved.deposit.rao, 400n); assert.equal(approved.fee.rao, 20n)
  assert.deepEqual(f.timeline, ['finalized:0', 'submit:0', 'finalized:1', 'submit:1', 'finalized:2'])
  assert.equal(result.accounts.length, 2); assert.equal(result.finalizedBlockHash, 'finalized:2')
  for (const call of f.submissions) assert.deepEqual(call.params.calls.map(c => c.call), ['check_registration_deposit', 'register'])
  assert.ok(f.queries.filter(q => q[1] === 'HashedAccountRegistrationDeposit').every(q => q[2] === 'head'))
  const again = await registerWallet(f.adapter, f.wallet)
  assert.equal(again.result, null); assert.equal(again.deposit.rao, 0n); assert.equal(f.submissions.length, 2)
})

test('rotating PQ registrations batch and duplicate roles reserve once', async () => {
  const f = fixture([7, 10])
  await registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 400n })
  assert.equal(f.submissions.length, 1)
  assert.deepEqual(f.submissions[0].params.calls.map(c => c.call), ['check_registration_deposit', 'register', 'register'])
  const duplicate = fixture(); duplicate.wallet.hotkeypub = duplicate.wallet.coldkeypub
  const result = await registerWallet(duplicate.adapter, duplicate.wallet, { payer: duplicate.payer, maxRegistrationDepositRao: 200n })
  assert.equal(result.accounts.length, 1); assert.equal(result.deposit.rao, 200n)
  assert.equal(result.roles.coldkey, result.roles.hotkey)
})

test('invalid descriptors, disabled authorization, missing Shield, reserve and spendable limits fail before submit', async () => {
  for (const [mutate, expected] of [
    [f => { f.wallet.coldkeypub = { cryptoType: 4, ss58Address: f.payer.ss58Address, publicKey: f.payer.publicKey, authorizationDescriptor: () => f.keys[0].authorizationDescriptor() } }, /descriptor does not match/],
    [f => f.enabled(false), /disabled/], [f => f.shield(false), /Shield is unavailable/],
    [f => f.reserve(201n), /approved maximum/], [f => f.free(420n), /insufficient/],
    [f => { f.free(500n); f.frozen(100n) }, /insufficient/],
  ]) {
    const f = fixture(); mutate(f)
    await assert.rejects(registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 400n }), expected)
    assert.equal(f.submissions.length, 0)
  }
})

test('partial registration preserves the receipt and retry skips finalized keys', async () => {
  const f = fixture(); f.failAt(2)
  await assert.rejects(registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 400n }), error => {
    assert.ok(error instanceof WalletRegistrationError); assert.equal(error.wallet, f.wallet)
    assert.equal(error.receipt.extrinsicHash, 'receipt:1'); assert.equal(error.quote.deposit.rao, 400n)
    return true
  })
  f.failAt(-1)
  const result = await registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 200n })
  assert.equal(result.deposit.rao, 200n); assert.equal(f.records.size, 2)
})

test('registration requires descriptor equality in finalized state and an existing payer', async () => {
  const f = fixture()
  await assert.rejects(registerWallet(f.adapter, f.wallet, { maxRegistrationDepositRao: 400n }), /existing funded payer/)
  await assert.rejects(registerWallet(f.adapter, f.wallet, { payer: f.keys[0], maxRegistrationDepositRao: 400n }), /cannot pay for its own/)
  f.records.set(f.keys[0].ss58Address, { descriptor: f.keys[1].authorizationDescriptor() })
  await assert.rejects(registerWallet(f.adapter, f.wallet, { payer: f.payer, maxRegistrationDepositRao: 400n }), /registered wallet descriptor/)
  assert.equal(f.submissions.length, 0)
})

test('fee admission includes the inner transaction and maximum Shield carrier without signing', async () => {
  const calls = [], encoded = [], payer = key(1, 1).publicOnly()
  const fee = await estimateWalletRegistrationFee({
    runtimeAt: async block => {
      assert.equal(block, 'head')
      return { composeCall: (p, c, args) => { calls.push([p, c, args]); return Uint8Array.of(1) },
        encodeSignedExtrinsic: (data, account, proof, type, params) => { encoded.push({ account, proof, type, params }); return { bytes: Uint8Array.of(2) } } }
    }, blockNumber: async () => 5, rpc: async (method, args) => { assert.equal(method, 'payment_queryInfo'); assert.equal(args[1], 'head'); return { partialFee: '12' } },
  }, { pallet: 'HashedAccounts', call: 'register', params: {} }, payer, 'head', publicKeyFromSs58)
  assert.equal(fee, 24n); assert.equal(calls[1][2].ciphertext.length, 8192)
  assert.ok(encoded.every(e => e.params.nonce === 0xffffffffn && e.proof.every(byte => byte === 0)))
})

test('complete wallet create preserves other hotkeys and defaults PQ mnemonic strength', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-create-')); t.after(() => fs.rm(root, { recursive: true, force: true }))
  const wallet = new Wallet({ name: 'wallet', path: root })
  const created = await wallet.create({ coldkey: { cryptoType: 7, allowPlaintext: true }, hotkey: { cryptoType: 1, allowPlaintext: true } })
  assert.equal(created.coldkey.mnemonic.split(' ').length, 24); assert.equal(created.hotkey.mnemonic.split(' ').length, 12)
  const other = new Wallet({ name: 'wallet', hotkey: 'other', path: root })
  await other.createNewHotkey({ allowPlaintext: true })
  const bytes = await fs.readFile(other.hotkeyFile.path)
  await wallet.create({ coldkey: { allowPlaintext: true }, hotkey: { allowPlaintext: true }, overwrite: true })
  assert.deepEqual(await fs.readFile(other.hotkeyFile.path), bytes)
  assert.deepEqual((await listWallets(root)).wallet, ['default', 'other'])
  for (const file of [wallet.coldkeyFile, wallet.coldkeypubFile, wallet.hotkeyFile, wallet.hotkeypubFile]) assert.equal((await fs.stat(file.path)).mode & 0o777, 0o600)
  for (const type of [7, 8, 9, 10, 11, 12]) assert.equal(Keypair.generateWithMnemonic(type).mnemonic.split(' ').length, 24)
})

// PROTECTED_REGRESSION: failed multi-key creation must preserve all existing recovery secrets; changes require explicit human approval.
test('wallet create validates both roles and all destinations before replacing existing files', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-preserve-')); t.after(() => fs.rm(root, { recursive: true, force: true }))
  const wallet = new Wallet({ path: root })
  await wallet.create({ coldkey: { allowPlaintext: true }, hotkey: { allowPlaintext: true } })
  const files = [wallet.coldkeyFile, wallet.coldkeypubFile, wallet.hotkeyFile, wallet.hotkeypubFile]
  const originals = await Promise.all(files.map(f => fs.readFile(f.path)))
  await assert.rejects(wallet.create({ coldkey: { allowPlaintext: true }, hotkey: { cryptoType: 7, nWords: 12, allowPlaintext: true }, overwrite: true }), /24-word/)
  for (let i = 0; i < files.length; i++) assert.deepEqual(await fs.readFile(files[i].path), originals[i])
  await assert.rejects(wallet.create({ coldkey: { allowPlaintext: true }, hotkey: { allowPlaintext: true } }), /already exists/)
  for (let i = 0; i < files.length; i++) assert.deepEqual(await fs.readFile(files[i].path), originals[i])
})

// PROTECTED_REGRESSION: password callbacks must never substitute a different signing identity after public approval; changes require explicit human approval.
test('wallet signer pins public identity before a password callback can replace a key', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-identity-')); t.after(() => fs.rm(root, { recursive: true, force: true }))
  const wallet = new Wallet({ path: root })
  await wallet.setColdkey(key(1, 1), { allowPlaintext: true })
  await assert.rejects(wallet.signer('coldkey', { passwordProvider: async () => {
    await wallet.setColdkey(key(1, 2), { allowPlaintext: true, overwrite: true }); return null
  } }), /does not match the public wallet/)
})

test('create and register refuses an unapproved reserve before persisting keys', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-preapprove-')); t.after(() => fs.rm(root, { recursive: true, force: true }))
  const wallet = new Wallet({ path: root })
  const { Balance } = require('../dist/balance.js')
  const client = { registrationDepositForTypes: async () => Balance.fromRao(700n), registerWallet: async () => assert.fail('must not submit') }
  await assert.rejects(wallet.createAndRegister(client, { coldkey: { cryptoType: 4, allowPlaintext: true }, hotkey: { allowPlaintext: true } }, { payer: key(1, 3), maxRegistrationDepositRao: 200n }), /approved maximum/)
  assert.equal(await wallet.coldkeyFile.exists(), false)
})

test('Node and browser registration adapters preserve external ECDSA payer identity and required Shield options', async () => {
  const sdk = require('../dist/index.js'), browser = require('../dist/browser.js')
  const { pathToFileURL } = require('node:url')
  browser.configureBrowserWasm(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports); instance.exports.__wbindgen_start?.()
    const loaded = { ...bindings }; delete loaded.default; return loaded
  })
  await browser.initBrowser()
  const publicKey = Buffer.from('0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798', 'hex')
  for (const surface of [sdk, browser]) {
    const client = new surface.Client('local')
    let accountReads = 0, signs = 0
    const runtime = { ss58Format: 42, composeCall: () => Uint8Array.of(1) }
    client.runtimeAt = async () => runtime
    const payer = {
      async getAccount(context) {
        accountReads++; assert.equal(this, payer); assert.equal(context.runtime, runtime)
        return { address: surface.ss58FromEcdsaPublic(publicKey), publicKey: '0x' + publicKey.toString('hex'), cryptoType: 2 }
      },
      async signBytes(bytes) { signs++; assert.equal(this, payer); assert.deepEqual(bytes, Uint8Array.of(9)); return new Uint8Array(65) },
    }
    const adapter = client.walletRegistrationAdapter(), expected = await adapter.publicKey(payer, 'coldkey')
    assert.deepEqual(Buffer.from(expected.publicKey), sdk.accountIdFromEcdsaPublic(publicKey))
    assert.equal(signs, 0)
    const submit = async (call, signer, options) => {
      assert.equal(options.mevShield, true); assert.equal(options.waitForFinalization, true); assert.equal(options.allowRawCall, true)
      assert.equal(signer.cryptoType, 2); assert.equal(signer.ss58Address, expected.ss58Address)
      await signer.signBytes(Uint8Array.of(9)); return { success: true }
    }
    client.submitRawShielded = submit; client.submit = submit
    await adapter.submit({ pallet: 'HashedAccounts', call: 'register', params: {} }, payer, expected, {})
    assert.equal(signs, 1); assert.ok(accountReads >= 4)
    const addressOnly = await adapter.publicKey({ address: key(1, 9).ss58Address, cryptoType: 1 }, 'coldkey')
    assert.deepEqual(Buffer.from(addressOnly.publicKey), key(1, 9).publicKey)
    client.runtimeAt = async () => assert.fail('fixed key selection must not query chain')
    const fixed = surface.Keypair.fromSeed(new Uint8Array(32).fill(8), 8)
    assert.equal((await client.registerWallet(fixed)).deposit.rao, 0n)
    assert.equal(surface.Keypair.generateWithMnemonic(8).mnemonic.split(' ').length, 24)
    await client.close()
  }
})

test('wallet account mode selects hashed identities without changing the signing scheme', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-mode-')); t.after(() => fs.rm(root, { recursive: true, force: true }))
  const { parseCryptoType } = require('../dist/wallet.js')
  const wallet = new Wallet({ path: root })
  assert.equal(parseCryptoType('falcon512', 'hashed'), 10)
  assert.equal(parseCryptoType('hashed-ed25519', 'standard'), 0)
  await wallet.regenerateColdkey({ seed: new Uint8Array(32).fill(2) }, { cryptoType: 0, accountType: 'hashed', allowPlaintext: true })
  assert.equal((await wallet.coldkeypub).cryptoType, 6)
  assert.equal(Wallet.generateHotkey({ cryptoType: 8, accountType: 'hashed' }).keypair.cryptoType, 7)
})
