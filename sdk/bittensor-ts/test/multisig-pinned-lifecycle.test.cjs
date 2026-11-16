'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { buildIntent, wrapPreparedMultisig, checkIntentPolicy } = require('../dist/intents')
const { MultisigCallCache } = require('../dist/multisig-workflow')
const browser = require('../dist/browser')
const { BrowserMultisig } = require('../dist/browser-client')
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const account = { address: sdk.ss58FromPublic(Buffer.alloc(32, 3), 42), publicKey: Buffer.alloc(32, 3) }
const timepoint = { height: 40, index: 2 }
const spec = { op: 'add_stake', hotkey_ss58: bob.ss58Address, netuid: 1, amount_tao: '1' }
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const encode = call => Buffer.from(JSON.stringify({ pallet: call.pallet, call: call.call, params: call.params }, (_, value) => typeof value === 'bigint' ? value.toString() : value))

function fixture() {
  const state = { price: 2_000_000_000n, reads: 0, pending: new Map() }
  const reader = {
    runtimeCall: async (_api, method) => { assert.equal(method, 'current_alpha_price'); state.reads++; return state.price },
    query: async (pallet, item, args) => pallet === 'Multisig' ? state.pending.get(hex(args[1])) ?? null : { data: { free: 100_000_000_000n } },
    decodeAddress: sdk.publicKeyFromSs58,
    multisigAccount: () => account,
    hashCall: async call => sdk.blake2_256(encode(call)),
    composeCall: async call => encode(call),
    decodeCall: async data => JSON.parse(Buffer.from(data).toString()),
    specVersion: () => 500,
    estimateWeight: async () => ({ ref_time: 1n, proof_size: 0n }),
    estimateFee: async () => 1000n,
    constant: async () => 1000n,
  }
  return { state, reader }
}

test.before(async () => {
  await browser.initBrowser(async () => {
    const fs = require('node:fs'), path = require('node:path'), { pathToFileURL } = require('node:url')
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default; return module
  })
})

// PROTECTED_REGRESSION: mutable prices must not strand an approved multisig operation; changes require explicit human approval.
test('saved call bytes survive a price change through hash approval and final execution', async () => {
  for (const account_mode of ['legacy', 'safe']) {
    const { reader, state } = fixture()
    const args = { threshold: 2, other_signatories: [bob.ss58Address], call: spec, account_mode }
    const opening = await buildIntent('multisig_approve', args).prepare(reader, { coldkey: alice.ss58Address })
    const committed = opening.extras.multisig_call_hash
    state.pending.set(committed, { when: timepoint, approvals: [alice.ss58Address], deposit: 2000n })
    state.price *= 2n
    const reads = state.reads
    for (const op of ['multisig_approve', 'multisig_execute']) {
      const followup = await buildIntent(op, { ...args, other_signatories: [alice.ss58Address], timepoint,
        call_data: opening.extras.multisig_call_data, call_hash: committed,
        call_spec_version: opening.extras.multisig_call_spec_version,
      }).prepare(reader, { coldkey: bob.ss58Address })
      assert.equal(followup.extras.multisig_call_hash, committed)
      assert.deepEqual(followup.call.params.maybe_timepoint, timepoint)
      const inner = followup.call.params.call ?? followup.call.semanticCall
      assert.deepEqual(encode(inner), Buffer.from(opening.extras.multisig_call_data.slice(2), 'hex'))
    }
    assert.equal(state.reads, reads, 'follow-up must not read a new price')
  }
})

test('unpinned changed calls and wrong timepoints are rejected before approval', async () => {
  const { reader, state } = fixture()
  const args = { threshold: 2, other_signatories: [bob.ss58Address], call: spec }
  const opening = await buildIntent('multisig_approve', args).prepare(reader, { coldkey: alice.ss58Address })
  state.pending.set(opening.extras.multisig_call_hash, { when: timepoint })
  state.price *= 2n
  await assert.rejects(buildIntent('multisig_execute', { ...args, timepoint }).prepare(reader, { coldkey: alice.ss58Address }), /original multisig call_data/)
  await assert.rejects(buildIntent('multisig_execute', { ...args, timepoint: { height: 41, index: 2 }, call_data: opening.extras.multisig_call_data }).prepare(reader, { coldkey: alice.ss58Address }), /timepoint does not match/)
})

