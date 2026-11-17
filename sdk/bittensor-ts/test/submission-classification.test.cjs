'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const browser = require('../dist/browser')
const { WorkflowClient } = require('../dist/node-workflows')
const { ProtectedWalletClient } = require('../dist/protected-transactions')
const { markSubmissionNotSent, isDefiniteSubmissionRejection, SubmissionNotSentError } = require('../dist/transport')
const { buildIntent } = require('../dist/intents')
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const signer = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, sign: () => Buffer.alloc(64) }
const intent = () => buildIntent('transfer', { dest_ss58: bob.ss58Address, amount_tao: '1' })
const call = { pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: bob.ss58Address, value: 1n } }
const hash = '0x' + '11'.repeat(32)
const rejected = async (action, expected, definite) => assert.rejects(action, error => { assert.equal(error, expected); assert.equal(isDefiniteSubmissionRejection(error), definite); return true })

test('known-unsent tagging preserves frozen error identity, subclass and message', () => {
  const error = Object.freeze(new RangeError('nonce does not fit'))
  assert.equal(isDefiniteSubmissionRejection(error), false)
  assert.equal(markSubmissionNotSent(error), error)
  assert.equal(isDefiniteSubmissionRejection(error), true)
  assert(error instanceof RangeError)
  assert.equal(error.message, 'nonce does not fit')
  assert(markSubmissionNotSent('aborted') instanceof SubmissionNotSentError)
})

test('Node workflow marks planning errors but never classifies delegated synchronous or asynchronous failures as unsent', async () => {
  const client = { runtimeAt: async () => ({ ss58Format: 42 }), submit: async () => ({ success: true, events: [] }) }
  const flow = new WorkflowClient(client), failure = new Error('fee cap declined')
  flow.plan = async () => { throw failure }
  await rejected(() => flow.execute(intent(), signer), failure, true)
  flow.plan = async () => ({ signerAddress: alice.ss58Address, ok: true, violations: [], shielded: false, call, intent: { signerRole: 'coldkey', extras: {} } })
  for (const asyncFailure of [false, true]) {
    const error = new Error('submission callback lost connection')
    client.submit = asyncFailure ? async () => { throw error } : () => { throw error }
    await rejected(() => flow.execute(intent(), signer), error, false)
  }
})

test('Node shielded local signing errors are unsent, while the signed submission handoff is unknown', async () => {
  const client = { blockHash: async () => hash, query: async () => Buffer.alloc(1184), composeCall: async () => Buffer.from([1]) }
  const flow = new WorkflowClient(client), error = new Error('signer refused')
  client.signExtrinsic = async () => { throw error }
  await rejected(() => flow.submitShieldedCall(call, signer, alice.ss58Address, { nonce: 1 }), error, true)
  // Return small complete envelopes; cryptographic encryption still executes.
  client.signExtrinsic = async () => ({ bytes: Buffer.from([1, 2, 3]), hash: Buffer.alloc(32, 4) })
  const handed = new Error('socket closed after submit')
  client.submitSigned = () => { throw handed }
  await rejected(() => flow.submitShieldedCall(call, signer, alice.ss58Address, { nonce: 1 }), handed, false)
})

function protectedFixture() {
  const key = { publicKey: alice.publicKey, ss58Address: alice.ss58Address, cryptoType: 8 }
  const adapter = { submitBytes: async () => ({ success: true, events: [] }) }, session = new ProtectedWalletClient(adapter, key)
  const authority = { block: hash, nonce: 0n, generation: 0n, key, descriptor: new Uint8Array(), commitment: new Uint8Array() }
  session.selectAuthority = async () => authority
  session.prepareWorkflow = async () => ({ ok: true, violations: [], call, shielded: false, intent: { extras: {} } })
  session.signed = async () => ({ bytes: Uint8Array.of(1), hash: Uint8Array.of(2) })
  return { adapter, session }
}

test('protected workflow marks authority/planning/local signing refusals without changing errors', async () => {
  for (const method of ['selectAuthority', 'prepareWorkflow', 'signed']) {
    const { session } = protectedFixture(), error = new Error(`${method} refused`)
    session[method] = async () => { throw error }
    await rejected(() => session.execute(intent()), error, true)
  }
})

test('protected workflow does not mark synchronous or asynchronous adapter submission errors', async () => {
  for (const asynchronous of [false, true]) {
    const { session, adapter } = protectedFixture(), error = new Error('adapter may have sent')
    adapter.submitBytes = asynchronous ? async () => { throw error } : () => { throw error }
    await rejected(() => session.execute(intent()), error, false)
  }
})

async function browserFixture() {
  browser.configureBrowserWasm(async () => ({ ss58Decode: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic }))
  await browser.initBrowser()
  const client = new browser.Client('local')
  client.planForSigner = async () => ({ prepared: { feePayer: alice.ss58Address, mevShieldDefault: false, mevShieldRequired: false, extras: {} }, call, violations: [], blockHash: hash, nonce: 0, signingOptions: {} })
  client.prepareRaw = async () => ({})
  client.signPrepared = async () => ({ bytes: Uint8Array.of(1), hash: Uint8Array.of(2) })
  return client
}

test('browser marks planning and signing refusals while preserving exact error objects', async () => {
  for (const method of ['planForSigner', 'prepareRaw', 'signPrepared']) {
    const client = await browserFixture(), error = new Error(`${method} rejected`)
    client[method] = async () => { throw error }
    try { await rejected(() => client.execute(intent(), signer, { nonce: 0 }), error, true) }
    finally { await client.close() }
  }
})

test('browser leaves synchronous and asynchronous broadcast failures uncertain', async () => {
  for (const asynchronous of [false, true]) {
    const client = await browserFixture(), error = new Error('broadcast may have happened')
    client.broadcastExtrinsic = asynchronous ? async () => { throw error } : () => { throw error }
    try { await rejected(() => client.execute(intent(), signer, { nonce: 0 }), error, false) }
    finally { await client.close() }
  }
})
