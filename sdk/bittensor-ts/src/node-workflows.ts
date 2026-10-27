import { Balance, assetIdValue, taoTransactionAmountRao } from './balance'
import { nativeRustClient, type Client, type SignerLike, type ChainSigner, type ExtrinsicResult, type SubmitOptions, type CallLike } from './client'
import { Intent, intentFromJSON, intentSignerRole, checkIntentPolicy, encodeIntentCall, type IntentContext, type IntentDescriptor, type IntentPolicy, type IntentReader, type PreparedIntent } from './intents'
import { IntentCall, Policy, rawCall, type NativeChainClient } from './transaction'
import { Wallet, type WalletPasswordOptions } from './wallet'
import { Keypair, publicKeyFromSs58, ss58FromPublic } from './keys'
import { blake2_256, hexToBytes, sealMevShieldTransaction } from './crypto'
import { Runtime, multisigAccountId, storagePrefixFor } from './runtime'
import { fromWire } from './wire'
import type { NativeExternalSigningOptions } from './native'
import { generateCommitV2 } from './timelock'
import { iterCallLeaves } from './fee-filters'
import { resolveTransactionFeePayer } from './fee-payer'
import { prepareMultisigApproval } from './multisig-workflow'
import { followShieldedResult } from './shielded-receipt'
import { runSignerQueue, signerQueueOptions, bindSignerIdentity, type BoundSignerIdentity, type SignerQueuePermit } from './signer-queue'
import type { ScaleValue } from './types'

export type WorkflowSigner = SignerLike | Wallet
export interface WorkflowOptions extends Omit<SubmitOptions, 'policy' | 'allowRawCall' | 'rawSignerRole'>, WalletPasswordOptions {
  policy?: IntentPolicy
  /** Defaults to the semantic operation's MEV setting; mandatory shielding cannot be disabled. */
  mevShield?: boolean
  proxyFor?: string
  proxyType?: string | null
  hotkey?: string
  hotkeyPublicKey?: Uint8Array
  /** When preparing a multisig, refuse if its exact call hash changes before signing. */
  expectedCallHash?: string
}
export interface WorkflowPlan {
  readonly intent: PreparedIntent
  readonly call: IntentCall
  readonly signerAddress: string
  readonly feePayer: string
  readonly fee: Balance
  readonly carrierFee: Balance
  readonly shielded: boolean
  readonly blockHash: string
  readonly violations: readonly string[]
  readonly ok: boolean
}
export interface WorkflowResult extends ExtrinsicResult {
  /** Includes reveal round, multisig coordinates, or shielded-inner hash where applicable. */
  data?: Record<string, unknown>
}

/** Python-style semantic planning/execution on the native client, including external signers. */
export class WorkflowClient {
  constructor(readonly client: Client) {}

