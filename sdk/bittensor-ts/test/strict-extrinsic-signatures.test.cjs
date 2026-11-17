const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const { SigningKey, getBytes } = require('ethers')
const node = require('../dist')
const browser = require('../dist/browser.js')
const { serializeUnsignedTransaction } = require('../dist/unsigned-transaction')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = node.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const genesis = '0x' + '11'.repeat(32)

test.before(async () => {
  await browser.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default
    return module
  })
})

function signer(cryptoType) {
  if (cryptoType !== 2) return node.Keypair.fromSeed(Buffer.alloc(32, 23), cryptoType)
  const key = new SigningKey('0x' + '17'.repeat(32))
  const publicKey = Buffer.from(getBytes(key.compressedPublicKey))
  return {
    publicKey, ss58Address: node.ss58FromPublic(node.accountIdFromEcdsaPublic(publicKey)), cryptoType,
    sign(payload) {
      const signature = key.sign(node.blake2_256(payload))
      return Buffer.concat([Buffer.from(getBytes(signature.r)), Buffer.from(getBytes(signature.s)), Buffer.from([signature.yParity])])
    },
  }
}

for (const surface of ['Node', 'browser']) {
  // PROTECTED_REGRESSION: imported transactions must reject legacy message-wrapped signatures before broadcast while preserving valid offline signing; changes require explicit human approval.
  test(`${surface} imported extrinsics verify only the exact signing payload`, async t => {
    for (const cryptoType of [0, 1, 2]) for (const size of [1, 1024]) {
      const key = signer(cryptoType), runtime = new node.Runtime(metadata, 419, 1, 42)
      const callData = runtime.composeCall('System', 'remark', { remark: Buffer.alloc(size, 7) })
      const txParams = { era: '00', nonce: 7, tip: 0n, genesisHash: Buffer.from(genesis.slice(2), 'hex'), eraBlockHash: Buffer.from(genesis.slice(2), 'hex') }
      const state = { runtime, address: key.ss58Address, publicKey: node.publicKeyFromSs58(key.ss58Address), cryptoType, callData, payload: runtime.signaturePayload(callData, txParams), ...runtime.signaturePayloadParts(txParams), txParams }
      if (size > 256) assert.equal(state.payload.length, 32)
      const serialized = serializeUnsignedTransaction(state, node, metadata)
      const client = surface === 'Node' ? new node.Client('local') : new browser.Client('local'), sent = []
      t.after(() => client.close())
      Object.assign(client, { runtimeAt: async () => runtime, genesisHash: async () => genesis, blockHash: async () => genesis, blockNumber: async () => 0, runtimeEntry: async () => ({ runtime, metadata }) })
      const broadcast = async bytes => { sent.push(bytes); return { status: 'submitted', events: [], extrinsicHash: '0xok' } }
      client.submitSigned = broadcast; client.broadcastExtrinsic = broadcast
      const imported = await client.importPreparedCall(serialized, { allowRawCall: true })
      const wrapped = Buffer.concat([Buffer.from('<Bytes>'), state.payload, Buffer.from('</Bytes>')]), signature = key.sign(wrapped)
      assert.equal(node.verifySignature(state.payload, signature, key.ss58Address, cryptoType), true, 'message verification retains wrapper compatibility')
      assert.equal(browser.verifySignature(state.payload, signature, key.ss58Address, cryptoType), true)
      await assert.rejects(client.submitSignature(imported, signature), /does not verify|invalid transaction signature/)
      assert.equal(sent.length, 0)
      await client.submitSignature(imported, key.sign(state.payload))
      assert.equal(sent.length, 1)
    }
  })
}