// PROTECTED_REGRESSION: co-signers must reuse a pending prepared call without weakening its spend policy; changes require explicit human approval.
test('high-level call cache and wrapping preserve original limits and bounded policy', async () => {
  const { reader, state } = fixture(), cache = new MultisigCallCache(), intent = buildIntent('add_stake', spec)
  const first = await cache.prepare(reader, intent, { coldkey: account.address, feePayer: alice.ss58Address })
  const hash = hex(await reader.hashCall(first.call))
  state.pending.set(hash, { when: timepoint })
  state.price *= 2n
  const second = await cache.prepare(reader, buildIntent('add_stake', spec), { coldkey: account.address, feePayer: bob.ss58Address })
  assert.equal(second, first)
  const wrapped = wrapPreparedMultisig('multisig_execute', { threshold: 2, other_signatories: [alice.ss58Address], call: spec, timepoint }, second)
  const result = await wrapped.prepare(reader, { coldkey: bob.ss58Address })
  assert.equal(result.extras.multisig_call_hash, hash)
  assert.equal(result.spendRao, 1_000_000_000n)
  assert.equal(result.affectsAllSubnets, false)
  assert.deepEqual(result.netuids, [1])
  assert.deepEqual(checkIntentPolicy(result, { maxSpendRao: 1_000_000_000n, allowedNetuids: [1] }), [])
  assert.equal(state.reads, 1)
})

test('imported bytes enforce their hash and runtime version and retain conservative policy', async () => {
  const { reader } = fixture()
  const args = { threshold: 2, other_signatories: [bob.ss58Address], call: spec }
  const opening = await buildIntent('multisig_approve', args).prepare(reader, { coldkey: alice.ss58Address })
  const saved = { ...args, call_data: opening.extras.multisig_call_data }
  await assert.rejects(buildIntent('multisig_execute', { ...saved, call_hash: '0x' + 'ff'.repeat(32) }).prepare(reader, { coldkey: alice.ss58Address }), /original commitment/)
  await assert.rejects(buildIntent('multisig_execute', { ...saved, call_spec_version: 499 }).prepare(reader, { coldkey: alice.ss58Address }), /runtime changed/)
  const prepared = await buildIntent('multisig_execute', saved).prepare(reader, { coldkey: alice.ss58Address })
  assert.equal(prepared.spendRao, 'unbounded')
  assert.equal(prepared.global, true)
})

// PROTECTED_REGRESSION: public multisig approval must preserve the first commitment across price changes and submission visibility gaps; changes require explicit human approval.
test('public multisig approvals keep original bytes for two signers before and after inclusion', async () => {
  for (const pendingVisible of [false, true]) for (const mode of ['legacy', 'safe']) {
    const { reader, state } = fixture(), prepared = []
    reader.multisigAccount = (members, threshold, safe) => {
      const publicKey = sdk.multisigAccountId(members.map(sdk.publicKeyFromSs58), threshold, safe).accountId
      return { publicKey, address: sdk.ss58FromPublic(publicKey, 42) }
    }
    const client = {
      intentReader: async () => reader,
      execute: async (intent, signer, options) => {
        const result = await intent.prepare(reader, { coldkey: signer.ss58Address })
        assert.deepEqual(checkIntentPolicy(result, options.policy), [])
        assert.equal(result.extras.multisig_call_hash, options.expectedCallHash)
        prepared.push(result)
        return { success: true, status: 'submitted', events: [], data: result.extras }
      },
    }
    const group = new BrowserMultisig(client, [alice.ss58Address, bob.ss58Address], 2, mode)
    const signer = key => ({ ss58Address: key.ss58Address, publicKey: key.publicKey, cryptoType: 1, sign: async () => { throw new Error('unexpected signing') } })
    const options = { waitForInclusion: false, policy: { maxSpendRao: 1_000_000_000n, allowedNetuids: [1] } }
    const first = await group.approve(buildIntent('add_stake', spec), signer(alice), options)
    const hash = first.data.multisig_call_hash
    if (pendingVisible) state.pending.set(hash, { when: timepoint, approvals: [alice.ss58Address] })
    state.price *= 2n
    const second = await group.approve(buildIntent('add_stake', spec), signer(bob), options)
    assert.equal(second.data.multisig_call_hash, hash)
    assert.deepEqual(encode(prepared[1].call.params.call), encode(prepared[0].call.params.call))
    assert.deepEqual(prepared[1].call.params.maybe_timepoint, pendingVisible ? timepoint : null)
    assert.equal(state.reads, 1)
    await assert.rejects(group.forgetPreparedCall(buildIntent('add_stake', spec), '0x' + 'ff'.repeat(32)), /expectedCallHash/)
    // A caller explicitly releases only after reconciling the previous operation.
    state.pending.delete(hash)
    await group.forgetPreparedCall(buildIntent('add_stake', spec), hash)
    const fresh = await group.approve(buildIntent('add_stake', spec), signer(alice), options)
    assert.notEqual(fresh.data.multisig_call_hash, hash)
    assert.equal(state.reads, 2)
  }
})

