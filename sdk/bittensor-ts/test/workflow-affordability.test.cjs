const test = require('node:test')
const assert = require('node:assert/strict')
const { Client } = require('../dist/client.js')
const { Keypair, publicKeyFromSs58 } = require('../dist/keys.js')
const { IntentCall } = require('../dist/transaction.js')
const { buildIntent } = require('../dist/intents.js')

const alice = Keypair.fromUri('//Alice'), bob = Keypair.fromUri('//Bob'), charlie = Keypair.fromUri('//Charlie')
const transfer = (amount, dest = charlie.ss58Address) => ({ op: 'transfer', dest_ss58: dest, amount_tao: amount })
function clientFixture(funds) {
  const client = new Client('local')
  client.requireNativeClient = async () => ({ accountNextIndex: async () => 0, externalSigningPlanForIntent: async () => ({ feeRao: 1n }) })
  client.blockHash = async () => 'block'
  client.blockNumber = async () => 1
  client.runtimeAt = async () => ({ ss58Format: 42 })
  client.query = async (pallet, item, params) => {
    assert.equal(pallet + '.' + item, 'System.Account')
    return { data: { free: funds[params[0]] ?? 100_000_000_000n } }
  }
  return client
}

test('Node affordability checks each debit account in a batch of direct and proxied transfers', async () => {
  const client = clientFixture({ [alice.ss58Address]: 100_000_000_000n, [bob.ss58Address]: 1n })
  const original = IntentCall.fromDescriptor
  IntentCall.fromDescriptor = () => ({})
  try {
    const batch = buildIntent('batch', { intents: [transfer('1'), { op: 'execute_proxy_announced', delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address, inner_op: 'transfer', inner_args: transfer('10') }] })
    const plan = await client.plan(batch, alice, { mevShield: false })
    assert.equal(plan.ok, false)
    assert.ok(plan.violations.some(reason => reason.includes(bob.ss58Address) && reason.includes('10000000000')))
    assert.equal(plan.intent.spendByAccount.length, 2)
  } finally { IntentCall.fromDescriptor = original; await client.close() }
})

test('Node affordability combines a multisig opening reserve with the same signer outgoing spend', async () => {
  const funds = { [alice.ss58Address]: 25n, [charlie.ss58Address]: 100n }
  const client = clientFixture(funds), original = IntentCall.fromDescriptor
  const batch = buildIntent('batch', { intents: [transfer('0.000000010'), { op: 'multisig_execute', threshold: 2, other_signatories: [bob.ss58Address], call: transfer('0.000000001') }] })
  const reader = {
    query: async (_pallet, item, [address]) => { assert.equal(item, 'Account'); return { data: { free: funds[address] ?? 100n } } },
    constant: async (_pallet, name) => name === 'DepositBase' ? 10n : 5n,
    decodeAddress: publicKeyFromSs58,
    multisigAccount: () => ({ address: charlie.ss58Address, publicKey: charlie.publicKey }),
    hashCall: async () => new Uint8Array(32),
    estimateWeight: async () => ({ ref_time: 1n, proof_size: 0n }),
    estimateFee: async () => 1n,
  }
  const prepared = await batch.prepare(reader, { coldkey: alice.ss58Address, feePayer: alice.ss58Address })
  assert.equal(prepared.requiredFreeRao, 21n)
  const preview = new Proxy(batch, { get: (target, key, receiver) => key === 'prepare' ? async () => prepared : Reflect.get(target, key, receiver) })
  IntentCall.fromDescriptor = () => ({})
  try {
    const plan = await client.plan(preview, alice, { mevShield: false })
    assert.equal(plan.ok, false)
    assert.ok(plan.violations.some(reason => reason.includes('reserves, outgoing spend')))
  } finally { IntentCall.fromDescriptor = original; await client.close() }
})

test('Node affordability enforces each reserve account alongside its own proxied spend', async () => {
  const funds = { [alice.ss58Address]: 100n, [bob.ss58Address]: 25n }
  const client = clientFixture(funds), original = IntentCall.fromDescriptor
  const batch = buildIntent('batch', { intents: [
    transfer('0.000000001'),
    { op: 'multisig_execute', threshold: 2, other_signatories: [bob.ss58Address], call: transfer('0.000000001') },
    { op: 'execute_proxy_announced', delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address, inner_op: 'multisig_execute', inner_args: { threshold: 2, other_signatories: [alice.ss58Address], call: transfer('0.000000001') } },
    { op: 'execute_proxy_announced', delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address, inner_op: 'transfer', inner_args: transfer('0.000000010') },
  ] })
  const reader = {
    query: async (_pallet, item, [address]) => { assert.equal(item, 'Account'); return { data: { free: funds[address] ?? 100n } } },
    constant: async (_pallet, name) => name === 'DepositBase' ? 10n : 5n,
    decodeAddress: publicKeyFromSs58,
    multisigAccount: () => ({ address: charlie.ss58Address, publicKey: charlie.publicKey }),
    hashCall: async () => new Uint8Array(32),
    estimateWeight: async () => ({ ref_time: 1n, proof_size: 0n }),
    estimateFee: async () => 1n,
  }
  const prepared = await batch.prepare(reader, { coldkey: alice.ss58Address, feePayer: alice.ss58Address })
  assert.equal(prepared.requiredFreeRao, undefined)
  assert.deepEqual(prepared.requiredFreeByAccount.map(value => [value.address, value.requiredRao]), [[alice.ss58Address, 22n], [bob.ss58Address, 20n]])
  const preview = new Proxy(batch, { get: (target, key, receiver) => key === 'prepare' ? async () => prepared : Reflect.get(target, key, receiver) })
  IntentCall.fromDescriptor = () => ({})
  try {
    const plan = await client.plan(preview, alice, { mevShield: false })
    assert.equal(plan.ok, false)
    assert.ok(plan.violations.some(reason => reason.includes(bob.ss58Address) && reason.includes('reserves, outgoing spend')))
    assert.ok(!plan.violations.some(reason => reason.includes(alice.ss58Address)))
  } finally { IntentCall.fromDescriptor = original; await client.close() }
})
