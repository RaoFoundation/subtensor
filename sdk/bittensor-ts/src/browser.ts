import type {
  ModuleError,
  ScaleValue,
} from './types'

export const CRYPTO_ED25519 = 0
export const CRYPTO_SR25519 = 1
export const DEFAULT_SS58_FORMAT = 42

export type BrowserByteLike = Uint8Array | ArrayBuffer | ArrayBufferView
export type BrowserMessage = string | BrowserByteLike
export type BrowserIntegerLike = number | bigint
export type SubstrateKeyType = 'sr25519' | 'ed25519'
export type KeypairKind = 'Ed25519' | 'Sr25519' | 'PublicOnly'

export interface KeypairMetadata {
  address?: string
  name?: string
  type?: SubstrateKeyType
  [key: string]: unknown
}

export interface KeypairSignOptions {
  /** Prefix the raw signature with its Substrate MultiSignature variant byte. */
  withType?: boolean
}

export interface BrowserStorageEntry {
  pallet: string
  name: string
  prefix: string
  modifier: string
  valueType: string
  paramTypes: string[]
  paramHashers: string[]
  defaultBytes: Uint8Array
}

export interface BrowserStorageChange {
  key: string
  value?: string | null
}

export interface BrowserMapPair<K = ScaleValue, V = ScaleValue> {
  key: K
  value: V
}

export interface BrowserPayloadParts {
  includedInExtrinsic: Uint8Array
  includedInSignedData: Uint8Array
}

export interface BrowserTransactionParams {
  era: ScaleValue
  nonce: BrowserIntegerLike
  tip?: BrowserIntegerLike
  tipAssetId?: BrowserIntegerLike | null
  genesisHash: BrowserByteLike
  eraBlockHash: BrowserByteLike
  metadataHash?: BrowserByteLike | null
}

export interface BrowserSignedExtrinsicParams {
  era: ScaleValue
  nonce: BrowserIntegerLike
  tip?: BrowserIntegerLike
  tipAssetId?: BrowserIntegerLike | null
  metadataHashEnabled?: boolean
}

export interface BrowserSignedExtrinsic {
  bytes: Uint8Array
  hash: Uint8Array
}

export interface BrowserMultisigAccount {
  accountId: Uint8Array
  sortedSignatories: Uint8Array[]
}

export interface BrowserEpochScheduleState {
  lastEpochBlock: BrowserIntegerLike
  pendingEpochAt: BrowserIntegerLike
  subnetEpochIndex: BrowserIntegerLike
  tempo: number
  blocksSinceLastStep: BrowserIntegerLike
  currentBlock: BrowserIntegerLike
}

export type BrowserRuntimeApiMap = Record<
  string,
  Record<
    string,
    {
      name: string
      inputs: Array<[string, string]>
      output: string
      docs: string[]
    }
  >
>

export interface BrowserMetadataIrCall {
  name: string
  args: string[]
  docs: string
}

export interface BrowserMetadataIrError {
  index: number
  name: string
  docs: string
}

export interface BrowserMetadataIrPallet {
  name: string
  index: number
  calls: BrowserMetadataIrCall[]
  errors: BrowserMetadataIrError[]
  storage: string[]
  constants: string[]
}

export interface BrowserMetadataIr {
  specVersion: number
  pallets: BrowserMetadataIrPallet[]
  runtimeApis: Array<{ name: string; methods: string[] }>
}