test('cached nested approvals refresh the current signer fee account and all available reserves', async () => {
  const { reader, state } = fixture(), cache = new MultisigCallCache()
  const balances = new Map(), baseQuery = reader.query
  reader.query = async (pallet, item, args) => pallet === 'System' && item === 'Account'
    ? { data: { free: balances.get(args[0]) ?? 100_000_000_000n } } : baseQuery(pallet, item, args)
  const inner = buildIntent('multisig_execute', { threshold: 2, other_signatories: [alice.ss58Address], call: spec })
  const saved = await cache.prepare(reader, inner, { coldkey: account.address, feePayer: alice.ss58Address })
  const hash = hex(await reader.hashCall(saved.call))
  state.pending.set(hash, { when: timepoint })
  balances.set(account.address, 0n)
  balances.set(bob.ss58Address, 0n)
  const wrapped = wrapPreparedMultisig('multisig_execute', { threshold: 2, other_signatories: [alice.ss58Address], call: inner.toJSON(), timepoint }, saved)
  const result = await wrapped.prepare(reader, { coldkey: bob.ss58Address })
  assert.equal(result.feePayer, bob.ss58Address)
  assert.ok(result.requiredFreeByAccount.some(row => row.address === account.address && row.requiredRao > row.availableRao))
  assert.ok(result.requiredFreeByAccount.some(row => row.address === bob.ss58Address && row.requiredRao > row.availableRao))
  assert.ok(result.requiredFreeByAccount.every(row => row.address !== alice.ss58Address))
  assert.ok(result.requiredFreeByAccount.every(row => row.availableRao === 0n))
  assert.match(result.violations.join('; '), /fee/)
})

test('a refused initial preparation can be retried after its funding is restored', async () => {
  const { reader, state } = fixture(), cache = new MultisigCallCache(), baseQuery = reader.query
  let free = 0n
  reader.query = async (pallet, item, args) => pallet === 'System' && item === 'Account' ? { data: { free } } : baseQuery(pallet, item, args)
  const inner = buildIntent('multisig_execute', { threshold: 2, other_signatories: [alice.ss58Address], call: spec })
  const context = { coldkey: account.address, feePayer: alice.ss58Address }
  const refused = await cache.prepare(reader, inner, context)
  assert.ok(refused.violations.length)
  free = 100_000_000_000n
  const funded = await cache.prepare(reader, inner, context)
  assert.deepEqual(funded.violations, [])
  assert.notEqual(funded, refused)
  assert.equal(state.reads, 2)
})

test('cached calls reject a runtime upgrade even if their encoded hash is unchanged', async () => {
  const { reader, state } = fixture(), cache = new MultisigCallCache(), intent = buildIntent('add_stake', spec)
  const context = { coldkey: account.address, feePayer: alice.ss58Address }
  await cache.prepare(reader, intent, context)
  reader.specVersion = () => 501
  await assert.rejects(cache.prepare(reader, intent, context), /runtime changed since multisig approval/)
  assert.equal(state.reads, 1)
})

// PROTECTED_REGRESSION: equivalent intent property order must not change a pending multisig commitment; changes require explicit human approval.
test('equivalent nested intent property order preserves the original commitment and release key', async () => {
  const { reader, state } = fixture(), cache = new MultisigCallCache()
  const context = { coldkey: account.address, feePayer: alice.ss58Address }
  const first = buildIntent('batch', { intents: [spec] })
  const reordered = buildIntent('batch', { intents: [{ amount_tao: '1', netuid: 1, hotkey_ss58: bob.ss58Address, op: 'add_stake' }] })
  const original = await cache.prepare(reader, first, context)
  state.price *= 2n
  assert.equal(await cache.prepare(reader, reordered, context), original)
  assert.equal(state.reads, 1)
  await cache.forget(reordered, hex(await reader.hashCall(original.call)))
  assert.notEqual(await cache.prepare(reader, first, context), original)
  assert.equal(state.reads, 2)
})

