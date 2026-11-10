'use strict'
const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
const { Wallet, listWallets, listWalletsDetailed, resolveWalletPassword, parseCryptoType, formatCryptoType, isBittensorAddress } = require('../dist/wallet.js')
const { Keypair } = require('../dist/keys.js')
const { WALLET_TYPES, isProtectedCrypto } = require('../dist/wallet-types.js')

test('every release wallet mode persists its public descriptor and exact crypto identity', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'bt-wallet-release-'))
  t.after(() => fs.rm(root,{recursive:true,force:true}))
  for (const [name,cryptoType] of Object.entries(WALLET_TYPES)) {
    assert.equal(parseCryptoType(name),cryptoType)
    assert.equal(formatCryptoType(cryptoType),name)
    const wallet = new Wallet({name,path:root})
    await wallet.regenerateColdkey({seed:new Uint8Array(32).fill(6)},{cryptoType,allowPlaintext:true})
    const restored = new Wallet({name,path:root}), privateKey = await restored.coldkey, publicKey = await restored.coldkeypub
    assert.equal(privateKey.cryptoType,cryptoType)
    assert.equal(publicKey.cryptoType,cryptoType)
    assert.equal(privateKey.ss58Address,publicKey.ss58Address)
    if (isProtectedCrypto(cryptoType)) {
      assert.deepEqual(publicKey.authorizationDescriptor(),privateKey.authorizationDescriptor())
      assert.throws(() => publicKey.signAuthorization(Buffer.from('forbidden')))
      await restored.regenerateHotkeypub({descriptor:publicKey.authorizationDescriptor()})
      assert.deepEqual((await restored.hotkeypub).authorizationDescriptor(),publicKey.authorizationDescriptor())
    }
  }
})

test('wallet seed/public recovery creates safe keyfiles and naturally sorted inventory', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-parity-'))
  t.after(() => fs.rm(root, { recursive: true, force: true }))
  for (const name of ['wallet10', 'wallet2']) {
    const wallet = new Wallet({ name, hotkey: 'hot10', path: root })
    await wallet.regenerateColdkey({ seed: new Uint8Array(32).fill(1) }, { allowPlaintext: true })
    await wallet.regenerateHotkey({ seed: new Uint8Array(32).fill(2) }, { allowPlaintext: true })
    const pub = await wallet.coldkeypub
    assert.equal(pub.kind, 'PublicOnly')
    assert.equal(await wallet.coldkeyFile.isEncrypted(), false)
    assert.equal(await wallet.coldkeyFile.isReadable(), true)
    assert.equal((await fs.stat(wallet.coldkeyFile.path)).mode & 0o777, 0o600)
    assert.equal(isBittensorAddress(pub.ss58Address), true)
    const restored = new Wallet({ name, hotkey: 'hot2', path: root })
    await restored.regenerateHotkeypub({ ss58Address: pub.ss58Address })
    assert.equal((await restored.hotkeypub).kind, 'PublicOnly')
    assert.equal(await restored.hotkeyFile.exists(), false)
  }
  const inventory = await listWallets(root)
  assert.deepEqual(Object.keys(inventory), ['wallet2', 'wallet10'])
  assert.deepEqual(inventory.wallet2, ['hot10'])
  const detailed = await listWalletsDetailed(root)
  assert.equal(detailed[0].hotkeys[0].ss58, Keypair.fromSeed(new Uint8Array(32).fill(2)).ss58Address)
})

test('wallet password resolution respects explicit/environment/file/provider precedence', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-wallet-password-'))
  const previous = process.env.BT_WALLET_PASSWORD
  t.after(async () => { if (previous == null) delete process.env.BT_WALLET_PASSWORD; else process.env.BT_WALLET_PASSWORD = previous; await fs.rm(root, { recursive: true, force: true }) })
  const wallet = new Wallet({ path: root }), file = path.join(root, 'password')
  await fs.writeFile(file, 'file-password\n', { mode: 0o600 })
  process.env.BT_WALLET_PASSWORD = 'environment-password'
  assert.equal(await resolveWalletPassword(wallet, { password: 'explicit', passwordFile: file }), 'explicit')
  assert.equal(await resolveWalletPassword(wallet, { passwordFile: file }), 'environment-password')
  delete process.env.BT_WALLET_PASSWORD
  assert.equal(await resolveWalletPassword(wallet, { passwordFile: file }), 'file-password')
  assert.equal(await resolveWalletPassword(wallet, { passwordProvider: async () => 'provider-password' }), 'provider-password')
  assert.equal(parseCryptoType(' ED25519 '), 0)
  assert.equal(formatCryptoType(null), 'sr25519')
  assert.throws(() => parseCryptoType('rsa'), /crypto type/)
})
