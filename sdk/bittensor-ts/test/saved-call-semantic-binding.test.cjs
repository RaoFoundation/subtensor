'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const sdk = require('../dist')
const browser = require('../dist/browser')
const { WorkflowClient } = require('../dist/node-workflows')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = sdk.decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2), 'hex'))
const alice = sdk.Keypair.fromUri('//Alice'), bob = sdk.Keypair.fromUri('//Bob')
const charlie = sdk.Keypair.fromUri('//Charlie'), dave = sdk.Keypair.fromUri('//Dave')
const timepoint = { height: 40, index: 2 }
const hex = bytes => '0x' + Buffer.from(bytes).toString('hex')
const transfer = { op: 'transfer', dest_ss58: charlie.ss58Address, amount_tao: '0.000000001' }
const payment = (dest = charlie.ss58Address, value = 1n, call = 'transfer_keep_alive') => ({ pallet: 'Balances', call, params: { dest, value } })

test.before(async () => {
  await browser.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': bindings })
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }; delete module.default
    return module
  })
})

function fixture(surface) {
  const api = surface === 'Node' ? sdk : browser
  const runtime = new api.Runtime(metadata, 419, 1, 42)
  const state = { price: 2_000_000_000n, priceReads: 0, pending: new Map() }
  const compose = call => sdk.encodeIntentCall(call, async (pallet, name, args) => runtime.composeCall(pallet, name, args))
  const reader = {
    query: async (pallet, item, args) => pallet === 'Multisig' && item === 'Multisigs'
      ? state.pending.get(hex(args[1])) ?? null : { data: { free: 100_000_000_000n } },
    runtimeCall: async (_api, method) => { assert.equal(method, 'current_alpha_price'); state.priceReads++; return state.price },
    decodeAddress: sdk.publicKeyFromSs58,
    encodeAddress: publicKey => sdk.ss58FromPublic(publicKey, 42),
    multisigAccount: (members, threshold, safe) => {
      const publicKey = sdk.multisigAccountId(members.map(sdk.publicKeyFromSs58), threshold, safe).accountId
      return { publicKey, address: sdk.ss58FromPublic(publicKey, 42) }
    },
    composeCall: compose,
    decodeCall: async data => runtime.decodeCall(data),
    hashCall: async call => sdk.blake2_256(await compose(call)),
    hashCallData: sdk.blake2_256,
    specVersion: () => runtime.specVersion,
    estimateFee: async () => 1000n,
    estimateWeight: async () => ({ ref_time: 1n, proof_size: 0n }),
    constant: async () => 1000n,
  }
  return { state, reader, compose, runtime }
}

async function imported(fixture, wrapper, declared, committed) {
  const data = hex(await fixture.compose(committed)), hash = hex(sdk.blake2_256(Buffer.from(data.slice(2), 'hex')))
  fixture.state.pending.set(hash, { when: timepoint, approvals: [bob.ss58Address], deposit: 2000n })
  if (wrapper === 'execute_proxy_announced') {
    const { op, ...args } = declared
    return sdk.buildIntent(wrapper, { delegate_ss58: alice.ss58Address, real_ss58: bob.ss58Address,
      inner_op: op, inner_args: args, announced_call_data: data, expected_call_hash: hash, announced_spec_version: 419 })
  }
  return sdk.buildIntent(wrapper, { threshold: 2, other_signatories: [bob.ss58Address], call: declared,
    timepoint, call_data: data, call_hash: hash, call_spec_version: 419 })
}

function workflow(surface, fixture, t, context = {}) {
  const counts = { signatures: 0, submissions: 0 }
  const signer = { address: alice.ss58Address, publicKey: alice.publicKey, cryptoType: 1,
    sign: async () => { counts.signatures++; return Buffer.alloc(64) } }
  const prepared = intent => intent.prepare(fixture.reader, { coldkey: alice.ss58Address, ...context })
  if (surface === 'Node') {
    const client = { runtimeAt: async () => fixture.runtime, submit: async (_call, signer) => {
      await signer.sign(Buffer.from('payload')); counts.submissions++; return { success: true, events: [] }
    } }
    const workflow = new WorkflowClient(client)
    workflow.plan = async intent => {
      const intentPlan = await prepared(intent)
      return { intent: intentPlan, call: intentPlan.call, signerAddress: alice.ss58Address, shielded: false, ok: true, violations: [] }
    }
    return { counts, execute: intent => workflow.execute(intent, signer, { nonce: 0, mevShield: false }) }
  }
  const client = new browser.Client('http://fixture.invalid')
  t.after(() => client.close())
  client.planForSigner = async intent => {
    const intentPlan = await prepared(intent)
    return { prepared: intentPlan, call: intentPlan.call, violations: [], nonce: 0, signingOptions: {}, blockHash: 'pinned' }
  }
  client.submitPreparedCall = async (_call, signer) => {
    await signer.sign(Buffer.from('payload')); counts.submissions++; return { success: true, events: [] }
  }
  return { counts, execute: intent => client.execute(intent, signer, { nonce: 0, mevShield: false }) }
}

