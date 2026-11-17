const test = require('node:test')
const assert = require('node:assert/strict')
const emission = require('../dist/emission-weights.js')
const { readRootClaimRelationships, quoteRootClaimPreviews } = require('../dist/root-claim.js')
const { createHttpAuth, clearVerifier, InMemoryNonceStore } = require('../dist/http-auth.js')
const { Keypair, publicKeyFromSs58, ss58FromPublic, verifySignature } = require('../dist/keys.js')
const { blake2_256 } = require('../dist/crypto.js')

test('emission burn and recycle retain omitted shares and share the exact combined cap', () => {
  const burn = emission.parseShare('10%'), recycle = emission.parseShare('15%')
  const validator = emission.parseShare('40%')
  const weights = [burn, recycle, 0n, validator, emission.ONE - burn - recycle - validator]
  assert.deepEqual(emission.validate(weights), [])
  assert.deepEqual(emission.mergeWeights(weights, { validator }), weights)
  const atCap = [emission.MAX_BURN_RECYCLE, 0n, 0n, 1n, emission.ONE - emission.MAX_BURN_RECYCLE - 1n]
  assert.deepEqual(emission.validate(atCap), [])
  const over = [...atCap]; over[1] = 1n; over[4]--
  assert.deepEqual(emission.validate(over).map(row => row.error), ['BurnRecycleEmissionAboveMaximum'])
  assert.equal(emission.mergeWeights(weights, { burn: 0n })[1], recycle)
})

test('emission renormalization includes all free slots and preserves exact rounding', () => {
  const before = [1n, 2n, 3n, 4n, 5n]
  const owner = emission.parseShare('20%')
  const after = emission.renormalize(before, { owner })
  const remaining = emission.ONE - owner
  assert.equal(after[0], remaining / 12n)
  assert.equal(after[1], remaining * 2n / 12n)
  assert.equal(after[2], owner)
  assert.equal(after[3], remaining * 4n / 12n)
  assert.equal(after.reduce((a, b) => a + b), emission.ONE)
  assert.equal(emission.renormalize(emission.defaultWeights(), { burn: emission.parseShare('10%') })[0], emission.parseShare('10%'))
  const zeroBase = emission.renormalize([], { miner: emission.parseShare('40%') })
  assert.equal(zeroBase[0], zeroBase[1])
  assert.equal(zeroBase.reduce((a, b) => a + b), emission.ONE)
})

test('HTTP verifier registries are copied, receiver-scoped, atomic and fail closed', async () => {
  const crypto = { publicKeyFromSs58, ss58FromPublic, blake2_256, verify: verifySignature }
  const auth = createHttpAuth(crypto), otherInstance = createHttpAuth(crypto)
  const signer = Keypair.fromUri('//Alice', 0), receiver = Keypair.fromUri('//Bob').ss58Address
  const nonceNs = 123456789000n, body = Buffer.from('trusted request')
  const headers = await auth.sign(signer, { method: 'POST', path: '/signed', body, receiverSs58: receiver, nonceNs })
  const options = () => ({ method: 'POST', path: '/signed', selfHotkeySs58: receiver, nowNs: nonceNs, nonceStore: new InMemoryNonceStore() })
  clearVerifier(receiver)
  try {
    await assert.rejects(auth.verify(headers, body, options()), { name: 'BadSignature' })
    const policy = new Map([[signer.ss58Address, 0]])
    auth.configureVerifier(receiver, { trustedSenderSchemes: policy })
    policy.set(signer.ss58Address, 1)
    assert.equal((await otherInstance.verify(headers, body, options())).cryptoType, 0)
    assert.throws(() => auth.configureVerifier(receiver, { trustedSenderSchemes: { [signer.ss58Address]: 7 } }), /unsupported trusted/)
    assert.equal((await auth.verify(headers, body, options())).cryptoType, 0)
    assert.throws(() => auth.configureVerifier(receiver, { trustedSenderSchemes: { [signer.ss58Address]: false } }), /unsupported trusted/)
    await assert.rejects(auth.verify(headers, body, { ...options(), selfHotkeySs58: signer.ss58Address }), { name: 'BadSignature' })
    await assert.rejects(auth.verify(headers, body, { ...options(), expectedCryptoType: 1 }), /trusted sender/)
    auth.configureVerifier(receiver, { trustedSenderSchemes: {} })
    await assert.rejects(auth.verify(headers, body, options()), { name: 'BadSignature' })
    assert.equal((await auth.verify(headers, body, { ...options(), expectedCryptoType: 0 })).hotkeySs58, signer.ss58Address)
    auth.configureVerifier(receiver, { trustedSenderSchemes: { [signer.ss58Address]: 0 } })
    otherInstance.clearVerifier(receiver)
    await assert.rejects(auth.verify(headers, body, options()), { name: 'BadSignature' })
  } finally { clearVerifier(receiver) }
})

