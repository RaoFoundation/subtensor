import { transactionAffordability } from './transaction-affordability'
import { evaluatePolicy } from './policy'
import { isDefiniteSubmissionRejection, markSubmissionNotSent, withRequestSignal, type RpcRequestOptions } from './transport'
import { bytesHex as hex, concatBytes, hexBytes } from './portable'
import { isProtectedCrypto, isRotatingCrypto, u64 } from './wallet-types'
import { signingAuthority, assertAuthorityCurrent, registrationCall, rotationCall, type SigningAuthority, type ProtectedAccountClient, type ProtectedKeypair } from './protected-accounts'
import { Intent, checkIntentPolicy, encodeIntentCall, intentSignerRole, PROXY_TYPES, type IntentContext, type IntentPolicy, type IntentDescriptor } from './intents'
import { resolveTransactionFeePayer } from './fee-payer'
import { authorizeRegistrationSponsorship, type RegistrationSponsorshipOptions } from './registration-sponsorship'
import { prepareOrder, type PreparedOrder, type VersionedOrder } from './limit-orders'
import { shieldedCiphertextSize, validateShieldedCiphertextSize } from './shield-size'
import type { ShieldedReceipt, ShieldedResult } from './shielded-receipt'

export interface ProtectedTransactionOptions {
  nonce?: number
  period?: number | null
  tip?: bigint
  tipAssetId?: bigint | null
  metadataHash?: Uint8Array | null
  waitForInclusion?: boolean
  waitForFinalization?: boolean
  timeoutMs?: number
  signal?: AbortSignal
  mevShield?: boolean
  allowRawCall?: boolean
}
export interface ProtectedWorkflowAdapter extends ProtectedAccountClient {
  decodeAddress(address: string): Uint8Array
  hash(bytes: Uint8Array): Uint8Array
  rpc(method: string, params: unknown[]): Promise<any>
  eraBirth(period: number, current: number): number
  seal(key: Uint8Array, bytes: Uint8Array): Uint8Array
  verify(proof: Uint8Array, payload: Uint8Array, descriptor: Uint8Array, generation: bigint, commitment: Uint8Array): boolean
  intentReader(block: string): Promise<any>
  submitBytes(bytes: Uint8Array, address: string, options: ProtectedTransactionOptions): Promise<any>
  followShielded(carrier: any, innerHash: string, period: number, options: ProtectedTransactionOptions): Promise<any>
  findExtrinsic?(hash: string, block: string, finalized: boolean, options?: RpcRequestOptions): Promise<ShieldedReceipt | null>
}
export interface ProtectedWorkflowOptions extends ProtectedTransactionOptions, RegistrationSponsorshipOptions {
  context?: IntentContext
  policy?: IntentPolicy
  proxyFor?: string
  proxyType?: string | null
  hotkey?: string
  hotkeyPublicKey?: Uint8Array
  expectedCallHash?: string
}
const queues = new WeakMap<object, Map<string, Promise<unknown>>>()
interface SubmittedAuthority {
  nonce: bigint
  generation: bigint
  shield?: { carrier: ShieldedReceipt; innerHash: string; period: number }
}
const submitted = new WeakMap<object, Map<string, SubmittedAuthority>>()
/** A chain-bound signing session for rotating and fixed post-quantum wallets.
 * Never tracks a local generation counter: finalized chain authority selects it.
 */