export interface BrowserWasmModule {
  default?: () => Promise<unknown> | unknown
  coreVersion(): string
  Keypair: BrowserWasmKeypairConstructor
  Runtime: BrowserWasmRuntimeConstructor
  CryptoType: {
    Ed25519: number
    Sr25519: number
  }
  verifySignature(
    message: BrowserMessage,
    signature: Uint8Array,
    ss58Address: string,
    cryptoType?: number,
  ): boolean
  ss58Decode(ss58Address: string): Uint8Array
  ss58Encode(publicKey: Uint8Array, ss58Format?: number): string
  metadataDigest(
    metadataBytes: Uint8Array,
    specVersion: number,
    specName: string,
    base58Prefix?: number,
    decimals?: number,
    tokenSymbol?: string,
  ): Uint8Array
  generateExtrinsicProof(
    callData: Uint8Array,
    includedInExtrinsic: Uint8Array,
    includedInSignedData: Uint8Array,
    metadataBytes: Uint8Array,
    specVersion: number,
    specName: string,
    base58Prefix?: number,
    decimals?: number,
    tokenSymbol?: string,
  ): Uint8Array
  getEncryptedCommitment(
    data: string,
    blocksUntilReveal: BrowserIntegerLike,
    blockTime?: number,
  ): [Uint8Array, number]
  getEncryptedCommitV2(
    uids: Uint16Array,
    weights: Uint16Array,
    versionKey: BrowserIntegerLike,
    lastEpochBlock: BrowserIntegerLike,
    pendingEpochAt: BrowserIntegerLike,
    subnetEpochIndex: BrowserIntegerLike,
    tempo: number,
    blocksSinceLastStep: BrowserIntegerLike,
    currentBlock: BrowserIntegerLike,
    subnetRevealPeriodEpochs: BrowserIntegerLike,
    blockTime: number,
    hotkey: Uint8Array,
  ): [Uint8Array, number]
  encrypt(data: Uint8Array, nBlocks: BrowserIntegerLike, blockTime?: number): [Uint8Array, number]
  encryptAtRound(data: Uint8Array, revealRound: BrowserIntegerLike): [Uint8Array, number]
  revealRound(encryptedData: Uint8Array): number
  decryptWithSignature(encryptedData: Uint8Array, signatureHex: string): Uint8Array
  encryptMlkem768(
    publicKey: Uint8Array,
    plaintext: Uint8Array,
    includeKeyHash?: boolean,
  ): Uint8Array
  mlkemKdfId(): Uint8Array
  eraBirth(period: BrowserIntegerLike, current: BrowserIntegerLike): number
  multisigAccountId(signatories: Uint8Array[], threshold: number): [Uint8Array, Uint8Array[]]
}

export interface BrowserWasmKeypair {
  readonly cryptoType: number
  readonly kind: KeypairKind
  readonly publicKey: Uint8Array
  readonly ss58Address: string
  readonly ss58Format: number
  derive(path: string): BrowserWasmKeypair
  sign(message: BrowserMessage): Uint8Array
  verify(message: BrowserMessage, signature: Uint8Array): boolean
}

export interface BrowserWasmKeypairConstructor {
  new (
    ss58Address?: string | null,
    publicKey?: Uint8Array | null,
    cryptoType?: number,
    ss58Format?: number,
  ): BrowserWasmKeypair
  fromMnemonic(
    mnemonic: string,
    cryptoType?: number,
    password?: string | null,
  ): BrowserWasmKeypair
  fromSeed(seed: Uint8Array, cryptoType?: number): BrowserWasmKeypair
  fromUri(uri: string, cryptoType?: number): BrowserWasmKeypair
  fromPrivateKey(privateKey: string, cryptoType?: number): BrowserWasmKeypair
  generateMnemonic(nWords?: number): string
}

