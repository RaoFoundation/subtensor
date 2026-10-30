'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const b = require('../dist/basket-index.js')
const { Balance } = require('../dist/balance.js')
const close = (a, c) => assert.ok(Math.abs(a - c) < 1e-12 * Math.max(1, Math.abs(c)), `${a} != ${c}`)

test('frozen index samples and migrated fund yield match Python fallback formulas', () => {
  assert.equal(b.indexLevel(b.BASKET_INDEX_EPOCH_BLOCK - 1), 1)
  assert.equal(b.indexLevel(b.BASKET_INDEX_EPOCH_BLOCK + 3599), 1)
  close(b.indexLevel(), 1.1196418675452464)
  assert.equal(b.BASKET_INDEX_GENERATED_BLOCK, 8925961)
  const [hotkey, [baseline, first, rate]] = Object.entries(b.BASKET_BASELINES)[0]
  const record = { hotkey, beta_price_tao: 2, nav_tao: Balance.fromTao(10), spot_nav_tao: Balance.fromTao(20), beta: 3, basket_rate: rate + 0.5 }
  assert.equal(b.spotBetaPrice(record), 4)
  assert.equal(b.stakerYield(record), 2)
  const output = b.normalizeBasketPosition(record)
  assert.equal(output, record)
  close(output.display_price_tao, 4 / baseline)
  close(output.display_beta * output.display_price_tao, 12)
  close(output.stake_value, 3 * b.trIndexLevel(first))
  assert.equal(b.basketAgeDays(first, first + 7200), 1)
  assert.equal(b.basketAgeDays(null, first), null)
  assert.equal(b.stakerYield({ ...record, basket_rate: rate - 1 }), 0)
})
test('chain pricing is canonical and unpriced rows use the same chain index', () => {
  const priced = { hotkey: 'new', beta: 5, chain_pricing: { display_price: 2, bag_index: 1.5, stake_price: 3, stake_index: 2.5, spot_price: 4, first_block: 0, provisional: true, staker_yield: 0.25, staker_twr: 1.4 } }
  const unpriced = { hotkey: 'unknown', beta_price_tao: 7, nav_tao: 2, basket_rate: 0.5 }
  assert.equal(b.normalizeBasketPositions([priced, unpriced]), 1.5)
  assert.equal(priced.display_beta, 10)
  assert.equal(priced.display_price_tao, 2)
  assert.equal(priced.index_first_block, null)
  assert.equal(priced.staker_twr, 1.4)
  assert.equal(priced.stake_value, 3)
  close(priced.stake_vs_index, 0.2)
  assert.equal(unpriced.basket_index, 1.5)
  assert.equal(unpriced.stake_index, 2.5)
})
test('fallback board uses spot weights, excludes dust, and anchors new funds provisionally', () => {
  const [hotkey, [baseline]] = Object.entries(b.BASKET_BASELINES)[0]
  const rows = [
    { hotkey, beta_price_tao: baseline * 2, nav_tao: 1, spot_nav_tao: 2 },
    { hotkey, beta_price_tao: baseline, nav_tao: 4 },
    { hotkey, beta_price_tao: baseline * 1000, nav_tao: 0.099 },
  ]
  close(b.normalizeBasketPositions(rows), (2 * 4 + 4 * 1) / 6)
  assert.equal(rows[2].display_price_tao, 1000)
  const unknown = b.normalizeBasketPosition({ hotkey: 'toString', beta_price_tao: 10, beta: 3 })
  assert.equal(unknown.index_provisional, true)
  close(unknown.vs_index, 0)
  close(unknown.display_beta * unknown.display_price_tao, 30)
  assert.equal(unknown.stake_value, null)
  assert.equal(b.normalizeBasketPositions([]), b.indexLevel())
})
