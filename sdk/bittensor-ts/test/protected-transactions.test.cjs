const test = require('node:test')
const assert = require('node:assert/strict')
const sdk = require('../dist')
const { ProtectedWalletClient } = sdk
function littleU64(value) { const bytes = Buffer.alloc(8); bytes.writeBigUInt64LE(BigInt(value)); return bytes }
const remark = {pallet:'System',call:'remark',params:{remark:new Uint8Array(400).fill(4)}}
function fixture(crypto = sdk.CRYPTO_HASHED) {
  const key = sdk.Keypair.fromSeed(Buffer.alloc(32,34),crypto)
  const state = {generation:2n,nonce:3,pending:3,enabled:true,fee:10n,submissions:[],signatures:[],queries:[],encoded:[]}
  const runtime = {
    supportsMldsaProfile: () => true,
    composeCall: (p,f,a) => p === 'MevShield' ? Buffer.concat([Buffer.from([9,0]),Buffer.from(a.ciphertext)]) : Buffer.concat([Buffer.from([0,0]),Buffer.from(a.remark ?? [2,3])]),
    signaturePayloadParts: params => ({includedInExtrinsic: littleU64(params.nonce), includedInSignedData: Buffer.alloc(64,8)}),
    encodeSignedExtrinsic: (call,account,proof,crypto,params) => {state.encoded.push({call,proof,params});return {bytes:Buffer.concat([Buffer.from(proof),Buffer.from(call)]),hash:Buffer.alloc(32,7)}},
  }
  const adapter = {
    decodeAddress:sdk.publicKeyFromSs58,hash:sdk.blake2_256,
    finalizedHead: async () => '0x'+'22'.repeat(32),blockHash:async () => '0x'+'11'.repeat(32),blockNumber:async () => 100,
    accountNextIndex:async () => state.pending,runtimeAt:async () => runtime,eraBirth:()=>64,
    query: async (p,s,a,b) => {
      state.queries.push([p,s,b])
      if (s === 'HashedAccountsEnabled') return state.enabled
      if (s === 'Account') return {nonce:state.nonce,data:{free:100000n}}
      if (s === 'Accounts' || s === 'MlDsaAccounts') return {descriptor:key.authorizationDescriptor(),generation:state.generation,commitment:key.atGeneration(state.generation).authorizationCurrentCommitment()}
      if (s === 'NextKey') return Buffer.alloc(1184,1)
      return null
    },
    rpc:async () => ({partialFee:state.fee}),
    intentReader:async () => ({query:adapter.query,decodeAddress:sdk.publicKeyFromSs58}),
    seal: (_,bytes) => {state.inner = bytes;return Buffer.alloc(1234,2)},
    verify:(...args) => {state.signatures.push(args);return sdk.verifyAuthorization(...args)},
    submitBytes:async (bytes,address,options) => {state.submissions.push({bytes,address,options});return {success:true,extrinsicHash:'0xabc',events:[]}},
    followShielded:async (carrier,hash,period) => ({success:true,carrier,hash,period}),
  }
  return {key,state,adapter,session:new ProtectedWalletClient(adapter,key)}
}
test('rotating submission selects finalized generation and signs the complete payload', async () => {
  const {session,state,key} = fixture()
  await session.submit(remark,{allowRawCall:true})
  assert.equal(state.signatures.length,1)
  assert.equal(state.signatures[0][3],2n)
  assert(state.signatures[0][1].length > 256)
  assert.equal(state.encoded.at(-1).params.nonce,3n)
  assert.equal(state.submissions.length,1)
  await assert.rejects(session.submit(remark,{allowRawCall:true}),/not reconciled/)
  state.generation++; state.nonce++;state.pending++
  await session.submit(remark,{allowRawCall:true})
  assert.equal(state.signatures.at(-1)[3],3n)
  assert(state.queries.filter(row => ['Accounts','Account'].includes(row[1])).every(row => row[2]))
})
test('pending nonce, changed authority and disabled authorization refuse before broadcast', async () => {
  const {session,state,adapter} = fixture()
  state.pending++
  await assert.rejects(session.submit(remark,{allowRawCall:true}),/finalized nonce/)
  state.pending--; state.enabled = false
  await assert.rejects(session.submit(remark,{allowRawCall:true}),/disabled/)
  state.enabled = true
  adapter.verify = (...args) => {state.pending++;return sdk.verifyAuthorization(...args)}
  await assert.rejects(session.submit(remark,{allowRawCall:true}),/nonce changed/)
  assert.equal(state.submissions.length,0)
})
test('shielded pair shares authority with consecutive generation and nonce; oversize fails closed', async () => {
  const {session,state} = fixture(sdk.CRYPTO_MLDSA65)
  const result = await session.submit(remark,{allowRawCall:true,mevShield:true})
  assert.deepEqual(state.signatures.map(row => row[3]),[3n,2n])
  assert.deepEqual(state.encoded.map(row => row.params.nonce),[4n,3n])
  assert.equal(result.period,8)
  const other = fixture()
  other.adapter.seal = () => Buffer.alloc(8193)
  await assert.rejects(other.session.submit(remark,{allowRawCall:true,mevShield:true}),/8192/)
  assert.equal(other.state.submissions.length,0)
})
test('watch-only fee quotations use dummy proofs and account for both carrier and tips', async () => {
  const {key,state,adapter} = fixture(sdk.CRYPTO_MLDSA65)
  const session = new ProtectedWalletClient(adapter,key.publicOnly())
  assert.equal(await session.estimateFee(remark,{mevShield:true,tip:7n}),34n)
  assert.equal(state.signatures.length,0)
  assert.equal(state.encoded.length,2)
  for (const row of state.encoded) assert.deepEqual(Buffer.from(row.proof),key.dummyAuthorization())
  const preview = await session.plan(sdk.buildIntent('transfer',{dest_ss58:key.ss58Address,amount_tao:'0.000001'}),{mevShield:false})
  assert.equal(preview.ok,true)
  assert.equal(preview.feeRao,10n)
  assert.equal(state.signatures.length,0)
})
test('fixed wallets serialize submissions and retain nonce reservations when RPC lags', async () => {
  const {session,state} = fixture(sdk.CRYPTO_FALCON512)
  await Promise.all([session.submit(remark,{allowRawCall:true}),session.submit(remark,{allowRawCall:true})])
  assert.deepEqual(state.encoded.map(row => row.params.nonce),[3n,4n])
  assert.deepEqual(state.signatures.map(row => row[3]),[0n,0n])
  assert.equal(state.queries.some(row => row[1] === 'MlDsaAccounts'),false)
})
test('raw permission, cancellation and fee caps are checked before private signing', async () => {
  const {session,state} = fixture()
  await assert.rejects(session.submit(remark),/allowRawCall/)
  await assert.rejects(session.signExtrinsic(remark,{allowRawCall:true}),/explicit nonce/)
  const abort = new AbortController();abort.abort()
  await assert.rejects(session.submit(remark,{allowRawCall:true,signal:abort.signal}),/abort/i)
  await assert.rejects(session.execute(sdk.buildIntent('transfer',{dest_ss58:session.keypair.ss58Address,amount_tao:'1'}),{policy:{maxFeeRao:1n}}),/maxFeeRao/)
  assert.equal(state.signatures.length,0)
})
test('receiving payments register before transfer, guard existing registration, and reject mismatched descriptors', async () => {
  const {adapter,key} = fixture(sdk.CRYPTO_MLDSA65)
  const sender = sdk.Keypair.fromUri('//Alice'), descriptor = key.authorizationDescriptor()
  let registered = null
  const reader = {
    query:async (p,s) => s === 'HashedAccountsEnabled' ? true : s === 'MlDsaAccounts' ? registered : {data:{free:10000000000n}},
    constant:async () => 100n, supportsPqProfile:()=>true,finalizedHead:adapter.finalizedHead,
    decodeAddress:sdk.publicKeyFromSs58,
    parseRecipient:(address,explicit) => sdk.parseRecipient(address,sdk.decodeReceivingAddress,sdk.Keypair.fromDescriptor,sdk.publicKeyFromSs58,explicit),
  }
  const intent = sdk.buildIntent('transfer',{dest_ss58:key.receivingAddress(),amount_tao:'1'})
  let plan = await intent.prepare(reader,{coldkey:sender.ss58Address})
  assert.equal(plan.call.call,'batch_all')
  assert.equal(plan.call.params.calls[0].call,'register')
  assert.equal(plan.call.params.calls[1].params.dest,key.ss58Address)
  assert.equal(plan.requiredFreeRao,100n)
  registered = {descriptor}
  plan = await intent.prepare(reader,{coldkey:sender.ss58Address})
  assert.equal(plan.call.params.calls[0].call,'check_registered')
  assert.throws(() => reader.parseRecipient(sender.ss58Address,descriptor),/does not match/)
  registered = {descriptor:sdk.Keypair.fromSeed(Buffer.alloc(32,55),sdk.CRYPTO_MLDSA65).authorizationDescriptor()}
  await assert.rejects(intent.prepare(reader,{coldkey:sender.ss58Address}),/differs/)
})
test('hashed classical HTTP signing requires retired authority and verification requires trusted scheme', async () => {
  const {key,adapter,state} = fixture(sdk.CRYPTO_HASHED_ED25519)
  const request = {method:'GET',path:'/',receiverSs58:key.ss58Address,nonceNs:100n}
  await assert.rejects(sdk.httpAuth.sign(key,request),/prepareHttpSigner/)
  state.generation = 0n
  await assert.rejects(sdk.prepareHttpSigner(adapter,key),/generation zero/)
  state.generation = 2n
  const active = await sdk.prepareHttpSigner(adapter,key), headers = await sdk.httpAuth.sign(active,request)
  const options = {method:'GET',path:'/',selfHotkeySs58:key.ss58Address,nowNs:100n}
  await assert.rejects(sdk.httpAuth.verify(headers,Buffer.alloc(0),options),/expectedCryptoType/)
  await assert.rejects(sdk.httpAuth.verify(headers,Buffer.alloc(0),{...options,expectedCryptoType:0}),/trusted sender/)
  assert.equal((await sdk.httpAuth.verify(headers,Buffer.alloc(0),{...options,expectedCryptoType:6})).cryptoType,6)
})
test('automatic registration reserve requires an explicit allowance and callback cannot override its cap', async () => {
  const plan = {extras:{registrationSponsorships:[{payer:'payer',recipient:'recipient',depositRao:100n}]}}
  await assert.rejects(sdk.authorizeRegistrationSponsorship(plan,{}),/requires/)
  await assert.rejects(sdk.authorizeRegistrationSponsorship(plan,{maxRegistrationDepositRao:99n,onRegistrationSponsorship:()=>true}),/exceeds/)
  await assert.rejects(sdk.authorizeRegistrationSponsorship(plan,{maxRegistrationDepositRao:100n,onRegistrationSponsorship:()=>false}),/declined/)
  await sdk.authorizeRegistrationSponsorship(plan,{maxRegistrationDepositRao:100n})
  await sdk.authorizeRegistrationSponsorship(plan,{onRegistrationSponsorship:quote=>quote.recipient === 'recipient'})
})
test('orders use account approval and reject removed PQ delegation', async () => {
  const {key,adapter,session} = fixture(sdk.CRYPTO_FALCON512)
  const order = {V1:{signer:key.ss58Address,hotkey:key.ss58Address,fee_recipient:key.ss58Address,netuid:1,order_type:'LimitBuy',amount:20n,limit_price:30n,fee_rate:0}}
  adapter.intentReader = async () => ({query:adapter.query,composeCall:async call => {
    const nonce = Buffer.alloc(8); nonce.writeBigUInt64LE(call.params.order.Authorized.nonce)
    return Buffer.concat([Buffer.from([4,2,2]),nonce,Buffer.from(JSON.stringify(call.params.order.Authorized.terms,(_,v) => typeof v === 'bigint' ? v.toString() : v))])
  }})
  const prepared = await session.prepareOrder(4n,order), signed = prepared.accountApproved()
  assert.equal(signed.signature,'AccountApproved')
  assert.equal(signed.order.Authorized.nonce,4n)
  assert.equal(sdk.validateSignedOrders([signed]).length,1)
  for (const scheme of ['MlDsa','Falcon']) assert.throws(() => sdk.validateSignedOrders([{...signed,signature:{[scheme]:{epoch:1n,public_key:'0x',signature:'0x'}}}]),/unknown order authorization/)
  assert.equal(session.signOrder,undefined)
  assert.equal(session.signMessage,undefined)
  assert.equal(sdk.installCapabilityCall,undefined)
  assert.equal(sdk.pqHttpAuth,undefined)
  for (const op of ['install_order_capability','revoke_order_capability']) assert.throws(() => sdk.buildIntent(op,{}))
  assert.throws(() => key.sign(Buffer.from('arbitrary message')),/not supported/)
  const changed = prepared.order; changed.V1.amount = 999n
  assert.equal(prepared.order.V1.amount,20n)
  assert.notEqual((await session.prepareOrder(5n,order)).orderId,prepared.orderId)
})
test('safe derivative and multisig calls preserve explicit account mode', async () => {
  const {adapter,key} = fixture(sdk.CRYPTO_FALCON512)
  assert.equal((await sdk.derivativeCall(adapter,key,4,remark)).call,'as_safe_derivative')
  const client = new sdk.Client('local'), members = [key.ss58Address,sdk.Keypair.fromUri('//Alice').ss58Address]
  const {NodeMultisig} = require('../dist/node-workflows')
  const legacy = new NodeMultisig(client,members,2), safe = new NodeMultisig(client,members,2,42,'safe')
  assert.notEqual(legacy.address,safe.address)
  assert.equal(safe.accountMode,'safe')
  await client.close()
})
