'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { WorkflowClient } = require('../dist/node-workflows')
const { buildIntent } = require('../dist/intents')
const { authorizeRegistrationSponsorship } = require('../dist/registration-sponsorship')
const { registrationDescriptor } = require('../dist/protected-accounts')

const delegate = sdk.Keypair.fromUri('//Alice')
const real = sdk.Keypair.fromUri('//Bob').ss58Address
const descriptor = Uint8Array.from([1, 1, ...Array(32).fill(7)])
const recipient = sdk.Keypair.fromDescriptor(descriptor).ss58Address
const reserve = 700_000_000n
const saved = '0x010203'

function fixture({ current = reserve, maximum = reserve, configurable = true, registered = false, guarded = true } = {}) {
  const calls = []
  if (guarded) calls.push({ pallet: 'HashedAccounts', call: 'check_registration_deposit', params: { max_deposit: maximum } })
  calls.push({ pallet: 'HashedAccounts', call: registered ? 'check_registered' : 'register', params: { descriptor: registrationDescriptor(descriptor) } })
  calls.push({ pallet: 'Balances', call: 'transfer_keep_alive', params: { dest: recipient, value: 1_000_000_000n } })
  const committed = { pallet: 'Utility', call: 'batch_all', params: { calls } }
  const reads = []
  const reader = {
    descriptorAddress: value => sdk.Keypair.fromDescriptor(value).ss58Address,
    decodeAddress: sdk.publicKeyFromSs58,
    blockHash: async () => 'pinned',
    hasStorage: async () => configurable,
    constant: async () => 200_000_000n,
    decodeCall: async () => committed,
    hashCallData: async () => new Uint8Array(32).fill(17),
    runtimeCall: async () => assert.fail('must not rebuild descriptive intent'),
    query: async (pallet, name, params, block) => {
      reads.push([pallet, name, params, block])
      if (name === 'HashedAccountRegistrationDeposit') return current
      if (name === 'Account') return { data: { free: 10_000_000_000n } }
      if (name === 'NextKey') return null
      throw new Error(`unexpected query ${pallet}.${name}`)
    },
  }
  const intent = buildIntent('execute_proxy_announced', {
    delegate_ss58: delegate.ss58Address, real_ss58: real,
    inner_op: 'transfer', inner_args: { dest_ss58: recipient, amount_tao: '1' },
    announced_call_data: saved, expected_call_hash: '0x' + '11'.repeat(32),
  })
  const prepare = () => intent.prepare(reader, { coldkey: delegate.ss58Address })
  return { intent, prepare, reads }
}

// PROTECTED_REGRESSION: saved proxy calls must not reserve registration funds without sponsor consent; changes require explicit human approval.
test('saved announced bytes preserve the real payer reserve and reject max zero before submission', async () => {
  const { intent, prepare, reads } = fixture()
  const plan = await prepare()
  assert.deepEqual(plan.extras.registrationSponsorships, [{ payer: real, recipient, depositRao: reserve }])
  assert.equal(plan.requiredFreeAccount, real)
  assert.equal(plan.requiredFreeRao, reserve)
  assert.equal(plan.call.params.call.callData, saved)
  assert.equal(plan.call.params.call.params.calls.at(-1).params.value, 1_000_000_000n)
  assert(reads.filter(row => row[1] === 'HashedAccountRegistrationDeposit' || row[1] === 'Account').every(row => row[3] === 'pinned'))
  let submitted = 0, prompted = 0
  const client = { runtimeAt: async () => ({ ss58Format: 42 }), submit: async () => { submitted++; return { success: true } } }
  const workflow = new WorkflowClient(client)
  workflow.plan = async () => ({ intent: await prepare(), call: plan.call, signerAddress: delegate.ss58Address, shielded: false, ok: true, violations: [] })
  await assert.rejects(workflow.execute(intent, delegate, { maxRegistrationDepositRao: 0n, onRegistrationSponsorship: () => { prompted++; return true } }), /exceeds maxRegistrationDepositRao/)
  assert.equal(submitted, 0)
  assert.equal(prompted, 0)
  await assert.rejects(workflow.execute(intent, delegate, { onRegistrationSponsorship: quote => {
    prompted++
    assert.deepEqual(quote, { payer: real, recipient, depositRao: reserve })
    return false
  } }), /sponsorship declined/)
  assert.equal(submitted, 0)
  assert.equal(prompted, 1)
  await workflow.execute(intent, delegate, { maxRegistrationDepositRao: reserve, onRegistrationSponsorship: () => true })
  assert.equal(submitted, 1)
})

