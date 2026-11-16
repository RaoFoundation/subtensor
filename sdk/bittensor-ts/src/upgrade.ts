import { evaluatePolicy } from './policy'
import { bytesHex, hexBytes, concatBytes, unsignedInteger, type PortableCrypto } from './portable'
import type { IntentDescriptor } from './intents'

export const DEFAULT_UPGRADE_REPO = 'RaoFoundation/subtensor'
export const UPGRADE_ASSETS = Object.freeze({ manifest: 'upgrade-manifest.json', call_data: 'proxy_proxy_blob.hex', wasm: 'subtensor.wasm', digest: 'subtensor-digest.json', pending_release: 'pending-release.json' })
export interface UpgradeWeight { ref_time: bigint | number | string; proof_size: bigint | number | string }
/** Published deployment weights from .github/scripts/deploy; callers may supply a newer release's weights. */
export const CI_PROPOSAL_WEIGHT = Object.freeze({ ref_time: 50_000_000_000n, proof_size: 0n })
export const CI_FINALIZE_WEIGHT = Object.freeze({ ref_time: 80_000_000_000n, proof_size: 50_000n })
export interface UpgradeAdapter {
  query(item: readonly [string, string], params?: unknown[]): Promise<unknown>
  queryMap(item: readonly [string, string], params?: unknown[]): Promise<Array<[unknown, unknown]>>
  compose(call: IntentDescriptor | Uint8Array): Promise<Uint8Array>
  crypto: Pick<PortableCrypto, 'blake2_256' | 'publicKeyFromSs58' | 'ss58FromPublic'>
  multisig(signatories: string[], threshold: number): { address: string } | Promise<{ address: string }>
  estimateWeight(call: Uint8Array, address: string): Promise<UpgradeWeight>
  /** Pin storage and metadata to the same block. */
  at(): Promise<UpgradeAdapter>
}
export class UpgradeRelease {
  constructor(readonly repo: string, readonly tag: string) {
    if (!/^[\w.-]+\/[\w.-]+$/.test(repo) || !tag || /[/?#\\]/.test(tag)) throw new TypeError('invalid GitHub release reference')
  }
  get url(): string { return `https://github.com/${this.repo}/releases/tag/${encodeURIComponent(this.tag)}` }
  assetUrl(name: string): string { return `https://github.com/${this.repo}/releases/download/${encodeURIComponent(this.tag)}/${encodeURIComponent(name)}` }
}
export function parseUpgradeRelease(value: string): UpgradeRelease {
  const short = /^([\w.-]+\/[\w.-]+)@([\w.-]+)$/.exec(value.trim())
  if (short) return new UpgradeRelease(short[1], short[2])
  const url = new URL(value)
  const match = /^\/([\w.-]+\/[\w.-]+)\/releases\/(?:tag|download)\/([^/]+)/.exec(url.pathname)
  if (url.protocol !== 'https:' || url.hostname !== 'github.com' || !match || url.username || url.password) throw new TypeError('expected a GitHub HTTPS release URL or owner/repo@tag')
  return new UpgradeRelease(match[1], decodeURIComponent(match[2]))
}
export interface UpgradeFetchOptions {
  fetch?: typeof globalThis.fetch
  githubToken?: string
  maxBytes?: number
  timeoutMs?: number
  signal?: AbortSignal
}
export class UpgradeFetchError extends Error { constructor(message: string, readonly status?: number) { super(message); this.name = 'UpgradeFetchError' } }
export async function fetchUpgradeBytes(url: string, options: UpgradeFetchOptions = {}, api = false): Promise<Uint8Array> {
  const parsed = new URL(url)
  if (parsed.protocol !== 'https:') throw new TypeError('upgrade artifacts require HTTPS')
  const max = options.maxBytes ?? 64 * 1024 * 1024
  if (!Number.isSafeInteger(max) || max <= 0) throw new RangeError('maxBytes must be positive')
  const timeoutMs = options.timeoutMs ?? 60_000
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) throw new RangeError('timeoutMs must be positive')
  const controller = new AbortController(), cancel = () => controller.abort(options.signal?.reason)
  options.signal?.addEventListener('abort', cancel, { once: true })
  if (options.signal?.aborted) cancel()
  const timer = setTimeout(() => controller.abort(new Error('artifact fetch timed out')), timeoutMs)
  try {
    const headers: Record<string, string> = {}
    if (api) {
      if (parsed.hostname !== 'api.github.com') throw new TypeError('GitHub API credentials may only be sent to api.github.com')
      headers.Accept = 'application/vnd.github+json'
      if (options.githubToken) headers.Authorization = `Bearer ${options.githubToken}`
    }
    const response = await (options.fetch ?? globalThis.fetch)(url, { headers, signal: controller.signal, redirect: api ? 'error' : 'follow' })
    if (!response.ok) throw new UpgradeFetchError(`fetching ${url} failed: HTTP ${response.status}`, response.status)
    const declared = response.headers.get('content-length')
    if (declared != null && Number(declared) > max) { await response.body?.cancel(); throw new UpgradeFetchError('artifact exceeds fetch size cap') }
    const reader = response.body?.getReader()
    if (!reader) return new Uint8Array()
    const chunks: Uint8Array[] = []; let size = 0
    try { for (;;) { const result = await reader.read(); if (result.done) break; size += result.value.length; if (size > max) throw new UpgradeFetchError('artifact exceeds fetch size cap'); chunks.push(result.value) } }
    catch (error) { await reader.cancel().catch(() => undefined); throw error }
    finally { reader.releaseLock() }
    return concatBytes(...chunks)
  } finally { clearTimeout(timer); options.signal?.removeEventListener('abort', cancel) }
}
function object(value: unknown, name = 'value'): Record<string, any> {
  if (!value || typeof value !== 'object' || Array.isArray(value) || value instanceof Uint8Array) throw new TypeError(`${name} must be an object`)
  return value as Record<string, any>
}
async function json(url: string, options: UpgradeFetchOptions, api = false): Promise<unknown> { return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await fetchUpgradeBytes(url, options, api))) }
export function parseUpgradeHex(raw: Uint8Array | string): Uint8Array {
  const text = (typeof raw === 'string' ? raw : new TextDecoder('utf-8', { fatal: true }).decode(raw)).trim().replace(/^0x/, '')
  if (!text || text.length % 2 || !/^[0-9a-f]+$/i.test(text)) throw new TypeError('call data is not a valid hex string')
  return hexBytes(text)
}
export interface UpgradeBundle {
  blob: Uint8Array
  release?: UpgradeRelease
  manifest?: Record<string, any>
  wasm?: Uint8Array
  digest?: Record<string, any>
  wasmSource?: 'local' | 'release'
  notes: string[]
}
export async function fetchUpgradeBundle(source: { releaseUrl?: string; hexUrl?: string; hex?: string | Uint8Array; wasm?: Uint8Array }, options: UpgradeFetchOptions = {}): Promise<UpgradeBundle> {
  if ([source.releaseUrl, source.hexUrl, source.hex].filter(value => value != null).length !== 1) throw new TypeError('provide exactly one of releaseUrl, hexUrl or hex')
  const release = source.releaseUrl ? parseUpgradeRelease(source.releaseUrl) : undefined
  const bundle: UpgradeBundle = { blob: parseUpgradeHex(source.hex ?? await fetchUpgradeBytes(source.hexUrl ?? release!.assetUrl(UPGRADE_ASSETS.call_data), options)), release, notes: [] }
  if (release) {
    for (const key of ['manifest', 'digest'] as const) {
      try { bundle[key] = object(await json(release.assetUrl(UPGRADE_ASSETS[key]), options), key) }
      catch (error) { if (!(error instanceof UpgradeFetchError) || error.status !== 404) throw error; bundle.notes.push(`release has no ${UPGRADE_ASSETS[key]} asset`) }
    }
  }
  if (source.wasm) { bundle.wasm = source.wasm.slice(); bundle.wasmSource = 'local' }
  else if (release) {
    try { bundle.wasm = await fetchUpgradeBytes(release.assetUrl(UPGRADE_ASSETS.wasm), options); bundle.wasmSource = 'release' }
    catch (error) { if (!(error instanceof UpgradeFetchError) || error.status !== 404) throw error; bundle.notes.push('release has no subtensor.wasm asset') }
  }
  return bundle
}
export async function fetchUpgradeManifests(repo = DEFAULT_UPGRADE_REPO, options: UpgradeFetchOptions & { limit?: number } = {}): Promise<Record<string, any>[]> {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new TypeError('invalid repository')
  const limit = options.limit ?? 15
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) throw new RangeError('release limit must be 1..100')
  const releases = await json(`https://api.github.com/repos/${repo}/releases?per_page=${limit}`, options, true)
  if (!Array.isArray(releases)) throw new TypeError('GitHub releases response must be an array')
  const manifests: Record<string, any>[] = []
  for (const entry of releases) {
    const release = object(entry), asset = (release.assets ?? []).find((item: any) => item.name === UPGRADE_ASSETS.manifest)
    if (!asset) continue
    const ref = parseUpgradeRelease(String(release.html_url))
    // Never trust a release asset URL to send a credential or redirect to an arbitrary host.
    if (ref.repo !== repo) throw new TypeError('release repository mismatch')
    const manifest = object(await json(ref.assetUrl(UPGRADE_ASSETS.manifest), options), 'manifest')
    manifests.push({ ...manifest, release_url: ref.url, prerelease: Boolean(release.prerelease) })
  }
  return manifests
}
export function findUpgradeManifest(manifests: Record<string, any>[], callHash: string): Record<string, any> | null { return manifests.find(item => evaluatePolicy<boolean>('upgrade.hash_matches', { expected: String(item.call_hash), actual: callHash })) ?? null }