const attacks = [
  ['transfer_all instead of one rao', transfer, { pallet: 'Balances', call: 'transfer_all', params: { dest: charlie.ss58Address, keep_alive: true } }],
  ['different destination', transfer, payment(dave.ss58Address)],
  ['different amount', transfer, payment(charlie.ss58Address, 2n)],
  ['different keep-alive behavior', transfer, payment(charlie.ss58Address, 1n, 'transfer_allow_death')],
  ['undeclared batch sibling', { op: 'batch', intents: [transfer] }, { pallet: 'Utility', call: 'batch_all', params: { calls: [payment(), payment(dave.ss58Address, 10_000_000_000n)] } }],
]

for (const surface of ['Node', 'browser']) {
  // PROTECTED_REGRESSION: saved multisig and announced proxy bytes cannot spend beyond the declared intent before any signing; changes require explicit human approval.
  test(`${surface} rejects contradictory saved calls before signing or submission`, async t => {
    for (const wrapper of ['multisig_approve', 'multisig_execute', 'execute_proxy_announced']) {
      for (const [name, declared, committed] of attacks) await t.test(`${wrapper}: ${name}`, async t => {
        const setup = fixture(surface), execution = workflow(surface, setup, t)
        const intent = await imported(setup, wrapper, declared, committed)
        await assert.rejects(execution.execute(intent), /saved call bytes do not match the declared semantic intent/)
        assert.deepEqual(execution.counts, { signatures: 0, submissions: 0 })
      })
    }
  })

  // PROTECTED_REGRESSION: committed staking may preserve a derived price but cannot change the declared amount, beneficiary, subnet or explicit limit; changes require explicit human approval.
  test(`${surface} binds saved staking to every user-specified economic argument`, async t => {
    for (const wrapper of ['multisig_approve', 'multisig_execute', 'execute_proxy_announced']) {
      const setup = fixture(surface)
      const staking = { op: 'add_stake', hotkey_ss58: charlie.ss58Address, netuid: 1, amount_tao: '1' }
      const prepared = await sdk.buildIntent('add_stake', staking).prepare(setup.reader, { coldkey: alice.ss58Address })
      const mutations = [
        ['different hotkey', { ...staking, hotkey_ss58: dave.ss58Address }],
        ['different subnet', { ...staking, netuid: 2 }],
        ['different stake amount', { ...staking, amount_tao: '0.5' }],
        ['different explicit limit', { ...staking, op: 'add_stake_limit', limit_price_rao: BigInt(prepared.call.params.limit_price) + 1n }],
      ]
      for (const [name, declared] of mutations) await t.test(`${wrapper}: ${name}`, async t => {
        const execution = workflow(surface, setup, t), intent = await imported(setup, wrapper, declared, prepared.call)
        await assert.rejects(execution.execute(intent), /saved call bytes do not match the declared semantic intent/)
        assert.deepEqual(execution.counts, { signatures: 0, submissions: 0 })
      })
    }
  })

  // PROTECTED_REGRESSION: semantic binding must preserve valid committed payments and state-dependent staking through multisig execution; changes require explicit human approval.
  test(`${surface} accepts matching saved payments and preserves approved staking prices`, async t => {
    const matching = [
      [transfer, payment()],
      [{ ...transfer, dest_ss58: sdk.ss58FromPublic(charlie.publicKey, 0) }, payment()],
      [{ op: 'batch', intents: [transfer, { ...transfer, dest_ss58: dave.ss58Address, amount_tao: '0.000000002' }] },
        { pallet: 'Utility', call: 'batch_all', params: { calls: [payment(), payment(dave.ss58Address, 2n)] } }],
    ]
    for (const wrapper of ['multisig_approve', 'multisig_execute', 'execute_proxy_announced']) {
      for (const [declared, committed] of matching) {
        const setup = fixture(surface), execution = workflow(surface, setup, t)
        const intent = await imported(setup, wrapper, declared, committed)
        assert.equal((await execution.execute(intent)).success, true)
        assert.deepEqual(execution.counts, { signatures: 1, submissions: 1 })
      }
    }
    const setup = fixture(surface)
    const staking = { op: 'add_stake', hotkey_ss58: charlie.ss58Address, netuid: 1, amount_tao: '1' }
    const opening = await sdk.buildIntent('multisig_approve', { threshold: 2, other_signatories: [bob.ss58Address], call: staking })
      .prepare(setup.reader, { coldkey: alice.ss58Address })
    setup.state.pending.set(opening.extras.multisig_call_hash, { when: timepoint, approvals: [alice.ss58Address], deposit: 2000n })
    setup.state.price *= 2n
    const reads = setup.state.priceReads
    for (const wrapper of ['multisig_approve', 'multisig_execute']) {
      const followup = await sdk.buildIntent(wrapper, { threshold: 2, other_signatories: [alice.ss58Address], call: staking, timepoint,
        call_data: opening.extras.multisig_call_data, call_hash: opening.extras.multisig_call_hash,
        call_spec_version: opening.extras.multisig_call_spec_version }).prepare(setup.reader, { coldkey: bob.ss58Address })
      assert.equal(followup.extras.multisig_call_hash, opening.extras.multisig_call_hash)
      assert.equal(hex(await setup.compose(followup.call.params.call ?? followup.call.semanticCall)), opening.extras.multisig_call_data)
      assert.deepEqual(followup.call.params.maybe_timepoint, timepoint)
    }
    assert.equal(setup.state.priceReads, reads, 'saved staking execution must not re-read the current subnet price')
  })

  // PROTECTED_REGRESSION: saved stake movement batches must retain every approved movement and payment after mutable price changes; changes require explicit human approval.
  test(`${surface} replays movement batches without rebuilding approved calls`, async t => {
    const movements = [
      { op: 'move_stake', origin_hotkey_ss58: charlie.ss58Address, origin_netuid: 1, dest_hotkey_ss58: dave.ss58Address, dest_netuid: 2, amount_alpha: '1' },
      { op: 'move_swap_stake', origin_hotkey_ss58: charlie.ss58Address, origin_netuid: 1, dest_hotkey_ss58: dave.ss58Address, dest_netuid: 2, amount_alpha: '1' },
      { op: 'swap_stake', hotkey_ss58: charlie.ss58Address, origin_netuid: 1, dest_netuid: 2, amount_alpha: '1' },
      { op: 'transfer_stake', hotkey_ss58: charlie.ss58Address, origin_netuid: 1, dest_coldkey_ss58: dave.ss58Address, dest_netuid: 2, amount_alpha: '1' },
    ]
    for (const movement of movements) for (const paymentFirst of [false, true]) await t.test(`${movement.op}, payment ${paymentFirst ? 'first' : 'last'}`, async () => {
      const setup = fixture(surface)
      const declared = { op: 'batch', intents: paymentFirst ? [transfer, movement] : [movement, transfer] }
      const prepared = await sdk.intentFromJSON(declared).prepare(setup.reader, { coldkey: alice.ss58Address })
      const committed = hex(await setup.compose(prepared.call))
      setup.state.price *= 2n
      const reads = setup.state.priceReads
      for (const wrapper of ['multisig_approve', 'multisig_execute', 'execute_proxy_announced']) {
        const intent = await imported(setup, wrapper, declared, prepared.call)
        const followup = await intent.prepare(setup.reader, { coldkey: alice.ss58Address })
        assert.equal(hex(await setup.compose(followup.call.params.call ?? followup.call.semanticCall)), committed)
      }
      assert.equal(setup.state.priceReads, reads)
    })
  })

  // PROTECTED_REGRESSION: named Gamma services must preserve committed mint quotes while refusing unauthorized spender substitution before signing; changes require explicit human approval.
  test(`${surface} binds named Gamma service authority without requoting its committed mint`, async t => {
    for (const op of ['mint_gamma', 'mint_gamma_with_stake']) for (const wrapper of ['multisig_approve', 'multisig_execute', 'execute_proxy_announced']) await t.test(`${wrapper}: ${op}`, async t => {
      const setup = fixture(surface), block = '0x' + '22'.repeat(32)
      const origin = wrapper === 'execute_proxy_announced' ? bob.ss58Address : setup.reader.multisigAccount([alice.ss58Address, bob.ss58Address], 2).address
      const state = { quote: 10_000_000_000n, reads: 0, spender: dave.ss58Address }
      // The golden SCALE metadata predates Gamma; mock only its call-byte
      // boundary and chain state, keeping shared binding and workflows real.
      setup.compose = async call => Buffer.from(JSON.stringify({ pallet: call.pallet, call: call.call, params: call.params }, (_, value) => {
        if (typeof value === 'bigint') return value.toString()
        if (value && typeof value === 'object' && !Array.isArray(value)) return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)))
        return value
      }))
      const query = setup.reader.query
      Object.assign(setup.reader, {
        blockHash: async () => block,
        composeCall: setup.compose,
        hashCall: async call => sdk.blake2_256(await setup.compose(call)),
        decodeCall: async data => JSON.parse(Buffer.from(data).toString()),
        query: async (pallet, item, args, at) => {
          if (pallet === 'SubtensorModule') {
            assert.equal(at, block)
            if (item === 'Owner') return origin
            if (item === 'GammaDeposits') return null
            if (item === 'ColdkeyGammaHotkeys') return []
            if (item === 'Services') return { spender: state.spender }
          }
          return query(pallet, item, args)
        },
        runtimeCall: async (_api, method) => {
          if (method === 'get_stake_info_for_hotkey_coldkey_netuid') return { stake: 10_000_000_000n }
          assert.ok(method.startsWith('sim_mint_gamma'))
          state.reads++
          return { Ok: [1_000_000_000n, 1_000_000_000n, state.quote] }
        },
      })
      const declared = { op, netuid: 1, hotkey_ss58: charlie.ss58Address, service_hotkey_ss58: alice.ss58Address,
        ...(op === 'mint_gamma' ? { amount_tao: '1' } : { origin_hotkey_ss58: bob.ss58Address, amount_alpha: '1' }) }
      const prepared = await sdk.intentFromJSON(declared).prepare(setup.reader, { coldkey: origin })
      const committed = hex(await setup.compose(prepared.call)), reads = state.reads
      state.quote *= 2n
      const intent = await imported(setup, wrapper, declared, prepared.call)
      const followup = await intent.prepare(setup.reader, { coldkey: alice.ss58Address })
      assert.equal(hex(await setup.compose(followup.call.params.call ?? followup.call.semanticCall)), committed)
      assert.equal(state.reads, reads, 'service authority resolution must not change the committed quote')

      const changedService = workflow(surface, setup, t)
      state.spender = bob.ss58Address
      await assert.rejects(changedService.execute(intent), /saved call bytes do not match the declared semantic intent/)
      assert.deepEqual(changedService.counts, { signatures: 0, submissions: 0 })
      state.spender = dave.ss58Address
      const substituted = structuredClone(prepared.call)
      substituted.params.spender = bob.ss58Address
      const forgedIntent = await imported(setup, wrapper, declared, substituted), execution = workflow(surface, setup, t)
      await assert.rejects(execution.execute(forgedIntent), /saved call bytes do not match the declared semantic intent/)
      assert.deepEqual(execution.counts, { signatures: 0, submissions: 0 })
      assert.equal(state.reads, reads)
    })
  })

  // PROTECTED_REGRESSION: an announced multisig wrapper must preserve a nested Gamma quote while binding the inner amount and spender before signing; changes require explicit human approval.
  test(`${surface} preserves Gamma commitments inside announced multisig calls`, async t => {
    const modes = [
      { name: 'legacy', mode: 'legacy', args: { account_mode: 'legacy' }, context: {} },
      { name: 'safe', mode: 'safe', args: { account_mode: 'safe' }, context: {} },
      { name: 'inherited safe', mode: 'safe', args: {}, context: { accountMode: 'safe' } },
      { name: 'null inherited safe', mode: 'safe', args: { account_mode: null }, context: { accountMode: 'safe' } },
    ]
    for (const op of ['mint_gamma', 'mint_gamma_with_stake']) for (const nested of ['multisig_threshold_1', 'multisig_execute', 'multisig_approve']) for (const mode of modes) await t.test(`${nested}, ${mode.name}: ${op}`, async t => {
      const setup = fixture(surface), block = '0x' + '33'.repeat(32)
      const threshold = nested === 'multisig_threshold_1' ? 1 : 2
      const origin = setup.reader.multisigAccount([alice.ss58Address, bob.ss58Address], threshold, mode.mode === 'safe').address
      const state = { quote: 10_000_000_000n, reads: 0 }
      // Gamma is newer than the SCALE metadata fixture. Preserve a deterministic
      // nested call-byte boundary while exercising real intent reconstruction.
      setup.compose = async call => Buffer.from(JSON.stringify({ pallet: call.pallet, call: call.call, params: call.params }, (_, value) => {
        if (typeof value === 'bigint') return value.toString()
        if (value && typeof value === 'object' && !Array.isArray(value)) return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)))
        return value
      }))
      const query = setup.reader.query
      Object.assign(setup.reader, {
        blockHash: async () => block, composeCall: setup.compose,
        hashCall: async call => sdk.blake2_256(await setup.compose(call)),
        decodeCall: async data => JSON.parse(Buffer.from(data).toString()),
        query: async (pallet, item, args, at) => {
          if (pallet === 'SubtensorModule') {
            assert.equal(at, block)
            if (item === 'Owner') return origin
            if (item === 'GammaDeposits') return null
            if (item === 'ColdkeyGammaHotkeys') return []
            if (item === 'Services') return { spender: dave.ss58Address }
          }
          return query(pallet, item, args)
        },
        runtimeCall: async (_api, method) => {
          if (method === 'get_stake_info_for_hotkey_coldkey_netuid') return { stake: 10_000_000_000n }
          assert.ok(method.startsWith('sim_mint_gamma'))
          state.reads++
          return { Ok: [1_000_000_000n, 1_000_000_000n, state.quote] }
        },
      })
      const mint = { op, netuid: 1, hotkey_ss58: charlie.ss58Address, service_hotkey_ss58: alice.ss58Address,
        ...(op === 'mint_gamma' ? { amount_tao: '1' } : { origin_hotkey_ss58: bob.ss58Address, amount_alpha: '1' }) }
      const declared = { op: nested, other_signatories: [alice.ss58Address], call: mint, ...mode.args,
        ...(nested === 'multisig_threshold_1' ? {} : { threshold }) }
      if (nested === 'multisig_execute') {
        const inner = await sdk.intentFromJSON(mint).prepare(setup.reader, { coldkey: origin })
        setup.state.pending.set(hex(await setup.reader.hashCall(inner.call)), { when: timepoint, approvals: [alice.ss58Address], deposit: 2000n })
        declared.timepoint = timepoint
      }
      const prepared = await sdk.intentFromJSON(declared).prepare(setup.reader, { coldkey: bob.ss58Address, ...mode.context })
      const committed = hex(await setup.compose(prepared.call)), reads = state.reads
      state.quote *= 2n
      const intent = await imported(setup, 'execute_proxy_announced', declared, prepared.call)
      const followup = await intent.prepare(setup.reader, { coldkey: alice.ss58Address, ...mode.context })
      assert.equal(hex(await setup.compose(followup.call.params.call)), committed)
      assert.equal(state.reads, reads, 'the nested multisig must retain its original Gamma quote')
      for (const field of ['spender', op === 'mint_gamma' ? 'amount' : 'alpha_amount']) {
        const forged = structuredClone(prepared.call)
        forged.params.call.params[field] = field === 'spender' ? bob.ss58Address : 2_000_000_000n
        const forgedIntent = await imported(setup, 'execute_proxy_announced', declared, forged), execution = workflow(surface, setup, t, mode.context)
        await assert.rejects(execution.execute(forgedIntent), /saved call bytes do not match the declared semantic intent/)
        assert.deepEqual(execution.counts, { signatures: 0, submissions: 0 })
      }
      assert.equal(state.reads, reads)
    })
  })
}
