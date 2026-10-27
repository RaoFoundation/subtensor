const test = require('node:test')
const assert = require('node:assert/strict')
const { createHash } = require('node:crypto')
const u = require('../dist/upgrade')
const { blake2_256 } = require('../dist/crypto')
const hex = b => '0x' + Buffer.from(b).toString('hex')
const digest = b => createHash('sha256').update(b).digest('hex')
const key = n => hex(Buffer.alloc(32, n))
function compact(n) { n = BigInt(n); if (n < 64n) return Buffer.from([Number(n * 4n)]); if (n < 16384n) { const b = Buffer.alloc(2); b.writeUInt16LE(Number(n * 4n + 1n)); return b }; if (n < 1n << 30n) { const b = Buffer.alloc(4); b.writeUInt32LE(Number(n * 4n + 2n)); return b }; const bytes = []; while(n) { bytes.push(Number(n & 255n)); n >>= 8n }; return Buffer.from([((bytes.length - 4) << 2) | 3, ...bytes]) }
const proposalWeight = u.CI_PROPOSAL_WEIGHT
function proposal(real, wasm) { return Buffer.concat([Buffer.from([16, 0, 0]), Buffer.from(real.slice(2), 'hex'), Buffer.from([0, 12, 1, 0, 2]), compact(wasm.length), wasm, compact(proposalWeight.ref_time), compact(proposalWeight.proof_size)]) }
function wasm(spec = 500) {
  const version = Buffer.alloc(8); version.writeUInt32LE(1); version.writeUInt32LE(spec, 4)
  const custom = Buffer.concat([Buffer.from([15]), Buffer.from('runtime_version'), compact(1), Buffer.from('x'), compact(1), Buffer.from('y'), version])
  return Buffer.concat([Buffer.from([0,97,115,109,1,0,0,0]), Buffer.from([0, custom.length]), custom])
}
function fixture() {
  const signers = [key(1), key(2), key(3)], ci = key(4)
  const multi = (s,t) => ({ address: hex(createHash('sha256').update(s.slice().sort().join(',') + t).digest()) })
  const sudo = multi(signers,2).address, deploy = multi([ci,sudo],2).address, code = wasm(), blob = proposal(sudo,code), hash = hex(blake2_256(blob))
  const state = { approvals: null, snapshots: 0, estimated: 0, mismatch: false }
  const adapter = {
    at: async () => { state.snapshots++; return pinned },
    crypto: { blake2_256, publicKeyFromSs58: a => { if (!/^0x[0-9a-f]{64}$/.test(a)) throw Error('address'); return Buffer.from(a.slice(2),'hex') }, ss58FromPublic: hex },
    multisig: multi,
    query: async ([p,n],args=[]) => p==='Sudo' ? sudo : p==='Proxy' ? [[{delegate:deploy,proxy_type:'SudoUncheckedSetCode',delay:0}],0n] : args[0]===sudo && state.approvals!=null ? {when:{height:20,index:2}, approvals:state.approvals,depositor:signers[0]} : null,
    queryMap: async () => [[hash,{depositor:ci,when:{height:10,index:1},approvals:[ci]}]],
    compose: async call => { if(call.pallet==='Proxy') return state.mismatch ? Buffer.from([1]) : proposal(call.params.real,Buffer.from(call.params.call.params.call.params.code)); return Buffer.from(JSON.stringify(call,(_,v)=>typeof v==='bigint'?String(v):v)) },
    estimateWeight: async () => {state.estimated++;return {ref_time:1000n,proof_size:100n}},
  }
  const pinned = {...adapter,at:async()=>pinned}
  const release = new u.UpgradeRelease('RaoFoundation/subtensor','v500')
  const bundle = { blob,wasm:code,wasmSource:'local',digest:{sha256:digest(code)},release,notes:[],manifest:{kind:'runtime-upgrade-proposal',repo:release.repo,tag:release.tag,commit:'a'.repeat(40),spec_version:500,call_hash:hash,ci_address:ci,sudo_key:sudo,deployment_multisig:deploy,sudo:{signatories:signers,threshold:2},wasm_sha256:digest(code),max_weight:proposalWeight,assets:Object.fromEntries(Object.entries(u.UPGRADE_ASSETS).map(([k,n])=>[k,release.assetUrl(n)]))} }
  return {adapter,state,bundle,signers,options:{proposalWeight,expectedSpecVersion:500,expectedCommit:'a'.repeat(40)}}
}

