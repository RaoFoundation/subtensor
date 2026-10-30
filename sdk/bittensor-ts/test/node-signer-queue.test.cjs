const test = require('node:test')
const assert = require('node:assert/strict')
const { Client } = require('../dist/client.js')
const { Keypair } = require('../dist/keys.js')
const { Runtime, decodeOptionalOpaqueMetadata } = require('../dist/runtime.js')
const { runSignerQueue, signerQueueOptions } = require('../dist/signer-queue.js')
const golden = require('../../python/tests/fixtures/golden.json')
const runtime = new Runtime(decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const alice = Keypair.fromUri('//Alice'), bob = Keypair.fromUri('//Bob')
const delay = () => new Promise(resolve => setImmediate(resolve))

function fixture() {
  const client = new Client('local'), order = []
  const call = runtime.composeCall('System', 'remark', { remark: Buffer.from('queue') })
  const params = { era: '00', nonce: 0, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32) }
  const plan = { publicKey: alice.publicKey, cryptoType: 1, callData: call, payload: runtime.signaturePayload(call, params), ...runtime.signaturePayloadParts(params), txParams: params, nonce: 0 }
  const signed = runtime.encodeSignedExtrinsic(call, alice.publicKey, alice.sign(plan.payload), 1, params)
  const outcome = { success: true, message: 'Submitted', extrinsicHash: '0x' + '12'.repeat(32), events: [] }
  const native = {
    assembleExternal: async () => ({ bytes: signed.bytes, hash: '0x' + signed.hash.toString('hex') }),
    submitExternal: async () => { order.push('submit'); return outcome },
    submitEncoded: async () => { order.push('encoded'); return outcome },
  }
  client.runtimeAt = async () => runtime
  client.requireNativeClient = async () => native
  client.nativeSigningPlan = async (_call, signer) => {
    const resolved = await client.resolveSigner(signer, runtime)
    order.push('prepare')
    return { native, runtime, resolved, plan }
  }
  return { client, call, signed, order }
}

test('Node direct, detached and nonce-reserving APIs respect a held workflow signer queue', async () => {
  const { client, call, signed, order } = fixture()
  let release, entered
  const enteredPromise = new Promise(resolve => { entered = resolve })
  const held = runSignerQueue(client, alice.ss58Address, async permit => {
    await client.signExtrinsic(call, alice, signerQueueOptions({ allowRawCall: true, nonce: 1 }, permit))
    entered()
    await new Promise(resolve => { release = resolve })
    await client.submitSigned(signed, alice.ss58Address, signerQueueOptions({}, permit))
  })
  await enteredPromise
  const direct = client.submit(call, alice, { allowRawCall: true })
  const detached = client.submitSigned(signed)
  const preparing = client.prepareCall(call, alice, { allowRawCall: true })
  const signing = client.signExtrinsic(call, alice, { allowRawCall: true })
  const watched = await client.watchSigned(signed)
  await delay()
  assert.deepEqual(order, ['prepare'])
  release()
  await Promise.all([held, direct, detached, preparing, signing, watched.result])
  assert.equal(order[1], 'encoded', 'the workflow carrier is broadcast before any unrelated transaction')
  assert.equal(order.filter(item => item === 'encoded').length, 3)
  assert.equal(order.filter(item => item === 'submit').length, 1)
  await assert.rejects(client.submitSigned(signed, bob.ss58Address), /differs from signerAddress/)
  await client.close()
})

test('Node aborts before signing when an extension changes accounts while waiting for its queue', async () => {
  const { client, call, order } = fixture()
  let release, entered, selected = alice, reads = 0, signatures = 0
  const enteredPromise = new Promise(resolve => { entered = resolve })
  const held = runSignerQueue(client, alice.ss58Address, async () => { entered(); await new Promise(resolve => { release = resolve }) })
  await enteredPromise
  const signer = { getAccount: async () => { reads++; return { address: selected.ss58Address, publicKey: selected.publicKey, cryptoType: 1 } }, sign: bytes => { signatures++; return selected.sign(bytes) } }
  const submitted = client.submit(call, signer, { allowRawCall: true })
  await delay()
  assert.equal(reads, 1)
  selected = bob
  release()
  await assert.rejects(submitted, /account changed/)
  await held
  assert.equal(signatures, 0)
  assert.deepEqual(order, [])
  await client.close()
})

