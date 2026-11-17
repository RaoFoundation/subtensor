'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
const { execFile } = require('node:child_process')
const { promisify } = require('node:util')
const sdk = require('../dist')
const browser = require('../dist/browser')
const HASH = '0x' + '11'.repeat(32), GENESIS = '0x' + '22'.repeat(32), PARENT = '0x' + '33'.repeat(32)
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const crypto = { accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256, fromDescriptor: sdk.Keypair.fromDescriptor }

test('file workflow journals serialize concurrent processes and preserve corrupt data', async t => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-proxy-journal-'))
  t.after(() => fs.rm(dir, { recursive: true, force: true }))
  const store = new sdk.FileJsonStore(dir)
  await store.write('protected_setups', { count: 0 })
  const program = `const {FileJsonStore}=require(process.argv[1]);const store=new FileJsonStore(process.argv[2]);(async()=>{for(let i=0;i<8;i++)await store.update('protected_setups',v=>({value:{count:v.count+1},result:null}))})().catch(e=>{console.error(e);process.exitCode=1})`
  await Promise.all(Array.from({ length: 3 }, () => promisify(execFile)(process.execPath, ['-e', program, path.resolve(__dirname, '../dist'), dir])))
  assert.deepEqual(await store.read('protected_setups'), { count: 24 })
  const file = store.path('protected')
  await fs.writeFile(file, '{broken')
  await assert.rejects(store.update('protected', () => ({ value: [], result: null })), SyntaxError)
  assert.equal(await fs.readFile(file, 'utf8'), '{broken')
  assert(!(await fs.readdir(dir)).some(name => name.endsWith('.lock') || name.endsWith('.tmp')))
  if (process.platform !== 'win32') assert.equal((await fs.stat(store.path('protected_setups'))).mode & 0o777, 0o600)
})

test('memory journal updates are atomic and detach stored and returned values', async () => {
  const store = new sdk.MemoryJsonStore()
  await Promise.all(Array.from({ length: 30 }, () => store.update('protected', current => ({ value: { count: (current?.count ?? 0) + 1 }, result: null }))))
  const value = await store.read('protected'); value.count = -1
  assert.equal((await store.read('protected')).count, 30)
  await assert.rejects(store.update('protected', () => { throw new Error('abort') }), /abort/)
  assert.equal((await store.read('protected')).count, 30)
})

function fixture({ registered = null, publicKey = undefined } = {}) {
  const calls = [], envelopes = [], proof = Uint8Array.of(7, 8, 9, 10)
  const runtime = {
    specVersion: 491,
    metadataIr: () => ({ pallets: [{ name: 'HashedAccounts', storage: ['MlDsaAccounts', 'Accounts'] }] }),
    constant: (p, s) => `${p}.${s}`,
    composeCall: () => Uint8Array.of(5, 3),
    decodeCall: () => ({ call_module: 'Balances', call_function: 'transfer_keep_alive', call_args: [] }),
    encodeSignedExtrinsic: (data, account, signature, type, params) => { envelopes.push({ data, account, signature, type, params }); return { bytes: Uint8Array.of(type, ...signature) } },
    storageEntry: () => ({ valueType: 9, defaultBytes: Uint8Array.of(0) }), storageKey: () => Uint8Array.of(1),
    decode: () => [{ module_id: 'Proxy', event_id: 'PureCreated', attributes: { pure: bob.ss58Address }, extrinsic_idx: 2 }],
  }
  const client = {
    blockHash: async () => HASH, genesisHash: async () => GENESIS, blockNumber: async hash => { assert.equal(hash, HASH); return 50 }, blockTime: async hash => { assert.equal(hash, HASH); return 12 },
    runtimeAt: async hash => { calls.push(['runtime', hash]); assert([HASH, PARENT].includes(hash)); return runtime },
    query: async (pallet, item, params, hash) => { calls.push([pallet, item, params, hash]); assert.equal(hash, HASH); return item === 'MlDsaAccounts' ? registered : item === 'Accounts' ? null : { nonce: 17, data: { free: 100n } } },
    queryMap: async (_p, _i, _a, hash) => { assert.equal(hash, HASH); return [] },
    rpc: async (method, params) => { calls.push([method, params]); if (method === 'chain_getHeader') return { number: '0x32', parentHash: PARENT }; assert.equal(params[1], HASH); return method === 'payment_queryInfo' ? { partialFee: '9007199254740993' } : '0x00' },
  }
  const backend = sdk.createProtectedProxyBackend(client, {
    reader: async hash => { assert.equal(hash, HASH); return {} }, submit: async () => { throw new Error('inspection must not submit') },
    publicKeyFor: async () => publicKey,
    crypto: { ...crypto, fromDescriptor: () => ({ ss58Address: alice.ss58Address, cryptoType: 9, dummyAuthorization: () => proof }) },
  })
  return { backend, runtime, calls, envelopes, proof }
}

