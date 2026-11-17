const test = require('node:test')
const assert = require('node:assert/strict')
const { buildIntent } = require('../dist/intents.js')
const { quoteRegistrationDeposit } = require('../dist/registration-deposit.js')
const { authorizeRegistrationSponsorship } = require('../dist/registration-sponsorship.js')

const descriptor = Uint8Array.from([1, 1, ...Array(32).fill(7)])
function fixture(amount = 700_000_000n, configurable = true) {
  const reads = []
  const reader = {
    blockHash: async () => '0xquoted',
    hasStorage: async (pallet, item, block) => {
      reads.push(['metadata', pallet, item, block])
      return configurable
    },
    query: async (pallet, item, params, block) => {
      reads.push(['query', pallet, item, block])
      if (item === 'HashedAccountRegistrationDeposit') return amount
      if (item === 'HashedAccountsEnabled') return true
      if (item === 'Accounts') return null
      if (item === 'MlDsaAccounts') return params[0] === 'sender' ? { descriptor } : null
      if (item === 'Account') return { data: { free: 10_000_000_000n } }
      if (item === 'NextKey') return null
      throw new Error(`unexpected query ${pallet}.${item}`)
    },
    constant: async (pallet, name, block) => {
      reads.push(['constant', pallet, name, block])
      return pallet === 'HashedAccounts' ? 200_000_000n : 10n
    },
    parseRecipient: () => ({ account: 'recipient', descriptor }),
    decodeAddress: address => Uint8Array.from({ length: 32 }, () => address === 'sender' ? 0 : 1),
    supportsPqProfile: () => true,
  }
  return { reader, reads }
}

// PROTECTED_REGRESSION: live registration reserve increases must not reserve more funds than the sponsor approved; changes require explicit human approval.
test('a raised live reserve is refused before signing and the approved amount is guarded on chain', async () => {
  const { reader, reads } = fixture()
  const intent = buildIntent('transfer', { dest_ss58: 'tao_recipient', amount_tao: '1' })
  const plan = await intent.prepare(reader, { coldkey: 'sender' })
  let prompted = false
  await assert.rejects(authorizeRegistrationSponsorship(plan, {
    maxRegistrationDepositRao: 200_000_000n,
    onRegistrationSponsorship: () => { prompted = true; return true },
  }), /exceeds maxRegistrationDepositRao/)
  assert.equal(prompted, false)
  await authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 700_000_000n })
  assert.equal(plan.requiredFreeRao, 700_000_000n)
  assert.equal(plan.extras.registrationSponsorships[0].depositRao, 700_000_000n)
  assert.equal(plan.call.call, 'batch_all')
  assert.deepEqual(plan.call.params.calls.map(call => call.call), ['check_registration_deposit', 'register', 'transfer_keep_alive'])
  assert.deepEqual(plan.call.params.calls[0].params, { max_deposit: 700_000_000n })
  assert.deepEqual(reads.filter(row => row[2] === 'HashedAccountRegistrationDeposit'), [
    ['metadata', 'HashedAccounts', 'HashedAccountRegistrationDeposit', '0xquoted'],
    ['query', 'HashedAccounts', 'HashedAccountRegistrationDeposit', '0xquoted'],
  ])
  assert.equal(reads.some(row => row[0] === 'constant'), false)
})

test('zero live reserve remains zero and retains a zero maximum guard', async () => {
  const { reader } = fixture(0n)
  const plan = await buildIntent('transfer', { dest_ss58: 'tao_recipient', amount_tao: '1' }).prepare(reader, { coldkey: 'sender' })
  await authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 0n })
  assert.equal(plan.call.params.calls[0].params.max_deposit, 0n)
  assert.equal(plan.requiredFreeRao, undefined)
})

test('only metadata absence permits legacy constants; read and metadata failures fail closed', async () => {
  const { reader, reads } = fixture(700_000_000n, false)
  assert.deepEqual(await quoteRegistrationDeposit(reader, 'RegistrationDeposit'), { amountRao: 200_000_000n, configurable: false })
  assert.deepEqual(reads.at(-1), ['constant', 'HashedAccounts', 'RegistrationDeposit', '0xquoted'])
  assert.equal(reads.some(row => row[0] === 'query'), false)
  for (const changes of [
    { hasStorage: undefined },
    { hasStorage: async () => { throw new Error('metadata unavailable') } },
    { hasStorage: async () => true, query: async () => { throw new Error('RPC unavailable') } },
    { hasStorage: async () => true, query: async () => null },
  ]) {
    await assert.rejects(quoteRegistrationDeposit({ ...reader, ...changes, constant: async () => assert.fail('must not use stale constant') }, 'RegistrationDeposit'))
  }
})

test('protected pure proxies quote the shared live reserve and guard their atomic creation', async () => {
  const { reader } = fixture()
  const plan = await buildIntent('create_pure_proxy', {}).prepare(reader, { coldkey: 'sender' })
  assert.equal(plan.requiredFreeRao, 700_000_020n)
  assert.equal(plan.call.call, 'batch_all')
  assert.deepEqual(plan.call.params.calls.map(call => call.call), ['check_registration_deposit', 'create_protected_pure'])
  assert.equal(plan.call.params.calls[0].params.max_deposit, 700_000_000n)
})
