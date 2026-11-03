const test = require('node:test')
const assert = require('node:assert/strict')
const { Runtime, decodeOptionalOpaqueMetadata } = require('../dist/runtime')
const { Keypair, publicKeyFromSs58 } = require('../dist/keys')
const { blake2_256 } = require('../dist/crypto')
const { Client } = require('../dist/client')
const { serializeUnsignedTransaction, restoreUnsignedTransaction } = require('../dist/unsigned-transaction')
const golden = require('../../python/tests/fixtures/golden.json')
const metadata = decodeOptionalOpaqueMetadata(Buffer.from(golden.metadata.v15_hex.slice(2),'hex'))
const crypto = { blake2_256, publicKeyFromSs58 }
const genesis = '0x' + '11'.repeat(32)
function fixture() {
  const runtime = new Runtime(metadata, 419, 1, 42), key = Keypair.fromUri('//Alice')
  const callData = runtime.composeCall('System','remark',{remark:Buffer.from('offline')})
  const txParams = {era:'00',nonce:7n,tip:13n,genesisHash:Buffer.from(genesis.slice(2),'hex'),eraBlockHash:Buffer.from(genesis.slice(2),'hex')}
  const state = {runtime,address:key.ss58Address,publicKey:key.publicKey,cryptoType:key.cryptoType,callData,payload:runtime.signaturePayload(callData,txParams),...runtime.signaturePayloadParts(txParams),txParams}
  return {runtime,key,state,serialized:serializeUnsignedTransaction(state,crypto,metadata)}
}

test('unsigned transaction JSON survives a process boundary and recomputes the exact signing context',()=>{
  const f=fixture(), json=JSON.parse(JSON.stringify(f.serialized))
  const restored=restoreUnsignedTransaction(json,f.runtime,crypto,metadata,genesis)
  assert.deepEqual(Buffer.from(restored.payload),f.state.payload)
  assert.equal(restored.txParams.tip,13n);assert.equal(restored.nonce,7)
  assert.equal(restored.signerPayload.method,'0x'+f.state.callData.toString('hex'))
})

test('unsigned imports reject modified payloads, unknown policy fields, changed metadata, chain or runtime',()=>{
  const f=fixture()
  for(const [field,value] of [['payload','0x00'],['nonce','8'],['includedInSignedData','0x'],['publicKey','0x'+'00'.repeat(32)],['era','0x000000'],['tip','-1'],['trustedPolicy',true]]) {
    assert.throws(()=>restoreUnsignedTransaction({...f.serialized,[field]:value},f.runtime,crypto,metadata,genesis),undefined,field)
  }
  assert.throws(()=>restoreUnsignedTransaction(f.serialized,new Runtime(metadata,420,1,42),crypto,metadata,genesis),/upgraded/)
  assert.throws(()=>restoreUnsignedTransaction(f.serialized,f.runtime,crypto,Buffer.from([1]),genesis),/metadata/)
  assert.throws(()=>restoreUnsignedTransaction(f.serialized,f.runtime,crypto,metadata,'0x'+'22'.repeat(32)),/another chain/)
})

test('Node imports require raw policy admission, verify signatures and never trust mutable displayed fields',async()=>{
  const f=fixture(), client = new Client('local')
  client.runtimeAt=async()=>f.runtime;client.genesisHash=async()=>genesis
  let submitted
  client.submitSigned=async bytes=>{submitted=bytes;return {status:'submitted',events:[],message:'submitted',extrinsicHash:'0xok'}}
  await assert.rejects(client.importPreparedCall(f.serialized),/raw-call permission/)
  await assert.rejects(client.importPreparedCall(f.serialized,{allowRawCall:true,policy:{maxSpendRao:1n}}),/cannot prove/)
  const imported=await client.importPreparedCall(JSON.parse(JSON.stringify(f.serialized)),{allowRawCall:true})
  assert.deepEqual(client.exportPreparedCall(imported),f.serialized)
  await assert.rejects(client.submitSignature(imported,Buffer.alloc(64)),/does not verify/)
  imported.callData.fill(0)
  await client.submitSignature(imported,f.key.sign(f.state.payload))
  assert.ok(submitted.bytes.length>64)
  client.runtimeAt=async()=>new Runtime(metadata,420,1,42)
  await assert.rejects(client.submitSignature(imported,f.key.sign(f.state.payload)),/upgraded/)
})


test('mortal era round-trips through exact canonical period and phase',()=>{
  const f=fixture()
  for (const era of [{period:128,current:999}, {period:65536,current:123456}]) {
    const txParams={...f.state.txParams,era}
    const state={...f.state,txParams,payload:f.runtime.signaturePayload(f.state.callData,txParams),...f.runtime.signaturePayloadParts(txParams)}
    const serialized=serializeUnsignedTransaction(state,crypto,metadata)
    const restored=restoreUnsignedTransaction(JSON.parse(JSON.stringify(serialized)),f.runtime,crypto,metadata,genesis)
    assert.deepEqual(Buffer.from(restored.payload),state.payload)
    assert.deepEqual(f.runtime.encodeEra(restored.txParams.era),f.runtime.encodeEra(era))
  }
})
