'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const { evaluatePolicy, policyEvaluate } = require('../dist/policy')
const { normalizeSignatureBytes } = require('../dist/signatures')
const { requireWalletIdentity } = require('../dist/wallet-registration')
const { nestedDispatchError } = require('../dist/receipt')

const account = '0x' + '11'.repeat(32)
const other = '0x' + '22'.repeat(32)
const vectors = [
  ['transaction.intent_policy', { max_spend_rao: '9007199254740994', spend_rao: '9007199254740993', additional_spend_rao: '2', allowed_netuids: [1], netuids: [2], affects_all_subnets: true }],
  ['transaction.intent_metadata', { op: 'add_proxy', args: { proxy_type: 'Validate', from_hotkey: false } }],
  ['transaction.simple_call', { op: 'transfer', args: { dest_ss58: account, amount_tao: '9007199254740993', keep_alive: true } }],
  ['transaction.merge_intents', { batch: true, children: [{ signer: 'coldkey', spend_rao: '4', netuids: [2] }, { signer: 'coldkey', spend_rao: 'unbounded', netuids: [1] }] }],
  ['transaction.multisig_members', { members: [other, account], threshold: 2, signer: account }],
  ['transaction.multisig_deposit', { base: '9007199254740993', factor: '4', threshold: 2 }],
  ['transaction.signature_normalize', { signature: '0x02' + '33'.repeat(65), crypto_type: 2, strict: true }],
  ['transaction.descriptor', { descriptor: '0x0101' + '44'.repeat(32) }],
  ['transaction.account_type', { crypto_type: 9, mode: 'hashed' }],
  ['transaction.derivative', { crypto_type: 9, index: 65535 }],
  ['transaction.receiving_registration', { op: 'associate_hotkey', field: 'hotkey_ss58', registered: false }],
  ['transaction.execution', { shield_required: true, shielded: false, expected_hash: '0x1234', actual_hash: '0x5678' }],
  ['transaction.authority_nonce', { finalized: '4', pending: '5', requested: '4' }],
  ['transaction.signer_identity', { actual: { account, public_key: account, crypto_type: 1 }, expected: { account: other, public_key: other, crypto_type: 1 } }],
  ['transaction.evm_mirror', { address: '0x' + 'ab'.repeat(20) }],
  ['transaction.evm_association_payload', { hotkey: account, block: '9007199254740993' }],
  ['transaction.http_admission', { phase: 'scheme', declared: 1, expected: 4 }],
  ['transaction.http_admission', { phase: 'retention', retention: '12', max_age: '10', allowed_skew: '2' }],
  ['transaction.external_signer', { crypto_type: 2, account, public_key: account, metadata_disabled: true, requires_metadata_proof: true }],
  ['financial.staking_call', { op: 'move_swap_stake', args: { origin_netuid: 1, dest_netuid: 2, dest_hotkey_ss58: other, slippage_protection: true }, hotkey: account, origin_hotkey: account, amount: '9007199254740993', limit_price: '1000000000' }],
  ['transaction.http_freshness', { now: '1000000000000000001', nonce: '1000000000000000000', max_age: '1', allowed_skew: '0' }],
  ['transaction.era', { era: '0x1500' }],
  ['transaction.children', { children: [['0.5', account], ['0.25', other]] }],
  ['transaction.take', { op: 'set_take', take: 100, current: 101 }],
  ['transaction.pure_proxy', { op: 'create_pure_proxy', index: 0, crypto_type: 9 }],
  ['transaction.serving', { op: 'serve_axon_tls', args: { netuid: 1, ip: '2001:db8::1', port: 443, version: 1, protocol: 4, certificate: '0x0102' } }],
  ['transaction.nested_dispatch_error', { events: [{ invalid: true }, { module_id: 'Utility', event_id: 'BatchInterrupted', attributes: [0, { BadOrigin: null }] }] }],
]

test('transaction policies return identical native and real WASM decisions', async () => {
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  for (const [operation, input] of vectors) {
    const encoded = JSON.stringify(input)
    const native = JSON.parse(policyEvaluate(operation, encoded))
    assert.equal(native.error, undefined, operation)
    assert.deepEqual(JSON.parse(binding.policyEvaluate(operation, encoded)), native, operation)
  }
})

test('core-built transfer retains exact codec bigint and unchanged cap enforcement', async () => {
  const alice = sdk.Keypair.fromUri('//Alice')
  const intent = sdk.buildIntent('transfer', { dest_ss58: alice.ss58Address, amount_tao: '9007199.254740993' })
  const prepared = await intent.prepare({ query: async () => null }, { coldkey: alice.ss58Address })
  assert.equal(prepared.call.call, 'transfer_keep_alive')
  assert.equal(prepared.call.params.value, 9007199254740993n)
  assert.equal(prepared.spendRao, 9007199254740993n)
  assert.match(sdk.checkIntentPolicy(prepared, { maxSpendRao: 9007199254740992n })[0], /exceeds maxSpendRao/)
  assert.equal(sdk.intentSignerRole(sdk.buildIntent('add_proxy', { delegate_ss58: alice.ss58Address, proxy_type: 'Validate', from_hotkey: false })), 'coldkey')
})

