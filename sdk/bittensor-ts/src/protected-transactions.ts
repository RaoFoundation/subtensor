import { isDefiniteSubmissionRejection } from './transport'
import { bytesHex as hex, concatBytes, hexBytes } from './portable'
import { isProtectedCrypto, isRotatingCrypto, u64 } from './wallet-types'
import { signingAuthority, assertAuthorityCurrent, registrationCall, rotationCall, type SigningAuthority, type ProtectedAccountClient, type ProtectedKeypair } from './protected-accounts'
import { Intent, checkIntentPolicy, encodeIntentCall, intentSignerRole, PROXY_TYPES, type IntentContext, type IntentPolicy, type IntentDescriptor } from './intents'
import { resolveTransactionFeePayer } from './fee-payer'
import { authorizeRegistrationSponsorship, type RegistrationSponsorshipOptions } from './registration-sponsorship'
import { prepareOrder, type PreparedOrder, type VersionedOrder } from './limit-orders'

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
const submitted = new WeakMap<object, Map<string, { nonce: bigint; generation: bigint }>>()
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
  private async signingPlan(call: IntentDescriptor | Uint8Array, authority: SigningAuthority, options: ProtectedTransactionOptions, offset = 0, signing = true) {
    options.signal?.throwIfAborted()
    const runtime = await this.adapter.runtimeAt(authority.block), current = await this.adapter.blockNumber(authority.block)
    const period = options.period === undefined ? 64 : options.period
    if (period != null && (!Number.isInteger(period) || period < 4 || period > 65536)) throw new RangeError('invalid mortality period')
    const anchor = Math.max(0, current - 1), genesisHash = hexBytes(await this.adapter.blockHash(0))
    const eraBlockHash = period == null ? genesisHash : hexBytes(await this.adapter.blockHash(this.adapter.eraBirth(period, anchor)))
    const nonce = authority.nonce + BigInt(offset)
    if (nonce >= 0xffffffffn) throw new RangeError('account nonce exhausted')
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
    if (shield && includeCarrier) fee += await this.quote({pallet:'MevShield',call:'submit_encrypted',params:{ciphertext:new Uint8Array(8192)}},authority,{...options,mevShield:false,period:8})
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
    const authority = selected ?? await this.selectAuthority(options), account = hex(this.keypair.publicKey)
    const previous = submitted.get(this.queueOwner)?.get(account)
    const restore = () => {
      const entries = submitted.get(this.queueOwner)
      if (previous) entries?.set(account, previous); else entries?.delete(account)
    }
    const submit = async (bytes: Uint8Array, count: number, submissionOptions: ProtectedTransactionOptions) => {
      markSubmitted(count)
      try {
        const result = await this.adapter.submitBytes(bytes, this.keypair.ss58Address, submissionOptions)
        if (result.data?.submission_rejected === true && !result.blockHash) restore()
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
      return submit(signed.bytes, 1, options)
    }
    if (options.period === null || options.period != null && (options.period < 4 || options.period > 8)) throw new RangeError('shielding requires a 4..8 block era')
    const signing = { ...options, period: options.period ?? 8 }
    const raw = await this.adapter.query('MevShield', 'NextKey', [], authority.block)
    const publicKey = typeof raw === 'string' ? hexBytes(raw) : Uint8Array.from(raw ?? [])
    if (publicKey.length !== 1184) throw new Error('Shield key unavailable')
    const inner = await this.signed(call, authority, signing, 1)
    const ciphertext = this.adapter.seal(publicKey, inner.bytes)
    if (ciphertext.length > 8192) throw new Error('shielded ciphertext exceeds 8192 bytes')
    const outer = await this.signed({ pallet: 'MevShield', call: 'submit_encrypted', params: { ciphertext } }, authority, signing)
    await assertAuthorityCurrent(this.adapter, authority)
    options.signal?.throwIfAborted()
    const carrier = await submit(outer.bytes, 2, { ...options, waitForInclusion: true })
    if (carrier.data?.submission_rejected === true || carrier.data?.submission_uncertain === true) return carrier
    const result = await this.adapter.followShielded(carrier, typeof inner.hash === 'string' ? inner.hash : hex(inner.hash), signing.period, options)
    if (carrier.success === false && carrier.blockHash || result.data?.carrier_only && result.chainError?.name === 'ExtrinsicExpired') markSubmitted(1)
    return result
  }
  async execute(intent: Intent, options: ProtectedWorkflowOptions = {}): Promise<any> {
    options = { ...structuredOptions(options), context: structuredClone(options.context), policy: structuredClone(options.policy) }
    return this.serial(async () => {
      const authority = await this.selectAuthority(options)
      const plan = await this.prepareWorkflow(intent,options,authority)
      if (!plan.ok) throw new Error(plan.violations.join('; '))
      await authorizeRegistrationSponsorship(plan.intent,options)
      return this.submitSelected(plan.call, { ...options, mevShield: plan.shielded },authority)
    })
  }
  async plan(intent: Intent, options: ProtectedWorkflowOptions = {}) {
    options = { ...structuredOptions(options), context: structuredClone(options.context), policy: structuredClone(options.policy) }
    return this.prepareWorkflow(intent,options,await this.selectAuthority(options,false))
  }
  private async prepareWorkflow(intent: Intent, options: ProtectedWorkflowOptions, authority: SigningAuthority) {
      const block = authority.block
      const reader = await this.adapter.intentReader(block)
      const context: IntentContext = options.context ?? { coldkey: this.keypair.ss58Address, hotkey: options.hotkey ?? this.keypair.ss58Address, hotkeyPublicKey: options.hotkeyPublicKey, accountMode: isRotatingCrypto(this.keypair.cryptoType) ? 'legacy' : 'safe' }
      const role = intentSignerRole(intent), signer = context.feePayer ?? (role === 'hotkey' ? context.hotkey ?? context.coldkey : context.coldkey)
      if (hex(this.adapter.decodeAddress(signer)) !== hex(this.keypair.publicKey)) throw new Error('protected session context must use the signing account')
      if (options.proxyType != null && !PROXY_TYPES.includes(options.proxyType)) throw new Error('unknown proxy type')
      if (options.proxyFor) { context.coldkey = options.proxyFor; context.feePayer = this.keypair.ss58Address; if (role === 'hotkey') {context.hotkey = options.proxyFor;context.hotkeyPublicKey = this.adapter.decodeAddress(options.proxyFor)} }
      const wrap = (call: IntentDescriptor): IntentDescriptor => options.proxyFor ? {pallet:'Proxy',call:'proxy',params:{real:options.proxyFor,force_proxy_type:options.proxyType ?? null,call}} : call
      const prepared = await intent.prepare({ ...reader, estimateFee: (call: IntentDescriptor) => this.quote(wrap(call), authority, options), resolveFeePayer: async (call: IntentDescriptor) => (await resolveTransactionFeePayer(reader,wrap(call),this.keypair.ss58Address)).address }, context)
      if (options.expectedCallHash != null && String(prepared.extras.multisig_call_hash).toLowerCase() !== options.expectedCallHash.toLowerCase()) throw new Error('multisig call changed from expectedCallHash')
      const call = wrap(prepared.call)
      const shield = options.mevShield ?? prepared.mevShieldDefault
      if (prepared.mevShieldRequired && !shield) throw new Error('operation requires shielding')
      const fee = await this.quote(call, authority, { ...options, mevShield: shield })
      const innerFee = shield ? await this.quote(call,authority,{...options,mevShield:true},false) : fee
      const carrierFee = fee - innerFee
      const errors = checkIntentPolicy(prepared, options.policy, fee)
      const payer = await resolveTransactionFeePayer(reader, call, this.keypair.ss58Address)
      const amounts = new Map<string,{address:string; amount:bigint}>()
      const add = (address: string, amount: bigint, maximum = false) => {
        const id = hex(this.adapter.decodeAddress(address)), prior = amounts.get(id)?.amount ?? 0n
        amounts.set(id,{address,amount:maximum ? (prior > amount ? prior : amount) : prior + amount})
      }
      for (const debit of prepared.spendByAccount) add(debit.address,debit.amountRao)
      add(payer.address,innerFee)
      add(this.keypair.ss58Address,carrierFee)
      for (const reserve of prepared.requiredFreeByAccount) {
        const id = hex(this.adapter.decodeAddress(reserve.address))
        const spend = prepared.spendByAccount.filter(debit => hex(this.adapter.decodeAddress(debit.address)) === id).reduce((total,debit) => total + debit.amountRao,0n)
        const carrier = id === hex(this.keypair.publicKey) ? carrierFee : 0n
        add(reserve.address,reserve.requiredRao + spend + carrier, true)
      }
      // New receiving deposits are additional to the transaction fee, whereas
      // some workflow headroom constraints already include their fee quote.
      const registrations = new Map<string,{payer:string;depositRao:bigint}>()
      for (const quote of (prepared.extras.registrationSponsorships ?? []) as Array<{payer:string;depositRao:bigint}>) {
        const id = hex(this.adapter.decodeAddress(quote.payer))
        registrations.set(id,{payer:quote.payer,depositRao:quote.depositRao + (registrations.get(id)?.depositRao ?? 0n)})
      }
      for (const quote of registrations.values()) {
        const samePayer = hex(this.adapter.decodeAddress(quote.payer)) === hex(this.adapter.decodeAddress(payer.address))
        const spend = prepared.spendByAccount.filter(debit => hex(this.adapter.decodeAddress(debit.address)) === hex(this.adapter.decodeAddress(quote.payer))).reduce((total,debit) => total + debit.amountRao,0n)
        add(quote.payer,quote.depositRao + spend + (samePayer ? innerFee : 0n) + (hex(this.adapter.decodeAddress(quote.payer)) === hex(this.keypair.publicKey) ? carrierFee : 0n),true)
      }
      for (const {address,amount} of amounts.values()) {
        const account = await reader.query('System','Account',[address]) as any
        if (BigInt(account?.data?.free ?? 0) < amount) errors.push(`account ${address} cannot cover spend, reserves and fees`)
      }
      return {intent:prepared,call,feeRao:fee,carrierFeeRao:carrierFee,feePayer:payer.address,shielded:shield,blockHash:block,violations:errors,ok:errors.length === 0}
  }
}
function structuredOptions<T extends ProtectedTransactionOptions>(options: T): T { return { ...options, metadataHash: options.metadataHash === null ? null : options.metadataHash?.slice() } }
