import native, {
  type NativeBlockHeader,
  type NativeChainInfo,
  type NativeClientHandle,
  type NativeCancellationTokenHandle,
  type NativeExecutorHandle,
  type NativeExternalSigner,
  type NativeExternalSigningOptions,
  type NativeExternalSigningPlanHandle,
  type NativeIntentCallHandle,
  type NativeMapPair,
  type NativePlan,
  type NativePolicyHandle,
  type NativePolicyOptions,
  type NativeSignedExtrinsic,
  type NativeSubmitOptions,
  type NativeSubnetHyperparameter,
  type NativeSubnetInfo,
  type NativeSwapQuote,
  type NativeTxOutcome,
  type NativeWalletHandle,
} from './native'
import { nativeAsync, nativeCall } from './errors'
import { fromWire, toWire } from './wire'
import type { ScaleValue, SubnetHyperparameterValueType, SubnetHyperparameters } from './types'
import { Keypair, nativeKeypairHandle } from './keys'
import { Runtime } from './runtime'

export type SignerRoleName = 'coldkey' | 'hotkey'
export type SignerRoleLike = SignerRoleName | number

export type NativeCancellationToken = NativeCancellationTokenHandle

export const SignerRole = Object.freeze({
  Coldkey: native.NativeSignerRole.Coldkey,
  Hotkey: native.NativeSignerRole.Hotkey,
  coldkey: native.NativeSignerRole.Coldkey,
  hotkey: native.NativeSignerRole.Hotkey,
})

export function createNativeCancellationToken(): NativeCancellationToken {
  return nativeCall(() => new native.NativeCancellationToken())
}

export interface PolicyOptions {
  maxFeeRao?: bigint | number | string | null
  maxSpendRao?: bigint | number | string | null
  allowedNetuids?: number[] | null
  allowRawCalls?: boolean | null
  allowGlobal?: boolean | null
}

export interface RawCallOptions {
  op?: string
  signerRole?: SignerRoleLike
}

export class Policy {
  readonly native: NativePolicyHandle
  private readonly options: PolicyOptions

  constructor(options: PolicyOptions = {}) {
    this.options = normalizePolicyOptions(options)
    this.native = nativeCall(() => native.NativePolicy.fromOptions(policyOptionsToNative(this.options)))
  }

  static from(options: Policy | PolicyOptions | null | undefined): Policy {
    return options instanceof Policy ? options : new Policy(options ?? {})
  }

  get allowRawCalls(): boolean {
    return this.native.allowRawCalls
  }

  get allowGlobal(): boolean {
    return this.native.allowGlobal
  }

  withRawCalls(): Policy {
    return this.allowRawCalls ? this : new Policy({ ...this.options, allowRawCalls: true })
  }

  withGlobal(): Policy {
    return this.allowGlobal ? this : new Policy({ ...this.options, allowGlobal: true })
  }

  hasOpaqueByteRestrictions(): boolean {
    return (
      this.options.maxFeeRao != null ||
      this.options.maxSpendRao != null ||
      this.options.allowedNetuids != null
    )
  }

  check(intent: IntentCall, feeRao?: bigint | number | string | null): string[] {
    return nativeCall(() =>
      this.native.check(intent.native, feeRao == null ? undefined : bigintValue(feeRao, 'feeRao')),
    )
  }
}

export class IntentCall {
  readonly native: NativeIntentCallHandle

  private constructor(nativeIntent: NativeIntentCallHandle) {
    this.native = nativeIntent
  }

  static fromNative(nativeIntent: NativeIntentCallHandle): IntentCall {
    return new IntentCall(nativeIntent)
  }