export class ProtectedWalletClient {
  constructor(private readonly adapter: ProtectedWorkflowAdapter, readonly keypair: ProtectedKeypair, private readonly queueOwner: object = adapter) {
    if (!isProtectedCrypto(keypair.cryptoType)) throw new TypeError('expected a protected wallet')
  }
  private async serial<T>(action: () => Promise<T>): Promise<T> {
    let entries = queues.get(this.queueOwner)
    if (!entries) { entries = new Map(); queues.set(this.queueOwner, entries) }
    const address = hex(this.keypair.publicKey), prior = entries.get(address) ?? Promise.resolve()
    const next = prior.catch(() => {}).then(action)
    entries.set(address, next)
    try { return await next } finally { if (entries.get(address) === next) entries.delete(address) }
  }
  registrationCall(): IntentDescriptor { return registrationCall(this.keypair) }
  prepareOrder(nonce: bigint | number, order: VersionedOrder): Promise<PreparedOrder> { return prepareOrder(this.adapter,nonce,order) }
  rotationCall(): IntentDescriptor { return rotationCall(this.keypair) }
  authority(nonce?: number): Promise<SigningAuthority> { return signingAuthority(this.adapter, this.keypair, nonce) }
  /** Resume tracking the retained Shield envelope against finalized history.
   * Finalized expiry or carrier failure releases the unused inner reservation;
   * no signing or resubmission occurs. The envelope belongs to this client/account.
   */
  async reconcileShielded(options: Pick<ProtectedTransactionOptions, 'signal' | 'timeoutMs'> = {}): Promise<ShieldedResult> {
    return this.serial(async () => {
      options.signal?.throwIfAborted()
      const account = hex(this.keypair.publicKey), entries = submitted.get(this.queueOwner)
      const reservation = entries?.get(account), pending = reservation?.shield
      if (!reservation || !pending) throw new Error('no pending Shield envelope for this wallet on this client')
      const request = withRequestSignal({ ...options, timeoutMs: options.timeoutMs ?? 120_000 })
      const aborted = new Promise<never>((_resolve, reject) => request.onAbort(reject))
      const guarded = <T>(run: () => Promise<T>): Promise<T> => {
        if (request.signal?.aborted) throw request.error()
        return Promise.race([run(), aborted])
      }
      try {
        let carrier = pending.carrier
        // A carrier observed on the best fork can be finalized with a dispatch
        // failure after a reorg. Verify its actual canonical receipt before
        // releasing the inner authorization that failure could never dispatch.
        if (this.adapter.findExtrinsic && carrier.blockNumber != null) {
          const finalizedHash = await guarded(() => this.adapter.finalizedHead())
          if (await guarded(() => this.adapter.blockNumber(finalizedHash)) >= carrier.blockNumber) {
            const block = await guarded(() => this.adapter.blockHash(carrier.blockNumber))
            const verified = await guarded(() => this.adapter.findExtrinsic!(carrier.extrinsicHash, block, true, { signal: request.signal, timeoutMs: 0 }))
            if (verified) {
              if (verified.extrinsicHash !== carrier.extrinsicHash || verified.blockHash !== block || verified.blockNumber !== carrier.blockNumber || !verified.finalized) throw new Error('Shield reconciliation returned a different carrier')
              carrier = verified
            }
          }
        }
        const result: ShieldedResult = await guarded(() => this.adapter.followShielded(carrier, pending.innerHash, pending.period, { signal: request.signal, timeoutMs: 0, waitForFinalization: true }))
        if (result.innerExtrinsicHash !== pending.innerHash || result.carrierExtrinsicHash !== pending.carrier.extrinsicHash) throw new Error('Shield reconciliation returned a different transaction')
        if (entries?.get(account) !== reservation) throw new Error('wallet reservation changed during Shield reconciliation')
        const failedCarrier = carrier.success === false && carrier.finalized === true && result.extrinsicHash === carrier.extrinsicHash && result.data?.carrier_only === true
        if (failedCarrier || result.data?.inner_expiry_finalized === true && result.data?.carrier_only === true) {
          if (isRotatingCrypto(this.keypair.cryptoType)) entries.set(account, { nonce: reservation.nonce - 1n, generation: reservation.generation - 1n })
          else entries.delete(account)
        } else if (result.finalized && result.blockHash && result.extrinsicHash === pending.innerHash && result.data?.carrier_only === false) {
          // Keep the rotating generation floor until finalized authority observes
          // this inclusion; fixed wallets can have been reaped by the inner call.
          if (isRotatingCrypto(this.keypair.cryptoType)) entries.set(account, { nonce: reservation.nonce, generation: reservation.generation })
          else entries.delete(account)
        }
        return result
      } finally { request.cleanup() }
    })
  }
  private async signingPlan(call: IntentDescriptor | Uint8Array, authority: SigningAuthority, options: ProtectedTransactionOptions, offset = 0, signing = true) {
    options.signal?.throwIfAborted()
    const runtime = await this.adapter.runtimeAt(authority.block), current = await this.adapter.blockNumber(authority.block)
    const period = options.period === undefined ? 64 : options.period
    if (period != null && !Number.isSafeInteger(period)) throw new RangeError('invalid mortality period')
    const anchor = Math.max(0, current - 1), genesisHash = hexBytes(await this.adapter.blockHash(0))
    const eraBlockHash = period == null ? genesisHash : hexBytes(await this.adapter.blockHash(this.adapter.eraBirth(period, anchor)))
    const nonce = BigInt(evaluatePolicy<string>('transaction.signing_bounds', { nonce: authority.nonce, offset, period }))
    const txParams = { era: period == null ? '00' : { period, current: anchor }, nonce, tip: u64(options.tip ?? 0n), tipAssetId: options.tipAssetId, genesisHash, eraBlockHash, metadataHash: options.metadataHash }
    const callData: Uint8Array = call instanceof Uint8Array ? call.slice() : await encodeIntentCall(call, async (p, f, params) => runtime.composeCall(p, f, params))
    const parts = runtime.signaturePayloadParts(txParams)
    const payload = concatBytes(callData, parts.includedInExtrinsic, parts.includedInSignedData)
    const generation = authority.generation + (isRotatingCrypto(this.keypair.cryptoType) ? BigInt(offset) : 0n)
    u64(generation)
    const key = signing ? authority.key.atGeneration(generation) : authority.key
    return { runtime, callData, txParams, payload, key, generation, nonce }
  }
  private async signed(call: IntentDescriptor | Uint8Array, authority: SigningAuthority, options: ProtectedTransactionOptions, offset = 0) {
    const plan = await this.signingPlan(call, authority, options, offset)
    await assertAuthorityCurrent(this.adapter, authority)
    options.signal?.throwIfAborted()
    const signature = plan.key.signAuthorization(plan.payload)
    if (!this.adapter.verify(signature, plan.payload, authority.descriptor, plan.generation, plan.key.authorizationCurrentCommitment())) throw new Error('wallet authorization proof failed verification')
    const signed = plan.runtime.encodeSignedExtrinsic(plan.callData, this.keypair.publicKey, signature, this.keypair.cryptoType, { ...plan.txParams, metadataHashEnabled: plan.txParams.metadataHash != null })
    return { ...signed, hex: hex(signed.bytes), nonce: Number(plan.nonce), signerAddress: this.keypair.ss58Address }
  }
  async signExtrinsic(call: IntentDescriptor | Uint8Array, options: ProtectedTransactionOptions = {}) {
    if (!options.allowRawCall) throw new Error('raw calls require allowRawCall')
    if (options.nonce == null) throw new Error('detached signing requires an explicit nonce')
    call = structuredClone(call); options = structuredOptions(options)
    return this.serial(async () => this.signed(call, await this.authority(options.nonce), options))
  }
  async estimateFee(call: IntentDescriptor | Uint8Array, options: ProtectedTransactionOptions = {}): Promise<bigint> {
    const authority = await signingAuthority(this.adapter, this.keypair, options.nonce, false)
    return this.quote(call, authority, options)
  }
  private async quote(call: IntentDescriptor | Uint8Array, authority: SigningAuthority, options: ProtectedTransactionOptions, includeCarrier = true): Promise<bigint> {
    const shield = options.mevShield === true
    const plan = await this.signingPlan(call, authority, { ...options, period: shield ? 8 : options.period }, shield ? 1 : 0, false)
    // Fee quotations must never reveal a real authorization. Native encoding
    // accepts shape-correct dummy proofs supplied by the runtime bridge.
    const extrinsic = plan.runtime.encodeSignedExtrinsic(plan.callData, this.keypair.publicKey, this.keypair.dummyAuthorization(), this.keypair.cryptoType, { ...plan.txParams, metadataHashEnabled: plan.txParams.metadataHash != null })
    const info = await this.adapter.rpc('payment_queryInfo', [hex(extrinsic.bytes), authority.block])
    const reader = await this.adapter.intentReader(authority.block)
    const payer = call instanceof Uint8Array ? {dropsTip:false} : await resolveTransactionFeePayer(reader, call, this.keypair.ss58Address)
    let fee = BigInt(info.partialFee ?? info.partial_fee) + (payer.dropsTip ? 0n : u64(options.tip ?? 0n))
    if (shield && includeCarrier) fee += await this.quote({pallet:'MevShield',call:'submit_encrypted',params:{ciphertext:new Uint8Array(shieldedCiphertextSize(extrinsic.bytes.length))}},authority,{...options,mevShield:false,period:8})
    return fee
  }
  async submit(call: IntentDescriptor | Uint8Array, options: ProtectedTransactionOptions = {}): Promise<any> {
    if (!options.allowRawCall) throw new Error('raw calls require allowRawCall; use execute for semantic policy')
    call = structuredClone(call); options = structuredOptions(options)
    return this.serial(() => this.submitSelected(call, options))
  }
  private async selectAuthority(options: ProtectedTransactionOptions, signing = true): Promise<SigningAuthority> {
    const account = hex(this.keypair.publicKey), last = submitted.get(this.queueOwner)?.get(account)
    let nonce = options.nonce
    if (!isRotatingCrypto(this.keypair.cryptoType) && last && nonce == null) {
      const pending = BigInt(await this.adapter.accountNextIndex(this.keypair.ss58Address))
      nonce = Number(last.nonce > pending ? last.nonce : pending)
    }
    const authority = await signingAuthority(this.adapter,this.keypair,nonce,signing)
    if (last && (isRotatingCrypto(this.keypair.cryptoType) ? authority.generation <= last.generation : authority.nonce < last.nonce)) throw new Error('previous wallet submission is not reconciled; wait for finalized authority')
    return authority
  }
  private async submitSelected(call: IntentDescriptor | Uint8Array, options: ProtectedTransactionOptions, selected?: SigningAuthority): Promise<any> {
    let handedOff = false
    try {
    const authority = selected ?? await this.selectAuthority(options), account = hex(this.keypair.publicKey)
    const previous = submitted.get(this.queueOwner)?.get(account)
    const restore = () => {
      const entries = submitted.get(this.queueOwner)
      if (previous) entries?.set(account, previous); else entries?.delete(account)
    }
    const submit = async (bytes: Uint8Array, count: number, submissionOptions: ProtectedTransactionOptions) => {
      markSubmitted(count)
      handedOff = true
      try {
        const result = await this.adapter.submitBytes(bytes, this.keypair.ss58Address, submissionOptions)
        if (result.data?.submission_rejected === true && !result.blockHash) restore()
        // Fixed-key accounts can be reaped by a coldkey swap and restart at zero.
        // Rotating keys still require finalized generation reconciliation.
        if (count === 1 && result.blockHash && !isRotatingCrypto(this.keypair.cryptoType)) submitted.get(this.queueOwner)?.delete(account)
        return result
      } catch (error) {
        if (isDefiniteSubmissionRejection(error)) restore()
        throw error
      }
    }
    const markSubmitted = (count: number) => {
      let entries = submitted.get(this.queueOwner)
      if (!entries) {entries = new Map(); submitted.set(this.queueOwner,entries)}
      entries.set(account,{nonce:authority.nonce + BigInt(count),generation:authority.generation + BigInt(count - 1)})
    }
    if (!options.mevShield) {
      const signed = await this.signed(call, authority, options)
      await assertAuthorityCurrent(this.adapter, authority)
      options.signal?.throwIfAborted()
      return await submit(signed.bytes, 1, options)
    }
    evaluatePolicy('transaction.signing_bounds', { nonce: authority.nonce, offset: 1, period: options.period === undefined ? 8 : options.period, shielded: true })
    const signing = { ...options, period: options.period ?? 8 }
    // Encryption keys rotate at the best head, independently of finalized
    // signing authority. A finalized snapshot can already contain a retired key.
    const raw = await this.adapter.query('MevShield', 'NextKey', [], await this.adapter.blockHash())
    const publicKey = typeof raw === 'string' ? hexBytes(raw) : Uint8Array.from(raw ?? [])
    if (publicKey.length !== 1184) throw new Error('Shield key unavailable')
    const inner = await this.signed(call, authority, signing, 1)
    const ciphertext = this.adapter.seal(publicKey, inner.bytes)
    validateShieldedCiphertextSize(ciphertext.length)
    const outer = await this.signed({ pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext } }, authority, signing)
    await assertAuthorityCurrent(this.adapter, authority)
    options.signal?.throwIfAborted()
    const carrier = await submit(outer.bytes, 2, { ...options, waitForInclusion: true })
    if (carrier.data?.submission_rejected === true || carrier.data?.submission_uncertain === true) return carrier
    const innerHash = typeof inner.hash === 'string' ? inner.hash : hex(inner.hash)
    const reservation = submitted.get(this.queueOwner)?.get(account)
    if (reservation) reservation.shield = { carrier: { ...carrier, events: [...(carrier.events ?? [])], data: carrier.data && { ...carrier.data } }, innerHash, period: signing.period }
    const result = await this.adapter.followShielded(carrier, innerHash, signing.period, options)
    if (carrier.success === false && carrier.finalized || result.data?.inner_expiry_finalized === true) markSubmitted(1)
    if (!isRotatingCrypto(this.keypair.cryptoType) && result.blockHash && result.extrinsicHash === innerHash) submitted.get(this.queueOwner)?.delete(account)
    return result
    } catch (error) { throw handedOff ? error : markSubmissionNotSent(error) }
  }
  async execute(intent: Intent, options: ProtectedWorkflowOptions = {}): Promise<any> {
    let handedOff = false
    try {
    options = { ...structuredOptions(options), context: structuredClone(options.context), policy: structuredClone(options.policy) }
    return await this.serial(async () => {
      const authority = await this.selectAuthority(options)
      const plan = await this.prepareWorkflow(intent,options,authority)
      if (!plan.ok) throw new Error(plan.violations.join('; '))
      await authorizeRegistrationSponsorship(plan.intent,options)
      handedOff = true
      return this.submitSelected(plan.call, { ...options, mevShield: plan.shielded },authority)
    })
    } catch (error) { throw handedOff ? error : markSubmissionNotSent(error) }
  }
  async plan(intent: Intent, options: ProtectedWorkflowOptions = {}) {
    options = { ...structuredOptions(options), context: structuredClone(options.context), policy: structuredClone(options.policy) }
    return this.prepareWorkflow(intent,options,await this.selectAuthority(options,false))
  }
  private async prepareWorkflow(intent: Intent, options: ProtectedWorkflowOptions, authority: SigningAuthority) {
      const block = authority.block
      const reader = await this.adapter.intentReader(block)
      const context: IntentContext = {
        ...options.context,
        coldkey: options.context?.coldkey ?? this.keypair.ss58Address,
        hotkey: options.context?.hotkey ?? options.hotkey ?? this.keypair.ss58Address,
        hotkeyPublicKey: options.context?.hotkeyPublicKey ?? options.hotkeyPublicKey,
        accountMode: options.context?.accountMode ?? (isRotatingCrypto(this.keypair.cryptoType) ? 'legacy' : 'safe'),
      }
      const role = intentSignerRole(intent), signer = context.feePayer ?? (role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey)
      if (hex(this.adapter.decodeAddress(signer)) !== hex(this.keypair.publicKey)) throw new Error('protected session context must use the signing account')
      if (options.proxyType != null && !PROXY_TYPES.includes(options.proxyType)) throw new Error('unknown proxy type')
      if (options.proxyFor) { context.coldkey = options.proxyFor; context.feePayer = this.keypair.ss58Address; if (role === 'hotkey') {context.hotkey = options.proxyFor;context.hotkeyPublicKey = this.adapter.decodeAddress(options.proxyFor)} }
      if (context.hotkey) {
        // Timelock commitments bind the dispatch AccountId, not the wallet's
        // generation-specific or post-quantum verification key.
        const accountId = this.adapter.decodeAddress(context.hotkey)
        if (context.hotkeyPublicKey && hex(context.hotkeyPublicKey) !== hex(accountId)) throw new Error('hotkey public key differs from its account ID')
        context.hotkeyPublicKey = accountId
      }
      const wrap = (call: IntentDescriptor): IntentDescriptor => options.proxyFor ? {pallet:'Proxy',call:'proxy',params:{real:options.proxyFor,force_proxy_type:options.proxyType ?? null,call}} : call
      const prepared = await intent.prepare({ ...reader, estimateFee: (call: IntentDescriptor) => this.quote(wrap(call), authority, options), resolveFeePayer: async (call: IntentDescriptor) => (await resolveTransactionFeePayer(reader,wrap(call),this.keypair.ss58Address)).address }, context)
      const execution = evaluatePolicy<{ shielded: boolean; violations: string[] }>('transaction.execution', { shielded: options.mevShield, shield_default: prepared.mevShieldDefault, shield_required: prepared.mevShieldRequired, expected_hash: options.expectedCallHash, actual_hash: prepared.extras.multisig_call_hash })
      if (execution.violations.includes('call_hash')) throw new Error('multisig call changed from expectedCallHash')
      const call = wrap(prepared.call)
      const shield = execution.shielded
      if (execution.violations.includes('shield_required')) throw new Error('operation requires shielding')
      const fee = await this.quote(call, authority, { ...options, mevShield: shield })
      const innerFee = shield ? await this.quote(call,authority,{...options,mevShield:true},false) : fee
      const carrierFee = fee - innerFee
      const errors = checkIntentPolicy(prepared, options.policy, fee)
      const payer = await resolveTransactionFeePayer(reader, call, this.keypair.ss58Address)
      const affordability = await transactionAffordability({
        spends: prepared.spendByAccount.map(row => ({ address: row.address, amount: row.amountRao })),
        charges: [{ address: payer.address, amount: innerFee }, { address: this.keypair.ss58Address, amount: carrierFee }, ...((prepared.extras.registrationSponsorships ?? []) as Array<{payer:string;depositRao:bigint}>).map(row => ({ address: row.payer, amount: row.depositRao }))],
        reserves: prepared.requiredFreeByAccount.map(row => ({ address: row.address, required: row.requiredRao, available: row.availableRao })),
        reserveCharges: [{ address: this.keypair.ss58Address, amount: carrierFee }],
      }, this.adapter.decodeAddress.bind(this.adapter), async address => BigInt((await reader.query('System','Account',[address]) as any)?.data?.free ?? 0))
      for (const row of [...affordability.required, ...affordability.reserves]) if (!row.affordable) errors.push(`account ${row.address} cannot cover spend, reserves and fees`)
      return {intent:prepared,call,feeRao:fee,carrierFeeRao:carrierFee,feePayer:payer.address,shielded:shield,blockHash:block,violations:errors,ok:errors.length === 0}
  }
}
function structuredOptions<T extends ProtectedTransactionOptions>(options: T): T { return { ...options, metadataHash: options.metadataHash === null ? null : options.metadataHash?.slice() } }