function relationshipReader(states) {
  return {
    async query(_, item, [hotkey]) {
      const state = states[hotkey] ?? {}
      if (item === 'BasketClaimed') return state.watermark ?? 0n
      if (item === 'DeferredBasketDebts') return state.debts ?? {}
      if (item === 'DeferredBasketCashClaimed') return state.cashClaimed ?? 0n
      if (item === 'DeferredBasketCashRate') return state.cashRate ?? { bits: 0n }
      if (item === 'DeferredBasketCashReserve') return state.reserve ?? 0n
      throw new Error(item)
    },
    async queryMap(_, item, [hotkey]) { assert.equal(item, 'DeferredBasketRates'); return states[hotkey]?.rates ?? [] },
    async countStorageKeys(_, item, [hotkey], limit) { assert.equal(item, 'DeferredBasketDebts'); assert.equal(limit, 1); return states[hotkey]?.debtKey ? 1 : 0 },
  }
}

test('root claim relationships retain unstaked debt and cash claimants with exact signed state', async () => {
  const bits = 1n << 64n
  const states = {
    debt: { debtKey: true, debts: new Map([[7, { generation: 4n, amount: { bits: -3n * bits } }]]) },
    cash: { cashClaimed: -9007199254740993n, reserve: 9007199254740993n },
    root: { rates: [[7, { generation: 4n, rate: { bits: bits / 2n }, debt_count: 1 }]], cashRate: { bits: bits / 4n } },
    shares: { watermark: -2n },
    emptyDebt: { debtKey: true },
    unrelated: {},
  }
  const rows = await readRootClaimRelationships(relationshipReader(states), 'coldkey', Object.keys(states), new Set(['root']))
  assert.deepEqual(rows.filter(row => row.selected).map(row => row.hotkey), ['debt', 'cash', 'root', 'shares', 'emptyDebt'])
  assert.deepEqual(rows[0].deferredDebts, [{ netuid: 7, generation: 4n, amountBits: -3n * bits }])
  assert.equal(rows[1].deferredCash.claimedRao, -9007199254740993n)
  assert.equal(rows[1].deferredCash.reserveRao, 9007199254740993n)
  assert.equal(rows[2].deferredRates[0].rateBits, bits / 2n)
  assert.equal(rows[2].deferredCash.rateBits, bits / 4n)
  assert.deepEqual(rows.filter(row => row.hasUnquotedDeferredEntitlement).map(row => row.hotkey), ['debt', 'cash', 'root', 'emptyDebt'])
})

test('root claim relationship read failures and malformed counters cannot hide claimants', async () => {
  const reader = relationshipReader({})
  await assert.rejects(readRootClaimRelationships({ ...reader, countStorageKeys: async () => 2 }, 'coldkey', ['hotkey'], new Set()), /storage-key count/)
  await assert.rejects(readRootClaimRelationships({ ...reader, query: async () => { throw new Error('RPC unavailable') } }, 'coldkey', ['hotkey'], new Set()), /RPC unavailable/)
  await assert.rejects(readRootClaimRelationships(relationshipReader({ hotkey: { reserve: -1n } }), 'coldkey', ['hotkey'], new Set()), /nonnegative/)
})

test('deferred claims report unknown total yield instead of a false zero or loss', () => {
  const partial = quoteRootClaimPreviews([null], 10000000n, 500000n, 1, 256, 1)
  assert.equal(partial.previewComplete, false)
  assert.equal(partial.accruedRao, null)
  assert.equal(partial.redeemableRao, null)
  assert.equal(partial.losesMoney, null)
  assert.equal(partial.basketRedeemableRao, 0n)
  assert.equal(partial.unquotedHotkeys, 1)
  assert.equal(quoteRootClaimPreviews([null], 10000000n, 500000n, 1).redeemableRao, 0n)
  assert.throws(() => quoteRootClaimPreviews([null], 1n, 1n, 1, 256, 2), /exceed selected/)
})
