'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { Intent } = require('../dist/intents')
const { Balance } = require('../dist/balance')
const { fundingBudget, controllerFunding, planControllerRefill, includeControllerRefill, pendingControllerRefillRao, fundController } = require('../dist/protected-funding')
const genesis = '0x' + 'ab'.repeat(32), block = '0x' + '45'.repeat(32)
const identity = { accountId(address) { const match = /^(?:id|alias):(\d+)$/.exec(address); if (!match) throw new Error('invalid account'); return new Uint8Array(32).fill(Number(match[1])) } }
const record = { controller_wallet: 'main', controller_ss58: 'id:1', protected_ss58: 'id:2', genesis_hash: genesis, delay_blocks: 10, setup: { status: 'ready' }, coldkey_swap: { status: 'announced' } }
function budget(changes = {}) {
  return fundingBudget({ controller: 'id:1', account: { data: { free: 0n, reserved: 93_000_000n } }, announcement_count: 0, announcement_deposit: 0n, deposit_base: 36_000_000n, deposit_factor: 68_000_000n, existential_deposit: 500n, announce_fee: 1000n, transfer_execution_fee: 2000n, staking_execution_fee: 5000n, fee_payer: 'id:1', ...changes })
}
function backend(options = {}) {
  const calls = [], submitted = []
  const view = {
    hash: block, genesisHash: genesis, block: 100, specVersion: 491, blockTime: 12,
    query: async (pallet, item, params) => {
      calls.push(['query', pallet, item, params, block])
      if (options.error) throw options.error
      switch (pallet + '.' + item) {
        case 'System.Account': return { data: { free: options.free ?? 200_000_000n, reserved: 0n, frozen: 0n } }
        case 'Proxy.Announcements': return [[], 0n]
        case 'Proxy.Proxies': return [options.grants ?? [{ delegate: 'alias:1', proxy_type: 'Any', delay: 10 }], 0n]
        case 'SubtensorModule.ColdkeySwapAnnouncements': return options.migration ?? null
        case 'SubtensorModule.ColdkeySwapDisputes': return options.dispute ?? null
        default: throw new Error('unexpected query ' + pallet + '.' + item)
      }
    },
    constant: async (pallet, name) => { calls.push(['constant', pallet, name, block]); return { AnnouncementDepositBase: 36_000_000n, AnnouncementDepositFactor: 68_000_000n, ExistentialDeposit: 500n }[name] },
    fee: async (call, payer) => { calls.push(['fee', call, payer, block]); return call.call === 'announce' ? 1000n : call.params.call.params.calls[1].call === 'add_stake' ? 5000n : 2000n },
  }
  return { ...identity, calls, submitted, view,
    snapshot: async () => { calls.push(['snapshot']); return view },
    submit: async (intent, options) => { submitted.push([intent, options]); return { status: 'finalized', success: true, events: [] } },
  }
}

test('funding budget counts refundable reserves once and preserves exact rao', () => {
  const state = budget()
  assert.equal(state.target_rao, 1_396_060_500n)
  assert.equal(state.warning_rao, 376_015_500n)
  assert.equal(state.next_announcement_deposit_rao, 104_000_000n)
  assert.equal(state.transfer_fee_rao, 3000n)
  assert.equal(state.staking_fee_rao, 6000n)
  assert.equal(state.low, true)
  assert.equal(state.can_announce, false)
  const reserved = budget({ announcement_count: 1, announcement_deposit: 104_000_000n })
  assert.equal(reserved.next_announcement_deposit_rao, 68_000_000n)
  assert.equal(reserved.target_rao, state.target_rao - 36_000_000n)
  const frozen = budget({ account: { data: { free: 500_000_000n, frozen: 450_000_000n, reserved: 10n ** 22n } } })
  assert.equal(frozen.available_rao, 50_000_000n)
  assert.equal(frozen.top_up_rao, frozen.target_rao - 50_000_000n)
  assert.equal(budget({ account: { data: { free: 1n, frozen: 2n, reserved: 0n } } }).available_rao, 0n)
  assert.throws(() => budget({ announce_fee: Number.MAX_SAFE_INTEGER + 1 }), /safe integer/)
  assert.throws(() => budget({ announcement_count: -1 }), /u32/)
  assert(Object.isFrozen(state))
})

