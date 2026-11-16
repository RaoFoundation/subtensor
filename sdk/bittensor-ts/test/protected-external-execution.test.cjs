const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { SubmissionUncertainError } = require('../dist/transport')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const controller = sdk.Keypair.fromUri('//Alice').ss58Address, pure = sdk.Keypair.fromUri('//Bob').ss58Address, recipient = sdk.Keypair.fromUri('//Charlie').ss58Address
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const blockHash = block => '0x' + block.toString(16).padStart(64, '0'), genesis = blockHash(9999)

async function fixture(surface = sdk) {
  const record = { controller_wallet: 'alice', controller_ss58: controller, protected_ss58: pure, genesis_hash: genesis, delay_blocks: 10 }
  const state = { block: 100, announcements: [], submits: 0, receipts: new Map(), recipientRao: 0n, pureRao: 10_000_000_000n, beforeAction: null, loseResponse: false, failAction: false }
  const json = new surface.MemoryJsonStore(), journal = new surface.ProxyAnnouncementStore(json, address => hex(sdk.publicKeyFromSs58(address)))
  const compose = call => surface.encodeIntentCall(call, async (p, c, args) => runtime.composeCall(p, c, args))
  const query = async (p, item) => {
    if (p === 'Proxy' && item === 'Proxies') return [[{ delegate: controller, proxy_type: 'Any', delay: 10 }], 0n]
    if (p === 'Proxy' && item === 'Announcements') return [structuredClone(state.announcements), 0n]
    if (p === 'System' && item === 'Account') return { data: { free: state.pureRao } }
    throw Error(`unexpected query ${p}.${item}`)
  }
  const reader = { query, queryMap: async () => [], runtimeCall: async () => null, composeCall: compose, decodeCall: bytes => runtime.decodeCall(bytes), hashCallData: sdk.blake2_256, specVersion: () => 419, decodeAddress: sdk.publicKeyFromSs58, blockHash: async () => blockHash(state.block), blockNumber: async () => state.block }
  function externalExecute(row) {
    assert(state.announcements.some(item => item.call_hash === row.call_hash && item.height + 10 <= state.block))
    const decoded = runtime.decodeCall(Buffer.from(row.call_data.slice(2), 'hex'))
    assert.equal(decoded.call_module, 'Balances')
    const amount = BigInt(decoded.call_args.find(arg => arg.name === 'value').value)
    state.pureRao -= amount; state.recipientRao += amount
    state.announcements = state.announcements.filter(item => item.call_hash !== row.call_hash || item.height + 10 > state.block)
  }
  const backend = {
    accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256,
    snapshot: async () => ({ genesisHash: genesis, hash: blockHash(state.block), block: state.block, specVersion: 419, blockTime: 12, query, queryMap: reader.queryMap, constant: async () => 0n, prepare: (intent, context) => intent.prepare(reader, context), compose, decodeCall: reader.decodeCall, hashCallData: sdk.blake2_256, fee: async () => 1n }),
    receipt: async hash => state.receipts.get(hash) ?? null,
    submit: async intent => {
      state.submits++; state.block++
      const receipt = { status: 'finalized', success: true, blockNumber: state.block, blockHash: blockHash(state.block), extrinsicHash: blockHash(state.submits + 1000), events: [] }
      if (intent.op === 'announce_proxy_call') state.announcements.push({ real: pure, call_hash: intent.args.call_hash, height: state.block })
      else {
        if (state.beforeAction) { const hook = state.beforeAction; state.beforeAction = null; hook() }
        if (state.failAction) receipt.success = false
        else if (intent.op === 'remove_proxy_announcement') state.announcements = state.announcements.filter(item => item.call_hash !== intent.args.call_hash)
        else if (intent.op === 'execute_proxy_announced') {
          externalExecute({ call_hash: intent.args.expected_call_hash, call_data: intent.args.announced_call_data })
          receipt.events.push({ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Ok: null } } })
        } else throw Error(`unexpected action ${intent.op}`)
      }
      state.receipts.set(receipt.extrinsicHash, receipt)
      if (state.loseResponse) throw new SubmissionUncertainError(receipt.extrinsicHash, Error('response lost'))
      return receipt
    },
  }
  const engine = new surface.ProtectedAnnouncements(backend, journal)
  const announce = () => engine.announce(record, surface.buildIntent('transfer', { dest_ss58: recipient, amount_tao: '1' }))
  const original = (await announce()).row
  return { state, record, journal, engine, announce, original, externalExecute }
}

