/* eslint-disable @typescript-eslint/no-explicit-any */

export interface NativeKeypairHandle {
  readonly cryptoType: number
  readonly kind: 'Ed25519' | 'Sr25519' | 'PublicOnly'
  readonly publicKey: Buffer
  readonly ss58Address: string
  readonly ss58Format: number
  derive(path: string): NativeKeypairHandle
  sign(message: Buffer): Buffer
  verify(message: Buffer, signature: Buffer): boolean
  encrypt(message: Buffer): Buffer
  decrypt(ciphertext: Buffer): Buffer
}

export interface NativeStorageEntry {
  pallet: string
  name: string
  prefix: string
  modifier: string
  valueType: string
  valueTypeId: number
  paramTypes: string[]
  paramTypeIds: number[]
  paramHashers: string[]
  defaultBytes: Buffer
}

export interface NativeStorageChange {
  key: string
  value?: string | null
}

export interface NativeMapPair {
  key: unknown
  value: unknown
}

export interface NativeBlockHeader {
  hash: string
  parentHash: string
  number: bigint
}

export interface NativeSubnetInfo {
  netuid: number
  tempo: number
  burnRao: string
  neuronCount: number
}

export interface NativeSwapQuote {
  taoAmount: string
  alphaAmount: string
  taoFee: string
  alphaFee: string
  taoSlippage: string
  alphaSlippage: string
}

export interface NativeSignedExtrinsic {
  bytes: Buffer
  hash: string
}

export interface NativeTxParams {
  era: unknown
  nonce: bigint
  tip: bigint
  tipAssetId?: bigint | null
  genesisHash: Buffer
  eraBlockHash: Buffer
  metadataHash?: Buffer | null
}

export interface NativeSignerPayload {
  address: string
  blockHash: string
  blockNumber: string
  era: string
  genesisHash: string
  method: string
  nonce: string
  signedExtensions: string[]
  specVersion: string
  tip: string
  transactionVersion: string
  version: number
  assetId?: string | null
  metadataHash?: string | null
  mode?: number | null
}

export interface NativeExtrinsicParams {
  era: unknown
  nonce: bigint
  tip: bigint
  tipAssetId?: bigint | null
  metadataHashEnabled: boolean
}

export interface NativePartialDecode {
  value: unknown
  offset: number
  remaining: number
}

export interface NativeCursorHandle {
  readonly data: Buffer
  readonly offset: number
  readonly remaining: number
  readonly strict: boolean
  setStrict(strict: boolean): void
  seek(offset: number): void
  reset(data: Buffer, strict: boolean, offset: number): void
  take(length: number): Buffer
  byte(): number
  decodeCompactU128(): bigint
  decodeCompactLength(): bigint
}

export interface NativeCursorConstructor {
  fromBytes(data: Buffer, strict: boolean, offset: number): NativeCursorHandle
}