test('live funding quotes actual announce/refill/execution envelopes at one snapshot without signing', async () => {
  const api = backend(), funding = await controllerFunding(api, record, { feePayer: 'id:4' })
  assert.equal(api.calls.filter(row => row[0] === 'snapshot').length, 1)
  assert.equal(funding.block_hash, block)
  assert.equal(funding.genesis_hash, genesis)
  assert.equal(funding.fee_payer, 'id:4')
  assert.equal(funding.target_rao, budget().target_rao)
  assert.equal(api.submitted.length, 0)
  const quotes = api.calls.filter(row => row[0] === 'fee')
  assert.equal(quotes.length, 3)
  for (const [, call, payer, selected] of quotes) {
    assert.equal(payer, 'id:4'); assert.equal(selected, block)
    assert.equal(call.pallet, 'Proxy')
    assert.equal(call.params.real, 'id:2')
    if (call.call === 'proxy_announced') {
      assert.equal(call.params.delegate, 'id:1')
      assert.equal(call.params.force_proxy_type, 'Any')
      assert.equal(call.params.call.call, 'batch_all')
      assert.equal(call.params.call.params.calls[0].params.dest, 'id:1')
    }
  }
  const error = new Error('RPC offline')
  await assert.rejects(controllerFunding(backend({ error }), record), failure => failure === error)
  await assert.rejects(controllerFunding(api, { ...record, genesis_hash: '0x' + 'cc'.repeat(32) }), /another chain/)
})

test('refill offers are opt-in, bounded and suppress duplicate or uninspectable pending payloads', () => {
  const intent = new Intent('transfer', { dest_ss58: 'id:5', amount_tao: '1' }), state = budget({ account: { data: { free: 200_000_000n, reserved: 0n } } })
  const offer = planControllerRefill(identity, record, intent, state)
  assert.equal(offer.offered, true)
  assert.equal(offer.requires_delayed, true)
  assert.equal(offer.amount_rao, state.top_up_rao)
  assert.equal(includeControllerRefill(intent, { ...offer, offered: false }), intent)
  assert.equal(planControllerRefill(identity, record, intent, state, { enabled: false }).reason, 'disabled')
  assert.equal(planControllerRefill(identity, record, intent, budget()).reason, 'external_funding_required')
  const pending = [{ status: 'pending', inner_op: 'batch', inner_args: { intents: [{ op: 'transfer', dest_ss58: 'alias:1', amount_tao: '0.000000007' }] } }]
  assert.equal(pendingControllerRefillRao(identity, 'id:1', pending), 7n)
  assert.equal(planControllerRefill(identity, record, intent, state, { pending }).reason, 'already_pending')
  assert.equal(planControllerRefill(identity, record, intent, state, { pending: [{ inner_op: 'transfer_all', inner_args: { dest_ss58: 'id:1' } }] }).reason, 'already_pending')
  assert.equal(planControllerRefill(identity, record, intent, state, { pending: [{ plaintext_missing: true }] }).reason, 'unknown_pending_payload')
  assert.equal(planControllerRefill(identity, record, intent, state, { pending: [{ ...pending[0], status: 'cancelled' }] }).offered, true)
  assert.equal(planControllerRefill(identity, record, intent, budget({ account: { data: { free: 10n ** 12n, reserved: 0n } } })).reason, 'sufficient_funds')
})

// PROTECTED_REGRESSION: controller refills must preserve the exact approved amount and ceiling rather than silently spending additional protected funds; changes require explicit human approval.
test('controller refill preserves approved rao and refuses an increased amount above the cap', () => {
  const state = budget({ account: { data: { free: 200_000_000n, reserved: 0n } } }), intent = new Intent('transfer', { dest_ss58: 'id:5', amount_tao: '1' })
  assert.throws(() => planControllerRefill(identity, record, intent, state, { maxRefillRao: state.top_up_rao - 1n }), /maxRefillRao/)
  const offer = planControllerRefill(identity, record, intent, state, { maxRefillRao: state.top_up_rao })
  const prepared = includeControllerRefill(intent, offer).toJSON()
  assert.equal(prepared.op, 'batch')
  assert.equal(prepared.intents[0].dest_ss58, 'id:1')
  assert.equal(Balance.fromTao(prepared.intents[0].amount_tao).rao, state.top_up_rao)
  assert.deepEqual(prepared.intents[1], intent.toJSON())
  assert(Object.isFrozen(offer))
})

