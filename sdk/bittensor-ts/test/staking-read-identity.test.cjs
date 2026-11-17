'use strict'

// Regression coverage for SS58-prefix stake lookups and root unstaking quotes.

const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const { Balance } = require('../dist/balance')
const { ss58FromPublic } = require('../dist/keys')
const { Snapshot: NodeSnapshot } = require('../dist/client')
const { Snapshot: BrowserSnapshot } = require('../dist/browser-client')
const browser = require('../dist/browser')

test.before(async () => {
  await browser.initBrowser(async () => {
    const bindings = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
    const { instance } = await WebAssembly.instantiate(
      fs.readFileSync(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')),
      { './bittensor_core_wasm_bg.js': bindings },
    )
    bindings.__wbg_set_wasm(instance.exports)
    instance.exports.__wbindgen_start?.()
    const module = { ...bindings }
    delete module.default
    return module
  })
})

const canonical = ss58FromPublic(Buffer.alloc(32, 1), 42)
const alternate = ss58FromPublic(Buffer.alloc(32, 1), 0)
const missing = ss58FromPublic(Buffer.alloc(32, 2), 0)
const hotkey = ss58FromPublic(Buffer.alloc(32, 3), 42)

function snapshot(Snapshot, runtime) {
  return new Snapshot({
    runtime: async (descriptor, params) => {
      const value = runtime[descriptor.join('.')]
      assert.notEqual(value, undefined, `unexpected runtime call ${descriptor.join('.')}`)
      return typeof value === 'function' ? value(params) : value
    },
    queryMap: async descriptor => {
      assert.deepEqual(descriptor, ['SubtensorModule', 'SubnetState'])
      return [[0, 'Started'], [2, 'Started']]
    },
    balance: (rao, netuid, symbol) => Balance.fromRao(rao, netuid, symbol),
  }, 123, '0x' + 'ab'.repeat(32))
}

for (const [backend, Snapshot] of [['Node', NodeSnapshot], ['browser', BrowserSnapshot]]) {
  test(`${backend} stake valuation matches AccountIds while retaining requested address keys`, async () => {
    const amount = 9007199254740997n
    const rows = [
      { hotkey, coldkey: canonical, netuid: 0, stake: amount, is_registered: true },
      { hotkey, coldkey: canonical, netuid: 2, stake: amount, is_registered: true },
    ]
    const view = snapshot(Snapshot, {
      'StakeInfoRuntimeApi.get_stake_info_for_coldkeys': [[canonical, rows]],
      'SwapRuntimeApi.current_alpha_price_all': [{ netuid: 2, price: 1234567890n }],
    })
    const expected = amount + amount * 1234567890n / 1000000000n
    const single = await view.read('stake_value_for_coldkey', { coldkey_ss58: alternate })
    assert.equal(single.coldkey, alternate)
    assert.equal(single.stake_value.rao, expected)
    assert.equal(single.block, 123)
    assert.deepEqual(single.positions.map(row => [row.coldkey, row.stake.netuid, row.stake.rao]), [
      [canonical, 0, amount], [canonical, 2, amount],
    ])
    const batch = await view.read('stake_value_for_coldkeys', { coldkey_ss58s: [alternate, canonical, alternate, missing] })
    assert.deepEqual(Object.keys(batch), [alternate, canonical, missing])
    for (const address of [alternate, canonical]) {
      assert.equal(batch[address].coldkey, address)
      assert.equal(batch[address].stake_value.rao, expected)
      assert.equal(batch[address].positions.length, 2)
    }
    assert.equal(batch[missing].stake_value.rao, 0n)
    assert.deepEqual(batch[missing].positions, [])
  })

  for (const map of [false, true]) {
    test(`${backend} stake availability matches AccountIds in ${map ? 'Map' : 'object'} records`, async () => {
      const amount = 9007199254740997n
      const row = { total: amount + 2n, locked: 2n, available: amount }
      const root = { total: 30n, locked: 10n, available: 20n }
      const entries = map ? new Map([['2', row], [0, root]]) : { 2: row, 0: root }
      const view = snapshot(Snapshot, {
        'StakeInfoRuntimeApi.get_stake_availability_for_coldkeys': ([addresses, ids]) => {
          assert.equal(addresses.length, 1)
          assert.deepEqual(ids, [...new Set(ids)].sort((a, b) => a - b))
          return map ? new Map([[canonical, entries]]) : { [canonical]: entries }
        },
      })
      for (const address of [alternate, canonical]) {
        const single = await view.read('stake_availability', { coldkey_ss58: address, netuid: 2 })
        assert.equal(single.available.rao, amount)
        assert.equal(single.available.netuid, 2)
        const batch = await view.read('stake_availability_for_coldkey', { coldkey_ss58: address, netuids: [2, 0, 2, 1] })
        assert.deepEqual(batch.map(row => [row.netuid, row.total.rao, row.locked.rao, row.available.rao]), [
          [0, 30n, 10n, 20n], [1, 0n, 0n, 0n], [2, amount + 2n, 2n, amount],
        ])
        assert.equal(batch[0].available.netuid, 0)
      }
      const empty = await view.read('stake_availability', { coldkey_ss58: missing, netuid: 2 })
      assert.equal(empty.available.rao, 0n)
      assert.deepEqual(await view.read('stake_availability_for_coldkey', { coldkey_ss58: alternate, netuids: [] }), [])
    })
  }

  for (const netuid of [0, 2]) {
    // PROTECTED_REGRESSION: root unstaking quotes must remain usable with TAO denomination; changes require explicit human approval.
    test(`${backend} unstaking quote preserves exact ${netuid === 0 ? 'root TAO' : 'subnet alpha'} denomination`, async () => {
      const view = snapshot(Snapshot, {
        'SwapRuntimeApi.sim_swap_alpha_for_tao': ([id, amount]) => {
          assert.equal(id, netuid)
          assert.equal(amount, 9007199254740997n)
          return { tao_amount: 7n, alpha_amount: amount, tao_fee: 1n, alpha_fee: 2n, tao_slippage: 3n, alpha_slippage: 4n }
        },
      })
      const quote = await view.read('quote_unstake', { netuid, amount_alpha: '9007199.254740997' })
      assert.equal(quote.alpha.rao, 9007199254740997n)
      assert.equal(quote.alpha.netuid, netuid)
      assert.equal(quote.alpha_fee.netuid, netuid)
      assert.equal(quote.alpha_slippage.netuid, netuid)
      assert.equal(quote.tao.netuid, 0)
      assert.equal(quote.tao.rao, 7n)
      if (netuid === 0) assert.equal(quote.alpha.taoString, '9007199.254740997')
    })
  }
}