export interface BrowserWasmRuntime {
  readonly specVersion: number
  readonly transactionVersion: number
  readonly ss58Format: number
  readonly isV15: boolean
  readonly extrinsicVersion: number
  decode(typeString: string, data: Uint8Array, strict?: boolean): unknown
  batchDecode(typeStrings: string[], data: Uint8Array[]): unknown[]
  encode(typeString: string, value: ScaleValue): Uint8Array
  typeIdOf(name: string): number | undefined
  typeNameOf(id: number): string | undefined
  registryJson(): string
  composeCall(pallet: string, fn: string, params: ScaleValue): Uint8Array
  decodeCall(data: Uint8Array): unknown
  storageEntry(pallet: string, storageFunction: string): BrowserStorageEntry
  storagePrefix(pallet: string, storageFunction: string): Uint8Array
  storageKey(pallet: string, storageFunction: string, params: ScaleValue[]): Uint8Array
  storageKeyBatch(pallet: string, storageFunction: string, paramsList: ScaleValue[][]): Uint8Array[]
  decodeStorageKeyParams(
    pallet: string,
    storageFunction: string,
    key: Uint8Array,
    fixed?: number,
  ): unknown[]
  decodeMapPairs(
    pallet: string,
    storageFunction: string,
    rawKeys: Uint8Array[],
    rawValues: Uint8Array[],
    fixed?: number,
  ): Array<[unknown, unknown]>
  decodeMapChanges(
    pallet: string,
    storageFunction: string,
    changes: Array<[string, string | null]>,
    fixed?: number,
  ): Array<[unknown, unknown]>
  constant(pallet: string, name: string): unknown
  moduleError(moduleIndex: number, errorIndex: number): [string, string[]]
  signedExtensionIdentifiers(): string[]
  encodeEra(era: ScaleValue): Uint8Array
  signaturePayloadParts(
    era: ScaleValue,
    nonce: BrowserIntegerLike,
    tip: BrowserIntegerLike,
    tipAssetId: BrowserIntegerLike | null,
    genesisHash: Uint8Array,
    eraBlockHash: Uint8Array,
    metadataHash?: Uint8Array,
  ): [Uint8Array, Uint8Array]
  signaturePayload(
    callData: Uint8Array,
    era: ScaleValue,
    nonce: BrowserIntegerLike,
    tip: BrowserIntegerLike,
    tipAssetId: BrowserIntegerLike | null,
    genesisHash: Uint8Array,
    eraBlockHash: Uint8Array,
    metadataHash?: Uint8Array,
  ): Uint8Array
  encodeSignedExtrinsic(
    callData: Uint8Array,
    publicKey: Uint8Array,
    signature: Uint8Array,
    signatureVersion: number,
    era: ScaleValue,
    nonce: BrowserIntegerLike,
    tip: BrowserIntegerLike,
    tipAssetId: BrowserIntegerLike | null,
    metadataHashEnabled?: boolean,
  ): [Uint8Array, Uint8Array]
  decodeExtrinsic(data: Uint8Array, strict?: boolean): unknown
  runtimeApiMap(): BrowserRuntimeApiMap
  metadataIr(): BrowserMetadataIr
}

export interface BrowserWasmRuntimeConstructor {
  new (
    metadataBytes: Uint8Array,
    specVersion: number,
    transactionVersion: number,
    ss58Format?: number,
  ): BrowserWasmRuntime
}

export type BrowserWasmLoader = () => Promise<BrowserWasmModule>

let configuredLoader: BrowserWasmLoader | undefined
let defaultBrowserWasmLoader: BrowserWasmLoader | undefined
let wasmPromise: Promise<BrowserWasmModule> | undefined

const metadataStore = new WeakMap<object, KeypairMetadata>()

function sanitizeMetadata(metadata: KeypairMetadata): KeypairMetadata {
  const sanitized = { ...metadata }
  delete sanitized.suri
  return sanitized
}

function defaultLoader(): Promise<BrowserWasmModule> {
  if (defaultBrowserWasmLoader == null) {
    throw new Error(
      'No browser WASM loader is configured; import the ESM entry or call configureBrowserWasm()',
    )
  }
  return defaultBrowserWasmLoader()
}

export function setDefaultBrowserWasmLoader(loader: BrowserWasmLoader): void {
  defaultBrowserWasmLoader = loader
  wasmPromise = undefined
}

export function configureBrowserWasm(loader: BrowserWasmLoader): void {
  configuredLoader = loader
  wasmPromise = undefined
}

export async function initBrowser(
  loader: BrowserWasmLoader = configuredLoader ?? defaultLoader,
): Promise<BrowserWasmModule> {
  if (wasmPromise == null) {
    wasmPromise = loader().then(async (module) => {
      if (typeof module.default === 'function') {
        await module.default()
      }
      wasmSync = () => module
      return module
    })
  }
  return wasmPromise
}

function wasmReady(): BrowserWasmModule {
  throw new Error('Call await initBrowser() before using @bittensor/sdk/browser')
}

let wasmSync: () => BrowserWasmModule = wasmReady