export interface NativeRuntimeHandle {
  readonly specVersion: number
  readonly transactionVersion: number
  readonly ss58Format: number
  readonly isV15: boolean
  readonly extrinsicVersion: number
  readonly outerEventType?: number | null
  readonly metadataBytes: Buffer
  decode(typeString: string, data: Buffer, strict: boolean): unknown
  decodePartial(
    typeString: string,
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  decodeTypeId(typeId: number, data: Buffer, strict: boolean): unknown
  decodeTypeIdPartial(
    typeId: number,
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  decodeBatch(typeStrings: string[], data: Buffer[]): unknown[]
  encode(typeString: string, value: unknown): Buffer
  encodeTypeId(typeId: number, value: unknown): Buffer
  typeIdOf(name: string): number | null | undefined
  typeNameOf(id: number): string | null | undefined
  typeSpec(typeString: string): unknown
  decodeSpec(spec: unknown, data: Buffer, strict: boolean): unknown
  decodeSpecDescriptor(spec: unknown, data: Buffer, strict: boolean): unknown
  decodeValue(
    spec: unknown,
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  decodeValueDescriptor(
    spec: unknown,
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  decodeTypeIdDescriptor(typeId: number, data: Buffer, strict: boolean): unknown
  decodeTypeIdDescriptorPartial(
    typeId: number,
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  encodeSpec(spec: unknown, value: unknown): Buffer
  encodeSpecDescriptor(spec: unknown, value: unknown): Buffer
  encodeValue(spec: unknown, value: unknown, prefix?: Buffer | null): Buffer
  encodeValueDescriptor(spec: unknown, value: unknown, prefix?: Buffer | null): Buffer
  encodeId(typeId: number, value: unknown, prefix?: Buffer | null): Buffer
  encodeIdDescriptor(typeId: number, value: unknown, prefix?: Buffer | null): Buffer
  coerceAccountId(value: unknown): Buffer
  coerceAccountIdDescriptor(value: unknown): Buffer
  resolveType(id: number): unknown
  registryJson(): string
  registry(): unknown
  pallet(name: string): unknown | null | undefined
  palletAt(index: number): unknown | null | undefined
  pallets(): unknown[]
  extrinsicInfo(): unknown
  runtimeApis(): unknown
  encodeRuntimeApiInput(api: string, method: string, params: unknown): Buffer
  runtimeApiInfos(): unknown
  runtimeSnapshot(): unknown
  composeCall(pallet: string, fn: string, params: unknown): Buffer
  decodeCall(data: Buffer): unknown
  decodeCallValue(
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  decodeCallValueDescriptor(
    data: Buffer,
    offset: number,
    strict: boolean,
  ): NativePartialDecode
  storageEntry(pallet: string, storageFunction: string): NativeStorageEntry
  storagePrefix(pallet: string, storageFunction: string): Buffer
  storageKey(pallet: string, storageFunction: string, params: unknown): Buffer
  storageKeyBatch(
    pallet: string,
    storageFunction: string,
    paramsList: unknown,
  ): Buffer[]
  decodeStorageKeyParams(
    pallet: string,
    storageFunction: string,
    key: Buffer,
    fixed: number,
  ): unknown
  decodeMapPairs(
    pallet: string,
    storageFunction: string,
    rawKeys: Buffer[],
    rawValues: Buffer[],
    fixed: number,
  ): NativeMapPair[]
  decodeMapChanges(
    pallet: string,
    storageFunction: string,
    changes: NativeStorageChange[],
    fixed: number,
  ): NativeMapPair[]
  constant(pallet: string, name: string): { found: boolean; value: unknown }
  constantInfo(pallet: string, name: string): unknown | null | undefined
  moduleError(moduleIndex: number, errorIndex: number): { name: string; docs: string[] }
  signedExtensionIdentifiers(): string[]
  encodeEra(era: unknown): Buffer
  signaturePayloadParts(params: NativeTxParams): {
    includedInExtrinsic: Buffer
    includedInSignedData: Buffer
  }
  signaturePayload(callData: Buffer, params: NativeTxParams): Buffer
  signerPayload(
    address: string,
    callData: Buffer,
    params: NativeTxParams,
  ): NativeSignerPayload
  encodeSignedExtrinsic(
    callData: Buffer,
    publicKey: Buffer,
    signature: Buffer,
    signatureVersion: number,
    params: NativeExtrinsicParams,
  ): { bytes: Buffer; hash: Buffer }
  decodeExtrinsic(data: Buffer, strict: boolean): unknown
  runtimeApiMap(): unknown
  metadataIr(): unknown
}

export interface NativeRuntimeConstructor {
  fromMetadata(
    metadataBytes: Buffer,
    specVersion: number,
    transactionVersion: number,
    ss58Format: number,
  ): NativeRuntimeHandle
}

export interface NativeEpochScheduleState {
  lastEpochBlock: bigint
  pendingEpochAt: bigint
  subnetEpochIndex: bigint
  tempo: number
  blocksSinceLastStep: bigint
  currentBlock: bigint
}

export interface NativeLedgerHandle {
  appVersion(): Promise<{ major: number; minor: number; patch: number }>
  address(
    account: number,
    index: number,
    ss58Prefix: number,
    confirm: boolean,
  ): Promise<{ publicKey: Buffer; ss58Address: string }>
  sign(account: number, index: number, payload: Buffer, proof: Buffer): Promise<Buffer>
}

export interface NativeLedgerConstructor {
  open(): Promise<NativeLedgerHandle>
}

export interface NativePolicyOptions {
  maxFeeRao?: bigint | null
  maxSpendRao?: bigint | null
  allowedNetuids?: number[] | null
  allowRawCalls?: boolean | null
}

export interface NativePlan {
  op: string
  summary: string
  signerRole: string
  signerAddress: string
  feeRao?: string | null
  warnings: string[]
  violations: string[]
  ok: boolean
  callData: Buffer
}

export interface NativeDispatchError {
  pallet?: string | null
  name: string
  docs: string[]
  semanticCode: string
}

export interface NativeTxOutcome {
  success: boolean
  extrinsicHash: string
  blockHash?: string | null
  blockNumber?: bigint | null
  extrinsicIndex?: number | null
  feeRao?: string | null
  events: unknown[]
  error?: NativeDispatchError | null
  message: string
  data: unknown
}

export interface NativePolicyHandle {
  readonly allowRawCalls: boolean
  check(intent: NativeIntentCallHandle, feeRao?: bigint | null): string[]
}

export interface NativePolicyConstructor {
  fromOptions(options?: NativePolicyOptions | null): NativePolicyHandle
}

export interface NativeIntentCallHandle {
  readonly op: string
  readonly summary: string
  readonly signerRole: string
  readonly pallet: string
  readonly callFunction: string
  readonly params: unknown
  withSummary(summary: string): NativeIntentCallHandle
  forceRaw(): NativeIntentCallHandle
  asCallTuple(): unknown[]
}

export interface NativeIntentCallConstructor {
  rawCall(
    op: string,
    signerRole: number,
    pallet: string,
    callFunction: string,
    params: unknown,
  ): NativeIntentCallHandle
  transfer(dest: string, amountRao: bigint): NativeIntentCallHandle
  fundEvmKey(mirror: string, amountRao: bigint): NativeIntentCallHandle
  transferAllowDeath(dest: string, amountRao: bigint): NativeIntentCallHandle
  transferAll(dest: string, keepAlive: boolean): NativeIntentCallHandle
  addStake(hotkey: string, netuid: number, amountRao: bigint): NativeIntentCallHandle
  addStakeLimit(
    hotkey: string,
    netuid: number,
    amountRao: bigint,
    limitPriceRao: bigint,
    allowPartial: boolean,
  ): NativeIntentCallHandle
  removeStake(hotkey: string, netuid: number, amountAlphaRao: bigint): NativeIntentCallHandle
  removeStakeLimit(
    hotkey: string,
    netuid: number,
    amountAlphaRao: bigint,
    limitPriceRao: bigint,
    allowPartial: boolean,
  ): NativeIntentCallHandle
  registerSubnet(hotkey: string): NativeIntentCallHandle
  startCall(netuid: number): NativeIntentCallHandle
  setWeights(
    netuid: number,
    dests: number[],
    weights: number[],
    versionKey: bigint,
  ): NativeIntentCallHandle
  serveAxon(
    netuid: number,
    version: number,
    ip: bigint,
    port: number,
    ipType: number,
    protocol: number,
  ): NativeIntentCallHandle
  burnedRegister(netuid: number, hotkey: string): NativeIntentCallHandle
  rootRegister(hotkey: string): NativeIntentCallHandle
  moveStake(
    originHotkey: string,
    originNetuid: number,
    destinationHotkey: string,
    destinationNetuid: number,
    amountAlphaRao: bigint,
  ): NativeIntentCallHandle
  swapStake(
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amountAlphaRao: bigint,
  ): NativeIntentCallHandle
  transferStake(
    destinationColdkey: string,
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amountAlphaRao: bigint,
  ): NativeIntentCallHandle
  unstakeAll(hotkey: string): NativeIntentCallHandle
  unstakeAllAlpha(hotkey: string): NativeIntentCallHandle
  setHyperparameter(netuid: number, name: string, value: unknown): NativeIntentCallHandle
  setRootClaimType(claimType: string, subnets?: number[] | null): NativeIntentCallHandle
}

export interface NativeClientHandle {
  readonly endpoint: string
  readonly ss58Format: number
  readonly genesisHash: Buffer
  blockHash(block?: bigint | null): string
  finalizedHead(): string
  blockNumber(blockHash?: string | null): bigint
  header(blockHash?: string | null): NativeBlockHeader
  readCatalog(): string[]
  refreshRuntime(): boolean
  composeCall(pallet: string, callFunction: string, params: unknown): Buffer
  decodeScale(typeName: string, data: Buffer): unknown
  constant(pallet: string, name: string): unknown
  query(pallet: string, storage: string, params: unknown, blockHash?: string | null): unknown
  queryBatch(pallet: string, storage: string, paramSets: unknown, blockHash?: string | null): unknown[]
  queryMap(pallet: string, storage: string, fixedParams: unknown, blockHash?: string | null): NativeMapPair[]
  runtimeCall(api: string, method: string, params: unknown, blockHash?: string | null): unknown
  accountNextIndex(address: string): bigint
  signExtrinsic(
    callData: Buffer,
    signer: NativeKeypairHandle,
    nonce: bigint,
    period?: bigint | null,
  ): NativeSignedExtrinsic
  estimateFee(callData: Buffer, signer: NativeKeypairHandle): string
  submit(
    callData: Buffer,
    signer: NativeKeypairHandle,
    nonce?: bigint | null,
    period?: bigint | null,
    waitForFinalization?: boolean | null,
  ): NativeTxOutcome
  submitEncoded(
    extrinsic: Buffer,
    expectedHash: string,
    waitForFinalization?: boolean | null,
  ): NativeTxOutcome
  balanceRao(address: string): string
  existentialDepositRao(): string
  subnets(blockHash?: string | null): NativeSubnetInfo[]
  metagraph(netuid: number, blockHash?: string | null): unknown
  neurons(netuid: number, blockHash?: string | null): unknown[]
  subnetHyperparameters(netuid: number, blockHash?: string | null): unknown
  stakeRao(coldkey: string, hotkey: string, netuid: number, blockHash?: string | null): string
  quoteStake(netuid: number, amountRao: bigint, blockHash?: string | null): NativeSwapQuote
  composeIntent(intent: NativeIntentCallHandle): Buffer
}

export interface NativeClientConstructor {
  connect(endpoint: string): NativeClientHandle
}

export interface NativeWalletHandle {}

export interface NativeWalletConstructor {
  fromKeypairs(coldkey: NativeKeypairHandle, hotkey: NativeKeypairHandle): NativeWalletHandle
  fromUris(coldkeyUri: string, hotkeyUri: string): NativeWalletHandle
}

export interface NativeExecutorHandle {
  plan(intent: NativeIntentCallHandle, wallet: NativeWalletHandle): NativePlan
  planWithPolicy(
    intent: NativeIntentCallHandle,
    wallet: NativeWalletHandle,
    policy: NativePolicyHandle,
  ): NativePlan
  execute(
    intent: NativeIntentCallHandle,
    wallet: NativeWalletHandle,
    waitForFinalization?: boolean | null,
  ): NativeTxOutcome
  submitShielded(intent: NativeIntentCallHandle, wallet: NativeWalletHandle): NativeTxOutcome
}

export interface NativeExecutorConstructor {
  fromClient(client: NativeClientHandle): NativeExecutorHandle
  withPolicy(client: NativeClientHandle, policy: NativePolicyHandle): NativeExecutorHandle
}

export interface NativeBinding {
  NativeKeypair: { readonly prototype: NativeKeypairHandle }
  NativeRuntime: NativeRuntimeConstructor
  NativeCursor: NativeCursorConstructor
  NativeLedgerDevice: NativeLedgerConstructor
  NativeSignerRole: { readonly Coldkey: number; readonly Hotkey: number }
  NativeSpendKind: { readonly None: number; readonly Bounded: number; readonly Unbounded: number }
  NativePolicy: NativePolicyConstructor
  NativeIntentCall: NativeIntentCallConstructor
  NativeClient: NativeClientConstructor
  NativeWallet: NativeWalletConstructor
  NativeExecutor: NativeExecutorConstructor

  bindingVersion(): string
  ledgerEnabled(): boolean
  wireTag(): string
  wireRoundtrip(value: unknown): unknown
  valueToCorpusJson(value: unknown): unknown
  u256LeToDecimal(raw: Buffer): string
  coreValueDescriptorRoundtrip(value: unknown): unknown
  coreValueDescriptorToWire(value: unknown): unknown
  wireToCoreValueDescriptor(value: unknown): unknown
  coreValueDescriptorToCorpusJson(value: unknown): unknown
  coreValueDescriptorDisplay(value: unknown): string
  coreValueString(value: string): unknown
  coreValueHex(value: Buffer): unknown
  coreValueRecord(fields: Array<{ name: string; value: unknown }>): unknown
  coreValueNull(): unknown
  coreValueBool(value: boolean): unknown
  coreValueInt(value: bigint): unknown
  coreValueUint(value: bigint): unknown
  coreValueU256Le(raw: Buffer): unknown
  coreValueBytes(value: Buffer): unknown
  coreValueList(items: unknown[]): unknown
  coreValueTuple(items: unknown[]): unknown
  coreValueDict(entries: Array<{ key: unknown; value: unknown }>): unknown

  keypairNew(
    ss58Address: string | null | undefined,
    publicKey: Buffer | null | undefined,
    cryptoType: number,
    ss58Format: number,
  ): NativeKeypairHandle
  keypairFromMnemonic(
    mnemonic: string,
    cryptoType: number,
    password?: string | null,
  ): NativeKeypairHandle
  keypairFromSeed(seed: Buffer, cryptoType: number): NativeKeypairHandle
  keypairFromUri(uri: string, cryptoType: number): NativeKeypairHandle
  keypairFromPrivateKey(privateKey: string, cryptoType: number): NativeKeypairHandle
  keypairFromEncryptedJson(jsonData: string, passphrase: string): Promise<NativeKeypairHandle>
  generateMnemonic(nWords: number): string
  encryptFor(ss58Address: string, message: Buffer, cryptoType: number): Buffer
  verifySignature(
    message: Buffer,
    signature: Buffer,
    ss58Address: string,
    cryptoType: number,
  ): boolean
  publicKeyFromSs58(ss58Address: string): Buffer
  ss58FromPublic(publicKey: Buffer, ss58Format: number): string
  serializeKeypair(keypair: NativeKeypairHandle): Buffer
  dangerouslySerializeKeypair(keypair: NativeKeypairHandle): Buffer
  keypairToKeyfileData(
    keypair: NativeKeypairHandle,
    password?: string | null,
  ): Promise<Buffer>
  deserializeKeypair(keyfileData: Buffer): NativeKeypairHandle
  deserializeKeypairFromKeyfile(
    keyfileData: Buffer,
    password?: string | null,
  ): Promise<NativeKeypairHandle>
  readKeypairKeyfile(path: string, password?: string | null): Promise<NativeKeypairHandle>
  writeKeypairKeyfile(
    keypair: NativeKeypairHandle,
    path: string,
    password: string | null | undefined,
    overwrite: boolean,
    allowPlaintext: boolean,
  ): Promise<void>
  writeKeypairPairKeyfile(
    privateKeypair: NativeKeypairHandle,
    privatePath: string,
    privatePassword: string | null | undefined,
    publicKeypair: NativeKeypairHandle,
    publicPath: string,
    overwrite: boolean,
    allowPlaintext: boolean,
  ): Promise<void>
  encryptKeyfileData(keyfileData: Buffer, password: string): Promise<Buffer>
  decryptKeyfileData(keyfileData: Buffer, password?: string | null): Promise<Buffer>
  keyfileDataIsEncrypted(keyfileData: Buffer): boolean
  keyfileDataIsEncryptedNacl(keyfileData: Buffer): boolean
  keyfileDataIsEncryptedAnsible(keyfileData: Buffer): boolean
  keyfileDataIsEncryptedLegacy(keyfileData: Buffer): boolean
  keyfileDataEncryptionMethod(keyfileData: Buffer): string
  getPasswordFromEnvironment(name: string): string | null | undefined
  savePasswordToEnvironment(name: string, password: string): string
  cryptoEd25519(): number
  cryptoSr25519(): number
  defaultSs58Format(): number

  convertTypeString(name: string): string
  primitiveFromName(name: string): string | null | undefined
  normalizeTypeSpec(spec: unknown): unknown
  eraBirth(period: bigint, current: bigint): bigint
  multisigAccountId(
    signatories: Buffer[],
    threshold: number,
  ): { accountId: Buffer; sortedSignatories: Buffer[] }
  multisigSs58(accountId: Buffer, ss58Format: number): string
  encodeCompact(value: bigint): Buffer
  decodeCompactU128(
    data: Buffer,
    strict: boolean,
  ): { value: bigint; offset: number; remaining: number }
  decodeCompactLength(
    data: Buffer,
    strict: boolean,
  ): { value: bigint; offset: number; remaining: number }
  decodeOptionalOpaqueMetadata(data: Buffer): Buffer | null | undefined
  hashStorageParam(hasher: string, data: Buffer): Buffer
  storagePrefixFor(prefix: string, name: string): Buffer
  concatHashLength(hasher: string): number
  parallelDecodeThreshold(): number

  metadataDigest(metadataBytes: Buffer, info: Record<string, unknown>): Buffer
  generateExtrinsicProof(
    callData: Buffer,
    includedInExtrinsic: Buffer,
    includedInSignedData: Buffer,
    metadataBytes: Buffer,
    info: Record<string, unknown>,
  ): Buffer

  mlkemSeal(publicKey: Buffer, plaintext: Buffer, includeKeyHash: boolean): Buffer
  mlkemTwox128(data: Buffer): Buffer
  mlkemNonceLength(): number
  mlkemKdfId(): Buffer

  timelockEncryptAndCompress(data: Buffer, revealRound: bigint): Promise<Buffer>
  timelockDecryptAndDecompress(
    encryptedData: Buffer,
    signatureBytes: Buffer,
  ): Promise<Buffer>
  timelockGenerateCommitV2(
    uids: number[],
    values: number[],
    versionKey: bigint,
    state: NativeEpochScheduleState,
    subnetRevealPeriodEpochs: bigint,
    blockTime: number,
    hotkey: Buffer,
  ): Promise<{ ciphertext: Buffer; revealRound: bigint }>
  timelockEncryptCommitment(
    data: string,
    blocksUntilReveal: bigint,
    blockTime: number,
  ): Promise<{ ciphertext: Buffer; revealRound: bigint }>
  timelockEncryptNBlocks(
    data: Buffer,
    nBlocks: bigint,
    blockTime: number,
  ): Promise<{ ciphertext: Buffer; revealRound: bigint }>
  timelockEncryptAtRound(
    data: Buffer,
    revealRound: bigint,
  ): Promise<{ ciphertext: Buffer; revealRound: bigint }>
  timelockGetRoundInfo(round?: bigint | null): Promise<{ round: bigint; signature: string }>
  timelockGetRevealRoundSignature(
    revealRound: bigint | null | undefined,
    noErrors: boolean,
  ): Promise<string | null | undefined>
  timelockDecrypt(
    encryptedData: Buffer,
    noErrors: boolean,
  ): Promise<Buffer | null | undefined>
  timelockDecryptWithSignature(encryptedData: Buffer, signatureHex: string): Promise<Buffer>
  epochShouldRun(state: NativeEpochScheduleState, block: bigint): boolean
  epochCurrentPreRunCoinbase(state: NativeEpochScheduleState, block: bigint): bigint
  epochSimulateRunCoinbase(
    state: NativeEpochScheduleState,
    block: bigint,
  ): NativeEpochScheduleState
  epochAdvanceBlocks(
    state: NativeEpochScheduleState,
    start: bigint,
    end: bigint,
  ): NativeEpochScheduleState
  epochPredictFirstRevealBlock(
    state: NativeEpochScheduleState,
    revealPeriodEpochs: bigint,
  ): bigint
  epochPredictFirstRevealBlockResult(
    state: NativeEpochScheduleState,
    revealPeriodEpochs: bigint,
  ): { ok: boolean; block?: bigint | null; error?: string | null }
  encodeWeightsTlockPayload(value: Record<string, unknown>): Buffer
  decodeWeightsTlockPayload(data: Buffer): {
    hotkey: Buffer
    uids: number[]
    values: number[]
    versionKey: bigint
  }
  encodeTimelockUserData(value: Record<string, unknown>): Buffer
  decodeTimelockUserData(data: Buffer): { encryptedData: Buffer; revealRound: bigint }
  timelockMaxTempo(): number
  timelockMaxTempoU64(): bigint
  timelockDrandPublicKey(): string
  timelockGenesisTime(): bigint
  timelockDrandPeriod(): bigint
  timelockQuicknetChainHash(): string
  timelockDrandEndpoints(): string[]
  timelockSecurityBlockOffset(): bigint
  timelockCommitInclusionBlockOffset(): bigint
  timelockMaxSimulationBlocks(revealPeriodEpochs: bigint): bigint
}

// The generated napi-rs loader sits one directory above dist/native.js.
// eslint-disable-next-line @typescript-eslint/no-var-requires
const native = require('../native.cjs') as NativeBinding

export default native
