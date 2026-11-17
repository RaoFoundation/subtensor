'use strict'

const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { buildIntent } = require('../dist/intents')

const coldkey = sdk.Keypair.fromUri('//Alice')
const hotkey = sdk.Keypair.fromUri('//Bob').ss58Address
const canonical = coldkey.ss58Address
const alternate = sdk.ss58FromPublic(coldkey.publicKey, 0)
const available = 5_000_000_000n
const entry = { total: 8_000_000_000n, locked: 3_000_000_000n, available }

function reader(raw) {
  return {
    query: async () => { throw new Error('unexpected storage query') },
    runtimeCall: async (api, method, args) => {
      assert.equal(api, 'StakeInfoRuntimeApi')
      assert.equal(method, 'get_stake_availability_for_coldkeys')
      assert.equal(args[0].length, 1)
      assert.deepEqual(args[1], [1])
      return raw
    },
    decodeAddress: sdk.publicKeyFromSs58,
  }
}

// PROTECTED_REGRESSION: lock_stake(all) must lock available stake decoded as Maps; changes require explicit human approval.
test('lock all uses the selected subnet availability across decoded maps and SS58 prefixes', async () => {
  const subnets = [
    new Map([[1, entry], [2, { available: 99n }]]),
    new Map([['1', entry], ['2', { available: 99n }]]),
    { 1: entry, 2: { available: 99n } },
  ]
  for (const entries of subnets) {
    for (const raw of [new Map([[canonical, entries]]), { [canonical]: entries }]) {
      for (const address of [canonical, alternate]) {
        const plan = await buildIntent('lock_stake', { netuid: 1, amount_alpha: 'all', hotkey_ss58: hotkey })
          .prepare(reader(raw), { coldkey: address })
        assert.equal(plan.call.pallet, 'SubtensorModule')
        assert.equal(plan.call.call, 'lock_stake')
        assert.equal(plan.call.params.amount, available)
        assert.equal(plan.call.params.netuid, 1)
        assert.equal(plan.call.params.hotkey, hotkey)
      }
    }
  }
})

test('lock all still rejects absent and zero available stake', async () => {
  for (const raw of [new Map(), { [canonical]: {} }, new Map([[canonical, new Map([[2, entry]])]]),
    new Map([[canonical, new Map([[1, { ...entry, available: 0n }]])]])]) {
    await assert.rejects(buildIntent('lock_stake', { netuid: 1, amount_alpha: 'all', hotkey_ss58: hotkey })
      .prepare(reader(raw), { coldkey: canonical }), /no unlocked stake available/i)
  }
})