// PROTECTED_REGRESSION: permissionless execution must not let a younger identical payment transfer funds again without repeat approval; changes require explicit human approval.
test('external execution retains the replay guard while a younger duplicate survives', async () => {
  const f = await fixture()
  f.state.block = 111
  f.state.announcements.push({ ...f.state.announcements[0], height: 110 })
  const younger = (await f.engine.reconcile(f.record)).find(row => row.announced_at_block === 110)
  f.externalExecute(f.original)
  f.state.block = 121
  await assert.rejects(f.engine.execute(f.record, younger.index), /may have executed.*allowRepeat/)
  const saved = await f.journal.getByIndex('alice', f.original.index, { genesisHash: genesis })
  assert.equal(saved.status, 'gone'); assert.equal(saved.execution_uncertain, true)
  assert.equal(f.state.recipientRao, 1_000_000_000n); assert.equal(f.state.pureRao, 9_000_000_000n)
  await assert.rejects(f.journal.claimAction('alice', younger.index, 'execute', { genesisHash: genesis }), /may have executed.*allowRepeat/)
  assert.equal((await f.engine.execute(f.record, younger.index, { allowRepeat: true })).completed, true)
  assert.equal(f.state.recipientRao, 2_000_000_000n)
})

// PROTECTED_REGRESSION: a successful hash-based cancellation cannot hide an earlier payment and authorize replay; changes require explicit human approval.
test('direct and receipt-recovered cancellation preserve uncertainty about the original payment', async () => {
  for (const loseResponse of [false, true]) {
    const f = await fixture()
    f.state.block = 111
    f.state.beforeAction = () => {
      f.externalExecute(f.original)
      // A replacement with the same hash arrives before remove_announcement.
      f.state.announcements.push({ real: pure, call_hash: f.original.call_hash, height: f.state.block })
    }
    f.state.loseResponse = loseResponse
    if (loseResponse) await assert.rejects(f.engine.cancel(f.record, f.original.index), /submission uncertain/)
    else assert.equal((await f.engine.cancel(f.record, f.original.index)).completed, true)
    f.state.loseResponse = false
    await f.engine.reconcile(f.record)
    const saved = await f.journal.getByIndex('alice', f.original.index, { genesisHash: genesis })
    assert.equal(saved.status, 'cancelled'); assert.equal(saved.execution_uncertain, true)
    const replacement = (await f.announce()).row
    f.state.block += 10
    await assert.rejects(f.engine.execute(f.record, replacement.index), /may have executed.*allowRepeat/)
    assert.equal(f.state.recipientRao, 1_000_000_000n); assert.equal(f.state.pureRao, 9_000_000_000n)
  }
})

// PROTECTED_REGRESSION: a failed local action after an external payment must not erase its replay guard during receipt reconciliation; changes require explicit human approval.
test('failed action receipts retain uncertainty when the original announcement is absent', async () => {
  const f = await fixture()
  f.state.block = 111; f.state.failAction = true; f.state.loseResponse = true
  f.state.beforeAction = () => f.externalExecute(f.original)
  await assert.rejects(f.engine.execute(f.record, f.original.index), /submission uncertain/)
  f.state.failAction = false; f.state.loseResponse = false
  await f.engine.reconcile(f.record)
  const saved = await f.journal.getByIndex('alice', f.original.index, { genesisHash: genesis })
  assert.equal(saved.status, 'failed'); assert.equal(saved.execution_uncertain, true)
  const replacement = (await f.announce()).row
  f.state.block += 10
  await assert.rejects(f.engine.execute(f.record, replacement.index), /may have executed.*allowRepeat/)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
})

// PROTECTED_REGRESSION: browser protected payments must retain the same absence replay guard as Node; changes require explicit human approval.
test('browser shared policy also blocks a younger payment after external execution', async () => {
  const surface = await import('../dist/browser.mjs')
  const fs = require('node:fs/promises'), path = require('node:path'), { pathToFileURL } = require('node:url')
  await surface.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports); instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default; return module
  })
  const f = await fixture(surface)
  f.state.block = 111
  f.state.announcements.push({ ...f.state.announcements[0], height: 110 })
  const younger = (await f.engine.reconcile(f.record)).find(row => row.announced_at_block === 110)
  f.externalExecute(f.original); f.state.block = 121
  await assert.rejects(f.engine.execute(f.record, younger.index), /may have executed.*allowRepeat/)
  assert.equal(f.state.recipientRao, 1_000_000_000n)
})
