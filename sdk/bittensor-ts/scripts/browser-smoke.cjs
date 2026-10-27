'use strict'

const assert = require('node:assert/strict')
const { spawn, spawnSync } = require('node:child_process')
const fs = require('node:fs')
const http = require('node:http')
const os = require('node:os')
const path = require('node:path')

const esbuild = require('esbuild')

const root = path.join(__dirname, '..')
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-sdk-browser-'))
const configuredLiveEndpoint = process.env.BT_CHAIN_ENDPOINT
let liveEndpoint
if (configuredLiveEndpoint) {
  const url = new URL(configuredLiveEndpoint)
  if (!['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || !['ws:', 'wss:', 'http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('browser live tests only accept a loopback RPC endpoint')
  url.protocol = url.protocol === 'https:' || url.protocol === 'wss:' ? 'wss:' : 'ws:'
  liveEndpoint = url.toString()
}
const isCi = process.env.CI === 'true' || process.env.GITHUB_ACTIONS === 'true'

function compactLength(buffer, offset) {
  const first = buffer[offset]
  const mode = first & 0b11
  if (mode === 0) return { length: first >> 2, offset: offset + 1 }
  if (mode === 1) return { length: buffer.readUInt16LE(offset) >> 2, offset: offset + 2 }
  if (mode === 2) return { length: buffer.readUInt32LE(offset) >>> 2, offset: offset + 4 }
  const bytes = (first >> 2) + 4
  let length = 0
  for (let i = 0; i < bytes; i += 1) length += buffer[offset + 1 + i] * (256 ** i)
  return { length, offset: offset + 1 + bytes }
}

function goldenMetadataHex() {
  const golden = JSON.parse(
    fs.readFileSync(
      path.join(root, '..', 'python', 'tests', 'fixtures', 'golden.json'),
      'utf8',
    ),
  )
  const raw = Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')
  assert.equal(raw[0], 1)
  const decoded = compactLength(raw, 1)
  return raw.subarray(decoded.offset, decoded.offset + decoded.length).toString('hex')
}

function modulePath(filePath) {
  const relative = path.relative(tmp, filePath).replace(/\\/g, '/')
  return relative.startsWith('.') ? relative : `./${relative}`
}

function browserEntry(metadataHex, mode) {
  const useCustomLoader = mode === 'custom'
  return `
import * as sdk from ${JSON.stringify(mode === 'package' ? '@bittensor/sdk' : modulePath(path.join(root, 'dist', 'browser.mjs')))}
${useCustomLoader ? `import * as wasmBindings from ${JSON.stringify(modulePath(path.join(root, 'dist', 'wasm', 'bittensor_core_wasm_bg.js')))}
import wasmUrl from ${JSON.stringify(modulePath(path.join(root, 'dist', 'wasm', 'bittensor_core_wasm_bg.wasm')))}` : ''}

const metadataHex = ${JSON.stringify(metadataHex)}
const mnemonic = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
const mode = ${JSON.stringify(mode)}
const useCustomLoader = ${JSON.stringify(useCustomLoader)}
const liveEndpoint = ${JSON.stringify(liveEndpoint ?? null)}

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2)
  for (let i = 0; i < out.length; i += 1) out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16)
  return out
}

${useCustomLoader ? `async function loadWasm() {
  const response = await fetch(wasmUrl)
  if (!response.ok) throw new Error('failed to fetch WASM: ' + response.status)
  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {
    './bittensor_core_wasm_bg.js': wasmBindings,
  })
  wasmBindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  const module = { ...wasmBindings }
  delete module.default
  return module
}` : ''}

try {
  if (useCustomLoader) sdk.configureBrowserWasm(() => loadWasm())
  const wasm = await sdk.initBrowser()
  assert(typeof wasm.Runtime === 'function', 'WASM Runtime constructor is available')
  assert(typeof sdk.coreVersion() === 'string' && sdk.coreVersion().length > 0, 'WASM initialized')

  const keypair = sdk.Keypair.fromMnemonic(mnemonic)
  const message = new TextEncoder().encode('browser signing smoke')
  const signature = keypair.sign(message)
  assert(signature.length === 64, 'sr25519 signature length')
  assert(keypair.verify(message, signature), 'keypair verifies its own signature')
  assert(sdk.verifySignature(message, signature, keypair.ss58Address), 'browser verifySignature works')
  assert(sdk.ss58FromPublic(keypair.publicKey, 42) === keypair.ss58Address, 'SS58 roundtrip works')

  const metadata = hexToBytes(metadataHex)
  const runtime = new sdk.Runtime(metadata, 419, 1, 42)
  assert(runtime.specVersion === 419, 'runtime spec version')
  assert(runtime.transactionVersion === 1, 'runtime transaction version')
  assert(runtime.metadataIr().pallets.some((pallet) => pallet.name === 'System'), 'runtime metadata parsed')

  const call = runtime.composeCall('System', 'remark', { remark: new Uint8Array([1, 2, 3]) })
  const decodedCall = runtime.decodeCall(call)
  assert(decodedCall.call_module === 'System', 'call module decoded')
  assert(decodedCall.call_function === 'remark', 'call function decoded')

  const genesisHash = new Uint8Array(32)
  const eraBlockHash = new Uint8Array(32)
  genesisHash.fill(1)
  eraBlockHash.fill(2)
  const txParams = {
    era: '00',
    nonce: 0,
    tip: 0,
    tipAssetId: null,
    genesisHash,
    eraBlockHash,
  }
  const payloadParts = runtime.signaturePayloadParts(txParams)
  assert(payloadParts.includedInExtrinsic.length > 0, 'signature payload extrinsic parts')
  assert(payloadParts.includedInSignedData.length > 0, 'signature payload signed parts')
  const payload = runtime.signaturePayload(call, txParams)
  const extrinsicSignature = keypair.sign(payload)
  const signed = runtime.encodeSignedExtrinsic(
    call,
    keypair.publicKey,
    extrinsicSignature,
    keypair.cryptoType,
    { era: '00', nonce: 0, tip: 0, tipAssetId: null, metadataHashEnabled: false },
  )
  assert(signed.bytes.length > call.length, 'signed extrinsic bytes')
  assert(signed.hash.length === 32, 'signed extrinsic hash')
  const decodedExtrinsic = runtime.decodeExtrinsic(signed.bytes, false)
  assert(decodedExtrinsic.call.call_module === 'System', 'extrinsic call module decoded')
  assert(decodedExtrinsic.call.call_function === 'remark', 'extrinsic call function decoded')

  assert(typeof globalThis.Buffer === 'undefined', 'no Node Buffer polyfill is present')
  assert(sdk.listReads().length === 105, 'complete read catalog available')
  assert(sdk.listIntentTools().length === 86, 'complete intent catalog available')
  assert(typeof sdk.resolveTransactionFeePayer === 'function' && typeof sdk.serializeUnsignedTransaction === 'function', 'portable fee-payer and unsigned helpers are public browser exports')
  assert(sdk.blake2_256(new Uint8Array()).length === 32, 'WASM hash available')
  const client = new sdk.Client('local')
  const rpcCalls = []
  const accountEntry = runtime.storageEntry('System', 'Account')
  const hex = bytes => '0x' + Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
  const account = runtime.decode(accountEntry.valueType, accountEntry.defaultBytes, false)
  account.data.free = 1000000n
  const accountBytes = runtime.encode(accountEntry.valueType, account)
  assert(hex(sdk.storagePrefixFor('System', 'Account')) === hex(runtime.storagePrefix('System', 'Account')), 'metadata-independent storage prefix matches Rust metadata codec')
  const blockHash = '0x' + '11'.repeat(32)
  const genesis = '0x' + '22'.repeat(32)
  const signingAnchorHash = '0x' + '33'.repeat(32)
  client.rpc = async (method, params = []) => {
    rpcCalls.push([method, params])
    if (method === 'chain_getFinalizedHead') return blockHash
    if (method === 'chain_getBlockHash') return params[0] === 0 ? genesis : params[0] === 63 ? signingAnchorHash : blockHash
    if (method === 'chain_getHeader') return { number: '0x40', parentHash: genesis }
    if (method === 'state_getRuntimeVersion') return { specVersion: 419, transactionVersion: 1, specName: 'node-subtensor' }
    if (method === 'system_properties') return { ss58Format: 42, tokenDecimals: 9, tokenSymbol: 'TAO' }
    if (method === 'state_call') {
      if (params[0] === 'Metadata_metadata_at_version') return '0x00'
      throw new Error('unexpected runtime API ' + params[0])
    }
    if (method === 'state_getMetadata') return hex(metadata)
    if (method === 'system_accountNextIndex') return 0
    if (method === 'state_getStorage') return hex(accountBytes)
    if (method === 'state_queryStorageAt') return [{ changes: params[0].map(key => [key, hex(accountBytes)]) }]
    if (method === 'payment_queryInfo') return { partialFee: '123', weight: { refTime: 10, proofSize: 2 } }
    if (method === 'author_submitExtrinsic') return hex(sdk.blake2_256(hexToBytes(params[0].slice(2))))
    throw new Error('unexpected browser RPC ' + method)
  }
  await client.connect()
  const snapshot = await client.at()
  assert(snapshot.hash === blockHash && snapshot.block === 64, 'snapshot pins a hash and number')
  const balance = await snapshot.read('balance', { coldkey_ss58: keypair.ss58Address })
  assert(balance instanceof sdk.Balance && balance.rao === 1000000n, 'high-level browser balance read')
  const many = await snapshot.queryBatch(sdk.storage.System.Account, [[keypair.ss58Address], [keypair.ss58Address]])
  assert(many.length === 2, 'browser batched reads')
  assert(rpcCalls.filter(([method]) => method === 'state_getStorage').every(([, params]) => params[1] === blockHash), 'dependent storage reads stay pinned')
  const readRpc = client.rpc
  client.rpc = async (method, params = []) => {
    if (method === 'chain_getFinalizedHead') throw new Error('signing must not use potentially stale finalized head')
    return readRpc(method, params)
  }
  const unsigned = await client.prepareCall({ pallet: 'System', call: 'remark', params: { remark: new Uint8Array([4, 5]) } }, keypair, { allowRawCall: true, nonce: 0, period: null })
  assert(unsigned.signerPayload.address === keypair.ss58Address && unsigned.signerPayload.genesisHash === genesis, 'extension payload is bound to account and chain')
  const mortal = await client.prepareCall({ pallet: 'System', call: 'remark', params: { remark: new Uint8Array() } }, keypair, { allowRawCall: true, nonce: 0 })
  assert(mortal.blockNumber === 64 && mortal.blockHash === blockHash && mortal.txParams.era.current === 63 && hex(mortal.txParams.eraBlockHash) === signingAnchorHash, 'mortal signing uses fresh best-state metadata and the previous stored block hash')
  const serialized = await client.exportPreparedCall(unsigned)
  const imported = await client.importPreparedCall(JSON.parse(JSON.stringify(serialized)), { allowRawCall: true })
  assert(hex(imported.payload) === hex(unsigned.payload), 'browser unsigned JSON roundtrip preserves exact signing bytes')
  let importRejected = false
  try { await client.importPreparedCall(serialized) } catch { importRejected = true }
  assert(importRejected, 'serialized plans require explicit raw admission')
  const detached = await client.submitSignature(imported, keypair.sign(imported.payload), { waitForInclusion: false })
  assert(detached.status === 'submitted', 'detached signing and submission')
  assert((await client.submitExtrinsic(signed.bytes, { waitForInclusion: false })).status === 'submitted', 'pre-signed bytes resolve their account and use the submission queue')
  let detachedPolicyRejected = false
  try { await client.submitSignature(imported, keypair.sign(imported.payload), { waitForInclusion: false, policy: { maxSpendRao: 0n } }) } catch { detachedPolicyRejected = true }
  assert(detachedPolicyRejected, 'detached submission cannot silently ignore semantic policies')
  let rejected = false
  try { await client.submitSignature(unsigned, new Uint8Array(64), { waitForInclusion: false }) } catch { rejected = true }
  assert(rejected, 'invalid external signature is rejected')
  const intent = sdk.buildIntent('transfer', { dest_ss58: keypair.ss58Address, amount_tao: '0.000000001' })
  const plan = await client.plan(intent, { coldkey: keypair.ss58Address }, { maxSpendRao: 1n, maxFeeRao: 123n })
  assert(plan.prepared.spendRao === 1n && plan.violations.length === 0, 'semantic spend and fee policy')
  const tipped = await client.plan(intent, { coldkey: keypair.ss58Address }, { maxFeeRao: 123n }, { tip: 1n })
  assert(tipped.fee.rao === 124n && tipped.violations.length === 1, 'tip included in fee policy')
  const shieldPlan = await client.plan(intent, { coldkey: keypair.ss58Address }, {}, { mevShield: true, tip: 1n })
  assert(shieldPlan.fee.rao === 248n && shieldPlan.signingOptions.period === 8, 'shield inner and carrier tips and period8')
  assert(shieldPlan.nonce === 0 && (await client.plan(intent, { coldkey: keypair.ss58Address })).nonce === 0, 'fee plans do not reserve nonces')
  const beforeShieldRpc = client.rpc
  let carrierPendingNonce = 0
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') return carrierPendingNonce
    if (method === 'author_submitExtrinsic') carrierPendingNonce++
    return beforeShieldRpc(method, params)
  }
  const unshieldedQuery = client.query.bind(client), shieldSigning = []
  client.query = async (...args) => Array.isArray(args[0]) && args[0].join('.') === 'MevShield.NextKey' ? new Uint8Array(1184) : unshieldedQuery(...args)
  const shieldSigner = { ss58Address: keypair.ss58Address, sign: (payload, context) => { shieldSigning.push(context); return keypair.sign(payload) } }
  const shieldSubmitted = await client.execute(intent, shieldSigner, { mevShield: true, waitForInclusion: false })
  client.query = unshieldedQuery
  assert(shieldSubmitted.status === 'submitted' && shieldSigning.length === 2, 'shield performs both signing stages and submits its carrier')
  assert(shieldSigning.every(context => context.blockHash === blockHash && context.txParams.era.period === 8 && context.txParams.era.current === 63 && hex(context.txParams.eraBlockHash) === signingAnchorHash), 'both shield inner and carrier anchor at the previous stored block hash')
  assert(shieldSigning[0].nonce === 1 && shieldSigning[1].nonce === 0, 'shield inner and carrier retain their consecutive nonce order')
  let afterShieldNonce
  await client.execute(intent, { ss58Address: keypair.ss58Address, sign: (payload, context) => { afterShieldNonce = context.nonce; return keypair.sign(payload) } }, { period: null, waitForInclusion: false })
  assert(afterShieldNonce === 2, 'submitted-only carrier reserves its inner nonce even while RPC only advances to1')
  client.rpc = beforeShieldRpc
  const sponsor = sdk.ss58FromPublic(new Uint8Array(32).fill(7))
  const ownerKey = hex(runtime.storageKey('SubtensorModule', 'Owner', [keypair.ss58Address]))
  const ownerEntry = runtime.storageEntry('SubtensorModule', 'Owner')
  const sponsorBytes = runtime.encode(ownerEntry.valueType, sponsor)
  const signingAccountKey = hex(runtime.storageKey('System', 'Account', [keypair.ss58Address]))
  const ordinaryRpc = client.rpc
  client.rpc = async (method, params = []) => {
    if (method === 'state_getKeysPaged') return params[0] === ownerKey ? [ownerKey] : []
    if (method === 'state_getStorage' && params[0] === ownerKey) return hex(sponsorBytes)
    if (method === 'state_getStorage' && params[0] === signingAccountKey) return hex(accountEntry.defaultBytes)
    return ordinaryRpc(method, params)
  }
  const serve = sdk.buildIntent('serve_prometheus', { netuid: 1, ip: '127.0.0.1', port: 9090 })
  const sponsored = await client.plan(serve, { coldkey: sponsor, hotkey: keypair.ss58Address }, { maxFeeRao: 123n }, { tip: 1000n })
  assert(sponsored.ok && sponsored.fee.rao === 123n, 'direct hotkey fees charge funded owner and discard tip')
  const wrapped = { pallet: 'Utility', call: 'batch_all', params: { calls: [sponsored.prepared.call] } }
  assert((await client.estimateFee(wrapped, keypair.ss58Address, { tip: 1000n })).rao === 1123n, 'wrapping a hotkey call charges the signer and keeps tip')
  const sponsoredShield = await client.plan(serve, { coldkey: sponsor, hotkey: keypair.ss58Address }, {}, { tip: 1000n, mevShield: true })
  assert(sponsoredShield.fee.rao === 1246n && sponsoredShield.violations.includes('signing account cannot cover the MEV carrier fee'), 'shield carrier charges signing account separately from sponsored inner call')
  client.rpc = ordinaryRpc
  const sponsorAccountKey = hex(runtime.storageKey('System', 'Account', [sponsor]))
  const lowAccount = { ...account, data: { ...account.data, free: 100n } }
  client.rpc = async (method, params = []) => method === 'state_getStorage' && params[0] === sponsorAccountKey ? hex(runtime.encode(accountEntry.valueType, lowAccount)) : ordinaryRpc(method, params)
  const proxyTransfer = { op: 'execute_proxy_announced', real_ss58: sponsor, delegate_ss58: keypair.ss58Address, inner_op: 'transfer', inner_args: { dest_ss58: keypair.ss58Address, amount_tao: '0.00000008' } }
  const mixed = await client.plan(sdk.buildIntent('batch', { intents: [proxyTransfer, proxyTransfer, intent.toJSON()] }), { coldkey: keypair.ss58Address }, { maxSpendRao: 161n })
  assert(mixed.violations.some(message => message.includes(sponsor) && message.includes('outgoing spend')), 'mixed wrapper batch sums debits per real account rather than charging the delegate')
  client.rpc = ordinaryRpc
  const proxyConsentKey = hex(runtime.storageKey('Proxy', 'RealPaysFeeConsentV1', [sponsor, keypair.ss58Address]))
  let proxyConsent = true, proxySubmissions = 0
  client.rpc = async (method, params = []) => {
    if (method === 'state_getKeysPaged') return proxyConsent && params[0] === proxyConsentKey ? [proxyConsentKey] : []
    if (method === 'state_getStorage' && params[0] === signingAccountKey) return hex(accountEntry.defaultBytes)
    if (method === 'author_submitExtrinsic') {
      const decoded = runtime.decodeExtrinsic(hexToBytes(params[0].slice(2)))
      assert(decoded.call.call_module === 'Proxy' && decoded.call.call_function === 'proxy', 'workflow submits its outer proxy descriptor')
      proxySubmissions++
    }
    return ordinaryRpc(method, params)
  }
  const proxyOptions = { proxyFor: sponsor, proxyType: 'Any', tip: 1000n, period: null, waitForInclusion: false }
  const proxyPlan = await client.plan(intent, { coldkey: keypair.ss58Address }, { maxFeeRao: 123n, maxSpendRao: 1n }, proxyOptions)
  assert(proxyPlan.ok && proxyPlan.fee.rao === 123n && proxyPlan.prepared.dispatchOrigin === sponsor && proxyPlan.prepared.spendByAccount[0].address === sponsor, 'proxy workflow preserves real-account spend and resolves consented sponsor fees')
  await client.execute(intent, keypair, { ...proxyOptions, policy: { maxFeeRao: 123n, maxSpendRao: 1n } })
  assert(proxySubmissions === 1, 'proxy workflow signs as delegate and submits wrapped call')
  proxyConsent = false
  const revoked = await client.plan(intent, { coldkey: keypair.ss58Address }, {}, proxyOptions)
  assert(!revoked.ok && revoked.fee.rao === 1123n, 'revoked proxy consent restores delegate fees and tip')
  client.rpc = ordinaryRpc
  const originalIntentReader = client.intentReader.bind(client)
  client.intentReader = async block => {
    const reader = await originalIntentReader(block)
    return { ...reader, constant: (pallet, item) => pallet === 'Multisig' && item === 'DepositBase' ? 5n : pallet === 'Multisig' && item === 'DepositFactor' ? 0n : reader.constant(pallet, item) }
  }
  client.rpc = async (method, params = []) => {
    if (method === 'state_getKeysPaged') return []
    if (method === 'state_getStorage' && params[0] === sponsorAccountKey) return hex(runtime.encode(accountEntry.valueType, { ...account, data: { ...account.data, free: 12n } }))
    return ordinaryRpc(method, params)
  }
  const mixedReserves = await client.plan(sdk.buildIntent('batch', { intents: [
    { op: 'multisig_execute', threshold: 2, other_signatories: [keypair.ss58Address], call: intent.toJSON() },
    { op: 'transfer', dest_ss58: keypair.ss58Address, amount_tao: '0.00000001' },
  ] }), { coldkey: keypair.ss58Address }, {}, { proxyFor: sponsor })
  assert(mixedReserves.prepared.requiredFreeByAccount.length === 2 && mixedReserves.violations.some(message => message.includes(sponsor) && message.includes('reserves')), 'proxy batch combines real-account deposit and outgoing spend separately from delegate fee reserve')
  client.intentReader = originalIntentReader
  client.rpc = ordinaryRpc
  assert(rpcCalls.filter(([method]) => method === 'payment_queryInfo').every(([, params]) => params[1] === blockHash), 'fee estimates remain pinned')
  rejected = false
  try { await client.plan(intent, { coldkey: keypair.ss58Address }, {}, { mevShield: true, period: 64 }) } catch { rejected = true }
  assert(rejected, 'shield period above8 rejected before signing')
  const sent = await client.execute(intent, keypair, { period: null, waitForInclusion: false, policy: { maxSpendRao: 1n } })
  assert(sent.status === 'submitted', 'browser intent execution')
  const feeBase = await client.prepareCall(plan.prepared.call, keypair, { allowRawCall: true, period: null, nonce: 0 })
  const baseLength = runtime.encodeSignedExtrinsic(feeBase.callData, feeBase.publicKey, new Uint8Array(64), 1, feeBase.txParams).bytes.length
  let metadataSigns = 0, quotedLength = 0, quotedFee = 0, quotedMetadataMode = false
  const metadataSigner = { ss58Address: keypair.ss58Address, publicKey: keypair.publicKey, cryptoType: 1, requiresMetadataHash: true, sign: payload => { metadataSigns++; return keypair.sign(payload) } }
  client.rpc = async (method, params = []) => {
    if (method === 'payment_queryInfo') {
      quotedLength = (params[0].length - 2) / 2
      quotedMetadataMode = JSON.stringify(runtime.decodeExtrinsic(hexToBytes(params[0].slice(2))).mode).includes('Enabled')
      quotedFee = quotedLength + (quotedMetadataMode ? 10 : 0)
      return { partialFee: String(quotedFee), weight: { refTime: 10, proofSize: 2 } }
    }
    if (method === 'author_submitExtrinsic') {
      assert((params[0].length - 2) / 2 === quotedLength, 'metadata signer actual length matches fee quote')
      assert(JSON.stringify(runtime.decodeExtrinsic(hexToBytes(params[0].slice(2))).mode).includes('Enabled') === quotedMetadataMode, 'metadata signer actual mode matches fee quote')
    }
    return ordinaryRpc(method, params)
  }
  rejected = false
  try { await client.execute(intent, metadataSigner, { period: null, waitForInclusion: false, policy: { maxFeeRao: BigInt(baseLength) } }) } catch { rejected = true }
  assert(rejected && metadataSigns === 0 && quotedMetadataMode, 'signer metadata mode is quoted before fee-policy approval and signing')
  await client.execute(intent, metadataSigner, { period: null, waitForInclusion: false, policy: { maxFeeRao: BigInt(quotedFee) } })
  assert(metadataSigns === 1, 'approved metadata signer executes the quoted transaction')
  client.rpc = ordinaryRpc
  let nextNonce = 0, nonceReads = 0
  client.invalidateNonce(keypair.ss58Address)
  const submittedNonces = []
  client.rpc = async (method, params = []) => {
    if (method === 'system_accountNextIndex') { nonceReads++; return nextNonce }
    if (method === 'author_submitExtrinsic') { submittedNonces.push(Number(runtime.decodeExtrinsic(hexToBytes(params[0].slice(2))).nonce)); nextNonce++ }
    return ordinaryRpc(method, params)
  }
  let releaseSigning, signingEntered
  const signingGate = new Promise(resolve => { releaseSigning = resolve })
  const enteredSigning = new Promise(resolve => { signingEntered = resolve })
  const heldSigner = { ss58Address: keypair.ss58Address, sign: async payload => { signingEntered(); await signingGate; return keypair.sign(payload) } }
  const rawPending = client.submit(plan.prepared.call, heldSigner, { allowRawCall: true, period: null, waitForInclusion: false })
  await enteredSigning
  const nonceReadsBefore = nonceReads
  const semanticPending = client.execute(intent, keypair, { period: null, waitForInclusion: false })
  await new Promise(resolve => setTimeout(resolve, 0))
  assert(nonceReads === nonceReadsBefore, 'raw and semantic submissions share account nonce queue')
  releaseSigning()
  await Promise.all([rawPending, semanticPending])
  assert(submittedNonces.join(',') === '0,1', 'queued raw and semantic calls obtain successive pending nonces')
  let mutableAddress = keypair.ss58Address, mutableSigns = 0
  const mutableSigner = { get ss58Address() { return mutableAddress }, sign: payload => { mutableSigns++; return keypair.sign(payload) } }
  const changedIdentity = client.execute(intent, mutableSigner, { period: null, waitForInclusion: false }).then(() => false, error => error.message.includes('identity changed'))
  mutableAddress = sponsor
  assert(await changedIdentity && mutableSigns === 0, 'signer account cannot change after queue admission or before signing')
  client.rpc = ordinaryRpc
  rejected = false
  try { await client.execute(intent, keypair, { waitForInclusion: false, policy: { maxSpendRao: 0n } }) } catch { rejected = true }
  assert(rejected, 'browser policy blocks excessive spend before signing')
  const ext = new sdk.ExtensionSigner({ address: keypair.ss58Address, type: 'sr25519', genesisHash: genesis }, { accounts: { get: async () => [] }, signer: { signPayload: async payload => { assert(payload.address === keypair.ss58Address, 'selected extension account'); return { signature: hex(keypair.sign(unsigned.payload)) } } } })
  assert((await ext.signPayload(unsigned.signerPayload)).signature.length === 130, 'extension adapter signs selected account')
  const wrongChainSigner = { ss58Address: keypair.ss58Address, genesisHash: '0x' + '00'.repeat(32), sign: payload => keypair.sign(payload) }
  rejected = false
  try { await client.prepareCall({ pallet: 'System', call: 'remark', params: { remark: new Uint8Array() } }, wrongChainSigner, { allowRawCall: true }) } catch { rejected = true }
  assert(rejected, 'signer account genesis is checked before signing')
  const vault = new sdk.VaultSigner(keypair.ss58Address, keypair.publicKey, async request => { assert(request.unsigned.genesisHash === genesis, 'Vault browser genesis uses hex'); return keypair.sign(request.unsigned.payload) }, 1, { blake2_256: sdk.blake2_256, publicKeyFromSs58: sdk.publicKeyFromSs58, ss58FromPublic: sdk.ss58FromPublic, verify: sdk.verifySignature })
  const vaultSignature = await vault.signPayload(unsigned.signerPayload, unsigned)
  assert(sdk.verifySignature(unsigned.payload, vaultSignature, keypair.ss58Address), 'Vault signs browser prepared context')
  const receiptClient = new sdk.Client('local')
  const receiptEvents = [{ extrinsic_idx: 0, module_id: 'System', event_id: 'ExtrinsicSuccess', attributes: {} }, { extrinsic_idx: 0, module_id: 'Proxy', event_id: 'ProxyExecuted', attributes: { result: { Err: 'BadOrigin' } } }, { extrinsic_idx: null, module_id: 'System', event_id: 'CodeUpdated' }]
  const decodedAt = []
  receiptClient.runtimeAt = async hash => { decodedAt.push(hash); return { storageKey: () => new Uint8Array(), storageEntry: () => ({ valueType: 0 }), decode: () => receiptEvents } }
  receiptClient.rpc = async method => method === 'chain_getBlock' ? { block: { header: { number: '0x40', parentHash: genesis }, extrinsics: [hex(signed.bytes)] } } : '0x00'
  const receipt = await receiptClient.findExtrinsic(hex(signed.hash), blockHash)
  assert(receipt.success === false && receipt.message.includes('BadOrigin') && receipt.chainError.code === 'not_authorized', 'nested dispatch failure overrides outer success')
  assert(decodedAt.length === 1 && decodedAt[0] === genesis, 'upgrade-block events decode using parent metadata')
  const carrier = { status: 'inBlock', success: true, message: 'Success', extrinsicHash: 'carrier', blockNumber: 64, events: [], fee: sdk.Balance.fromRao(5) }
  receiptClient.rpc = async method => method === 'chain_getHeader' ? { number: '0x48' } : blockHash
  receiptClient.findExtrinsic = async () => ({ ...carrier, status: 'failed', success: false, message: 'inner dispatch failed', extrinsicHash: 'inner', fee: sdk.Balance.fromRao(7) })
  const innerFailure = await receiptClient.resolveShieldedInner(carrier, 'inner', 8, {})
  assert(innerFailure.success === false && innerFailure.extrinsicHash === 'inner' && innerFailure.fee.rao === 12n, 'shield follows inner outcome and combines actual fees')
  receiptClient.findExtrinsic = async () => null
  const expired = await receiptClient.resolveShieldedInner(carrier, 'inner', 8, {})
  assert(expired.success === false && expired.message.includes('era expired'), 'carrier acceptance cannot mask missing inner execution')
  await receiptClient.close()
  await client.close()

  if (liveEndpoint && mode === 'package') {
    const live = new sdk.Client('local', { endpoint: liveEndpoint })
    try {
      await live.connect()
      const alice = sdk.Keypair.fromUri('//Alice')
      const at = await live.at()
      const balance = await at.read('balance', { coldkey_ss58: alice.ss58Address })
      assert(balance.rao > 1000000n, 'localnet Alice is funded')
      const transfer = sdk.buildIntent('transfer', { dest_ss58: alice.ss58Address, amount_tao: '0.000000001' })
      const plan = await live.plan(transfer, { coldkey: alice.ss58Address }, { maxSpendRao: 1n })
      assert(plan.ok && plan.prepared.spendRao === 1n, 'live browser bounded transfer preflight')
      const receipt = await live.execute(transfer, alice, { mevShield: false, policy: { maxSpendRao: 1n }, waitForInclusion: true, timeoutMs: 60000 })
      assert(receipt.success === true, 'live browser transfer succeeded: ' + receipt.message)
      assert(receipt.blockNumber != null && receipt.events.length > 0, 'live browser decoded receipt')
    } finally { await live.close() }
  }
  window.__SDK_BROWSER_RESULT__ = {
    ok: true,
    mode,
    address: keypair.ss58Address,
    callLength: call.length,
    extrinsicLength: signed.bytes.length,
  }
} catch (error) {
  window.__SDK_BROWSER_ERROR__ = {
    message: String(error?.message ?? error),
    stack: String(error?.stack ?? ''),
  }
}
`
}

async function bundleBrowserEntry() {
  const metadataHex = goldenMetadataHex()
  const customEntry = path.join(tmp, 'custom-entry.js')
  const defaultEntry = path.join(tmp, 'default-entry.js')
  const packageEntry = path.join(tmp, 'package-entry.js')
  const scope = path.join(tmp, 'node_modules', '@bittensor')
  fs.mkdirSync(scope, { recursive: true })
  fs.symlinkSync(root, path.join(scope, 'sdk'), 'dir')
  fs.writeFileSync(customEntry, browserEntry(metadataHex, 'custom'))
  fs.writeFileSync(defaultEntry, browserEntry(metadataHex, 'default'))
  fs.writeFileSync(packageEntry, browserEntry(metadataHex, 'package'))
  const outdir = path.join(tmp, 'site')
  fs.mkdirSync(outdir, { recursive: true })
  await esbuild.build({
    entryPoints: [customEntry, defaultEntry, packageEntry],
    bundle: true,
    format: 'esm',
    platform: 'browser',
    target: ['chrome110'],
    outdir,
    entryNames: '[name]',
    assetNames: 'assets/[name]-[hash]',
    loader: { '.wasm': 'file' },
    logLevel: 'silent',
  })
  fs.writeFileSync(
    path.join(outdir, 'custom.html'),
    '<!doctype html><meta charset="utf-8"><title>Bittensor SDK Browser Smoke</title><script type="module" src="/custom-entry.js"></script>',
  )
  fs.writeFileSync(
    path.join(outdir, 'default.html'),
    '<!doctype html><meta charset="utf-8"><title>Bittensor SDK Browser Smoke</title><script type="module" src="/default-entry.js"></script>',
  )
  fs.writeFileSync(path.join(outdir, 'package.html'), '<!doctype html><meta charset="utf-8"><title>Bittensor Package Browser Smoke</title><script type="module" src="/package-entry.js"></script>')
  return outdir
}

function serve(directory) {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1')
    const pathname = url.pathname === '/' ? '/index.html' : url.pathname
    const filePath = path.join(directory, pathname)
    if (!filePath.startsWith(directory) || !fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
      response.writeHead(404)
      response.end('not found')
      return
    }
    const extension = path.extname(filePath)
    const contentType = extension === '.html'
      ? 'text/html; charset=utf-8'
      : extension === '.js'
        ? 'text/javascript; charset=utf-8'
        : extension === '.wasm'
          ? 'application/wasm'
          : 'application/octet-stream'
    response.writeHead(200, {
      'content-type': contentType,
      'content-security-policy': `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self' ${liveEndpoint ? new URL(liveEndpoint).origin : ""}; base-uri 'none'; object-src 'none'`,
    })
    response.end(fs.readFileSync(filePath))
  })
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      server.off('error', reject)
      resolve({
        server,
        url: `http://127.0.0.1:${server.address().port}/`,
      })
    })
  })
}

