'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
const { Wallet } = require('../dist/wallet')
const { Keypair } = require('../dist/keys')

async function legacyWallet(t, password) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-legacy-hotkey-'))
  t.after(() => fs.rm(root, { recursive: true, force: true }))
  const wallet = new Wallet({ path: root })
  const key = Keypair.fromSeed(new Uint8Array(32).fill(17), 1)
  await key.writeKeyfile(wallet.hotkeyFile.path, password ? { password } : { allowPlaintext: true })
  return { root, wallet, key }
}

test('legacy plaintext hotkey-only wallets expose a public view and remain signable', async t => {
  const { wallet, key } = await legacyWallet(t)
  const publicKey = await wallet.hotkeypub
  assert.equal(publicKey.kind, 'PublicOnly')
  assert.equal(publicKey.ss58Address, key.ss58Address)
  const signer = await wallet.signer('hotkey')
  assert.equal(signer.ss58Address, key.ss58Address)
  const message = Buffer.from('legacy wallet remains usable')
  assert.equal(signer.verify(message, signer.sign(message)), true)
  assert.equal(await wallet.hotkeypubFile.exists(), false, 'reading a public view must not rewrite the wallet')
})

test('malformed or symbolic public companions never fall back to the private hotkey', async t => {
  const { wallet } = await legacyWallet(t)
  await fs.writeFile(wallet.hotkeypubFile.path, 'invalid public keyfile')
  await assert.rejects(wallet.hotkeypub)
  let passwordRequests = 0
  await assert.rejects(wallet.signer('hotkey', { passwordProvider: async () => { passwordRequests++; return null } }))
  assert.equal(passwordRequests, 0)
  await fs.rm(wallet.hotkeypubFile.path)
  await fs.symlink(wallet.hotkeyFile.path, wallet.hotkeypubFile.path)
  const symlinkWallet = new Wallet({ path: wallet.path })
  await assert.rejects(symlinkWallet.hotkeypub, /symlink|symbolic|without following/i)
})

// PROTECTED_REGRESSION: legacy hotkey password hooks must not replace the approved signer with a different account; changes require explicit human approval.
test('legacy fallback pins the signer before a password callback can replace its keyfile', async t => {
  const { wallet } = await legacyWallet(t)
  const replacement = Keypair.fromSeed(new Uint8Array(32).fill(18), 1)
  await assert.rejects(wallet.signer('hotkey', { passwordProvider: async () => {
    await replacement.writeKeyfile(wallet.hotkeyFile.path, { allowPlaintext: true, overwrite: true })
    return null
  } }), /does not match the public wallet/)
})

test('legacy encrypted hotkeys support explicit passwords and reject invalid unlocks', async t => {
  const password = 'legacy-test-only-password'
  const { wallet, key } = await legacyWallet(t, password)
  await assert.rejects(wallet.hotkeypub)
  const explicit = new Wallet({ path: wallet.path })
  assert.equal((await explicit.signer('hotkey', { password })).ss58Address, key.ss58Address)
  const wrong = new Wallet({ path: wallet.path })
  await assert.rejects(wrong.signer('hotkey', { password: 'wrong-password' }))
})

// PROTECTED_REGRESSION: an encrypted legacy hotkey password hook must not substitute a different signing account by replacing its keyfile; changes require explicit human approval.
test('provider-only legacy unlocking signs with the captured original key despite file replacement', async t => {
  const password = 'legacy-test-only-password'
  const { wallet, key } = await legacyWallet(t, password)
  const originalCiphertext = await fs.readFile(wallet.hotkeyFile.path)
  const replacement = Keypair.fromSeed(new Uint8Array(32).fill(18), 1)
  let passwordRequests = 0
  const signer = await wallet.signer('hotkey', { passwordProvider: async (requestedWallet, role) => {
    assert.equal(requestedWallet, wallet)
    assert.equal(role, 'hotkey')
    passwordRequests++
    await wallet.setHotkey(replacement, { keyfilePassword: password, overwrite: true })
    assert.notDeepEqual(await fs.readFile(wallet.hotkeyFile.path), originalCiphertext)
    return password
  } })
  assert.equal(passwordRequests, 1)
  assert.equal(signer.ss58Address, key.ss58Address)
  const message = Buffer.from('the original encrypted wallet approved this signature')
  const signature = signer.sign(message)
  assert.equal(key.verify(message, signature), true)
  assert.equal(replacement.verify(message, signature), false)
  assert.equal((await wallet.hotkeypub).ss58Address, key.ss58Address)
  assert.equal((await wallet.hotkeypub).kind, 'PublicOnly')
  assert.equal((await Keypair.fromKeyfile(wallet.hotkeyFile.path, password)).ss58Address, replacement.ss58Address)
})

test('legacy capture refuses a symbolic private keyfile before requesting a password', async t => {
  const { wallet } = await legacyWallet(t)
  const saved = wallet.hotkeyFile.path + '.original'
  await fs.rename(wallet.hotkeyFile.path, saved)
  await fs.symlink(saved, wallet.hotkeyFile.path)
  let passwordRequests = 0
  await assert.rejects(wallet.signer('hotkey', { passwordProvider: async () => { passwordRequests++; return 'unused-password' } }), /symlink|symbolic|without following/i)
  assert.equal(passwordRequests, 0)
})