test('shared identity and signature policies preserve strict external signing boundaries', () => {
  const actual = sdk.Keypair.fromUri('//Alice'), expected = sdk.Keypair.fromUri('//Bob')
  assert.throws(() => requireWalletIdentity(actual, expected, sdk.publicKeyFromSs58), /identity/)
  assert.throws(() => normalizeSignatureBytes(Buffer.alloc(64), 2), /65/)
  assert.throws(() => normalizeSignatureBytes(Buffer.concat([Buffer.from([0]), Buffer.alloc(65)]), 2), /scheme|cryptoType|prefix/)
  assert.equal(evaluatePolicy('transaction.signing_bounds', { nonce: 7, offset: 1, period: 8, shielded: true }), '8')
  assert.throws(() => evaluatePolicy('transaction.signing_bounds', { nonce: 4294967294, offset: 1, period: 8 }), /nonce exhausted/)
  assert.throws(() => evaluatePolicy('transaction.signing_bounds', { nonce: 1, period: null, shielded: true }), /4..8/)
})

test('nested failure classification skips unrelated rows and retains actual dispatch failure', () => {
  const failure = { Module: { index: 7, error: '0x01000000' } }
  assert.deepEqual(nestedDispatchError([{ invalid: true }, { event: { section: 'Proxy', method: 'ProxyExecuted', data: [{ Err: failure }] } }]), failure)
  assert.equal(nestedDispatchError([{ module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Ok: null } } }]), undefined)
})


test('shared child, serving and pure-proxy decisions retain exact bounds', () => {
  assert.throws(() => evaluatePolicy('transaction.children', { children: [['0.8', account], ['0.8', other]] }), /exceed/)
  assert.deepEqual(evaluatePolicy('transaction.take', { op: 'set_take', take: 100, current: 101 }), { call: 'decrease_take', take: '100' })
  assert.throws(() => evaluatePolicy('transaction.take', { op: 'set_take', take: 101, current: 101 }), /unchanged/)
  const serving = evaluatePolicy('transaction.serving', { op: 'serve_axon_tls', args: { netuid: 1, ip: '2001:db8::1', port: 443, version: 1, protocol: 4, certificate: '0x0102' } })
  assert.equal(BigInt(serving.params.ip), 0x20010db8000000000000000000000001n)
  assert.throws(() => evaluatePolicy('transaction.serving', { op: 'serve_axon_tls', args: { netuid: 1, ip: '127.0.0.1', port: 443, version: 1, protocol: 4, certificate: '0x' } }), /certificate/)
  assert.equal(evaluatePolicy('transaction.pure_proxy', { op: 'create_pure_proxy', index: 0, crypto_type: 9 }).call, 'create_safe_pure')
  assert.equal(evaluatePolicy('transaction.pure_proxy', { op: 'create_pure_proxy', index: 0, mode: 'legacy', pq_required: true }).call, 'create_protected_pure')
})


test('shared HTTP admission keeps trusted identity and replay retention mandatory', () => {
  assert.equal(evaluatePolicy('transaction.http_admission', { phase: 'scheme', declared: 1, expected: 4 }), 'mismatch')
  assert.equal(evaluatePolicy('transaction.http_admission', { phase: 'scheme', declared: 1 }), 'missing')
  assert.equal(evaluatePolicy('transaction.http_admission', { phase: 'receiver', receiver: other, self: account, required: true }), 'mismatch')
  assert.equal(evaluatePolicy('transaction.http_admission', { phase: 'retention', retention: '11.999', max_age: '10', allowed_skew: '2' }), 'too_short')
  assert.equal(evaluatePolicy('transaction.http_admission', { phase: 'retired', crypto_type: 4, generation: '0' }), 'generation_zero')
})

test('core staking construction restores nested exact bigint codec amounts', async () => {
  const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
  const intent = sdk.buildIntent('move_swap_stake', { origin_hotkey_ss58: alice.ss58Address, dest_hotkey_ss58: bob.ss58Address, origin_netuid: 1, dest_netuid: 2, amount_alpha: '9007199.254740993', slippage_protection: false })
  const prepared = await intent.prepare({ query: async () => null }, { coldkey: alice.ss58Address })
  assert.equal(prepared.call.call, 'batch_all')
  assert.equal(prepared.call.params.calls[0].params.alpha_amount, 9007199254740993n)
  assert.equal(prepared.call.params.calls[1].params.alpha_amount, 9007199254740992n)
  assert.equal(prepared.call.params.calls[0].params.destination_netuid, 1)
  assert.equal(prepared.call.params.calls[1].params.destination_netuid, 2)
})
