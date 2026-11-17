'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { evaluatePolicy } = require('../dist/policy')
const { fundingBudget } = require('../dist/protected-funding')
const { decodedCallDescriptor, validateAnnouncedCall } = require('../dist/proxy-announcements')
const { parseDelayToBlocks, resolveAllowItem } = require('../dist/protected-account-config')
const { policyEvaluate } = require('../dist/policy')
const fs = require('node:fs/promises')
const path = require('node:path')
const { pathToFileURL } = require('node:url')

test('native and WASM execute identical protected policy vectors', async () => {
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  const record = { controller_wallet: 'wallet', controller_ss58: 'controller', protected_ss58: 'pure', genesis_hash: '0xabc', delay_blocks: 30, coldkey_swap: { status: 'submitting' } }
  const vectors = [
    ['protected.delay', { raw: '42s', block_time: 12 }],
    ['protected.instant_proxy', { op: 'remove_stake', intent: { claim: true }, allows: ['Staking'] }],
    ['protected.rotation_complete', { grants: [{ delegate: 'new', proxy_type: 'Any', delay: 30 }, { delegate: 'old', proxy_type: 'Staking', delay: 0 }], new_controller: 'new', old_controller: 'old', delay_blocks: 30 }],
    ['protected.config_transition', { action: 'reserve_migration', rows: [record], wallet: 'wallet', genesis_hash: '0xabc', destination: 'pure' }],
    ['protected.scope_matches', { stored: null, selected: '0xabc' }],
    ['protected.registration_node', { pallet: 'Utility', call: 'if_else', expanded: true }],
    ['protected.pending_allow_grants', { rows: [{ status: 'uncertain', grants: [{ delay: 0, identity: 'key' }] }], allow_key: 'key', include_uncertain: true }],
    ['protected.allow_key_blockers', { old: 'a', new: 'b', allows: ['Staking'], grants: [{ delegate: 'a', proxy_type: 'Gamma', delay: 0 }] }],
    ['protected.pending_refills', { controller: 'a', legacy_python: true, rows: [{ inner_op: 'transfer', inner_args: { dest_ss58: 'a', amount_tao: '0.0000000015' } }] }],
    ['financial.reads.lock_gate', { conviction: 18, eligible: 100 }],
    ['upgrade.manifest_checks', { manifest: { call_hash: '0xAB' }, call_hash: '0xab', python_optional: true }],
    ['upgrade.approval', { present: true, threshold: 3, members: ['a', 'b', 'c'], approvals: ['a', 'b'], signer: 'c' }],
    ['upgrade.weight_pad', { ref_time: '18446744073709551615', proof_size: '10' }],
  ]
  for (const [operation, input] of vectors) {
    const encoded = JSON.stringify(input)
    const native = JSON.parse(policyEvaluate(operation, encoded))
    assert.equal(native.error, undefined, operation)
    assert.deepEqual(JSON.parse(binding.policyEvaluate(operation, encoded)), native, operation)
  }
})

test('upgrade policy keeps optional provenance checks and opaque address-book identities explicit', () => {
  assert.deepEqual(evaluatePolicy('upgrade.manifest_checks', { manifest: { call_hash: '0xAB', ci_address: 'ci' }, call_hash: '0xab', pending: true, ci_address: 'other', python_optional: true }), { 'manifest call hash': true, 'manifest CI key': false })
  assert.equal(evaluatePolicy('upgrade.digest_matches', { expected: 'bad', actual: 'bad' }), false)
  assert.deepEqual(evaluatePolicy('upgrade.signers', { members: [{ address: 'b', identity: '02' }, { address: 'a', identity: '01' }], threshold: 2 }), [1, 0])
  assert.throws(() => evaluatePolicy('upgrade.signers', { members: [{ identity: '01' }, { identity: '01' }], threshold: 2 }), /duplicate/)
  assert.throws(() => evaluatePolicy('transaction.multisig_members', { opaque_members: true, members: ['same', 'same'], threshold: 2 }), /duplicate/)
  assert.deepEqual(evaluatePolicy('transaction.multisig_members', { opaque_members: true, members: [' opaque ', 'opaque'], threshold: 2 }).indices, [0, 1])
})