export async function loadBrowser(
  loader: BrowserWasmLoader = configuredLoader ?? defaultLoader,
): Promise<BrowserWasmModule> {
  return initBrowser(loader)
}

function toBytes(value: BrowserByteLike, name = 'value'): Uint8Array {
  if (value instanceof Uint8Array) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
  if (value instanceof ArrayBuffer) return new Uint8Array(value)
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
  }
  throw new TypeError(`${name} must be a Uint8Array, ArrayBuffer, or ArrayBufferView`)
}

function copyBytes(value: BrowserByteLike, name = 'value'): Uint8Array {
  return new Uint8Array(toBytes(value, name))
}

function toInteger(value: BrowserIntegerLike, name = 'value'): number {
  if (typeof value === 'bigint') {
    if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) {
      throw new RangeError(`${name} must fit in a JavaScript safe integer`)
    }
    return Number(value)
  }
  if (!Number.isSafeInteger(value)) {
    throw new RangeError(`${name} must be a safe integer or bigint`)
  }
  return value
}

function cryptoTypeForKeyType(type: SubstrateKeyType): number {
  return type === 'ed25519' ? CRYPTO_ED25519 : CRYPTO_SR25519
}

function keyTypeForCryptoType(cryptoType: number): SubstrateKeyType {
  if (cryptoType === CRYPTO_ED25519) return 'ed25519'
  if (cryptoType === CRYPTO_SR25519) return 'sr25519'
  throw new RangeError(`unsupported crypto type ${cryptoType}`)
}

function copyStorageEntry(entry: BrowserStorageEntry): BrowserStorageEntry {
  return {
    ...entry,
    paramTypes: entry.paramTypes.slice(),
    paramHashers: entry.paramHashers.slice(),
    defaultBytes: copyBytes(entry.defaultBytes, 'defaultBytes'),
  }
}

function copyByteList(values: Uint8Array[]): Uint8Array[] {
  return values.map((value) => copyBytes(value))
}

function toUint16Array(values: number[], name: string): Uint16Array {
  const output = new Uint16Array(values.length)
  for (let i = 0; i < values.length; i += 1) {
    const value = values[i]
    if (!Number.isInteger(value) || value < 0 || value > 0xffff) {
      throw new RangeError(`${name}[${i}] must be an unsigned 16-bit integer`)
    }
    output[i] = value
  }
  return output
}

function mapPairs<K, V>(pairs: Array<[unknown, unknown]>): Array<BrowserMapPair<K, V>> {
  return pairs.map(([key, value]) => ({ key: key as K, value: value as V }))
}

function metadataHashArg(value?: BrowserByteLike | null): Uint8Array | undefined {
  return value == null ? undefined : toBytes(value, 'metadataHash')
}

export class Runtime {
  private readonly handle: BrowserWasmRuntime

  constructor(
    metadataBytes: BrowserByteLike,
    specVersion: number,
    transactionVersion: number,
    ss58Format = DEFAULT_SS58_FORMAT,
  ) {
    this.handle = new (wasmSync().Runtime)(
      toBytes(metadataBytes, 'metadataBytes'),
      specVersion,
      transactionVersion,
      ss58Format,
    )
  }

  get specVersion(): number {
    return this.handle.specVersion
  }

  get transactionVersion(): number {
    return this.handle.transactionVersion
  }

  get ss58Format(): number {
    return this.handle.ss58Format
  }

  get isV15(): boolean {
    return this.handle.isV15
  }

  get extrinsicVersion(): number {
    return this.handle.extrinsicVersion
  }