test('threshold-one browser operations prepare current prices for each immediate execution', async () => {
  const { reader, state } = fixture(), submitted = []
  reader.multisigAccount = (members, threshold, safe) => {
    const publicKey = sdk.multisigAccountId(members.map(sdk.publicKeyFromSs58), threshold, safe).accountId
    return { publicKey, address: sdk.ss58FromPublic(publicKey, 42) }
  }
  const client = {
    intentReader: async () => reader,
    execute: async (intent, signer) => {
      const prepared = await intent.prepare(reader, { coldkey: signer.ss58Address })
      submitted.push(prepared)
      return { success: true, status: 'submitted', events: [], data: prepared.extras }
    },
  }
  const group = new BrowserMultisig(client, [alice.ss58Address], 1)
  const signer = { ss58Address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, sign: async () => { throw new Error('unexpected signing') } }
  const first = await group.approve(buildIntent('add_stake', spec), signer)
  state.price *= 2n
  const second = await group.approve(buildIntent('add_stake', spec), signer)
  assert.notEqual(second.data.multisig_call_hash, first.data.multisig_call_hash)
  assert.equal(submitted[0].call.call, 'as_multi_threshold_1')
  assert.equal(submitted[1].call.call, 'as_multi_threshold_1')
  assert.equal(state.reads, 2)
})

test('safe public multisig preparation propagates its derivation mode to nested composites', async () => {
  const { reader } = fixture()
  reader.multisigAccount = (members, threshold, safe) => {
    const publicKey = sdk.multisigAccountId(members.map(sdk.publicKeyFromSs58), threshold, safe).accountId
    return { publicKey, address: sdk.ss58FromPublic(publicKey, 42) }
  }
  const client = {
    intentReader: async () => reader,
    execute: async (intent, signer) => intent.prepare(reader, { coldkey: signer.ss58Address }),
  }
  const group = new BrowserMultisig(client, [alice.ss58Address, bob.ss58Address], 2, 'safe')
  const signer = { ss58Address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1, sign: async () => { throw new Error('unexpected signing') } }
  const inner = buildIntent('multisig_threshold_1', { other_signatories: [bob.ss58Address], call: spec, account_mode: null })
  const prepared = await group.approve(inner, signer)
  assert.equal(prepared.call.call, 'as_multi_safe')
  assert.equal(prepared.call.params.call.call, 'as_multi_threshold_1_safe')
})

// PROTECTED_REGRESSION: a pinned management call must not acquire unrelated harmless semantic metadata; changes require explicit human approval.
test('prepared multisig bridge rejects forged semantic data for a management call', async () => {
  const { reader } = fixture(), management = buildIntent('remove_proxies', {})
  const prepared = await management.prepare(reader, { coldkey: account.address, feePayer: alice.ss58Address })
  const args = { threshold: 2, other_signatories: [bob.ss58Address], call: { op: 'transfer', dest_ss58: bob.ss58Address, amount_tao: '1' } }
  assert.throws(() => wrapPreparedMultisig('multisig_execute', args, prepared), /semantic intent differs/)
  const wrapped = wrapPreparedMultisig('multisig_execute', { ...args, call: management.toJSON() }, prepared)
  assert.equal(wrapped.args.call.op, 'remove_proxies')
  const reordered = { from_hotkey: false, op: 'remove_proxies' }
  assert.doesNotThrow(() => wrapPreparedMultisig('multisig_execute', { ...args, call: reordered }, prepared))
})