test('replay consents to its committed maximum even when the current deposit is lower', async () => {
  const plan = await fixture({ current: 600_000_000n }).prepare()
  await assert.rejects(authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 600_000_000n }), /exceeds/)
  await authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: reserve })
  await assert.rejects(fixture({ current: reserve + 1n }).prepare(), /exceeds the committed guard/)
})

test('configurable registrations require an effective saved guard; legacy constants remain supported', async () => {
  await assert.rejects(fixture({ guarded: false }).prepare(), /no deposit guard/)
  const plan = await fixture({ guarded: false, configurable: false }).prepare()
  assert.equal(plan.extras.registrationSponsorships[0].depositRao, 200_000_000n)
  await authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 200_000_000n })
})

test('saved check-only guards do not request another registration reserve', async () => {
  const { prepare, reads } = fixture({ registered: true })
  const plan = await prepare()
  await authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 0n, onRegistrationSponsorship: () => assert.fail('not a registration') })
  assert.deepEqual(plan.extras.registrationSponsorships, [])
  assert.equal(reads.some(row => row[1] === 'HashedAccountRegistrationDeposit'), false)
})

let wasm
async function browserBindings() {
  if (wasm) return wasm
  const fs = require('node:fs/promises'), path = require('node:path')
  const { pathToFileURL } = require('node:url')
  const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
  bindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  wasm = { ...bindings }
  delete wasm.default
  await require('../dist/browser').initBrowser(async () => wasm)
  return wasm
}

// PROTECTED_REGRESSION: browser saved-call execution must enforce registration consent before signing; changes require explicit human approval.
test('browser announced execution refuses both an insufficient cap and a rejecting callback', async t => {
  await browserBindings()
  const { Client } = require('../dist/browser-client')
  const client = new Client('http://fixture.invalid')
  t.after(() => client.close())
  const { intent, prepare } = fixture()
  let submitted = 0, prompted = 0
  client.planForSigner = async () => {
    const prepared = await prepare()
    return { prepared, call: prepared.call, violations: [], nonce: 0, signingOptions: {}, blockHash: 'pinned' }
  }
  client.submitPreparedCall = async () => { submitted++; return { success: true } }
  const signer = { address: delegate.ss58Address, publicKey: delegate.publicKey, cryptoType: 1, sign: () => assert.fail('must authorize before signing') }
  await assert.rejects(client.execute(intent, signer, { nonce: 0, maxRegistrationDepositRao: 0n }), /exceeds maxRegistrationDepositRao/)
  await assert.rejects(client.execute(intent, signer, { nonce: 0, onRegistrationSponsorship: quote => {
    prompted++
    assert.deepEqual(quote, { payer: real, recipient, depositRao: reserve })
    return false
  } }), /sponsorship declined/)
  assert.equal(submitted, 0)
  assert.equal(prompted, 1)
})

test('native and WASM recover committed deposit bounds and reject unknown conditional origins', async () => {
  const portable = await browserBindings()
  const { policyEvaluate } = require('../dist/policy')
  const evaluate = (binding, operation, input) => JSON.parse(binding(operation, JSON.stringify(input, (_, value) => typeof value === 'bigint' ? value.toString() : value instanceof Uint8Array ? '0x' + Buffer.from(value).toString('hex') : value)))
  const registration = { pallet: 'HashedAccounts', call: 'register', params: { descriptor: registrationDescriptor(descriptor) } }
  const guard = { pallet: 'HashedAccounts', call: 'check_registration_deposit', params: { max_deposit: reserve } }
  const call = { pallet: 'Utility', call: 'batch_all', params: { calls: [guard, registration, guard, registration] } }
  for (const binding of [policyEvaluate, portable.policyEvaluate]) {
    const extracted = evaluate(binding, 'financial.registration.committed_calls', { call, payer: real })
    assert.equal(extracted.error, undefined)
    assert.equal(extracted.value.length, 2)
    assert(extracted.value.every(row => row.maximum === reserve.toString() && row.payer === real))
    const quoted = evaluate(binding, 'financial.registration.committed_quote', { current: '600000000', configurable: true, maximum: extracted.value[0].maximum })
    assert.equal(quoted.value, '700000000')
    for (const field of ['main', 'fallback']) {
      const harmless = { pallet: 'System', call: 'remark', params: { remark: '0x' } }
      const conditional = { pallet: 'Utility', call: 'if_else', params: { main: harmless, fallback: harmless, [field]: registration } }
      assert.match(evaluate(binding, 'financial.registration.committed_calls', { call: conditional, payer: real }).error.message, /cannot determine committed registration sponsor/)
    }
  }
})