  decode<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: BrowserByteLike,
    strict = true,
  ): T {
    return this.handle.decode(typeString, toBytes(data, 'data'), strict) as T
  }

  decodeBatch<T extends ScaleValue = ScaleValue>(
    typeStrings: string[],
    data: BrowserByteLike[],
  ): T[] {
    return this.handle.batchDecode(typeStrings, data.map((value) => toBytes(value, 'data'))) as T[]
  }

  encode(typeString: string, value: ScaleValue): Uint8Array {
    return copyBytes(this.handle.encode(typeString, value))
  }

  typeIdOf(name: string): number | null {
    return this.handle.typeIdOf(name) ?? null
  }

  typeNameOf(id: number): string | null {
    return this.handle.typeNameOf(id) ?? null
  }

  registryJson(): string {
    return this.handle.registryJson()
  }

  composeCall(pallet: string, fn: string, params: ScaleValue): Uint8Array {
    return copyBytes(this.handle.composeCall(pallet, fn, params))
  }

  decodeCall<T extends ScaleValue = ScaleValue>(data: BrowserByteLike): T {
    return this.handle.decodeCall(toBytes(data, 'data')) as T
  }

  storageEntry(pallet: string, storageFunction: string): BrowserStorageEntry {
    return copyStorageEntry(this.handle.storageEntry(pallet, storageFunction))
  }

  storagePrefix(pallet: string, storageFunction: string): Uint8Array {
    return copyBytes(this.handle.storagePrefix(pallet, storageFunction))
  }

  storageKey(
    pallet: string,
    storageFunction: string,
    params: ScaleValue[] = [],
  ): Uint8Array {
    return copyBytes(this.handle.storageKey(pallet, storageFunction, params))
  }

  storageKeyBatch(
    pallet: string,
    storageFunction: string,
    paramsList: ScaleValue[][],
  ): Uint8Array[] {
    return copyByteList(this.handle.storageKeyBatch(pallet, storageFunction, paramsList))
  }

  decodeStorageKeyParams<T extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    key: BrowserByteLike,
    fixed = 0,
  ): T[] {
    return this.handle.decodeStorageKeyParams(
      pallet,
      storageFunction,
      toBytes(key, 'key'),
      fixed,
    ) as T[]
  }

  decodeMapPairs<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    rawKeys: BrowserByteLike[],
    rawValues: BrowserByteLike[],
    fixed = 0,
  ): Array<BrowserMapPair<K, V>> {
    return mapPairs<K, V>(
      this.handle.decodeMapPairs(
        pallet,
        storageFunction,
        rawKeys.map((value) => toBytes(value, 'rawKey')),
        rawValues.map((value) => toBytes(value, 'rawValue')),
        fixed,
      ),
    )
  }

  decodeMapChanges<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    changes: BrowserStorageChange[],
    fixed = 0,
  ): Array<BrowserMapPair<K, V>> {
    return mapPairs<K, V>(
      this.handle.decodeMapChanges(
        pallet,
        storageFunction,
        changes.map((change) => [change.key, change.value ?? null]),
        fixed,
      ),
    )
  }

  constant<T extends ScaleValue = ScaleValue>(pallet: string, name: string): T | undefined {
    return this.handle.constant(pallet, name) as T | undefined
  }

  moduleError(moduleIndex: number, errorIndex: number): ModuleError {
    const [name, docs] = this.handle.moduleError(moduleIndex, errorIndex)
    return { name, docs: docs.slice() }
  }

  signedExtensionIdentifiers(): string[] {
    return this.handle.signedExtensionIdentifiers().slice()
  }

  encodeEra(era: ScaleValue): Uint8Array {
    return copyBytes(this.handle.encodeEra(era))
  }

  signaturePayloadParts(params: BrowserTransactionParams): BrowserPayloadParts {
    const [includedInExtrinsic, includedInSignedData] = this.handle.signaturePayloadParts(
      params.era,
      params.nonce,
      params.tip ?? 0,
      params.tipAssetId ?? null,
      toBytes(params.genesisHash, 'genesisHash'),
      toBytes(params.eraBlockHash, 'eraBlockHash'),
      metadataHashArg(params.metadataHash),
    )
    return {
      includedInExtrinsic: copyBytes(includedInExtrinsic),
      includedInSignedData: copyBytes(includedInSignedData),
    }
  }

  signaturePayload(callData: BrowserByteLike, params: BrowserTransactionParams): Uint8Array {
    return copyBytes(
      this.handle.signaturePayload(
        toBytes(callData, 'callData'),
        params.era,
        params.nonce,
        params.tip ?? 0,
        params.tipAssetId ?? null,
        toBytes(params.genesisHash, 'genesisHash'),
        toBytes(params.eraBlockHash, 'eraBlockHash'),
        metadataHashArg(params.metadataHash),
      ),
    )
  }

  encodeSignedExtrinsic(
    callData: BrowserByteLike,
    publicKey: BrowserByteLike,
    signature: BrowserByteLike,
    signatureVersion: number,
    params: BrowserSignedExtrinsicParams,
  ): BrowserSignedExtrinsic {
    const [bytes, hash] = this.handle.encodeSignedExtrinsic(
      toBytes(callData, 'callData'),
      toBytes(publicKey, 'publicKey'),
      toBytes(signature, 'signature'),
      signatureVersion,
      params.era,
      params.nonce,
      params.tip ?? 0,
      params.tipAssetId ?? null,
      params.metadataHashEnabled ?? false,
    )
    return { bytes: copyBytes(bytes), hash: copyBytes(hash) }
  }

  decodeExtrinsic<T extends ScaleValue = ScaleValue>(
    data: BrowserByteLike,
    strict = true,
  ): T {
    return this.handle.decodeExtrinsic(toBytes(data, 'data'), strict) as T
  }

  runtimeApiMap(): BrowserRuntimeApiMap {
    return this.handle.runtimeApiMap()
  }

  runtimeApis(): BrowserRuntimeApiMap {
    return this.runtimeApiMap()
  }

  metadataIr(): BrowserMetadataIr {
    return this.handle.metadataIr()
  }
}