function findChrome() {
  const candidates = [
    process.env.CHROME_BIN,
    process.env.CHROMIUM_BIN,
    'chromium',
    'chromium-browser',
    'google-chrome',
    'google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/snap/bin/chromium',
    '/opt/google/chrome/chrome',
  ].filter(Boolean)
  for (const candidate of candidates) {
    const result = spawnSync(candidate, ['--version'], { stdio: 'ignore' })
    if (result.status === 0) return candidate
  }
  if (!isCi) return undefined
  throw new Error('Chromium is required for browser smoke tests in CI; install chromium or set CHROME_BIN')
}

function launchChrome(chrome, url) {
  const profile = path.join(tmp, 'chrome-profile')
  fs.mkdirSync(profile, { recursive: true })
  const args = [
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--disable-dev-shm-usage',
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    url,
  ]
  const child = spawn(chrome, args, { stdio: ['ignore', 'ignore', 'pipe'] })
  let stderr = ''
  const devtools = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`timed out waiting for Chromium DevTools\n${stderr}`)), 15_000)
    child.once('exit', (code, signal) => {
      clearTimeout(timeout)
      reject(new Error(`Chromium exited before DevTools was ready: ${code ?? signal}\n${stderr}`))
    })
    child.stderr.on('data', (chunk) => {
      stderr += String(chunk)
      const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/)
      if (match == null) return
      clearTimeout(timeout)
      resolve(match[1])
    })
  })
  return { child, devtools }
}

