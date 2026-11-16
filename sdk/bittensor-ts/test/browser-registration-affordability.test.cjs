'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { registrationDescriptor } = require('../dist/protected-accounts')

const delegate = sdk.Keypair.fromUri('//Alice'), real = sdk.Keypair.fromUri('//Bob')
const descriptor = Uint8Array.from([1, 1, ...Array(32).fill(7)])
const recipient = sdk.Keypair.fromDescriptor(descriptor).ss58Address
const reserve = 700_000_000n, transfer = 100_000_000n, feePerExtrinsic = 3_000_000n, fee = feePerExtrinsic * 2n
const block = '0x' + '11'.repeat(32)
const hex = value => '0x' + Buffer.from(value).toString('hex')

let initialized, browser
async function initialize() {
  if (!initialized) initialized = (async () => {
    browser = await import('../dist/browser.mjs')
    browser.configureBrowserWasm(async () => {
      const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
      const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
      bindings.__wbg_set_wasm(instance.exports)
      instance.exports.__wbindgen_start?.()
      const module = { ...bindings }; delete module.default
      return module
    })
    await browser.initBrowser()
  })()
  await initialized
}

async function fixture({ realFree = reserve + transfer, delegateFree = fee } = {}) {
  await initialize()
  const client = new browser.Client('local'), state = { signed: 0, submitted: 0, consented: 0 }
  const committed = { pallet: 'Utility', call: 'batch_all', params: { calls: [
    { pallet: 'HashedAccounts', call: 'check_registration_deposit', params: { max_deposit: reserve } },
    { pallet: 'HashedAccounts', call: 'register', params: { descriptor: registrationDescriptor(descriptor) } },
    { pallet: 'Balances', call: 'transfer_allow_death', params: { dest: recipient, value: transfer } },
  ] } }
  // Mock the chain codec at its byte boundary; the committed metadata fixture
  // predates HashedAccounts. All browser preparation and affordability are real.
  const saved = Buffer.from(JSON.stringify(committed, (_, value) => typeof value === 'bigint' ? value.toString() : value instanceof Uint8Array ? hex(value) : value))
  const query = async (pallet, item, params) => {
    if (pallet === 'HashedAccounts' && item === 'HashedAccountRegistrationDeposit') return reserve
    if (pallet === 'MevShield' && item === 'NextKey') return null
    if (pallet === 'System' && item === 'Account') return { data: { free: params[0] === real.ss58Address ? realFree : delegateFree } }
    throw Error(`unexpected query ${pallet}.${item}`)
  }
  const reader = {
    query, decodeAddress: sdk.publicKeyFromSs58, encodeAddress: sdk.ss58FromPublic,
    blockHash: async () => block, hasStorage: async () => true,
    descriptorAddress: value => sdk.Keypair.fromDescriptor(value).ss58Address,
    decodeCall: async data => { assert.deepEqual(Buffer.from(data), saved); return JSON.parse(Buffer.from(data).toString()) },
    hashCallData: sdk.blake2_256,
    runtimeCall: async () => assert.fail('saved transfer must not be rebuilt'),
    countStorageKeys: async () => 0,
  }
  const intent = browser.buildIntent('execute_proxy_announced', {
    delegate_ss58: delegate.ss58Address, real_ss58: real.ss58Address,
    inner_op: 'transfer', inner_args: { dest_ss58: recipient, amount_tao: '999' },
    announced_call_data: hex(saved), expected_call_hash: hex(sdk.blake2_256(saved)),
  })
  client.blockHash = async () => block
  client.at = async () => ({ hash: block, query: ([pallet, item], params) => query(pallet, item, params) })
  client.intentReader = async () => reader
  client.peekNonce = async () => 0
  client.estimateFee = async () => sdk.Balance.fromRao(feePerExtrinsic)
  client.prepareRaw = async call => {
    assert.equal(call.pallet, 'Proxy')
    assert.equal(call.call, 'proxy_announced')
    assert.equal(call.params.call.callData, hex(saved))
    assert.equal(BigInt(call.params.call.params.calls[2].params.value), transfer)
    return { payload: saved, callData: saved, publicKey: delegate.publicKey, cryptoType: 1, txParams: {}, runtime: { encodeSignedExtrinsic: () => ({ bytes: Buffer.concat([saved, Buffer.alloc(64)]) }) } }
  }
  client.signPrepared = async (unsigned, signer) => {
    const signature = await signer.signBytes(unsigned.payload)
    assert.equal(delegate.verify(unsigned.payload, signature), true)
    return { bytes: Buffer.concat([saved, signature]), hash: sdk.blake2_256(saved) }
  }
  client.broadcastExtrinsic = async bytes => {
    state.submitted++
    assert.deepEqual(Buffer.from(bytes).subarray(0, saved.length), saved)
    return { status: 'inBlock', success: true, extrinsicHash: hex(sdk.blake2_256(bytes)), blockHash: block, events: [] }
  }
  // Keep the network/Shield submission boundary deterministic while checking
  // that real planning and consent finish before a signer is invoked.
  client.submitPreparedCall = async (call, signer, options) => {
    assert.equal(options.mevShield, true)
    const unsigned = await client.prepareRaw(call)
    const signed = await client.signPrepared(unsigned, signer)
    return client.broadcastExtrinsic(signed.bytes)
  }
  const signer = { address: delegate.ss58Address, publicKey: delegate.publicKey, cryptoType: 1,
    signBytes: async payload => { state.signed++; return delegate.sign(payload) } }
  const options = { mevShield: true, maxRegistrationDepositRao: reserve, onRegistrationSponsorship: quote => {
    state.consented++
    assert.deepEqual(quote, { payer: real.ss58Address, recipient, depositRao: reserve })
    return true
  } }
  return { client, intent, signer, options, state }
}

test('browser signs committed registration with exactly one reserve, transfer and delegate fee', async t => {
  const f = await fixture()
  t.after(() => f.client.close())
  const result = await f.client.execute(f.intent, f.signer, f.options)
  assert.equal(result.success, true)
  assert.deepEqual(f.state, { signed: 1, submitted: 1, consented: 1 })
})

test('browser refuses committed registration before signing when the fee payer is one rao short', async t => {
  const f = await fixture({ delegateFree: fee - 1n })
  t.after(() => f.client.close())
  await assert.rejects(f.client.execute(f.intent, f.signer, f.options), /fee payer needs 6000000 rao; available 5999999/)
  assert.deepEqual(f.state, { signed: 0, submitted: 0, consented: 0 })
})

test('browser refuses committed registration before signing when its sponsor is one rao below the reserve', async t => {
  const f = await fixture({ realFree: reserve - 1n })
  t.after(() => f.client.close())
  await assert.rejects(f.client.execute(f.intent, f.signer, f.options), /transaction rejected/)
  assert.deepEqual(f.state, { signed: 0, submitted: 0, consented: 0 })
})