test('funding quotes use one pinned state and registered authorization without signing', async () => {
  const descriptor = Uint8Array.from({ length: 66 }, (_, i) => i === 0 ? 2 : i === 1 ? 9 : 1)
  const f = fixture({ registered: { descriptor } }), view = await f.backend.snapshot()
  assert.equal(await view.fee({ pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: bob.ss58Address, value: 1n } }, alice.ss58Address), 9007199254740993n)
  assert.equal(f.envelopes[0].type, 9)
  assert.deepEqual(f.envelopes[0].signature, f.proof)
  assert.equal(f.envelopes[0].params.nonce, 17n)
  assert.equal(f.envelopes[0].params.era.current, 50)
  assert.equal(f.calls.filter(([method]) => method === 'payment_queryInfo').length, 1)
  assert(!f.calls.some(([method]) => method === 'author_submitExtrinsic' || method === 'system_accountNextIndex'))
})

test('funding rejects mismatched public identities and missing protected proof sizing', async () => {
  for (const [publicKey, message] of [
    [{ ss58Address: bob.ss58Address, cryptoType: 1 }, /does not match/],
    [{ ss58Address: alice.ss58Address, cryptoType: 9 }, /authorization descriptor/],
  ]) {
    const f = fixture({ publicKey }), view = await f.backend.snapshot()
    await assert.rejects(view.fee({ pallet: 'Proxy', call: 'announce', params: {} }, alice.ss58Address), message)
    assert.equal(f.envelopes.length, 0)
  }
})

test('pure creation provenance decodes events with the runtime that executed the block', async () => {
  const f = fixture(), events = await f.backend.events(HASH)
  assert.equal(events[0].event_id, 'PureCreated')
  assert.equal(events[0].extrinsic_idx, 2)
  assert(f.calls.some(([name, hash]) => name === 'runtime' && hash === PARENT))
  assert(f.calls.some(([name, params]) => name === 'state_getStorage' && params[1] === HASH))
})

test('multisig provenance decodes the original extrinsic with inclusion-block metadata', async () => {
  const calls = [], decoded = { call_module: 'Proxy', call_function: 'announce', call_args: [] }
  const backend = sdk.createProtectedProxyBackend({
    blockHash: async height => { assert.equal(height, 50); return HASH },
    rpc: async (method, params) => { assert.equal(method, 'chain_getBlock'); assert.deepEqual(params, [HASH]); return { block: { header: { number: '0x32', parentHash: PARENT }, extrinsics: ['0x0102'] } } },
    runtimeAt: async hash => { calls.push(hash); return { decodeExtrinsic: bytes => { assert.deepEqual([...bytes], [1, 2]); return { call: decoded } } } },
  }, { crypto, reader: async () => ({}), submit: async () => { throw new Error('must not submit') } })
  assert.deepEqual(await backend.extrinsicCall(50, 0), decoded)
  assert.deepEqual(calls, [PARENT])
  assert.equal(await backend.extrinsicCall(HASH, 1), null)
  await assert.rejects(backend.extrinsicCall(HASH, -1), /invalid extrinsic index/)
})

test('controller fee quotes encode real nested RuntimeCalls without accessing a signer', async () => {
  const golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const quoted = []
  const backend = sdk.createProtectedProxyBackend({
    blockHash: async () => HASH, genesisHash: async () => GENESIS, blockNumber: async () => 50, blockTime: async () => 12,
    runtimeAt: async () => runtime, query: async () => ({ nonce: 17 }),
    rpc: async (method, [bytes, block]) => { assert.equal(method, 'payment_queryInfo'); assert.equal(block, HASH); quoted.push(runtime.decodeExtrinsic(Buffer.from(bytes.slice(2), 'hex'))); return { partialFee: '1200' } },
  }, { crypto, reader: async () => ({}), submit: async () => { throw new Error('must not submit') } })
  const call = { pallet: 'Proxy', call: 'proxy_announced', params: { delegate: alice.ss58Address, real: bob.ss58Address, force_proxy_type: 'Any', call: { pallet: 'Utility', call: 'batch_all', params: { calls: [{ pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: alice.ss58Address, value: 100n } }] } } } }
  assert.equal(await (await backend.snapshot()).fee(call, alice.ss58Address), 1200n)
  assert.equal(quoted[0].nonce, 17)
  assert.equal(quoted[0].call.call_module, 'Proxy')
  assert.equal(quoted[0].call.call_function, 'proxy_announced')
  const inner = quoted[0].call.call_args.find(arg => arg.name === 'call').value
  assert.equal(inner.call_function, 'batch_all')
  assert.equal(inner.call_args[0].value[0].call_function, 'transfer_keep_alive')
})

