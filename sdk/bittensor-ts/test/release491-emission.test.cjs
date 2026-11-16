'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const ew = require('../dist/emission-weights')
const { buildIntent, checkIntentPolicy } = require('../dist/intents')

const owner = ew.parseShare('18%'), validator = ew.parseShare('36%'), gamma = ew.parseShare('10%')
const weights = [0n, 0n, owner, validator, ew.ONE - owner - validator - gamma]
function reader(overrides = {}) {
  return {
    runtimeCall: async () => ({ weights, root_baseline_share: 1n, created_via_lease: false }),
    query: async (_pallet, name) => {
      if (name === 'SubnetGammaEmission') return [[7, gamma]]
      if (name === 'SubnetUidToLeaseId') return null
      if (name === 'SubnetOwner') return 'owner'
      if (name === 'LastRateLimitedBlock') return 0n
      throw new Error(`unexpected query ${name}`)
    },
    queryMap: async (_pallet, name) => name === 'SubnetGammaEmissionWhitelisted' ? [[7, true], [[9], true], [11, false]] : [[7, true], [9, true]],
    blockNumber: async () => 100,
    ...overrides,
  }
}
const prepare = (args, view = reader()) => buildIntent('set_subnet_emission_weights', { netuid: 2, slots: ['owner'], values: [owner], ...args }).prepare(view, { coldkey: 'owner' })

test('V2 emission updates retain unspecified Gamma targets and transmit all five fixed slots', async () => {
  const plan = await prepare({})
  assert.deepEqual(plan.violations, [])
  assert.equal(plan.call.call, 'sudo_set_subnet_emission_weights_v2')
  assert.deepEqual(plan.call.params.split, [['Burn', 0n], ['Recycle', 0n], ['Owner', owner], ['Validator', validator], ['Miner', weights[4]], [{ Gamma: { netuid: 7 } }, gamma]])
  assert.deepEqual(plan.preview.gamma, { before: { 7: gamma }, after: { 7: gamma } })
  assert.equal(plan.call.params.split.reduce((sum, [, share]) => sum + share, 0n), ew.ONE)
  assert.deepEqual(plan.netuids, [2, 7])
  assert.match(checkIntentPolicy(plan, { allowedNetuids: [2] }).join(), /netuid 7/)
})

test('Gamma-only changes remove explicit zero targets, sort remaining targets and extend subnet policy', async () => {
  const plan = await prepare({ slots: [], values: [], gamma: { 9: '5%', 7: 0 }, renormalize: true })
  assert.deepEqual(plan.violations, [])
  assert.deepEqual(plan.preview.gamma.after, { 9: ew.parseShare('5%') })
  assert.deepEqual(plan.call.params.split.at(-1), [{ Gamma: { netuid: 9 } }, ew.parseShare('5%')])
  assert.equal(plan.call.params.split.reduce((sum, [, share]) => sum + share, 0n), ew.ONE)
  assert.deepEqual(plan.netuids, [2, 7, 9])
  assert.match(checkIntentPolicy(plan, { allowedNetuids: [2, 7] }).join(), /netuid 9/)
  assert.match(plan.warnings.join(), /unconverted alpha/)
})

test('Gamma split preflight enforces target whitelist, activation, self-target, lease, count and per-target cap', async () => {
  for (const [updates, pattern] of [[{ 11: '1%' }, /GammaEmissionTargetNotWhitelisted/], [{ 2: '1%' }, /InvalidValue/], [{ 7: '11%' }, /GammaEmissionAboveMaximum/]]) {
    assert.match((await prepare({ gamma: updates, renormalize: true })).violations.join(), pattern)
  }
  const base = reader()
  const disabled = reader({ queryMap: async (pallet, name) => name === 'SubnetGammaEnabled' ? [] : base.queryMap(pallet, name) })
  assert.match((await prepare({}, disabled)).violations.join(), /GammaDisabled/)
  const leased = reader({ query: async (pallet, name) => name === 'SubnetUidToLeaseId' ? 0 : base.query(pallet, name) })
  assert.match((await prepare({}, leased)).violations.join(), /GammaEmissionNotSettableOnLeasedSubnet/)
  assert(ew.validateGamma(100, Object.fromEntries(Array.from({ length: 17 }, (_, n) => [n, 1n]))).some(row => row.error === 'TooManyGammaShares'))
  const noLeaseGamma = await prepare({ gamma: { 7: 0 }, renormalize: true }, leased)
  assert.deepEqual(noLeaseGamma.violations, [])
})

test('Gamma target inputs reject numeric aliases, invalid netuids and inexact parts', async () => {
  for (const gamma of [{ 7: '1%', '07': '2%' }, { '-1': '1%' }, { 65536: '1%' }, { '7.1': '1%' }, []]) await assert.rejects(prepare({ gamma }))
  assert.throws(() => ew.gammaEmissionShares([[7, 1n], [7, 2n]]), /duplicate/)
  assert.throws(() => ew.gammaEmissionShares([[7, Number.MAX_SAFE_INTEGER + 1]]), /exact integer/)
  assert.deepEqual(ew.gammaEmissionShares([[7, '9007199254740993']]), { 7: 9007199254740993n })
  await assert.rejects(prepare({ slots: [], values: [], gamma: {} }), /at least one/)
  await assert.rejects(prepare({ netuid: 0 }), /root has no emission/)
})

test('V2 rounding absorbs only at most 1000 parts through miners and preserves over-allocation errors', async () => {
  const small = await prepare({ values: [owner + 1n] })
  assert.deepEqual(small.violations, [])
  assert.equal(small.preview.after[4], weights[4] - 1n)
  assert.match((await prepare({ values: [owner + 1001n] })).violations.join(), /EmissionWeightsDoNotSumToOne/)
  const almost = ew.defaultWeights(); almost[4] -= 1000n
  assert.deepEqual(ew.absorbRounding(almost), ew.defaultWeights())
  assert.deepEqual(ew.renormalize(almost, Object.fromEntries(almost.map((v, i) => [i, v]))), ew.defaultWeights())
  const excess = ew.defaultWeights(); excess[4] += 1n
  assert.throws(() => ew.renormalize(excess, Object.fromEntries(excess.map((v, i) => [i, v]))), /exceed/)
  const unavailable = reader({ queryMap: async () => { throw new Error('archive unavailable') } })
  await assert.rejects(prepare({}, unavailable), /archive unavailable/)
})

test('Gamma whitelist root removal reports all-subnet effects while enabling remains scoped', async () => {
  for (const whitelisted of [false, true]) {
    const plan = await buildIntent('set_subnet_gamma_emission_whitelisted', { netuid: 7, whitelisted }).prepare(reader(), { coldkey: 'root' })
    assert.equal(plan.call.pallet, 'Sudo')
    assert.deepEqual(plan.call.params.call, { pallet: 'AdminUtils', call: 'sudo_set_subnet_gamma_emission_whitelisted', params: { netuid: 7, whitelisted } })
    assert.equal(plan.affectsAllSubnets, !whitelisted)
    assert.equal(checkIntentPolicy(plan, { allowedNetuids: [7] }).some(reason => reason.includes('all subnets')), !whitelisted)
  }
})