function weight(value: UpgradeWeight): { ref_time: bigint; proof_size: bigint } { return { ref_time: unsignedInteger(value.ref_time, 64, 'ref_time'), proof_size: unsignedInteger(value.proof_size, 64, 'proof_size') } }
function equal(a: Uint8Array, b: Uint8Array): boolean { return a.length === b.length && a.every((byte, i) => byte === b[i]) }
function addressEqual(adapter: UpgradeAdapter, a: string, b: string): boolean { return evaluatePolicy<boolean>('upgrade.identity_matches', { expected: adapter.crypto.publicKeyFromSs58(a), actual: adapter.crypto.publicKeyFromSs58(b) }) }
export function parseUpgradeBlob(blob: Uint8Array, proposalWeight: UpgradeWeight): { realPublicKey: Uint8Array; code: Uint8Array } {
  const result = evaluatePolicy<{ real: string; code: string }>('validation.upgrade.blob', { data: blob, weight: weight(proposalWeight) })
  return { realPublicKey: hexBytes(result.real), code: hexBytes(result.code) }
}
/** Extract runtime_version in Rust without executing the supplied runtime. */
export function upgradeRuntimeSpecVersion(wasm: Uint8Array): number | null {
  return evaluatePolicy('validation.upgrade.spec_version', { data: wasm })
}

