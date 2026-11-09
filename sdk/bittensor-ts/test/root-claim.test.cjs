const test = require('node:test')
const assert = require('node:assert/strict')
const { rootClaimChargedFeeRao, rootClaimSpentFeeRao, quoteRootClaimPreviews } = require('../dist/root-claim.js')

test('claim fee arithmetic agrees with the runtime combined-unit fee model', () => {
  // Independently evaluated from runtime/src/staking_fee.rs::root_claim_fee_weight
  // and the benchmarked claim_root/claim_root_scan functions, including DB weights.
  for (const [limit, expected] of [[0, 877912n], [1, 2506222n], [2, 4162033n], [3, 5817843n], [4, 7473653n], [130, 7473653n], [256, 7473653n]]) {
    assert.equal(rootClaimChargedFeeRao(limit), expected)
  }
  for (const [work, expected] of [
    [[1, 0, 0, 0], 4023589n], [[1, 1, 0, 0], 4023589n],
    [[1, 0, 100, 0], 10000000n], [[10, 2, 10, 15], 10000000n],
  ]) assert.equal(rootClaimSpentFeeRao(10000000n, ...work, 130), expected)
})

test('dust, threshold no-ops and sweep work remain distinct in claim previews', () => {
  const rows = [
    { accrued_tao: 3000000n, redeemable_tao: 2500000n, forfeited_tao_est: 500000n, rows: 4, rows_to_sell: 2, dust_rows: 2, swept: 1 },
    { accrued_tao: 400000n, redeemable_tao: 400000n, forfeited_tao_est: 0n, rows: 10, rows_to_sell: 10, dust_rows: 0, swept: 0 },
    null,
  ]
  const quote = quoteRootClaimPreviews(rows, 10000000n, 500000n, 3)
  assert.equal(quote.accruedRao, 3400000n)
  assert.equal(quote.redeemableRao, 2500000n)
  assert.equal(quote.eligibleHotkeys, 1)
  assert.equal(quote.belowThresholdHotkeys, 1)
  assert.equal(quote.dustRows, 2)
  assert.equal(quote.forfeitedRao, 500000n)
  assert.equal(quote.spentFeeFloorRao, rootClaimSpentFeeRao(10000000n, 3, 3, 12, 3))
  assert.equal(quote.losesMoney, true)
})