export class Keypair {
  private handle!: BrowserWasmKeypair

  constructor(
    ss58Address?: string | null,
    publicKey?: BrowserByteLike | null,
    cryptoType = CRYPTO_SR25519,
    ss58Format = DEFAULT_SS58_FORMAT,
  ) {
    this.handle = new (wasmSync().Keypair)(
      ss58Address ?? undefined,
      publicKey == null ? undefined : toBytes(publicKey, 'publicKey'),
      cryptoType,
      ss58Format,
    )
    metadataStore.set(this, {})
  }

  private static wrap(handle: BrowserWasmKeypair, metadata: KeypairMetadata = {}): Keypair {
    const keypair = Object.create(Keypair.prototype) as Keypair
    keypair.handle = handle
    metadataStore.set(keypair, sanitizeMetadata(metadata))
    return keypair
  }

  static fromMnemonic(
    mnemonic: string,
    cryptoType = CRYPTO_SR25519,
    password?: string | null,
  ): Keypair {
    return Keypair.wrap(
      wasmSync().Keypair.fromMnemonic(mnemonic, cryptoType, password ?? undefined),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromMnemonic(
    mnemonic: string,
    cryptoType = CRYPTO_SR25519,
    password?: string | null,
  ): Keypair {
    return Keypair.fromMnemonic(mnemonic, cryptoType, password)
  }

  static fromSeed(seed: BrowserByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      wasmSync().Keypair.fromSeed(toBytes(seed, 'seed'), cryptoType),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromSeed(seed: BrowserByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromSeed(seed, cryptoType)
  }

  static fromUri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      wasmSync().Keypair.fromUri(uri, cryptoType),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromUri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromUri(uri, cryptoType)
  }

  static fromPrivateKey(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      wasmSync().Keypair.fromPrivateKey(privateKey, cryptoType),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromPrivateKey(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromPrivateKey(privateKey, cryptoType)
  }

  static generateMnemonic(nWords = 12): string {
    return wasmSync().Keypair.generateMnemonic(nWords)
  }

  get cryptoType(): number {
    return this.handle.cryptoType
  }

  get kind(): KeypairKind {
    return this.handle.kind
  }

  get publicKey(): Uint8Array {
    return copyBytes(this.handle.publicKey)
  }

  get addressRaw(): Uint8Array {
    return this.publicKey
  }

  get ss58Address(): string {
    return this.handle.ss58Address
  }

  get address(): string {
    return this.ss58Address
  }

  get type(): SubstrateKeyType {
    return keyTypeForCryptoType(this.cryptoType)
  }

  get scheme(): 'Ed25519' | 'Sr25519' {
    return this.cryptoType === CRYPTO_ED25519 ? 'Ed25519' : 'Sr25519'
  }

  get ss58Format(): number {
    return this.handle.ss58Format
  }

  get meta(): KeypairMetadata {
    return { ...(metadataStore.get(this) ?? {}) }
  }

  get isLocked(): boolean {
    return false
  }

  sign(message: BrowserMessage, options: KeypairSignOptions = {}): Uint8Array {
    const signature = this.handle.sign(typeof message === 'string' ? message : toBytes(message, 'message'))
    if (!options.withType) return copyBytes(signature)
    const typed = new Uint8Array(signature.length + 1)
    typed[0] = this.cryptoType
    typed.set(signature, 1)
    return typed
  }

  verify(
    message: BrowserMessage,
    signature: BrowserByteLike,
    signerPublic?: string | BrowserByteLike,
  ): boolean {
    const suppliedSignature = toBytes(signature, 'signature')
    const hasType = suppliedSignature.length === 65 && suppliedSignature[0] <= CRYPTO_SR25519
    const cryptoType = hasType ? suppliedSignature[0] : this.cryptoType
    const rawSignature = hasType ? suppliedSignature.subarray(1) : suppliedSignature

    if (signerPublic == null) {
      return this.handle.verify(typeof message === 'string' ? message : toBytes(message, 'message'), rawSignature)
    }
    if (typeof signerPublic === 'string' && !signerPublic.startsWith('0x')) {
      return verifySignature(message, rawSignature, signerPublic, cryptoType)
    }
    const publicKey =
      typeof signerPublic === 'string'
        ? hexToBytes(signerPublic)
        : toBytes(signerPublic, 'signerPublic')
    return new Keypair(undefined, publicKey, cryptoType, this.ss58Format).verify(
      message,
      rawSignature,
    )
  }

  derive(suri: string, meta: KeypairMetadata = {}): Keypair {
    const derived = Keypair.wrap(
      this.handle.derive(suri),
      { type: keyTypeForCryptoType(this.cryptoType) },
    )
    derived.setMeta(meta)
    return derived
  }

  setMeta(meta: KeypairMetadata): void {
    metadataStore.set(
      this,
      sanitizeMetadata({ ...(metadataStore.get(this) ?? {}), ...meta }),
    )
  }
}

export async function ready(): Promise<BrowserWasmModule> {
  return loadBrowser()
}

export function coreVersion(): string {
  return wasmSync().coreVersion()
}

export function eraBirth(period: BrowserIntegerLike, current: BrowserIntegerLike): number {
  return wasmSync().eraBirth(period, current)
}

export function multisigAccountId(
  signatories: BrowserByteLike[],
  threshold: number,
): BrowserMultisigAccount {
  const [accountId, sortedSignatories] = wasmSync().multisigAccountId(
    signatories.map((value) => toBytes(value, 'signatory')),
    threshold,
  )
  return {
    accountId: copyBytes(accountId),
    sortedSignatories: copyByteList(sortedSignatories),
  }
}

export function verifySignature(
  message: BrowserMessage,
  signature: BrowserByteLike,
  ss58Address: string,
  cryptoType = CRYPTO_SR25519,
): boolean {
  return wasmSync().verifySignature(
    typeof message === 'string' ? message : toBytes(message, 'message'),
    toBytes(signature, 'signature'),
    ss58Address,
    cryptoType,
  )
}

export function publicKeyFromSs58(ss58Address: string): Uint8Array {
  return copyBytes(wasmSync().ss58Decode(ss58Address))
}

export function ss58FromPublic(
  publicKey: BrowserByteLike,
  ss58Format = DEFAULT_SS58_FORMAT,
): string {
  return wasmSync().ss58Encode(toBytes(publicKey, 'publicKey'), ss58Format)
}

export function metadataDigest(
  metadataBytes: BrowserByteLike,
  specVersion: number,
  specName: string,
  base58Prefix?: number,
  decimals?: number,
  tokenSymbol?: string,
): Uint8Array {
  return copyBytes(
    wasmSync().metadataDigest(
      toBytes(metadataBytes, 'metadataBytes'),
      specVersion,
      specName,
      base58Prefix,
      decimals,
      tokenSymbol,
    ),
  )
}

export function generateExtrinsicProof(
  callData: BrowserByteLike,
  includedInExtrinsic: BrowserByteLike,
  includedInSignedData: BrowserByteLike,
  metadataBytes: BrowserByteLike,
  specVersion: number,
  specName: string,
  base58Prefix?: number,
  decimals?: number,
  tokenSymbol?: string,
): Uint8Array {
  return copyBytes(
    wasmSync().generateExtrinsicProof(
      toBytes(callData, 'callData'),
      toBytes(includedInExtrinsic, 'includedInExtrinsic'),
      toBytes(includedInSignedData, 'includedInSignedData'),
      toBytes(metadataBytes, 'metadataBytes'),
      specVersion,
      specName,
      base58Prefix,
      decimals,
      tokenSymbol,
    ),
  )
}

export function getEncryptedCommitment(
  data: string,
  blocksUntilReveal: BrowserIntegerLike,
  blockTime?: number,
): [Uint8Array, number] {
  const [ciphertext, round] = wasmSync().getEncryptedCommitment(
    data,
    toInteger(blocksUntilReveal, 'blocksUntilReveal'),
    blockTime,
  )
  return [copyBytes(ciphertext), round]
}

export function generateCommitV2(
  uids: number[],
  values: number[],
  versionKey: BrowserIntegerLike,
  state: BrowserEpochScheduleState,
  subnetRevealPeriodEpochs: BrowserIntegerLike,
  blockTime: number,
  hotkey: BrowserByteLike,
): [Uint8Array, number] {
  const [ciphertext, round] = wasmSync().getEncryptedCommitV2(
    toUint16Array(uids, 'uids'),
    toUint16Array(values, 'values'),
    versionKey,
    state.lastEpochBlock,
    state.pendingEpochAt,
    state.subnetEpochIndex,
    state.tempo,
    state.blocksSinceLastStep,
    state.currentBlock,
    subnetRevealPeriodEpochs,
    blockTime,
    toBytes(hotkey, 'hotkey'),
  )
  return [copyBytes(ciphertext), round]
}

export function encrypt(
  data: BrowserByteLike,
  nBlocks: BrowserIntegerLike,
  blockTime?: number,
): [Uint8Array, number] {
  const [ciphertext, round] = wasmSync().encrypt(toBytes(data, 'data'), toInteger(nBlocks, 'nBlocks'), blockTime)
  return [copyBytes(ciphertext), round]
}

export function encryptAtRound(
  data: BrowserByteLike,
  revealRound: BrowserIntegerLike,
): [Uint8Array, number] {
  const [ciphertext, round] = wasmSync().encryptAtRound(
    toBytes(data, 'data'),
    toInteger(revealRound, 'revealRound'),
  )
  return [copyBytes(ciphertext), round]
}

export function decryptWithSignature(
  encryptedData: BrowserByteLike,
  signatureHex: string,
): Uint8Array {
  return copyBytes(
    wasmSync().decryptWithSignature(toBytes(encryptedData, 'encryptedData'), signatureHex),
  )
}

export function revealRound(encryptedData: BrowserByteLike): number {
  return wasmSync().revealRound(toBytes(encryptedData, 'encryptedData'))
}

export function sealMevShieldTransaction(
  publicKey: BrowserByteLike,
  plaintext: BrowserByteLike,
  includeKeyHash = false,
): Uint8Array {
  return copyBytes(
    wasmSync().encryptMlkem768(
      toBytes(publicKey, 'publicKey'),
      toBytes(plaintext, 'plaintext'),
      includeKeyHash,
    ),
  )
}

export function mlkemKdfId(): Uint8Array {
  return copyBytes(wasmSync().mlkemKdfId())
}

function hexToBytes(hex: string): Uint8Array {
  const value = hex.startsWith('0x') ? hex.slice(2) : hex
  if (value.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(value)) {
    throw new TypeError('hex string contains invalid bytes')
  }
  const bytes = new Uint8Array(value.length / 2)
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(value.slice(i * 2, i * 2 + 2), 16)
  }
  return bytes
}