  static rawCall(
    pallet: string,
    fn: string,
    params: ScaleValue = {},
    options: RawCallOptions = {},
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.rawCall(
        options.op ?? `${pallet}.${fn}`,
        signerRoleValue(options.signerRole ?? 'coldkey'),
        pallet,
        fn,
        toWire(params),
      ),
    ))
  }

  static transfer(dest: string, amountRao: bigint | number | string): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.transfer(dest, bigintValue(amountRao, 'amountRao')),
    ))
  }

  static fundEvmKey(mirror: string, amountRao: bigint | number | string): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.fundEvmKey(mirror, bigintValue(amountRao, 'amountRao')),
    ))
  }

  static transferAllowDeath(dest: string, amountRao: bigint | number | string): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.transferAllowDeath(dest, bigintValue(amountRao, 'amountRao')),
    ))
  }

  static transferAll(dest: string, keepAlive: boolean): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.transferAll(dest, keepAlive)))
  }

  static setWeights(
    netuid: number,
    dests: number[],
    weights: number[],
    versionKey: bigint | number | string,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.setWeights(netuid, dests, weights, bigintValue(versionKey, 'versionKey')),
    ))
  }

  static addStake(hotkey: string, netuid: number, amountRao: bigint | number | string): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.addStake(hotkey, netuid, bigintValue(amountRao, 'amountRao')),
    ))
  }

  static addStakeLimit(
    hotkey: string,
    netuid: number,
    amountRao: bigint | number | string,
    limitPriceRao: bigint | number | string,
    allowPartial: boolean,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.addStakeLimit(
        hotkey,
        netuid,
        bigintValue(amountRao, 'amountRao'),
        bigintValue(limitPriceRao, 'limitPriceRao'),
        allowPartial,
      ),
    ))
  }

  static removeStake(
    hotkey: string,
    netuid: number,
    amountAlphaRao: bigint | number | string,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.removeStake(
        hotkey,
        netuid,
        bigintValue(amountAlphaRao, 'amountAlphaRao'),
      ),
    ))
  }

  static removeStakeLimit(
    hotkey: string,
    netuid: number,
    amountAlphaRao: bigint | number | string,
    limitPriceRao: bigint | number | string,
    allowPartial: boolean,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.removeStakeLimit(
        hotkey,
        netuid,
        bigintValue(amountAlphaRao, 'amountAlphaRao'),
        bigintValue(limitPriceRao, 'limitPriceRao'),
        allowPartial,
      ),
    ))
  }

  static burnedRegister(netuid: number, hotkey: string): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.burnedRegister(netuid, hotkey)))
  }

  static rootRegister(hotkey: string): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.rootRegister(hotkey)))
  }

  static registerSubnet(hotkey: string): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.registerSubnet(hotkey)))
  }

  static startCall(netuid: number): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.startCall(netuid)))
  }

  static serveAxon(
    netuid: number,
    ip: bigint | number | string,
    port: number,
    version = 0,
    ipType = 4,
    protocol = 4,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.serveAxon(
        netuid,
        version,
        bigintValue(ip, 'ip'),
        port,
        ipType,
        protocol,
      ),
    ))
  }

  static moveStake(
    originHotkey: string,
    originNetuid: number,
    destinationHotkey: string,
    destinationNetuid: number,
    amountAlphaRao: bigint | number | string,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.moveStake(
        originHotkey,
        originNetuid,
        destinationHotkey,
        destinationNetuid,
        bigintValue(amountAlphaRao, 'amountAlphaRao'),
      ),
    ))
  }

  static swapStake(
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amountAlphaRao: bigint | number | string,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.swapStake(
        hotkey,
        originNetuid,
        destinationNetuid,
        bigintValue(amountAlphaRao, 'amountAlphaRao'),
      ),
    ))
  }

  static transferStake(
    destinationColdkey: string,
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amountAlphaRao: bigint | number | string,
  ): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.transferStake(
        destinationColdkey,
        hotkey,
        originNetuid,
        destinationNetuid,
        bigintValue(amountAlphaRao, 'amountAlphaRao'),
      ),
    ))
  }

  static unstakeAll(hotkey: string): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.unstakeAll(hotkey)))
  }

  static unstakeAllAlpha(hotkey: string): IntentCall {
    return new IntentCall(nativeCall(() => native.NativeIntentCall.unstakeAllAlpha(hotkey)))
  }

  static setHyperparameter(netuid: number, name: string, value: ScaleValue): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.setHyperparameter(netuid, name, toWire(value)),
    ))
  }

  static setRootClaimType(claimType: string, subnets?: number[] | null): IntentCall {
    return new IntentCall(nativeCall(() =>
      native.NativeIntentCall.setRootClaimType(claimType, subnets ?? undefined),
    ))
  }

  get op(): string { return this.native.op }
  get summary(): string { return this.native.summary }
  get signerRole(): string { return this.native.signerRole }
  get pallet(): string { return this.native.pallet }
  get function(): string { return this.native.callFunction }
  get params(): ScaleValue { return fromWire(this.native.params) }

  withSummary(summary: string): IntentCall {
    return new IntentCall(nativeCall(() => this.native.withSummary(summary)))
  }

  forceRaw(): IntentCall {
    return new IntentCall(nativeCall(() => this.native.forceRaw()))
  }

  asCallTuple(): [string, string, ScaleValue] {
    const [pallet, fn, params] = nativeCall(() => this.native.asCallTuple())
    return [String(pallet), String(fn), fromWire(params)]
  }
}