function stopChrome(child) {
  if (child == null || child.exitCode != null) return Promise.resolve()
  return new Promise((resolve) => {
    let settled = false
    let force
    let giveUp
    const finish = () => {
      if (settled) return
      settled = true
      clearTimeout(force)
      clearTimeout(giveUp)
      resolve()
    }
    child.once('exit', finish)
    force = setTimeout(() => {
      if (child.exitCode == null) child.kill('SIGKILL')
    }, 2_000)
    giveUp = setTimeout(finish, 5_000)
    child.kill()
  })
}

function cdp(wsUrl) {
  const socket = new WebSocket(wsUrl)
  let nextId = 1
  const pending = new Map()
  const events = []

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', () => reject(new Error('failed to connect to Chromium DevTools')), { once: true })
  })

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(String(event.data))
    if (message.id != null) {
      const entry = pending.get(message.id)
      if (entry == null) return
      pending.delete(message.id)
      if (message.error) entry.reject(new Error(message.error.message))
      else entry.resolve(message.result)
      return
    }
    events.push(message)
  })

  return {
    ready,
    events,
    send(method, params = {}) {
      const id = nextId++
      const payload = JSON.stringify({ id, method, params })
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject })
        socket.send(payload)
      })
    },
    close() {
      socket.close()
    },
  }
}