function call(pallet: string, name: string, params: Record<string, unknown>): IntentDescriptor { return { pallet, call: name, params } }
export async function reconstructUpgradeCall(adapter: UpgradeAdapter, wasm: Uint8Array, sudoKey: string, proposalWeight: UpgradeWeight): Promise<Uint8Array> {
  return adapter.compose(call('Proxy', 'proxy', { real: sudoKey, force_proxy_type: null, call: call('Sudo', 'sudo_unchecked_weight', { call: call('System', 'set_code', { code: wasm }), weight: weight(proposalWeight) }) }))
}
export interface UpgradeTimepoint { height: number; index: number }
export interface PendingUpgrade { kind: 'runtime-upgrade-proposal'; call_hash: string; timepoint: UpgradeTimepoint; timepoint_display: string; sudo_key: string; deployment_multisig: string; ci_address: string; deployment_approvals: string[] }
function safeNumber(value: unknown, name: string): number { const n = Number(value); if (!Number.isSafeInteger(n) || n < 0) throw new TypeError(`invalid ${name}`); return n }
function timepoint(value: unknown): UpgradeTimepoint { const o = object(value, 'timepoint'); return { height: safeNumber(o.height, 'height'), index: safeNumber(o.index, 'index') } }
function hexHash(value: unknown): string { const result = value instanceof Uint8Array ? bytesHex(value) : String(value); if (!/^0x[0-9a-f]{64}$/i.test(result)) throw new TypeError('expected 32-byte call hash'); return result.toLowerCase() }
async function discover(adapter: UpgradeAdapter): Promise<PendingUpgrade[]> {
  const sudo = await adapter.query(['Sudo', 'Key'])
  if (sudo == null) return []
  const sudoKey = String(sudo), proxies = await adapter.query(['Proxy', 'Proxies'], [sudoKey]) as [any[], unknown] | null, records: PendingUpgrade[] = []
  for (const proxy of proxies?.[0] ?? []) {
    const variant = typeof proxy.proxy_type === 'string' ? proxy.proxy_type : Object.keys(proxy.proxy_type ?? {})[0]
    const delay = Number(proxy.delay)
    if (!evaluatePolicy<boolean>('upgrade.proxy_eligible', { proxy_type: variant, delay: Number.isFinite(delay) ? delay : null })) continue
    for (const [key, value] of await adapter.queryMap(['Multisig', 'Multisigs'], [proxy.delegate])) {
      const pending = object(value), ci = String(pending.depositor)
      if (!addressEqual(adapter, (await adapter.multisig([ci, sudoKey], 2)).address, String(proxy.delegate))) continue
      const when = timepoint(pending.when), hash = hexHash(Array.isArray(key) ? key[key.length - 1] : key)
      records.push({ kind: 'runtime-upgrade-proposal', call_hash: hash, timepoint: when, timepoint_display: `${when.height}:${when.index}`, sudo_key: sudoKey, deployment_multisig: String(proxy.delegate), ci_address: ci, deployment_approvals: (pending.approvals ?? []).map(String) })
    }
  }
  return records
}
export interface UpgradeCheck { name: string; ok: boolean | null; detail: string }
export interface UpgradeCheckOutcome { ok: boolean; checks: UpgradeCheck[]; callHash: string; sudoKey: string; pending: PendingUpgrade | null; specVersion: number | null; codeSha256?: string }
export interface UpgradeCheckOptions { proposalWeight: UpgradeWeight; expectedSpecVersion?: number; expectedCommit?: string }
async function sha256(data: Uint8Array): Promise<string> { return bytesHex(new Uint8Array(await globalThis.crypto.subtle.digest('SHA-256', Uint8Array.from(data)))) }
export async function checkUpgradeProposal(adapter: UpgradeAdapter, bundle: UpgradeBundle, options: UpgradeCheckOptions): Promise<UpgradeCheckOutcome> {
  adapter = await adapter.at()
  const checks: UpgradeCheck[] = [], callHash = hexHash(adapter.crypto.blake2_256(bundle.blob)), sudoKey = String(await adapter.query(['Sudo', 'Key'])), pending = (await discover(adapter)).find(p => p.call_hash === callHash) ?? null
  const result: UpgradeCheckOutcome = { ok: false, checks, callHash, sudoKey, pending, specVersion: null }
  const check = (name: string, ok: boolean | null, detail: string) => checks.push({ name, ok, detail })
  let parsed: ReturnType<typeof parseUpgradeBlob> | undefined
  try { parsed = parseUpgradeBlob(bundle.blob, options.proposalWeight); check('call structure', true, 'exact proxy/sudo/set_code call with the supplied proposal weight') }
  catch (error) { check('call structure', false, String(error)) }
  if (parsed) {
    const real = adapter.crypto.ss58FromPublic(parsed.realPublicKey), rebuilt = await reconstructUpgradeCall(adapter, parsed.code, real, options.proposalWeight)
    check('re-encoding', equal(rebuilt, bundle.blob), 'reconstructed against metadata at the verification block')
    result.codeSha256 = await sha256(parsed.code)
    check('runtime match', bundle.wasm ? equal(parsed.code, bundle.wasm) : null, bundle.wasm ? `compared with ${bundle.wasmSource ?? 'supplied'} runtime bytes` : 'no independent runtime bytes supplied')
    if (bundle.digest) check('srtool digest', evaluatePolicy<boolean>('upgrade.digest_matches', { expected: String(bundle.digest.sha256), actual: result.codeSha256 }), 'SHA-256 compared with published digest')
    check('sudo key', addressEqual(adapter, real, sudoKey), 'proxied account must be the current sudo key')
    try { result.specVersion = upgradeRuntimeSpecVersion(parsed.code); check('runtime version', result.specVersion == null ? null : options.expectedSpecVersion == null || result.specVersion === options.expectedSpecVersion, result.specVersion == null ? 'runtime_version section unavailable (for compressed runtime, provide a reproduced build and expectedSpecVersion)' : `embedded spec_version ${result.specVersion}`) }
    catch (error) { check('runtime version', false, String(error)) }
  }
  check('on-chain proposal', pending != null, 'the exact call hash must be pending in the CI + sudo deployment multisig')
  const manifest = bundle.manifest
  if (manifest) {
    const provenance = evaluatePolicy<Record<string, boolean>>('upgrade.manifest_checks', {
      manifest, call_hash: callHash, code_sha256: result.codeSha256, manifest_version: Number.isFinite(Number(manifest.spec_version)) ? Number(manifest.spec_version) : null,
      spec_version: result.specVersion, expected_spec_version: options.expectedSpecVersion, expected_commit: options.expectedCommit,
      release: bundle.release ? { repo: bundle.release.repo, tag: bundle.release.tag, assets: Object.fromEntries(['wasm', 'digest', 'call_data', 'manifest'].map(key => [key, bundle.release!.assetUrl(UPGRADE_ASSETS[key as keyof typeof UPGRADE_ASSETS])])) } : undefined,
    })
    check('manifest call hash', provenance['manifest call hash'], 'manifest hash must identify these exact call bytes')
    if (result.codeSha256) check('manifest runtime hash', provenance['manifest runtime hash'], 'manifest wasm_sha256 must match embedded runtime')
    if (pending) {
      for (const [key, expected] of [['ci_address', pending.ci_address], ['sudo_key', sudoKey], ['deployment_multisig', pending.deployment_multisig]]) {
        try { check(`manifest ${key}`, addressEqual(adapter, String(manifest[key]), expected), 'manifest account must match chain state') } catch (error) { check(`manifest ${key}`, false, String(error)) }
      }
    }
    check('manifest spec version', provenance['manifest spec version'], 'manifest spec_version compared with embedded/expected version when available')
    check('manifest commit', provenance['manifest commit'], 'manifest commit must be an immutable Git SHA and match expectedCommit when supplied')
    try { const m = weight(object(manifest.max_weight) as UpgradeWeight), supplied = weight(options.proposalWeight); check('manifest proposal weight', m.ref_time === supplied.ref_time && m.proof_size === supplied.proof_size, 'published weight must match the supplied proposal weight') } catch (error) { check('manifest proposal weight', false, String(error)) }
    try { const sudo = object(manifest.sudo), signers = signerSet(sudo.signatories, sudo.threshold, adapter); check('manifest sudo signers', addressEqual(adapter, (await adapter.multisig(signers, Number(sudo.threshold))).address, sudoKey), 'published signer set must derive the live sudo key') } catch (error) { check('manifest sudo signers', false, String(error)) }
    if (bundle.release) {
      check('release identity', provenance['release identity'], 'manifest repository/tag must match the fetched release')
      for (const key of ['wasm', 'digest', 'call_data', 'manifest'] as const) check(`release ${key} asset`, provenance[`release ${key} asset`], 'manifest asset URL must refer to this exact release')
    }
  } else {
    if (options.expectedCommit != null) check('manifest commit', false, 'a release manifest is required to establish expectedCommit')
    if (options.expectedSpecVersion != null && result.specVersion == null) check('expected spec version', false, 'neither embedded version nor release manifest can establish expectedSpecVersion')
  }
  result.ok = checks.every(item => item.ok !== false)
  return result
}
function signerSet(value: unknown, threshold: unknown, adapter: UpgradeAdapter): string[] {
  if (!Array.isArray(value) || value.some(s => typeof s !== 'string')) throw new TypeError('signatories must be an address array')
  const n = safeNumber(threshold, 'threshold')
  const indices = evaluatePolicy<number[]>('upgrade.signers', { members: value.map(address => ({ address, identity: bytesHex(adapter.crypto.publicKeyFromSs58(address)) })), threshold: n })
  return indices.map(index => value[index])
}
export async function composeUpgradeFinalization(adapter: UpgradeAdapter, pending: PendingUpgrade, blob: Uint8Array, finalizeWeight: UpgradeWeight): Promise<Uint8Array> {
  return adapter.compose(call('Multisig', 'as_multi', { threshold: 2, other_signatories: [pending.ci_address], maybe_timepoint: pending.timepoint, call: blob, max_weight: weight(finalizeWeight) }))
}
export interface UpgradeApprovalPlan { position: 'first' | 'interior' | 'final' | 'signed'; checks: UpgradeCheckOutcome; finalizingCall: Uint8Array; finalizingHash: string; call: IntentDescriptor | null; approvals: string[]; remaining: string[] }
/** Re-verifies current chain state on every invocation; call immediately before approval submission. Never submits. */
export async function planUpgradeApproval(adapter: UpgradeAdapter, bundle: UpgradeBundle, options: UpgradeCheckOptions & { finalizeWeight: UpgradeWeight; signer: string; signatories?: string[]; threshold?: number }): Promise<UpgradeApprovalPlan> {
  adapter = await adapter.at()
  const checks = await checkUpgradeProposal(adapter, bundle, options)
  if (!checks.ok || !checks.pending) throw new Error(`upgrade verification failed: ${checks.checks.filter(c => c.ok === false).map(c => c.name).join(', ')}`)
  const threshold = options.threshold ?? bundle.manifest?.sudo?.threshold, signatories = signerSet(options.signatories ?? bundle.manifest?.sudo?.signatories, threshold, adapter)
  if (!signatories.some(address => addressEqual(adapter, address, options.signer))) throw new TypeError('signer is not in the sudo signer set')
  if (!addressEqual(adapter, (await adapter.multisig(signatories, Number(threshold))).address, checks.sudoKey)) throw new TypeError('signer set does not derive the chain sudo key')
  const finalizingCall = await composeUpgradeFinalization(adapter, checks.pending, bundle.blob, options.finalizeWeight), finalizingHash = hexHash(adapter.crypto.blake2_256(finalizingCall))
  const pending = await adapter.query(['Multisig', 'Multisigs'], [checks.sudoKey, finalizingHash]) as Record<string, any> | null
  const approvals: string[] = (pending?.approvals ?? []).map(String)
  const state = evaluatePolicy<{ position: UpgradeApprovalPlan['position']; remaining_indices: number[]; other_indices: number[] }>('upgrade.approval', { present: pending != null, threshold: Number(threshold), signer: adapter.crypto.publicKeyFromSs58(options.signer), members: signatories.map(address => adapter.crypto.publicKeyFromSs58(address)), approvals: approvals.map(address => adapter.crypto.publicKeyFromSs58(address)) })
  const position = state.position, remaining = state.remaining_indices.map(index => signatories[index])
  let approval: IntentDescriptor | null = null
  if (position !== 'signed') {
    const estimate = weight(await adapter.estimateWeight(finalizingCall, options.signer)), padded = evaluatePolicy<{ ref_time: string; proof_size: string }>('upgrade.weight_pad', estimate), max_weight = { ref_time: BigInt(padded.ref_time), proof_size: BigInt(padded.proof_size) }
    const params = { threshold: Number(threshold), other_signatories: state.other_indices.map(index => signatories[index]), maybe_timepoint: pending ? timepoint(pending.when) : null, max_weight }
    approval = position === 'final' ? call('Multisig', 'as_multi', { ...params, call: finalizingCall }) : call('Multisig', 'approve_as_multi', { ...params, call_hash: finalizingHash })
  }
  return { position, checks, finalizingCall, finalizingHash, call: approval, approvals, remaining }
}
export class UpgradeClient {
  constructor(readonly adapter: UpgradeAdapter) {}
  async pending(): Promise<PendingUpgrade[]> { return discover(await this.adapter.at()) }
  check(bundle: UpgradeBundle, options: UpgradeCheckOptions): Promise<UpgradeCheckOutcome> { return checkUpgradeProposal(this.adapter, bundle, options) }
  planApproval(bundle: UpgradeBundle, options: Parameters<typeof planUpgradeApproval>[2]): Promise<UpgradeApprovalPlan> { return planUpgradeApproval(this.adapter, bundle, options) }
}
export function upgradeReproductionRecipe(manifest?: Record<string, any>): { repository: string; revision: string; image: string; environment: Record<string, string>; runtimePackage: string } {
  return { repository: `https://github.com/${manifest?.repo ?? DEFAULT_UPGRADE_REPO}`, revision: String(manifest?.commit ?? manifest?.tag ?? '<commit>'), image: `paritytech/srtool:${manifest?.srtool_rustc ?? '<rustc-tag>'}`, environment: { PACKAGE: 'node-subtensor-runtime', BUILD_OPTS: '--features=metadata-hash', PROFILE: 'production', CONST_RANDOM_SEED: 'subtensor-runtime' }, runtimePackage: 'runtime/node-subtensor' }
}