export class NativeChainClient {
  readonly native: NativeClientHandle

  private constructor(nativeClient: NativeClientHandle) {
    this.native = nativeClient
  }

  static async connect(endpoint: string): Promise<NativeChainClient> {
    return new NativeChainClient(await nativeAsync(() => native.NativeClient.connect(endpoint)))
  }

  static async connectEndpoints(endpoints: string[]): Promise<NativeChainClient> {
    return new NativeChainClient(await nativeAsync(() => native.NativeClient.connectEndpoints(endpoints)))
  }

  get endpoint(): string {
    return this.native.endpoint
  }

  get ss58Format(): number {
    return this.native.ss58Format
  }

  get genesisHash(): Buffer {
    return Buffer.from(this.native.genesisHash)
  }

  readCatalog(): string[] {
    return nativeCall(() => this.native.readCatalog())
  }

  refreshRuntime(): Promise<boolean> {
    return nativeAsync(() => this.native.refreshRuntime())
  }

  runtime(): Runtime {
    return Runtime.fromNativeHandle(nativeCall(() => this.native.runtime()))
  }

  rpcValue(method: string, params: unknown[] = []): Promise<unknown> {
    return nativeAsync(() => this.native.rpcValue(method, params))
  }

  chainInfo(): Promise<NativeChainInfo> {
    return nativeAsync(() => this.native.chainInfo())
  }

  blockHash(block?: bigint | number | null): Promise<string> {
    return nativeAsync(() => this.native.blockHash(block == null ? undefined : bigintValue(block, 'block')))
  }

  finalizedHead(): Promise<string> {
    return nativeAsync(() => this.native.finalizedHead())
  }

  async blockNumber(blockHash?: string | null): Promise<number> {
    return Number(await nativeAsync(() => this.native.blockNumber(blockHash ?? undefined)))
  }

  header(blockHash?: string | null): Promise<NativeBlockHeader> {
    return nativeAsync(() => this.native.header(blockHash ?? undefined))
  }

  composeCall(pallet: string, fn: string, params: ScaleValue = {}): Promise<Buffer> {
    return nativeAsync(() => this.native.composeCall(pallet, fn, toWire(params)))
  }

  decodeScale(typeName: string, data: Buffer): ScaleValue {
    return fromWire(nativeCall(() => this.native.decodeScale(typeName, data)))
  }

  constant(pallet: string, name: string): ScaleValue {
    return fromWire(nativeCall(() => this.native.constant(pallet, name)))
  }