test('hash-only uncertain receipts recover from bounded finalized history', async () => {
  const transaction = '0x' + sdk.blake2_256(Uint8Array.of(1, 2)).toString('hex'), reads = []
  const backend = sdk.createProtectedProxyBackend({
    finalizedHead: async () => HASH,
    rpc: async (method, [cursor]) => { assert.equal(method, 'chain_getBlock'); reads.push(cursor); return { block: { header: { number: cursor === HASH ? '0x3' : '0x2', parentHash: cursor === HASH ? PARENT : GENESIS }, extrinsics: cursor === HASH ? [] : ['0x0102'] } } },
    findExtrinsic: async (hash, block, finalized) => { assert.equal(hash, transaction); assert.equal(block, PARENT); assert.equal(finalized, true); return { success: true, status: 'finalized', blockHash: block, extrinsicHash: hash, events: [] } },
  }, { crypto, reader: async () => ({}), submit: async () => {}, receiptSearchBlocks: 2 })
  assert.equal((await backend.receipt(transaction)).extrinsicHash, transaction)
  assert.deepEqual(reads, [HASH, PARENT])
  reads.length = 0
  assert.equal(await backend.receipt('0x' + 'ff'.repeat(32)), null)
  assert.equal(reads.length, 2, 'absence stays unknown after the bounded lookup')
})

test('explicit receipt block lookup recovers older work without scanning', async () => {
  const transaction = '0x' + 'aa'.repeat(32)
  const backend = sdk.createProtectedProxyBackend({
    rpc: async () => { throw new Error('block hint must avoid scanning') },
    findExtrinsic: async (hash, block) => { assert.equal(hash, transaction); assert.equal(block, PARENT); return { success: false, status: 'failed', blockHash: block, events: [] } },
  }, { crypto, reader: async () => ({}), submit: async () => {}, receiptBlockFor: hash => { assert.equal(hash, transaction); return PARENT } })
  assert.equal((await backend.receipt(transaction)).success, false)
  await assert.rejects(backend.receipt(transaction, 'bad block'), /invalid historical block hash/)
})

test('historical provenance rejects malformed heights and mismatched requested blocks', async () => {
  let number
  const backend = sdk.createProtectedProxyBackend({
    blockHash: async () => HASH,
    rpc: async () => ({ number, parentHash: PARENT }),
    runtimeAt: async () => { throw new Error('invalid headers must be rejected before decoding') },
  }, { crypto, reader: async () => ({}), submit: async () => {} })
  for (number of [undefined, null, '', false, -1, 1.5, 'junk']) await assert.rejects(backend.events(HASH), /block execution metadata unavailable/)
  number = '0x32'
  await assert.rejects(backend.events(51), /block execution metadata unavailable/)
})

test('receipt adaptation retains nested failures and multisig approval-only state', () => {
  const approval = sdk.protectedProxyReceipt({ status: 'finalized', success: true, events: [{ module_id: 'Multisig', event_id: 'NewMultisig', attributes: {} }] })
  assert.equal(approval.awaitingApproval, true)
  const failed = sdk.protectedProxyReceipt({ status: 'finalized', success: true, events: [{ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Err: 'BadOrigin' } } }] })
  assert.equal(failed.success, false)
})

test('Node and browser expose isolated protected-account sessions without reading or unlocking', async t => {
  for (const library of [sdk, browser]) {
    const client = new library.Client('local')
    t.after(() => client.close())
    client.rpc = async () => { throw new Error('constructing a session must not contact the chain') }
    const session = client.protectedAccount({ controllerWallet: 'main', store: new library.MemoryJsonStore(), signerFor: () => { throw new Error('must not unlock') } })
    assert(session instanceof library.ProtectedAccountSession)
    assert.equal(session.controllerWallet, 'main')
    for (const name of ['planSetup', 'setup', 'resumeSetup', 'inspect', 'submit', 'execute', 'cancel', 'updateSettings', 'rotate', 'finalizeRotation']) assert.equal(typeof session[name], 'function', name)
  }
})
