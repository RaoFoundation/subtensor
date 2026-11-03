const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { registrationDescriptor } = require('../dist/protected-accounts')

const sender = sdk.Keypair.fromUri('//Alice')
const real = sdk.Keypair.fromUri('//Bob')
const delegate = sdk.Keypair.fromUri('//Charlie')
const context = { coldkey: sender.ss58Address }
function fixture(crypto, registered = false) {
  const recipients = [21, 22].map(seed => sdk.Keypair.fromSeed(Buffer.alloc(32, seed), crypto))
  const accounts = new Map(recipients.map(key => [key.ss58Address, { descriptor: key.authorizationDescriptor() }]))
  const reader = {
    query: async (pallet, item, params) => {
      if (item === 'HashedAccountsEnabled') return true
      if (item === 'Accounts' || item === 'MlDsaAccounts') return registered ? accounts.get(params[0]) : null
      if (item === 'Account') return { data: { free: 10_000_000_000n } }
      if (item === 'NextKey') return null
      throw new Error(`unexpected query ${pallet}.${item}`)
    },
    constant: async () => 100n,
    supportsPqProfile: () => true,
    finalizedHead: async () => '0x' + '11'.repeat(32),
    decodeAddress: sdk.publicKeyFromSs58,
    parseRecipient: (address, explicit) => sdk.parseRecipient(address, sdk.decodeReceivingAddress, sdk.Keypair.fromDescriptor, sdk.publicKeyFromSs58, explicit),
    multisigAccount: () => ({ address: real.ss58Address, publicKey: real.publicKey }),
    hashCall: async () => new Uint8Array(32),
  }
  const intents = recipients.map((key, index) => ({ op: 'transfer', dest_ss58: key.receivingAddress(), amount_tao: String(index + 1) }))
  return { reader, recipients, intents }
}

for (const crypto of [sdk.CRYPTO_HASHED, sdk.CRYPTO_HASHED_ED25519, sdk.CRYPTO_MLDSA65, sdk.CRYPTO_FALCON512_HASHED, sdk.CRYPTO_FALCON1024_HASHED]) {
  for (const registered of [false, true]) {
    test(`batch keeps ${crypto} receiving guards atomic without nested batches (registered=${registered})`, async () => {
      const { reader, recipients, intents } = fixture(crypto, registered)
      const plan = await sdk.buildIntent('batch', { intents }).prepare(reader, context)
      assert.equal(plan.call.pallet, 'Utility')
      assert.equal(plan.call.call, 'batch_all')
      const calls = plan.call.params.calls
      assert.deepEqual(calls.map(call => [call.pallet, call.call]), [
        ['HashedAccounts', registered ? 'check_registered' : 'register'], ['Balances', 'transfer_keep_alive'],
        ['HashedAccounts', registered ? 'check_registered' : 'register'], ['Balances', 'transfer_keep_alive'],
      ])
      for (let index = 0; index < recipients.length; index++) {
        assert.deepEqual(calls[index * 2].params.descriptor, registrationDescriptor(recipients[index].authorizationDescriptor()))
        assert.equal(calls[index * 2 + 1].params.dest, recipients[index].ss58Address)
        assert.equal(calls[index * 2 + 1].params.value, BigInt(index + 1) * 1_000_000_000n)
      }
      assert.equal(plan.spendRao, 3_000_000_000n)
      assert.equal(plan.requiredFreeRao, registered ? undefined : 200n)
      assert.deepEqual(plan.extras.registrationSponsorships, registered ? [] : recipients.map(key => ({ payer: sender.ss58Address, recipient: key.ss58Address, depositRao: 100n })))
      if (!registered) {
        await assert.rejects(sdk.authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 199n }), /exceeds/)
        await sdk.authorizeRegistrationSponsorship(plan, { maxRegistrationDepositRao: 200n })
      }
    })
  }
}

test('announced proxies keep the flat receiving batch and sponsorship under the real account', async () => {
  const { reader, intents } = fixture(sdk.CRYPTO_MLDSA65)
  const proxy = sdk.buildIntent('execute_proxy_announced', { real_ss58: real.ss58Address, delegate_ss58: delegate.ss58Address, inner_op: 'batch', inner_args: { intents } })
  const plan = await proxy.prepare(reader, context)
  assert.equal(plan.call.call, 'proxy_announced')
  assert.equal(plan.call.params.real, real.ss58Address)
  assert.equal(plan.call.params.call.call, 'batch_all')
  assert.equal(plan.call.params.call.params.calls.length, 4)
  assert(plan.call.params.call.params.calls.every(call => call.pallet !== 'Utility'))
  assert(plan.extras.registrationSponsorships.every(quote => quote.payer === real.ss58Address))
  assert.equal(plan.requiredFreeAccount, real.ss58Address)
  assert.equal(plan.feePayer, sender.ss58Address)
  await assert.rejects(sdk.buildIntent('batch', { intents: [proxy] }).prepare(reader, context), /proxy with an inner batch/)
})

test('multisig receiving batches retain their own dispatch account and atomic boundary', async () => {
  const { reader, intents } = fixture(sdk.CRYPTO_HASHED)
  const multisig = sdk.buildIntent('multisig_threshold_1', { other_signatories: [delegate.ss58Address], call: { op: 'batch', intents } })
  const plan = await sdk.buildIntent('batch', { intents: [multisig] }).prepare(reader, context)
  const wrapper = plan.call.params.calls[0]
  assert.equal(wrapper.pallet, 'Multisig')
  assert.equal(wrapper.call, 'as_multi_threshold_1')
  assert.equal(wrapper.params.call.call, 'batch_all')
  assert.equal(wrapper.params.call.params.calls.length, 4)
  assert(wrapper.params.call.params.calls.every(call => call.pallet !== 'Utility'))
  assert(plan.extras.registrationSponsorships.every(quote => quote.payer === real.ss58Address))
  assert.equal(plan.requiredFreeAccount, real.ss58Address)
})

test('explicit nested batch intents remain unsupported', async () => {
  const { reader, intents } = fixture(sdk.CRYPTO_HASHED)
  await assert.rejects(sdk.buildIntent('batch', { intents: [{ op: 'batch', intents }] }).prepare(reader, context), /nested batches/)
})
