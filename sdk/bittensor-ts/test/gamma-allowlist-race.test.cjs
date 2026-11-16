'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { buildIntent, checkIntentPolicy } = require('../dist/intents')
const emission = require('../dist/emission-weights')

// PROTECTED_REGRESSION: a prepared Gamma replacement cannot redirect a later target's emission outside the subnet allowlist; changes require explicit human approval.
test('Gamma replacements retain global scope when another transaction adds a target', async () => {
  const owner = emission.parseShare('18%'), validator = emission.parseShare('36%'), gamma = emission.parseShare('10%')
  let shares = [[7, gamma]]
  const reader = {
    runtimeCall: async () => ({ weights: [0n, 0n, owner, validator, emission.ONE - owner - validator - gamma], root_baseline_share: 1n, created_via_lease: false }),
    query: async (_pallet, item) => {
      if (item === 'SubnetGammaEmission') return shares
      if (item === 'SubnetUidToLeaseId') return null
      if (item === 'SubnetOwner') return 'owner'
      if (item === 'LastRateLimitedBlock') return 0n
      throw Error(`unexpected query ${item}`)
    },
    queryMap: async () => [[7, true], [8, true]],
    blockNumber: async () => 100,
  }
  const intent = buildIntent('set_subnet_emission_weights', { netuid: 1, slots: ['owner'], values: [owner] })
  const prepared = await intent.prepare(reader, { coldkey: 'owner' })
  const batch = await buildIntent('batch', { intents: [intent] }).prepare(reader, { coldkey: 'owner' })
  assert.deepEqual(prepared.netuids, [1, 7])
  assert.deepEqual(prepared.violations, [])
  // The prepared split has no target 8 and would remove its newly added share.
  shares = [[7, gamma], [8, emission.parseShare('5%')]]
  assert.equal((await reader.query('SubtensorModule', 'SubnetGammaEmission')).at(-1)[0], 8)
  assert.equal(prepared.call.params.split.some(([slot]) => slot?.Gamma?.netuid === 8), false)
  for (const plan of [prepared, batch]) {
    assert.equal(plan.affectsAllSubnets, true)
    assert.match(checkIntentPolicy(plan, { allowedNetuids: [1, 7] }).join(';'), /all subnets/)
    assert.deepEqual(checkIntentPolicy(plan, {}), [])
  }
})

// PROTECTED_REGRESSION: browser policy must reject the same unauthorized Gamma replacement as native policy; changes require explicit human approval.
test('native and browser Gamma metadata keep all-subnet policy through batches', async () => {
  const fs = require('node:fs/promises'), path = require('node:path'), { pathToFileURL } = require('node:url')
  const { policyEvaluate } = require('../dist/policy')
  const binding = await import(pathToFileURL(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.js')).href)
  const { instance } = await WebAssembly.instantiate(await fs.readFile(path.join(__dirname, '../dist/wasm/bittensor_core_wasm_bg.wasm')), { './bittensor_core_wasm_bg.js': binding })
  binding.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start?.()
  for (const gamma of [{}, { 7: '0' }, { 7: '5%' }]) {
    const input = JSON.stringify({ op: 'set_subnet_emission_weights', args: { netuid: 1, gamma } })
    const native = JSON.parse(policyEvaluate('transaction.intent_metadata', input)).value
    const browser = JSON.parse(binding.policyEvaluate('transaction.intent_metadata', input)).value
    assert.deepEqual(browser, native)
    assert.equal(browser.affects_all_subnets, true)
    const batch = JSON.parse(binding.policyEvaluate('transaction.merge_intents', JSON.stringify({ children: [browser] }))).value
    const verdict = JSON.parse(binding.policyEvaluate('transaction.intent_policy', JSON.stringify({ ...batch, allowed_netuids: [1, 7] }))).value
    assert(verdict.violations.some(item => item.code === 'all_subnets'))
  }
})