test('Node cancellation while queued never prepares, signs or broadcasts after the lock is released', async () => {
  const { client, call, order } = fixture(), controller = new AbortController()
  let release, entered
  const enteredPromise = new Promise(resolve => { entered = resolve })
  const held = runSignerQueue(client, alice.ss58Address, async () => { entered(); await new Promise(resolve => { release = resolve }) })
  await enteredPromise
  const submitted = client.submit(call, alice, { allowRawCall: true, signal: controller.signal })
  await delay()
  controller.abort(new Error('cancelled while queued'))
  release()
  await assert.rejects(submitted, /cancelled while queued/)
  await held
  assert.deepEqual(order, [])
  await client.close()
})

test('Node nonce-range bridge validates range size and never narrows an unsafe native nonce', async () => {
  const { NativeChainClient } = require('../dist/transaction.js')
  const client = Object.create(NativeChainClient.prototype), calls = []
  client.native = { reserveNonces: async (address, count) => { calls.push([address, count]); return 11n } }
  assert.equal(await client.reserveNonces(alice.ss58Address, 2), 11)
  assert.deepEqual(calls, [[alice.ss58Address, 2n]])
  for (const count of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) await assert.rejects(client.reserveNonces(alice.ss58Address, count), /positive safe integer/)
  client.native.reserveNonces = async () => BigInt(Number.MAX_SAFE_INTEGER)
  await assert.rejects(client.reserveNonces(alice.ss58Address, 2), /safe integer range/)
  client.native.invalidateNonce = async address => { calls.push(['reset', address]) }
  await client.invalidateNonce(alice.ss58Address)
  assert.deepEqual(calls.at(-1), ['reset', alice.ss58Address])
})

test('Node releases automatic nonce on signing failure without broadcasting', async () => {
  const { client, call, order } = fixture(), native = await client.requireNativeClient(), released = []
  native.releaseNonces = async (...args) => released.push(args)
  client.signRustPlan = async () => { throw new Error('user declined') }
  await assert.rejects(client.submit(call, alice, { allowRawCall: true }), /user declined/)
  assert.deepEqual(released, [[alice.ss58Address, 0, 1]])
  assert(!order.includes('submit'))
  await assert.rejects(client.submit(call, alice, { allowRawCall: true, nonce: 9 }), /user declined/)
  assert.equal(released.length, 1, 'explicit nonces do not own reservations')
  await client.close()
})

test('Node exposes uncertain native outcomes with their hash and no rejection', async () => {
  const { client, call } = fixture(), native = await client.requireNativeClient()
  native.submitExternal = async () => ({ success: false, message: 'lost response', extrinsicHash: '0xabc', events: [], data: { submission_uncertain: true } })
  const result = await client.submit(call, alice, { allowRawCall: true })
  assert.equal(result.status, 'unknown'); assert.equal(result.success, undefined)
  assert.equal(result.chainError, undefined); assert.equal(result.extrinsicHash, '0xabc')
  assert.equal(result.data.submission_uncertain, true)
  await client.close()
})

test('detached Node signing releases an automatic nonce if the signer fails', async () => {
  const {client,call}=fixture(), native=await client.requireNativeClient(), released=[]
  native.releaseNonces=async(...args)=>released.push(args)
  client.signRustPlan=async()=>{throw new Error('user declined')}
  await assert.rejects(client.signExtrinsic(call,alice,{allowRawCall:true}),/user declined/)
  assert.deepEqual(released,[[alice.ss58Address,0n,1]])
  await client.close()
})

test('Node rejects invalid submission options before reserving or signing', async () => {
  const {client,call,order}=fixture()
  await assert.rejects(client.submit(call,alice,{allowRawCall:true,timeoutMs:-1}),/timeoutMs/)
  assert.deepEqual(order,[])
  await client.close()
})
