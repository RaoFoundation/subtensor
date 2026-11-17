'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { buildIntent, encodeIntentCall } = require('../dist/intents')
const { ProxyAnnouncementStore } = require('../dist/announcement-store')
const { ProtectedAnnouncements } = require('../dist/protected-announcements')
const { ProtectedAccountStore } = require('../dist/protected-account-config')
const { ProtectedAccountSession } = require('../dist/protected-account-lifecycle')
const { ShieldedTrackingError } = require('../dist/shielded-receipt')
const { SubmissionUncertainError } = require('../dist/transport')
const golden = require('../../python/tests/fixtures/golden.json')

const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
const controller = sdk.Keypair.fromUri('//Alice').ss58Address
const pure = sdk.Keypair.fromUri('//Bob').ss58Address
const recipient = sdk.Keypair.fromUri('//Charlie').ss58Address
const genesis = '0x' + 'aa'.repeat(32), blockHash = '0x' + 'bb'.repeat(32)
const innerHash = '0x' + 'cc'.repeat(32), carrierHash = '0x' + 'dd'.repeat(32)
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const transfer = () => buildIntent('transfer', { dest_ss58: recipient, amount_tao: '1' })
const trackingError = () => new ShieldedTrackingError(innerHash, {
  status: 'finalized', success: true, message: 'Carrier included', extrinsicHash: carrierHash,
  blockHash, blockNumber: 100, finalized: true, events: [],
}, Error('tracking connection lost'))