  submitShielded(value: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowResult> { return this.execute(value, signer, { ...options, mevShield: true }) }

  async multisig(signatories: readonly string[], threshold: number): Promise<NodeMultisig> {
    const runtime = await this.client.runtimeAt()
    return new NodeMultisig(this.client, signatories, threshold, runtime.ss58Format)
  }

  async plan(value: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowPlan> {
    const intent = value instanceof Intent ? value : intentFromJSON(value)
    const native = await nativeRustClient(this.client)
    if (!native) throw new Error('native workflow requires a native Client')
    const blockHash = await this.client.blockHash()
    const runtime = await this.client.runtimeAt(blockHash)
    const identity = await publicIdentity(this.client, intent, signer, options)
    const origin = options.proxyFor ?? identity.address
    const context: IntentContext = { coldkey: origin, hotkey: identity.hotkey, hotkeyPublicKey: identity.hotkeyPublicKey, feePayer: identity.address }
    if (options.proxyFor && intentSignerRole(intent) === 'hotkey') { context.hotkey = options.proxyFor; context.hotkeyPublicKey = publicKeyFromSs58(options.proxyFor) }
    const baseReader = await createNodeIntentReader(this.client, native, blockHash)
    const wrappedCall = (call: IntentDescriptor): IntentDescriptor => options.proxyFor ? { pallet: 'Proxy', call: 'proxy', params: { real: options.proxyFor, force_proxy_type: options.proxyType ?? null, call } } : call
    const reader: IntentReader = {
      ...baseReader,
      resolveFeePayer: async (call, address) => (await resolveTransactionFeePayer(baseReader, wrappedCall(call), address)).address,
      estimateFee: (call, address) => baseReader.estimateFee!(wrappedCall(call), address),
    }
    const prepared = await intent.prepare(reader, context)
    if (options.expectedCallHash != null && String(prepared.extras.multisig_call_hash).toLowerCase() !== options.expectedCallHash.toLowerCase()) throw new Error('multisig call changed from expectedCallHash')
    const descriptor = wrappedCall(prepared.call)
    const call = IntentCall.fromDescriptor(native, descriptor as unknown as ScaleValue)
    const shielded = options.mevShield ?? prepared.mevShieldDefault
    const estimateNonce = options.nonce ?? await native.accountNextIndex(identity.address)
    const signingOptions = nativeOptions({ ...options, nonce: estimateNonce + (shielded ? 1 : 0), period: shielded ? 8 : options.period === undefined ? 128 : options.period })
    const innerPlan = await native.externalSigningPlanForIntent(call, identity.address, identity.publicKey, identity.cryptoType, identity.requiresMetadataProof, new Policy(), signingOptions)
    const tip = taoTransactionAmountRao(options.tip ?? 0n, 'tip')
    const payer = await resolveTransactionFeePayer(reader, descriptor, identity.address)
    const innerTip = payer.dropsTip ? 0n : tip
    const innerFee = (innerPlan.feeRao == null ? await native.estimateFeeExternal(innerPlan) : BigInt(innerPlan.feeRao)) + innerTip
    const violations = [...prepared.violations]
    if (prepared.mevShieldRequired && !shielded) violations.push(`${intent.op} requires MEV shielding`)
    let carrierFee = 0n
    if (shielded) {
      const carrier = await this.client.composeCall('MevShield', 'submit_encrypted', { ciphertext: new Uint8Array(8192) }, blockHash)
      carrierFee = await estimateRawFee(native, carrier, identity.address, identity.publicKey, identity.cryptoType, { ...signingOptions, nonce: BigInt(estimateNonce) }, identity.requiresMetadataProof) + tip
    }
    const fee = innerFee + carrierFee
    // The semantic policy is checked before any key is unlocked or asked to sign.
    violations.push(...checkIntentPolicy(prepared, options.policy, fee).filter(item => !violations.includes(item)))
    const feePayer = payer.address
    const accountKey = (address: string) => publicKeyFromSs58(address).toString('hex')
    const spends = new Map<string, { address: string; amount: bigint }>()
    for (const debit of prepared.spendByAccount) {
      const key = accountKey(debit.address), old = spends.get(key)
      spends.set(key, { address: debit.address, amount: (old?.amount ?? 0n) + debit.amountRao })
    }
    const payerKey = accountKey(feePayer), signerKey = accountKey(identity.address)
    const required = new Map([...spends].map(([key, value]) => [key, { ...value }]))
    for (const [address, amount] of [[feePayer, innerFee], [identity.address, carrierFee]] as const) {
      const key = accountKey(address), old = required.get(key)
      required.set(key, { address, amount: (old?.amount ?? 0n) + amount })
    }
    for (const { address, amount } of required.values()) {
      const free = accountFree(await this.client.query('System', 'Account', [address], blockHash))
      if (free < amount) violations.push(`account ${address} needs ${amount} rao for fees and bounded outgoing spend, has ${free}`)
    }
    // Context-sensitive inner requirements (claim reserve, multisig deposit) also
    // need carrier headroom; adding it here avoids signing an unaffordable envelope.
    for (const reserve of prepared.requiredFreeByAccount) {
      const reserveKey = accountKey(reserve.address)
      if (reserve.availableRao < reserve.requiredRao + (spends.get(reserveKey)?.amount ?? 0n) + (reserveKey === signerKey ? carrierFee : 0n) + (reserveKey === payerKey ? innerTip : 0n)) violations.push(`account ${reserve.address} cannot cover the prepared reserves, outgoing spend, tip and MEV carrier fee`)
    }
    return Object.freeze({ intent: prepared, call, signerAddress: identity.address, feePayer, fee: Balance.fromRao(fee), carrierFee: Balance.fromRao(carrierFee), shielded, blockHash, violations: Object.freeze(violations), ok: violations.length === 0 })
  }

  /** Re-plan at the current head; a prior preview never authorizes a stale spend. */
  async execute(value: Intent | Record<string, unknown>, signer: WorkflowSigner, options: WorkflowOptions = {}): Promise<WorkflowResult> {
    const intent = value instanceof Intent ? value : intentFromJSON(value)
    const identity = await publicIdentity(this.client, intent, signer, options)
    const planningSigner = signer instanceof Wallet ? signer : bindSignerIdentity(signer, identity)
    return runSignerQueue(this.client, identity.address, async permit => {
      if (options.signal?.aborted) throw options.signal.reason ?? new Error('workflow aborted')
      let plan = await this.plan(intent, planningSigner, options)
      if (!publicKeyFromSs58(plan.signerAddress).equals(identity.publicKey)) throw new Error('signer account changed after preflight')
      if (!plan.ok) throw new Error(`intent preflight refused: ${plan.violations.join('; ')}`)
      let executionOptions = options
      if (plan.shielded && options.nonce == null) {
        const nextKey = chainBytes(await this.client.query('MevShield', 'NextKey', []))
        if (nextKey.length !== 1184) throw new Error('MevShield.NextKey is unavailable or not an ML-KEM-768 key')
        const native = await nativeRustClient(this.client)
        if (!native) throw new Error('native workflow requires a native Client')
        executionOptions = { ...options, nonce: await native.reserveNonces(identity.address, 2) }
        // The reservation may follow detached plans. Reprice at its exact nonce
        // so compact nonce length cannot invalidate a fee cap before signing.
        plan = await this.plan(intent, planningSigner, executionOptions)
        if (!publicKeyFromSs58(plan.signerAddress).equals(identity.publicKey)) throw new Error('signer account changed after preflight')
        if (!plan.ok) throw new Error(`intent preflight refused: ${plan.violations.join('; ')}`)
      }
      const resolvedSigner = bindSignerIdentity(planningSigner instanceof Wallet ? await planningSigner.signer(plan.intent.signerRole, options) : planningSigner, identity)
      const submission = signerQueueOptions(submitOptions({ ...executionOptions, period: plan.shielded ? 8 : options.period === undefined ? 128 : options.period }), permit)
      if (!plan.shielded) {
        const result = await this.client.submit(plan.call, resolvedSigner, { waitForInclusion: true, ...submission, policy: options.policy as any })
        return { ...result, data: { ...plan.intent.extras } }
      }
      return this.submitShieldedCall(plan.call, resolvedSigner, identity.address, { ...submission, policy: options.policy as any }, { ...plan.intent.extras })
    })
  }

  /** Encrypt a raw descriptor/call with an external signer, after explicit raw admission. */
  async submitRawShielded(call: CallLike, signer: SignerLike, options: SubmitOptions = {}): Promise<WorkflowResult> {
    const policy = Policy.from(options.policy)
    if (options.allowRawCall !== true && !policy.allowRawCalls) throw new Error('raw shielded calls require allowRawCall: true or a policy allowing raw calls')
    if (policy.hasOpaqueByteRestrictions()) throw new Error('raw shielded calls cannot prove fee, spend or subnet policy; use a semantic intent')
    const identity = await publicIdentity(this.client, 'coldkey', signer, {})
    const bound = bindSignerIdentity(signer, identity)
    return runSignerQueue(this.client, identity.address, permit => this.submitShieldedCall(call, bound, identity.address, signerQueueOptions({ ...options, allowRawCall: true }, permit)))
  }

  private async submitShieldedCall(call: CallLike, signer: SignerLike, address: string, options: SubmitOptions, extras: Record<string, unknown> = {}): Promise<WorkflowResult> {
    if (options.signal?.aborted) throw options.signal.reason ?? new Error('shielded submission aborted')
    if (options.nonce != null && (!Number.isSafeInteger(options.nonce) || options.nonce < 0 || options.nonce >= Number.MAX_SAFE_INTEGER)) throw new RangeError('invalid shielded nonce')
    const nextKey = await this.client.query('MevShield', 'NextKey', [])
    if (nextKey == null) throw new Error('MevShield.NextKey is unavailable')
    const publicKey = chainBytes(nextKey)
    if (publicKey.length !== 1184) throw new Error('MevShield.NextKey is unavailable or not an ML-KEM-768 key')
    const native = await nativeRustClient(this.client)
    if (!native && options.nonce == null) throw new Error('automatic shielded nonces require a native Client')
    const nonce = options.nonce ?? await native!.reserveNonces(address, 2)
    ;(signer as SignerLike & { beginTwoStageSigning?(): void }).beginTwoStageSigning?.()
    const submission = { ...options, period: 8 }
    const inner = await this.client.signExtrinsic(call, signer, { ...submission, nonce: nonce + 1 })
    const ciphertext = sealMevShieldTransaction(publicKey, inner.bytes)
    if (ciphertext.length > 8192) throw new RangeError('shielded ciphertext exceeds 8192 bytes')
    const carrier = await this.client.composeCall('MevShield', 'submit_encrypted', { ciphertext })
    // The carrier contains only the already checked signed inner call. Inner
    // semantic policy checks do not apply to this opaque encryption envelope.
    const { policy: _policy, ...carrierOptions } = submission
    const outer = await this.client.signExtrinsic(carrier, signer, { ...carrierOptions, nonce, allowRawCall: true })
    const wait = { ...carrierOptions, waitForInclusion: options.waitForInclusion ?? true }
    const result = await this.client.submitSigned(outer, address, wait)
    return followShieldedResult({ ...result, data: extras }, '0x' + Buffer.from(inner.hash).toString('hex'), 8, this.client, wait)
  }

}

export interface MultisigSubmitOptions extends WorkflowOptions { allowRawCall?: boolean; expectedCallHash?: string }

/** A deterministic signer set. Approvals resolve the live pending timepoint. */
export class NodeMultisig {
  readonly address: string
  readonly signatories: readonly string[]
  constructor(readonly client: Client, signatories: readonly string[], readonly threshold: number, ss58Format = 42) {
    const ids = signatories.map(publicKeyFromSs58), unique = new Set(ids.map(id => id.toString('hex')))
    if (unique.size !== ids.length) throw new RangeError('duplicate multisig signatory')
    if (!Number.isInteger(threshold) || threshold < 1 || threshold > ids.length || threshold > 65535) throw new RangeError('invalid multisig threshold')
    this.signatories = Object.freeze(signatories.slice().sort((a, b) => Buffer.compare(publicKeyFromSs58(a), publicKeyFromSs58(b))))
    this.address = ss58FromPublic(multisigAccountId(ids, threshold).accountId, ss58Format)
  }

  async approve(call: Intent | IntentDescriptor, signer: WorkflowSigner, options: MultisigSubmitOptions = {}): Promise<WorkflowResult> {
    const native = await nativeRustClient(this.client)
    if (!native) throw new Error('native multisig requires a native Client')
    const identity = await publicIdentity(this.client, 'coldkey', signer, options)
    const planningSigner = signer instanceof Wallet ? signer : bindSignerIdentity(signer, identity)
    const reader = await createNodeIntentReader(this.client, native)
    if (call instanceof Intent) {
      const hotkey = intentSignerRole(call) === 'hotkey'
      const inner = await call.prepare(reader, { coldkey: this.address, feePayer: identity.address, ...(hotkey ? { hotkey: this.address, hotkeyPublicKey: publicKeyFromSs58(this.address) } : {}) })
      const approval = await prepareMultisigApproval(reader, { call: inner.call, signatories: this.signatories, threshold: this.threshold, signerAddress: identity.address, expectedCallHash: options.expectedCallHash })
      const wrapped = new Intent(this.threshold === 1 ? 'multisig_threshold_1' : 'multisig_execute', {
        other_signatories: this.signatories.filter(member => !publicKeyFromSs58(member).equals(identity.publicKey)), call: call.toJSON(),
        ...(this.threshold === 1 ? {} : { threshold: this.threshold, timepoint: approval.timepoint }),
      })
      return new WorkflowClient(this.client).execute(wrapped, planningSigner, { ...options, expectedCallHash: approval.callHash })
    }
    return runSignerQueue(this.client, identity.address, permit => this.approveDescriptor(call, planningSigner, options, identity, permit))
  }

  private async approveDescriptor(call: IntentDescriptor, signer: WorkflowSigner, options: MultisigSubmitOptions, expectedIdentity: BoundSignerIdentity, permit: SignerQueuePermit): Promise<WorkflowResult> {
    const native = await nativeRustClient(this.client)
    if (!native) throw new Error('native multisig requires a native Client')
    const identity = await publicIdentity(this.client, 'coldkey', signer, options)
    if (!identity.publicKey.equals(Buffer.from(expectedIdentity.publicKey))) throw new Error('signer account changed after preflight')
    const reader = await createNodeIntentReader(this.client, native)
    const approval = await prepareMultisigApproval(reader, { call, signatories: this.signatories, threshold: this.threshold, signerAddress: identity.address, expectedCallHash: options.expectedCallHash })
    let trusted: IntentCall
    try { trusted = IntentCall.fromDescriptor(native, approval.call as unknown as ScaleValue) } catch (error) {
      if (!options.allowRawCall) throw error
      const encoded = await encodeIntentCall(call, (pallet, fn, params) => this.client.composeCall(pallet, fn, params as ScaleValue))
      trusted = rawCall(approval.call.pallet, approval.call.call, { ...approval.call.params, call: encoded } as ScaleValue)
    }
    const shielded = options.mevShield ?? false
    if (!shielded && [...iterCallLeaves(call)].some(([pallet, fn]) => pallet === 'SubtensorModule' && fn === 'add_collateral')) throw new Error('add_collateral requires MEV shielding')
    let nonce = options.nonce ?? await native.accountNextIndex(identity.address)
    if (!Number.isSafeInteger(nonce) || nonce < 0 || nonce >= Number.MAX_SAFE_INTEGER) throw new RangeError('invalid multisig nonce')
    const policy = Policy.from({ ...options.policy, allowedNetuids: options.policy?.allowedNetuids == null ? undefined : [...options.policy.allowedNetuids], allowRawCalls: options.allowRawCall })
    const carrier = shielded ? await this.client.composeCall('MevShield', 'submit_encrypted', { ciphertext: new Uint8Array(8192) }) : undefined
    const checkFunding = async () => {
      const signingOptions = nativeOptions({ ...options, nonce: nonce + (shielded ? 1 : 0), period: shielded ? 8 : options.period === undefined ? 128 : options.period })
      const inner = await native.externalSigningPlanForIntent(trusted, identity.address, identity.publicKey, identity.cryptoType, identity.requiresMetadataProof, policy, signingOptions)
      const tip = taoTransactionAmountRao(options.tip ?? 0n), innerFee = (inner.feeRao == null ? await native.estimateFeeExternal(inner) : BigInt(inner.feeRao)) + tip
      const carrierFee = carrier ? await estimateRawFee(native, carrier, identity.address, identity.publicKey, identity.cryptoType, { ...signingOptions, nonce: BigInt(nonce) }, identity.requiresMetadataProof) + tip : 0n
      const violations = [...approval.violations, ...policy.check(trusted, innerFee + carrierFee)]
      if (approval.availableFreeRao < approval.depositRao + innerFee + carrierFee) violations.push('multisig signer cannot cover deposit, fees and tips')
      if (violations.length) throw new Error(`multisig preflight refused: ${violations.join('; ')}`)
    }
    await checkFunding()
    const publicKey = shielded ? chainBytes(await this.client.query('MevShield', 'NextKey', [])) : undefined
    if (shielded && publicKey?.length !== 1184) throw new Error('MevShield.NextKey is unavailable or invalid')
    if (shielded && options.nonce == null) { nonce = await native.reserveNonces(identity.address, 2); await checkFunding() }
    const resolved = bindSignerIdentity(signer instanceof Wallet ? await signer.signer('coldkey', options) : signer, identity)
    const submission = signerQueueOptions({ ...submitOptions(options), policy, allowRawCall: options.allowRawCall }, permit)
    const data = { multisig_account: this.address, multisig_call_hash: approval.callHash, opening: approval.opening }
    if (!shielded) return { ...await this.client.submit(trusted, resolved, { waitForInclusion: true, ...submission }), data }
    ;(resolved as SignerLike & { beginTwoStageSigning?(): void }).beginTwoStageSigning?.()
    const signedInner = await this.client.signExtrinsic(trusted, resolved, { ...submission, nonce: nonce + 1, period: 8 })
    const ciphertext = sealMevShieldTransaction(publicKey!, signedInner.bytes)
    if (ciphertext.length > 8192) throw new RangeError('shielded ciphertext exceeds 8192 bytes')
    const encodedCarrier = await this.client.composeCall('MevShield', 'submit_encrypted', { ciphertext })
    const outer = await this.client.signExtrinsic(encodedCarrier, resolved, signerQueueOptions({ ...submitOptions(options), period: 8, nonce, allowRawCall: true }, permit))
    const result = await this.client.submitSigned(outer, identity.address, signerQueueOptions({ waitForInclusion: true, ...submitOptions(options) }, permit))
    return followShieldedResult({ ...result, data }, '0x' + Buffer.from(signedInner.hash).toString('hex'), 8, this.client, submission)
  }
}

async function publicIdentity(client: Client, intent: Intent | 'coldkey' | 'hotkey', signer: WorkflowSigner, options: WorkflowOptions) {
  const role = typeof intent === 'string' ? intent : intentSignerRole(intent)
  if (signer instanceof Wallet) {
    const coldkey = await signer.coldkeypub
    let hotkey: Keypair | undefined
    try { hotkey = await signer.hotkeypub } catch (error) { if (role === 'hotkey' && !options.hotkey) throw error }
    const identity = role === 'hotkey' ? hotkey : coldkey
    if (!identity) throw new Error('wallet hotkey public identity unavailable')
    return { address: identity.ss58Address, publicKey: identity.publicKey, cryptoType: identity.cryptoType, requiresMetadataProof: false, hotkey: options.hotkey ?? hotkey?.ss58Address, hotkeyPublicKey: options.hotkeyPublicKey ?? hotkey?.publicKey }
  }
  const runtime = await client.runtimeAt()
  const candidate = signer as ChainSigner
  const account = candidate.getAccount ? await candidate.getAccount({ client, runtime, ss58Format: runtime.ss58Format }) : candidate
  const address = account.ss58Address ?? account.address
  if (!address) throw new TypeError('signer must provide an account address')
  const publicKey = account.publicKey == null ? publicKeyFromSs58(address) : Buffer.from(account.publicKey)
  const cryptoType = account.cryptoType ?? candidate.cryptoType ?? 1
  const requiresMetadataProof = Boolean(account.requiresMetadataProof ?? account.requiresMetadataHash ?? candidate.requiresMetadataProof ?? candidate.requiresMetadataHash)
  return { address, publicKey, cryptoType, requiresMetadataProof, hotkey: options.hotkey ?? (role === 'hotkey' ? address : undefined), hotkeyPublicKey: options.hotkeyPublicKey ?? (role === 'hotkey' ? publicKey : undefined) }
}

export async function createNodeIntentReader(client: Client, native: NativeChainClient, at?: string): Promise<IntentReader> {
  const blockHash = at ?? await client.blockHash()
  const block = await client.blockNumber(blockHash)
  const runtime = await client.runtimeAt(blockHash)
  const compose = (call: IntentDescriptor) => encodeIntentCall(call, async (pallet, fn, params) => client.composeCall(pallet, fn, params as ScaleValue, blockHash))
  return {
    query: (pallet, item, params = []) => client.query(pallet, item, params as ScaleValue[], blockHash),
    queryMap: (pallet, item, params = [], limit) => client.queryMap(pallet, item, params as ScaleValue[], blockHash, { maxResults: limit }),
    countStorageKeys: async (pallet, item, params, limit) => {
      let prefix: Buffer
      if (pallet === 'SubtensorModule' && item === 'Alpha') {
        prefix = runtime.storageKey(pallet, 'AlphaV2', params as ScaleValue[])
        prefix = Buffer.concat([storagePrefixFor(pallet, item), prefix.subarray(32)])
      } else prefix = runtime.storageKey(pallet, item, params as ScaleValue[])
      const keys = await client.rpc('state_getKeysPaged', ['0x' + prefix.toString('hex'), limit, null, blockHash])
      if (!Array.isArray(keys)) throw new Error('storage key admission count unavailable')
      return keys.length
    },
    runtimeCall: (api, method, params = []) => client.runtimeCall(api, method, params as ScaleValue[], blockHash),
    constant: (pallet, name) => client.constant(pallet, name, blockHash),
    blockNumber: async () => block,
    blockHash: async () => blockHash,
    blockTime: () => client.blockTime(blockHash),
    decodeAddress: publicKeyFromSs58,
    encodeAddress: publicKey => ss58FromPublic(publicKey, runtime.ss58Format),
    multisigAccount: (signatories, threshold) => {
      const account = multisigAccountId(signatories.map(publicKeyFromSs58), threshold)
      return { address: ss58FromPublic(account.accountId, runtime.ss58Format), publicKey: account.accountId }
    },
    hashCall: async call => blake2_256(await compose(call)),
    evmMirror: address => ss58FromPublic(blake2_256(Buffer.concat([Buffer.from('evm:'), hexToBytes(address)])), runtime.ss58Format),
    encryptWeightsV2: async options => {
      const result = await generateCommitV2(options.uids, options.values, options.versionKey, options.state, options.subnetRevealPeriodEpochs, options.blockTime, options.hotkey)
      return { commit: result.ciphertext, revealRound: result.revealRound }
    },
    estimateFee: async (call, signer) => estimateRawFee(native, Buffer.from(await compose(call)), signer, publicKeyFromSs58(signer), 1),
    estimateWeight: async (call, signer) => {
      const unsigned = await native.externalSigningPlan(Buffer.from(await compose(call)), signer, publicKeyFromSs58(signer), 1, false, { nonce: BigInt(await native.accountNextIndex(signer)) })
      // Fee RPCs accept a placeholder signature; assembly for submission verifies
      // cryptography and must never be used with this read-only placeholder.
      const params = unsigned.txParams
      const ext = Runtime.fromNativeHandle(unsigned.runtime).encodeSignedExtrinsic(unsigned.callData, unsigned.publicKey, Buffer.alloc(64), 1, { era: fromWire(params.era), nonce: params.nonce, tip: params.tip, tipAssetId: params.tipAssetId, metadataHashEnabled: params.metadataHash != null })
      const info = await client.rpc('payment_queryInfo', ['0x' + ext.bytes.toString('hex'), blockHash]) as any
      const weight = info?.weight
      if (weight == null) throw new Error('weight estimate unavailable')
      return typeof weight === 'object' ? { ref_time: BigInt(weight.ref_time ?? weight.refTime), proof_size: BigInt(weight.proof_size ?? weight.proofSize ?? 0) } : { ref_time: BigInt(weight), proof_size: 0n }
    },
  }
}

async function estimateRawFee(native: NativeChainClient, data: Buffer, address: string, key: Buffer, cryptoType: number, options?: NativeExternalSigningOptions, requiresMetadataProof = false): Promise<bigint> {
  const plan = await native.externalSigningPlan(data, address, key, cryptoType, requiresMetadataProof, { ...options, nonce: options?.nonce ?? BigInt(await native.accountNextIndex(address)) })
  return plan.feeRao == null ? native.estimateFeeExternal(plan) : BigInt(plan.feeRao)
}
function nativeOptions(options: WorkflowOptions): NativeExternalSigningOptions {
  const result: NativeExternalSigningOptions = { tip: taoTransactionAmountRao(options.tip ?? 0n, 'tip') }
  if (options.nonce != null) result.nonce = BigInt(options.nonce)
  if (options.period === null) result.immortal = true
  else if (options.period !== undefined) result.period = BigInt(options.period)
  if (options.tipAssetId != null) result.tipAssetId = assetIdValue(options.tipAssetId)
  if (Object.hasOwn(options, 'metadataHash')) {
    result.metadataHashMode = options.metadataHash == null ? 'disabled' : 'explicit'
    if (options.metadataHash != null) result.metadataHash = chainBytes(options.metadataHash)
  }
  return result
}
function submitOptions(options: WorkflowOptions): SubmitOptions {
  return { nonce: options.nonce, period: options.period, tip: options.tip, tipAssetId: options.tipAssetId, ...(Object.hasOwn(options, 'metadataHash') ? { metadataHash: options.metadataHash } : {}), waitForInclusion: options.waitForInclusion ?? true, waitForFinalization: options.waitForFinalization, timeoutMs: options.timeoutMs, signal: options.signal }
}
function accountFree(value: unknown): bigint { const row = value as { data?: { free?: string | number | bigint } } | null; return BigInt(row?.data?.free ?? 0) }
function chainBytes(value: unknown): Buffer {
  if (value instanceof Uint8Array) return Buffer.from(value)
  if (typeof value === 'string') return hexToBytes(value)
  if (Array.isArray(value)) return Buffer.from(value)
  throw new Error('expected chain bytes')
}
