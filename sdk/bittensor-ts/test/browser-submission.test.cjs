const test = require('node:test')
const assert = require('node:assert/strict')
const core = require('../dist')

async function fixture() {
  const browser = await import('../dist/browser.mjs')
  browser.configureBrowserWasm(async () => ({ default: async()=>{}, ss58Decode:core.publicKeyFromSs58, blake2_256:core.blake2_256, metadataDigest:()=>new Uint8Array(32).fill(7) }))
  await browser.initBrowser()
  const client = new browser.Client('local'), key = core.Keypair.fromUri('//Alice')
  const signer = {address:key.ss58Address,cryptoType:1}
  client.blockHash = async()=> '0x'+'11'.repeat(32); client.blockNumber=async()=>10; client.genesisHash=client.blockHash
  client.rpc = async method=> { if (method==='system_accountNextIndex') return 7; throw new Error(method) }
  const runtime = {specVersion:419,signedExtensionIdentifiers:()=>['CheckMetadataHash'],signaturePayloadParts:()=>({includedInExtrinsic:new Uint8Array(),includedInSignedData:new Uint8Array()}),signaturePayload:()=>new Uint8Array([1]),signerPayload:()=>({})}
  client.runtimeEntry=async()=>({runtime,metadata:new Uint8Array(),version:{specName:'test'},properties:{}})
  client.compose=async()=>new Uint8Array([0,0])
  return {browser,client,signer}
}

test('browser signing binds metadata by default and refuses signer-required opt-outs',async()=>{
  const {client,signer}=await fixture()
  assert.deepEqual((await client.prepareRaw({},signer,{nonce:7,period:null})).txParams.metadataHash,new Uint8Array(32).fill(7))
  assert.equal((await client.prepareRaw({},signer,{nonce:7,period:null,metadataHash:null})).txParams.metadataHash,undefined)
  await assert.rejects(client.prepareRaw({},{...signer,requiresMetadataHash:true},{nonce:7,period:null,metadataHash:null}),/cannot be disabled/)
  await client.close()
})

test('browser reuses unsigned reservations but retains uncertain submissions',async()=>{
  const {client,signer}=await fixture(), nonces=[]
  client.prepareRaw=async(_call,_signer,options)=>{nonces.push(options.nonce);return {nonce:options.nonce}}
  client.signPrepared=async()=>{throw new Error('user declined')}
  await assert.rejects(client.submitPreparedCall({},signer,{}),/user declined/)
  client.signPrepared=async()=>({bytes:new Uint8Array([1]),hash:new Uint8Array(32)})
  client.broadcastExtrinsic=async()=>{throw new Error('lost response after send')}
  await assert.rejects(client.submitPreparedCall({},signer,{}),/lost response/)
  client.broadcastExtrinsic=async()=>({status:'submitted'})
  await client.submitPreparedCall({},signer,{})
  assert.deepEqual(nonces,[7,7,8])
  await client.close()
})

test('browser reconciles a lost watch response by hash without resubmitting',async()=>{
  const {client}=await fixture();let sent=0,checked=[]
  client.transport.subscribe=async()=>{sent++;throw new Error('response lost')}
  client.findExtrinsic=async(hash,block)=>{checked.push([hash,block]);return {status:'inBlock',success:true,extrinsicHash:hash}}
  const result=await client.broadcastExtrinsic(new Uint8Array([1]),{})
  assert.equal(result.success,true);assert.equal(sent,1);assert.equal(checked.length,1)
  client.findExtrinsic=async()=>null
  await assert.rejects(client.broadcastExtrinsic(new Uint8Array([1]),{}),error=>error.submissionUncertain===true && error.extrinsicHash===result.extrinsicHash)
  assert.equal(sent,2)
  await client.close()
})
