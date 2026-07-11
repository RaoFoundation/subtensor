/* eslint-disable @typescript-eslint/no-explicit-any */

export interface NativeKeypairHandle {
  readonly cryptoType: number
  readonly kind: 'Ed25519' | 'Sr25519' | 'PublicOnly'
  readonly publicKey: Buffer
  readonly privateKey?: Buffer | null
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

export interface NativeTxParams {
  era: unknown
  nonce: bigint
  tip: bigint
  tipAssetId?: bigint | null
  genesisHash: Buffer
  eraBlockHash: Buffer
  metadataHash?: Buffer | null
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
  appVersion(): { major: number; minor: number; patch: number }
  address(
    account: number,
    index: number,
    ss58Prefix: number,
    confirm: boolean,
  ): { publicKey: Buffer; ss58Address: string }
  sign(account: number, index: number, payload: Buffer, proof: Buffer): Buffer
}

export interface NativeLedgerConstructor {
  open(): NativeLedgerHandle
}

export interface NativeBinding {
  NativeKeypair: { readonly prototype: NativeKeypairHandle }
  NativeRuntime: NativeRuntimeConstructor
  NativeCursor: NativeCursorConstructor
  NativeLedgerDevice: NativeLedgerConstructor

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
  keypairFromEncryptedJson(jsonData: string, passphrase: string): NativeKeypairHandle
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
  deserializeKeypair(keyfileData: Buffer): NativeKeypairHandle
  encryptKeyfileData(keyfileData: Buffer, password: string): Buffer
  decryptKeyfileData(keyfileData: Buffer, password?: string | null): Buffer
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

  timelockEncryptAndCompress(data: Buffer, revealRound: bigint): Buffer
  timelockDecryptAndDecompress(encryptedData: Buffer, signatureBytes: Buffer): Buffer
  timelockGenerateCommitV2(
    uids: number[],
    values: number[],
    versionKey: bigint,
    state: NativeEpochScheduleState,
    subnetRevealPeriodEpochs: bigint,
    blockTime: number,
    hotkey: Buffer,
  ): { ciphertext: Buffer; revealRound: bigint }
  timelockEncryptCommitment(
    data: string,
    blocksUntilReveal: bigint,
    blockTime: number,
  ): { ciphertext: Buffer; revealRound: bigint }
  timelockEncryptNBlocks(
    data: Buffer,
    nBlocks: bigint,
    blockTime: number,
  ): { ciphertext: Buffer; revealRound: bigint }
  timelockEncryptAtRound(
    data: Buffer,
    revealRound: bigint,
  ): { ciphertext: Buffer; revealRound: bigint }
  timelockGetRoundInfo(round?: bigint | null): { round: bigint; signature: string }
  timelockGetRevealRoundSignature(
    revealRound: bigint | null | undefined,
    noErrors: boolean,
  ): string | null | undefined
  timelockDecrypt(
    encryptedData: Buffer,
    noErrors: boolean,
  ): Buffer | null | undefined
  timelockDecryptWithSignature(encryptedData: Buffer, signatureHex: string): Buffer
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