  async query(pallet: string, storage: string, params: ScaleValue[] = [], blockHash?: string | null): Promise<ScaleValue> {
    return fromWire(await nativeAsync(() =>
      this.native.query(pallet, storage, toWire(params), blockHash ?? undefined),
    ))
  }

  async queryBatch(
    pallet: string,
    storage: string,
    paramSets: ScaleValue[][] = [],
    blockHash?: string | null,
  ): Promise<ScaleValue[]> {
    return (await nativeAsync(() =>
      this.native.queryBatch(pallet, storage, toWire(paramSets), blockHash ?? undefined),
    )).map(fromWire)
  }

  async queryMap(
    pallet: string,
    storage: string,
    fixedParams: ScaleValue[] = [],
    blockHash?: string | null,
  ): Promise<Array<[ScaleValue, ScaleValue]>> {
    return (await nativeAsync(() =>
      this.native.queryMap(pallet, storage, toWire(fixedParams), blockHash ?? undefined),
    )).map((pair: NativeMapPair) => [fromWire(pair.key), fromWire(pair.value)])
  }

  async runtimeCall(api: string, method: string, params: ScaleValue[] = [], blockHash?: string | null): Promise<ScaleValue> {
    return fromWire(await nativeAsync(() =>
      this.native.runtimeCall(api, method, toWire(params), blockHash ?? undefined),
    ))
  }

  async accountNextIndex(address: string): Promise<number> {
    return Number(await nativeAsync(() => this.native.accountNextIndex(address)))
  }

  signExtrinsic(
    callData: Buffer,
    signer: Keypair,
    nonce: bigint | number,
    period?: bigint | number | null,
  ): Promise<NativeSignedExtrinsic> {
    return nativeAsync(() =>
      this.native.signExtrinsic(
        callData,
        nativeKeypairHandle(signer),
        bigintValue(nonce, 'nonce'),
        period == null ? undefined : bigintValue(period, 'period'),
      ),
    )
  }

  async estimateFee(callData: Buffer, signer: Keypair): Promise<bigint> {
    return BigInt(await nativeAsync(() => this.native.estimateFee(callData, nativeKeypairHandle(signer))))
  }

  submit(
    callData: Buffer,
    signer: Keypair,
    nonce?: bigint | number | null,
    period?: bigint | number | null,
    options?: NativeSubmitOptions | null,
    cancellation?: NativeCancellationToken | null,
  ): Promise<NativeTxOutcome> {
    return nativeAsync(() =>
      this.native.submit(
        callData,
        nativeKeypairHandle(signer),
        nonce == null ? undefined : bigintValue(nonce, 'nonce'),
        period == null ? undefined : bigintValue(period, 'period'),
        options ?? undefined,
        cancellation ?? undefined,
      ),
    )
  }

  submitEncoded(
    extrinsic: Buffer,
    expectedHash: string,
    options?: NativeSubmitOptions | null,
    cancellation?: NativeCancellationToken | null,
  ): Promise<NativeTxOutcome> {
    return nativeAsync(() =>
      this.native.submitEncoded(extrinsic, expectedHash, options ?? undefined, cancellation ?? undefined),
    )
  }

  externalSigningPlan(
    callData: Buffer,
    signerAddress: string,
    publicKey: Buffer,
    cryptoType: number,
    requiresMetadataProof: boolean,
    options?: NativeExternalSigningOptions | null,
  ): Promise<NativeExternalSigningPlanHandle> {
    const signer: NativeExternalSigner = {
      signerAddress,
      publicKey,
      cryptoType,
      requiresMetadataProof,
    }
    return nativeAsync(() =>
      this.native.externalSigningPlan(
        callData,
        signer,
        options ?? undefined,
      ),
    )
  }