function fixture() {
  const state = { block: 100, submissions: 0, announcements: [], failure: undefined }
  const values = new Map()
  const store = {
    read: async key => values.has(key) ? JSON.parse(values.get(key)) : undefined,
    write: async (key, value) => values.set(key, JSON.stringify(value)),
  }
  const record = { controller_wallet: 'alice', controller_ss58: controller, protected_ss58: pure, genesis_hash: genesis, delay_blocks: 10 }
  const compose = call => encodeIntentCall(call, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
  const query = async (pallet, name) => {
    if (name === 'Proxies') return [[{ delegate: controller, proxy_type: 'Any', delay: 10 }], 0n]
    if (name === 'Announcements') return [state.announcements, 0n]
    if (name === 'Account') return { data: { free: 10_000_000_000n, frozen: 0n, reserved: 0n } }
    if (name === 'DerivedAuthorities') return undefined
    throw Error(`unexpected query ${pallet}.${name}`)
  }
  const reader = { query, runtimeCall: async () => null, decodeAddress: sdk.publicKeyFromSs58 }
  const backend = {
    accountId: sdk.publicKeyFromSs58, ss58Encode: sdk.ss58FromPublic, hash: sdk.blake2_256,
    receipt: async () => null,
    snapshot: async () => ({
      genesisHash: genesis, hash: blockHash, block: state.block, specVersion: 419, blockTime: 12,
      query, queryMap: async () => [], constant: async () => 0n, fee: async () => 1n,
      prepare: (intent, context) => intent.prepare(reader, context), compose,
      decodeCall: async bytes => runtime.decodeCall(bytes), hashCallData: sdk.blake2_256,
    }),
    submit: async intent => {
      state.submissions++
      if (state.failure) throw state.failure
      assert.equal(intent.op, 'announce_proxy_call')
      state.announcements.push({ real: pure, call_hash: intent.args.call_hash, height: state.block })
      return { status: 'finalized', success: true, blockHash, blockNumber: state.block, extrinsicHash: '0x' + 'ee'.repeat(32), events: [] }
    },
  }
  const journal = new ProxyAnnouncementStore(store, address => hex(sdk.publicKeyFromSs58(address)))
  const records = new ProtectedAccountStore(store, backend)
  const announcements = new ProtectedAnnouncements(backend, journal)
  const session = new ProtectedAccountSession(backend, records, announcements, 'alice')
  return { state, store, record, journal, records, announcements, session }
}

function assertSaved(receipt) {
  assert.equal(receipt.status, 'unknown')
  assert.equal(receipt.success, null)
  assert.equal(receipt.extrinsicHash, innerHash)
  assert.equal(receipt.innerExtrinsicHash, innerHash)
  assert.equal(receipt.carrierExtrinsicHash, carrierHash)
  assert.equal(receipt.data.submission_uncertain, true)
  assert.equal(receipt.data.carrier_only, true)
  assert.equal(receipt.data.inner_extrinsic_hash, innerHash)
  assert.equal(receipt.data.carrier_extrinsic_hash, carrierHash)
  assert.equal(receipt.data.carrier_block_hash, blockHash)
  assert.equal(receipt.data.carrier_block_number, 100)
  assert.equal(receipt.blockHash, undefined)
  assert.equal(receipt.blockNumber, undefined)
  assert.equal(receipt.finalized, undefined)
  assert.deepEqual(receipt.events, [])
}

// PROTECTED_REGRESSION: interrupted Shield announcements must retain both hashes and prevent blind rebroadcast; changes require explicit human approval.
test('announcement journal persists both Shield hashes and blocks duplicate submission', async () => {
  const f = fixture()
  f.state.failure = trackingError()
  await assert.rejects(f.announcements.announce(f.record, transfer()), ShieldedTrackingError)
  assertSaved((await f.journal.all())[0].announcement_receipt)
  await assert.rejects(f.announcements.announce(f.record, transfer()), /identical announcement/)
  assert.equal(f.state.submissions, 1)
})

// PROTECTED_REGRESSION: uncertain shielded transfer execution must remain recoverable without paying twice; changes require explicit human approval.
test('execution journal persists both Shield hashes and keeps uncertain replay gated', async () => {
  const f = fixture()
  const queued = await f.announcements.announce(f.record, transfer())
  f.state.block = 120
  f.state.failure = trackingError()
  await assert.rejects(f.announcements.execute(f.record, queued.row.index), ShieldedTrackingError)
  assertSaved((await f.journal.all())[0].pending_action.receipt)
  await assert.rejects(f.announcements.execute(f.record, queued.row.index), /uncertain submission outcome/)
  assert.equal(f.state.submissions, 2)
})

// PROTECTED_REGRESSION: uncertain pure-account creation must retain Shield reconciliation identity instead of creating twice; changes require explicit human approval.
test('setup reservation persists both Shield hashes and resumes without another creation', async () => {
  const f = fixture()
  f.state.failure = trackingError()
  const options = { controllerAddress: controller, delay: 10, allows: [] }
  await assert.rejects(f.session.setup(options), ShieldedTrackingError)
  const saved = await f.records.getSetup('alice', genesis)
  assert.equal(saved.status, 'uncertain')
  assert.equal(saved.extrinsic_hash, innerHash)
  assert.equal(saved.block_hash, undefined)
  assertSaved(saved.receipt)
  const resumed = await f.session.resumeSetup(options)
  assert.equal(resumed.status, 'uncertain')
  assertSaved(resumed.receipt)
  assert.equal(f.state.submissions, 1)
})

test('ordinary uncertain errors keep existing announcement and setup receipt shapes', async () => {
  const f = fixture()
  f.state.failure = new SubmissionUncertainError(innerHash, Error('ordinary connection lost'))
  await assert.rejects(f.announcements.announce(f.record, transfer()), SubmissionUncertainError)
  assert.deepEqual((await f.journal.all())[0].announcement_receipt, { status: 'unknown', success: null, events: [], extrinsicHash: innerHash })
  await assert.rejects(f.session.setup({ controllerAddress: controller, delay: 10, allows: [] }), SubmissionUncertainError)
  assert.deepEqual((await f.records.getSetup('alice', genesis)).receipt, { status: 'uncertain', success: null, events: [], extrinsicHash: innerHash })
})
