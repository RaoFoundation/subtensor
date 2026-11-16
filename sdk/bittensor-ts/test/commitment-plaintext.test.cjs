const test = require('node:test')
const assert = require('node:assert/strict')
const { fetchCommitments, decodeRevealedCommitment } = require('../dist/metagraph')
const { Balance } = require('../dist/balance')
const { Runtime, decodeOptionalOpaqueMetadata } = require('../dist')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new Runtime(decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)

function view(data) {
  const pinned = {
    block: 100, at: async () => pinned, blockTime: async () => 12,
    queryMap: async ([, item]) => item === 'RevealedCommitments' ? [['hotkey', [[data, 95]]]] : [],
    balance: amount => Balance.fromRao(amount),
  }
  return pinned
}

test('revealed commitment reads preserve raw plaintext across transport representations', async () => {
  // The second payload also happens to be a valid SCALE vector: inference
  // would silently remove its opening parenthesis, so raw must be the default.
  for (const text of ['affine1|model|https://example.test', '(0123456789', 'hello', '🌐模型', '0x6869', '0xhello', '']) {
    const bytes = Buffer.from(text)
    const decoded = runtime.decode('Vec<u8>', runtime.encode('Vec<u8>', bytes))
    for (const data of [decoded, Uint8Array.from(bytes), [...bytes]]) {
      const records = await fetchCommitments(view(data), 2)
      assert.equal(records.hotkey.value, text)
      assert.deepEqual(records.hotkey.revealed, [[95, text]])
      assert.equal(records.hotkey.status, 'revealed')
    }
  }
  const binary = runtime.decode('Vec<u8>', runtime.encode('Vec<u8>', Uint8Array.from([255, 254])))
  assert.equal((await fetchCommitments(view(binary), 2)).hotkey.value, '0xfffe')
})

test('legacy commitment decoding requires an explicit canonical SCALE frame', async () => {
  const records = await fetchCommitments(view('0x086869'), 2, { revealedEncoding: 'scale' })
  assert.equal(records.hotkey.value, 'hi')
  assert.equal((await fetchCommitments(view('\bhi'), 2, { revealedEncoding: 'scale' })).hotkey.value, 'hi')
  assert.equal(decodeRevealedCommitment(Uint8Array.from([8, 104, 105])), '\bhi')
  assert.equal(decodeRevealedCommitment(Uint8Array.from([8, 104, 105]), 'scale'), 'hi')
  for (const data of ['0x0868', '0x08686900', '0x09006869', '0x']) {
    await assert.rejects(fetchCommitments(view(data), 2, { revealedEncoding: 'scale' }), /compact|length|SCALE/)
  }
  assert.equal(decodeRevealedCommitment(Uint8Array.from([0xff, 0xfe])), '0xfffe')
  assert.equal(decodeRevealedCommitment(Uint8Array.from([8, 0xff, 0xfe]), 'scale'), '0xfffe')
})

test('browser WASM preserves decoded plaintext and explicitly decodes legacy commitments', async () => {
  const fs = require('node:fs/promises'), path = require('node:path'), { pathToFileURL } = require('node:url')
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  const decode = input => JSON.parse(binding.policyEvaluate('validation.commitment.decode', JSON.stringify(input))).value
  for (const text of ['affine1|model', '(0123456789', '0x6869', '0xhello', '🌐模型', '0xfffe']) assert.equal(decode({ text }), text)
  for (const text of ['\bhi', '0x086869']) assert.equal(decode({ text, encoding: 'scale' }), 'hi')
})