  externalSigningPlanForIntent(
    intent: IntentCall,
    signerAddress: string,
    publicKey: Buffer,
    cryptoType: number,
    requiresMetadataProof: boolean,
    policy: Policy,
    options?: NativeExternalSigningOptions | null,
  ): Promise<NativeExternalSigningPlanHandle> {
    const signer: NativeExternalSigner = {
      signerAddress,
      publicKey,
      cryptoType,
      requiresMetadataProof,
    }
    return nativeAsync(() =>
      this.native.externalSigningPlanForIntent(
        intent.native,
        signer,
        policy.native,
        options ?? undefined,
      ),
    )
  }

  async estimateFeeExternal(plan: NativeExternalSigningPlanHandle): Promise<bigint> {
    return BigInt(await nativeAsync(() => this.native.estimateFeeExternal(plan)))
  }

  assembleExternal(
    plan: NativeExternalSigningPlanHandle,
    signature: Buffer,
    cryptoType?: number | null,
  ): Promise<NativeSignedExtrinsic> {
    return nativeAsync(() => this.native.assembleExternal(plan, signature, cryptoType ?? undefined))
  }

  submitExternal(
    plan: NativeExternalSigningPlanHandle,
    signature: Buffer,
    options?: NativeSubmitOptions | null,
    cryptoType?: number | null,
    cancellation?: NativeCancellationToken | null,
  ): Promise<NativeTxOutcome> {
    return nativeAsync(() =>
      this.native.submitExternal(
        plan,
        signature,
        options ?? undefined,
        cryptoType ?? undefined,
        cancellation ?? undefined,
      ),
    )
  }

  async balanceRao(address: string): Promise<bigint> {
    return BigInt(await nativeAsync(() => this.native.balanceRao(address)))
  }

  async existentialDepositRao(): Promise<bigint> {
    return BigInt(await nativeAsync(() => this.native.existentialDepositRao()))
  }

  subnets(blockHash?: string | null): Promise<NativeSubnetInfo[]> {
    return nativeAsync(() => this.native.subnets(blockHash ?? undefined))
  }

  async metagraph(netuid: number, blockHash?: string | null): Promise<ScaleValue> {
    return fromWire(await nativeAsync(() => this.native.metagraph(netuid, blockHash ?? undefined)))
  }

  async neurons(netuid: number, blockHash?: string | null): Promise<ScaleValue[]> {
    return (await nativeAsync(() => this.native.neurons(netuid, blockHash ?? undefined))).map(fromWire)
  }

  async subnetHyperparameters(
    netuid: number,
    blockHash?: string | null,
  ): Promise<SubnetHyperparameters | null> {
    const entries = await nativeAsync(() =>
      this.native.subnetHyperparameters(netuid, blockHash ?? undefined),
    )
    return entries == null ? null : entries.map(nativeSubnetHyperparameter)
  }

  async stakeRao(coldkey: string, hotkey: string, netuid: number, blockHash?: string | null): Promise<bigint> {
    return BigInt(await nativeAsync(() => this.native.stakeRao(coldkey, hotkey, netuid, blockHash ?? undefined)))
  }

  quoteStake(netuid: number, amountRao: bigint | number | string, blockHash?: string | null): Promise<NativeSwapQuote> {
    return nativeAsync(() =>
      this.native.quoteStake(netuid, bigintValue(amountRao, 'amountRao'), blockHash ?? undefined),
    )
  }

  composeIntent(intent: IntentCall): Promise<Buffer> {
    return nativeAsync(() => this.native.composeIntent(intent.native))
  }
}

export class RustWallet {
  readonly native: NativeWalletHandle

  private constructor(nativeWallet: NativeWalletHandle) {
    this.native = nativeWallet
  }

  static fromUris(coldkeyUri: string, hotkeyUri: string): RustWallet {
    return new RustWallet(nativeCall(() => native.NativeWallet.fromUris(coldkeyUri, hotkeyUri)))
  }

  static fromKeypair(signer: Keypair): RustWallet {
    return RustWallet.fromKeypairs(signer, signer)
  }