test('upgrade parser accepts exact CI shape and rejects wrappers, truncation, noncanonical length, weight and trailing bytes', () => {
  const {bundle}=fixture()
  assert.equal(u.upgradeRuntimeSpecVersion(bundle.wasm),500)
  assert.deepEqual(Buffer.from(u.parseUpgradeBlob(bundle.blob,proposalWeight).code),bundle.wasm)
  for (const bytes of [Buffer.from([26,0,...bundle.blob]),bundle.blob.subarray(0,20),bundle.blob.subarray(0,42),Buffer.concat([bundle.blob,Buffer.from([0])])]) assert.throws(()=>u.parseUpgradeBlob(bytes,proposalWeight))
  assert.throws(()=>u.parseUpgradeBlob(bundle.blob,{ref_time:1n,proof_size:0n}),/weight/)
  const noncanonical=Buffer.concat([bundle.blob.subarray(0,40),Buffer.from([1,0]),bundle.blob.subarray(41)])
  assert.throws(()=>u.parseUpgradeBlob(noncanonical,proposalWeight),/non-canonical/)
  assert.throws(()=>u.parseUpgradeHex('0xabc'),/hex/)
  assert.equal(u.parseUpgradeRelease('RaoFoundation/subtensor@v500').tag,'v500')
  assert.throws(()=>u.parseUpgradeRelease('https://github.com.evil.test/O/R/releases/tag/x'))
})

test('upgrade checks pin reads and verify exact metadata bytes, hashes, assets, version, signer set and commit',async()=>{
  const f=fixture(); const result=await u.checkUpgradeProposal(f.adapter,f.bundle,f.options)
  assert.equal(result.ok,true,JSON.stringify(result.checks)); assert.equal(f.state.snapshots,1);assert.equal(result.specVersion,500)
  for (const [field,value] of [['call_hash',key(8)],['wasm_sha256','b'.repeat(64)],['spec_version',501],['commit','main'],['ci_address',key(9)],['sudo',{signatories:[key(1),key(1)],threshold:2}],['assets',{}]]) {
    const changed={...f.bundle,manifest:{...f.bundle.manifest,[field]:value}}
    assert.equal((await u.checkUpgradeProposal(f.adapter,changed,f.options)).ok,false,field)
  }
  f.state.mismatch=true;assert.equal((await u.checkUpgradeProposal(f.adapter,f.bundle,f.options)).checks.find(c=>c.name==='re-encoding').ok,false)
})

test('approval plan adapts to first, final, and already-signed stages and requires independently derived sudo signers',async()=>{
  const f=fixture(), options={...f.options,finalizeWeight:u.CI_FINALIZE_WEIGHT,signer:f.signers[1]}
  const first=await u.planUpgradeApproval(f.adapter,f.bundle,options)
  assert.equal(first.position,'first');assert.equal(first.call.call,'approve_as_multi');assert.equal(first.call.params.maybe_timepoint,null);assert.equal(first.call.params.max_weight.ref_time,1100n)
  f.state.approvals=[f.signers[0]]
  const final=await u.planUpgradeApproval(f.adapter,f.bundle,options)
  assert.equal(final.position,'final');assert.equal(final.call.call,'as_multi');assert.deepEqual(final.call.params.call,final.finalizingCall)
  f.state.approvals.push(f.signers[1]);const signed=await u.planUpgradeApproval(f.adapter,f.bundle,options);assert.equal(signed.position,'signed');assert.equal(signed.call,null);assert.equal(f.state.estimated,2)
  await assert.rejects(u.planUpgradeApproval(f.adapter,f.bundle,{...options,signer:key(8)}),/not in/)
})

test('fetching distinguishes absent optional assets from malformed payloads, caps streamed bytes and confines GitHub credentials',async()=>{
  const f=fixture(),body=hex(f.bundle.blob),urls=[]
  const fetch=async(url,options)=>{urls.push([url,options]);if(url.endsWith('proxy_proxy_blob.hex'))return new Response(body);return new Response('missing',{status:404})}
  const bundle=await u.fetchUpgradeBundle({releaseUrl:f.bundle.release.url,wasm:f.bundle.wasm},{fetch})
  assert.deepEqual(bundle.blob,Uint8Array.from(f.bundle.blob));assert.equal(bundle.notes.length,2);assert.equal(bundle.wasmSource,'local')
  await assert.rejects(u.fetchUpgradeBundle({releaseUrl:f.bundle.release.url},{fetch:async url=>new Response(url.endsWith('.hex')?body:'not JSON')}),/JSON/)
  await assert.rejects(u.fetchUpgradeBytes('https://example.test/x',{fetch:async()=>new Response('1234'),maxBytes:3}),/cap/)
  await assert.rejects(u.fetchUpgradeBytes('https://example.test/x',{githubToken:'secret',fetch},true),/credentials/)
  await assert.rejects(u.fetchUpgradeBytes('http://example.test/x',{fetch}),/HTTPS/)
  assert.equal(urls.every(([,o])=>o.headers.Authorization==null),true)
})