// PROTECTED_REGRESSION: Node co-signers must execute the original priced call while retaining bounded spend and current funding checks; changes require explicit human approval.
test('Node public multisig workflow signs the original call after price changes', async t => {
  const { NodeMultisig } = require('../dist/node-workflows'), { IntentCall } = require('../dist/transaction')
  const { encodeIntentCall } = require('../dist/intents'), { toWire } = require('../dist/wire')
  const golden = require('../../python/tests/fixtures/golden.json')
  const runtime = new sdk.Runtime(sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')), 419, 1, 42)
  const client = new sdk.Client('local'), pending = new Map(), signed = [], balances = new Map()
  let price = 2_000_000_000n, reads = 0
  t.after(() => client.close())
  const original = IntentCall.fromDescriptor
  // Keep metadata-backed native policy construction outside this transport fixture;
  // semantic policy, real SCALE encoding, and signatures remain exercised below.
  IntentCall.fromDescriptor = (_native, descriptor) => ({ pallet: descriptor.pallet, function: descriptor.call, params: descriptor.params })
  t.after(() => { IntentCall.fromDescriptor = original })
  Object.assign(client, {
    runtimeAt: async () => runtime,
    blockHash: async () => '0x' + '11'.repeat(32),
    blockNumber: async () => 40,
    composeCall: async (pallet, call, params) => runtime.composeCall(pallet, call, params),
    query: async (pallet, item, args) => pallet === 'Multisig' ? pending.get(hex(args[1])) ?? null : item === 'Account' ? { data: { free: balances.get(args[0]) ?? 100_000_000_000n } } : null,
    constant: async () => 1000n,
    runtimeCall: async (_api, method) => { assert.equal(method, 'current_alpha_price'); reads++; return price },
    rpc: async method => { assert.equal(method, 'payment_queryInfo'); return { weight: { ref_time: 1, proof_size: 0 } } },
    requireNativeClient: async () => ({
      accountNextIndex: async () => 0,
      externalSigningPlan: async (data, _address, publicKey) => ({ runtime: runtime.handle, publicKey, callData: data, feeRao: 1000n, txParams: { era: toWire('00'), nonce: 0n, tip: 0n } }),
      externalSigningPlanForIntent: async () => ({ feeRao: 1000n }),
    }),
    submit: async (call, signer) => {
      const resolved = await client.resolveSigner(signer, runtime)
      const descriptor = { pallet: call.pallet, call: call.function, params: call.params }
      const data = await encodeIntentCall(descriptor, async (pallet, name, params) => runtime.composeCall(pallet, name, params))
      const tx = { era: '00', nonce: 0, tip: 0n, genesisHash: Buffer.alloc(32), eraBlockHash: Buffer.alloc(32) }
      const payload = runtime.signaturePayload(data, tx), signature = await resolved.signer.sign(payload)
      assert.equal(sdk.verifySignature(payload, signature, sdk.ss58FromPublic(resolved.publicKey), resolved.cryptoType), true)
      signed.push(descriptor)
      return { success: true, status: 'submitted', message: 'Submitted', events: [] }
    },
  })
  const group = new NodeMultisig(client, [alice.ss58Address, bob.ss58Address], 2)
  const options = { mevShield: false, policy: { maxSpendRao: 1_000_000_000n, allowedNetuids: [1] } }
  const first = await group.approve(buildIntent('add_stake', spec), alice, options)
  pending.set(first.data.multisig_call_hash, { when: timepoint, approvals: [alice.ss58Address] })
  price *= 2n
  balances.set(bob.ss58Address, 0n)
  await assert.rejects(group.approve(buildIntent('add_stake', spec), bob, options), /preflight refused/)
  assert.equal(signed.length, 1, 'new signer funding must be checked before signing')
  balances.set(bob.ss58Address, 100_000_000_000n)
  const second = await group.approve(buildIntent('add_stake', spec), bob, options)
  assert.equal(second.data.multisig_call_hash, first.data.multisig_call_hash)
  assert.equal(second.data.multisig_call_data, first.data.multisig_call_data)
  assert.deepEqual(signed[1].params.call, signed[0].params.call)
  assert.deepEqual(signed[1].params.maybe_timepoint, timepoint)
  assert.equal(reads, 1)

  // Threshold one has no approval commitment to retain between separate operations.
  const immediate = new NodeMultisig(client, [alice.ss58Address], 1)
  const before = await immediate.approve(buildIntent('add_stake', spec), alice, options)
  price *= 2n
  const after = await immediate.approve(buildIntent('add_stake', spec), alice, options)
  assert.notEqual(after.data.multisig_call_hash, before.data.multisig_call_hash)
  assert.equal(reads, 3)
})
