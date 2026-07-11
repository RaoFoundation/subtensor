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

export interface BrowserWasmModule {
  default?: () => Promise<unknown> | unknown
  coreVersion(): string
  Keypair: BrowserWasmKeypairConstructor
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