test('refills remain in one flat delayed batch after every TAO-releasing operation', () => {
  const state = budget({ account: { data: { free: 200_000_000n, reserved: 0n } } })
  const original = new Intent('batch', { intents: [
    { op: 'remove_stake', hotkey_ss58: 'id:7', netuid: 0, amount_alpha: '2' },
    { op: 'claim_root', subnets: [1] },
    { op: 'add_stake', hotkey_ss58: 'id:7', netuid: 0, amount_tao: '1' },
  ] })
  const prepared = includeControllerRefill(original, planControllerRefill(identity, record, original, state)).toJSON()
  assert.deepEqual(prepared.intents.map(row => row.op), ['remove_stake', 'claim_root', 'transfer', 'add_stake'])
  assert.deepEqual(prepared.intents.filter(row => row.op !== 'transfer'), original.toJSON().intents)
  assert.equal(prepared.intents[2].dest_ss58, 'id:1')
  const basket = new Intent('stake_into_basket', { hotkey_ss58: 'id:7', amount_tao: '1' })
  assert.equal(planControllerRefill(identity, record, basket, state).offered, true)
})

test('external controller funding checks live authority, migration, selected chain and approved amount', async () => {
  for (const options of [{ grants: [{ delegate: 'id:9', proxy_type: 'Any', delay: 10 }] }, { grants: [{ delegate: 'id:1', proxy_type: 'Any', delay: 0 }] }, { migration: [50, 'commitment'] }, { dispute: 0 }]) {
    const api = backend(options)
    await assert.rejects(fundController(api, record, 'id:4'), /live delayed Any|migration/)
    assert.equal(api.submitted.length, 0)
  }
  const api = backend(), quoted = await controllerFunding(api, record)
  await assert.rejects(fundController(api, record, 'alias:1'), /separate funding/)
  await assert.rejects(fundController(api, record, 'id:2'), /separate funding/)
  await assert.rejects(fundController(api, record, 'id:4', { expectedFundingRao: quoted.top_up_rao - 1n }), /changed since approval/)
  await assert.rejects(fundController(api, record, 'id:4', { maxFundingRao: quoted.top_up_rao - 1n }), /maxFundingRao/)
  assert.equal(api.submitted.length, 0)
  const result = await fundController(api, record, 'id:4', { expectedFundingRao: quoted.top_up_rao, maxFundingRao: quoted.top_up_rao })
  assert.equal(result.receipt.success, true)
  assert.equal(api.submitted[0][1].signerAddress, 'id:4')
  assert.equal(api.submitted[0][0].args.dest_ss58, 'id:1')
  assert.equal(Balance.fromTao(api.submitted[0][0].toJSON().amount_tao).rao, quoted.top_up_rao)
  const funded = backend({ free: 10n ** 12n })
  assert.equal((await fundController(funded, record, 'id:4')).receipt, null)
  assert.equal(funded.submitted.length, 0)
})

test('refill insertion rejects static whole-balance spends while retaining runtime transfer-all', () => {
  const state = budget({ account: { data: { free: 200_000_000n, reserved: 0n } } })
  for (const op of ['add_stake', 'add_stake_limit', 'stake_into_basket']) {
    const intent = new Intent(op, { hotkey_ss58: 'id:7', amount_tao: 'all', ...(op === 'stake_into_basket' ? {} : { netuid: 1 }), ...(op === 'add_stake_limit' ? { limit_price_rao: 1000000000n } : {}) })
    const offer = planControllerRefill(identity, record, intent, state)
    assert.equal(offer.offered, true)
    assert.throws(() => includeControllerRefill(intent, offer), { name: 'TypeError', message: /explicit amount or fund the controller separately/ })
    assert.equal(includeControllerRefill(intent, { ...offer, offered: false }), intent)
  }
  const batch = new Intent('batch', { intents: [{ op: 'contribute_crowdloan', crowdloan_id: 0, amount_tao: 'all' }] })
  assert.throws(() => includeControllerRefill(batch, planControllerRefill(identity, record, batch, state)), /static free-balance/)
  for (const intent of [new Intent('transfer_all', { dest_ss58: 'id:7' }), new Intent('transfer', { dest_ss58: 'id:7', amount_tao: 'all' })]) {
    const combined = includeControllerRefill(intent, planControllerRefill(identity, record, intent, state)).toJSON()
    assert.equal(combined.intents[0].op, 'transfer')
    assert.equal(combined.intents[0].dest_ss58, record.controller_ss58)
    assert.deepEqual(combined.intents[1], intent.toJSON())
  }
})