  static fromKeypairs(coldkey: Keypair, hotkey: Keypair): RustWallet {
    return new RustWallet(nativeCall(() =>
      native.NativeWallet.fromKeypairs(nativeKeypairHandle(coldkey), nativeKeypairHandle(hotkey)),
    ))
  }
}

export class Executor {
  readonly native: NativeExecutorHandle

  constructor(client: NativeChainClient, policy?: Policy | PolicyOptions | null) {
    this.native = nativeCall(() =>
      policy == null
        ? native.NativeExecutor.fromClient(client.native)
        : native.NativeExecutor.withPolicy(client.native, Policy.from(policy).native),
    )
  }

  plan(intent: IntentCall, wallet: RustWallet): Promise<NativePlan> {
    return nativeAsync(() => this.native.plan(intent.native, wallet.native))
  }

  execute(intent: IntentCall, wallet: RustWallet, waitForFinalization = true): Promise<NativeTxOutcome> {
    return nativeAsync(() => this.native.execute(intent.native, wallet.native, waitForFinalization))
  }
}

export function rawCall(
  pallet: string,
  fn: string,
  params: ScaleValue = {},
  options: RawCallOptions = {},
): IntentCall {
  return IntentCall.rawCall(pallet, fn, params, options)
}

export function isIntentCall(value: unknown): value is IntentCall {
  return value instanceof IntentCall
}

export function signerRoleValue(role: SignerRoleLike): number {
  if (typeof role === 'number') return role
  if (role === 'coldkey') return native.NativeSignerRole.Coldkey
  if (role === 'hotkey') return native.NativeSignerRole.Hotkey
  throw new TypeError(`unsupported signer role ${String(role)}`)
}

function policyOptionsToNative(options: PolicyOptions): NativePolicyOptions {
  return {
    maxFeeRao: options.maxFeeRao == null ? undefined : bigintValue(options.maxFeeRao, 'maxFeeRao'),
    maxSpendRao: options.maxSpendRao == null ? undefined : bigintValue(options.maxSpendRao, 'maxSpendRao'),
    allowedNetuids: options.allowedNetuids ?? undefined,
    allowRawCalls: options.allowRawCalls ?? undefined,
    allowGlobal: options.allowGlobal ?? undefined,
  }
}

function normalizePolicyOptions(options: PolicyOptions): PolicyOptions {
  return {
    maxFeeRao: options.maxFeeRao ?? undefined,
    maxSpendRao: options.maxSpendRao ?? undefined,
    allowedNetuids: options.allowedNetuids == null ? undefined : [...options.allowedNetuids],
    allowRawCalls: options.allowRawCalls ?? undefined,
    allowGlobal: options.allowGlobal ?? undefined,
  }
}

const SUBNET_HYPERPARAMETER_VALUE_TYPES = new Set<string>([
  'Bool',
  'U16',
  'U32',
  'U64',
  'U128',
  'TaoBalance',
  'I32F32',
  'U64F64',
])

function nativeSubnetHyperparameter(entry: NativeSubnetHyperparameter) {
  if (!SUBNET_HYPERPARAMETER_VALUE_TYPES.has(entry.valueType)) {
    throw new TypeError(`unknown subnet hyperparameter V3 value type ${entry.valueType}`)
  }
  return {
    name: entry.name,
    valueType: entry.valueType as SubnetHyperparameterValueType,
    value: fromWire(entry.value as ScaleValue),
  }
}

function bigintValue(value: bigint | number | string, name: string): bigint {
  if (typeof value === 'bigint') {
    if (value < 0n) throw new RangeError(`${name} must be non-negative`)
    return value
  }
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new RangeError(`${name} must be a non-negative safe integer`)
    }
    return BigInt(value)
  }
  if (!/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new RangeError(`${name} must be a non-negative integer decimal string`)
  }
  return BigInt(value)
}