async function pageWebSocketUrl(browserWsUrl) {
  const httpUrl = new URL(browserWsUrl.replace(/^ws:/, 'http:').replace(/^wss:/, 'https:'))
  const response = await fetch(`${httpUrl.origin}/json/list`)
  const targets = await response.json()
  const page = targets.find((target) => target.type === 'page')
  if (page?.webSocketDebuggerUrl == null) {
    throw new Error(`could not find Chromium page target: ${JSON.stringify(targets)}`)
  }
  return page.webSocketDebuggerUrl
}

async function waitForBrowserResult(client) {
  const deadline = Date.now() + (liveEndpoint ? 90_000 : 30_000)
  for (;;) {
    const result = await client.send('Runtime.evaluate', {
      expression: 'window.__SDK_BROWSER_RESULT__ || window.__SDK_BROWSER_ERROR__ || null',
      returnByValue: true,
      awaitPromise: false,
    })
    const value = result.result?.value
    if (value?.ok) return value
    if (value?.message) {
      throw new Error(`${value.message}\n${value.stack ?? ''}`)
    }
    const exception = client.events.find((event) => event.method === 'Runtime.exceptionThrown')
    if (exception != null) throw new Error(JSON.stringify(exception.params?.exceptionDetails ?? exception.params))
    if (Date.now() > deadline) throw new Error('timed out waiting for browser smoke result')
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
}

async function runBrowserPage(chromePath, url, mode) {
  let chrome
  let client
  try {
    chrome = launchChrome(chromePath, url)
    const browserWsUrl = await chrome.devtools
    client = cdp(await pageWebSocketUrl(browserWsUrl))
    await client.ready
    await client.send('Runtime.enable')
    await client.send('Page.enable')
    const result = await waitForBrowserResult(client)
    assert.equal(result.ok, true)
    assert.equal(result.mode, mode)
    assert.equal(typeof result.address, 'string')
    assert.ok(result.callLength > 0)
    assert.ok(result.extrinsicLength > result.callLength)
  } finally {
    client?.close()
    await stopChrome(chrome?.child)
  }
}

async function removeTmp() {
  let lastError
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      fs.rmSync(tmp, { recursive: true, force: true })
      return
    } catch (error) {
      lastError = error
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
  }
  try {
    fs.rmSync(tmp, { recursive: true, force: true, maxRetries: 50, retryDelay: 100 })
  } catch {
    throw lastError
  }
}

async function main() {
  let server
  try {
    const site = await bundleBrowserEntry()
    const chromePath = findChrome()
    if (chromePath == null) {
      console.warn('Skipping browser smoke tests because Chromium is not installed; CI requires Chromium.')
      return
    }
    const served = await serve(site)
    server = served.server
    await runBrowserPage(chromePath, new URL('/custom.html', served.url).href, 'custom')
    await runBrowserPage(chromePath, new URL('/default.html', served.url).href, 'default')
    await runBrowserPage(chromePath, new URL('/package.html', served.url).href, 'package')
  } finally {
    server?.close()
    await removeTmp()
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