test('shared pending funding preserves Python legacy rounding and TypeScript uncertainty accounting', () => {
  const input = { controller: 'a', rows: [{ inner_op: 'transfer', inner_args: { dest_ss58: 'a', amount_tao: '0.0000000015' } }] }
  assert.equal(evaluatePolicy('protected.pending_refills', { ...input, legacy_python: true }).rao, '2')
  assert.deepEqual(evaluatePolicy('protected.pending_refills', input), { rao: '0', unbounded: false, unknown: true })
  assert.deepEqual(evaluatePolicy('protected.pending_allow_grants', { rows: [{ status: 'uncertain', grants: [{ delay: 0, identity: 'key' }] }], allow_key: 'key' }), [])
  assert.deepEqual(evaluatePolicy('protected.allow_key_blockers', { old: 'a', new: 'a', allows: ['Staking'], grants: [] }).kinds, [])
})

test('protected facades use the same exact Rust funding and delay decisions', () => {
  const input = { controller: 'controller', account: { data: { free: 9007199254740993n, reserved: 0n } }, announcement_count: 0, announcement_deposit: 20n, deposit_base: 10n, deposit_factor: 2n, existential_deposit: 1n, announce_fee: 3n, transfer_execution_fee: 4n, staking_execution_fee: 8n, fee_payer: 'controller' }
  const core = evaluatePolicy('protected.funding', input), publicValue = fundingBudget(input)
  for (const [name, value] of Object.entries(core)) assert.deepEqual(publicValue[name], name.endsWith('_rao') ? BigInt(value) : value)
  assert.equal(publicValue.free_rao, 9007199254740993n)
  assert.equal(publicValue.target_rao, 171n)
  for (const raw of ['30s', '42s', '36h', '2d', 17]) assert.equal(parseDelayToBlocks(raw), evaluatePolicy('protected.delay', { raw, block_time: 12 }))
  assert.equal(resolveAllowItem('gamma'), evaluatePolicy('protected.allow_resolution', { item: 'gamma' }).proxy_type)
  assert.throws(() => resolveAllowItem('any'), /must remain delayed/)
})

test('shared call normalization preserves native integer and byte parameter representations', () => {
  const account = new Uint8Array(32).fill(3)
  const value = { call_module: 'Balances', call_function: 'transfer_keep_alive', call_args: [{ name: 'dest', value: account }, { name: 'value', value: 9007199254740993n }] }
  const call = decodedCallDescriptor(value)
  assert.equal(call.params.value, 9007199254740993n)
  assert.deepEqual(call.params.dest, account)
  validateAnnouncedCall(call)
  assert.throws(() => validateAnnouncedCall({ pallet: 'System', call: 'set_code', params: {} }), /unsupported saved announcement/)
})

test('shared receipt state retains failed atomic and approval-only boundaries', () => {
  const receipt = { success: true, status: 'finalized', blockNumber: 4, events: [] }
  assert.equal(evaluatePolicy('protected.receipt_state', receipt).confirmed, true)
  for (const event_id of ['BatchInterrupted', 'ItemFailed', 'BatchCompletedWithErrors']) {
    const result = evaluatePolicy('protected.receipt_state', { ...receipt, events: [{ module_id: 'Utility', event_id, attributes: {} }] })
    assert.equal(result.successful, false)
    assert.equal(result.confirmed, false)
  }
  const approval = evaluatePolicy('protected.receipt_state', { ...receipt, events: [{ module_id: 'Multisig', event_id: 'MultisigApproval', attributes: {} }] })
  assert.equal(approval.approval_only, true)
  assert.equal(approval.confirmed, false)
})
