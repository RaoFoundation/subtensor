import native, { type NativeKeypairHandle } from './native'
import { nativeAsync, nativeCall } from './errors'
import { coerceMessage, toBuffer } from './wire'
import type { ByteLike } from './types'

export const CRYPTO_ED25519 = nativeCall(() => native.cryptoEd25519())
export const CRYPTO_SR25519 = nativeCall(() => native.cryptoSr25519())
export const DEFAULT_SS58_FORMAT = nativeCall(() => native.defaultSs58Format())

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

export interface GeneratedKeypair {
  keypair: Keypair
  mnemonic: string
}

export interface WriteKeyfileOptions {
  password?: string | null
  overwrite?: boolean
  allowPlaintext?: boolean
}

/**
 * Signing-compatible subset of Polkadot.js/Moonwall keyring-pair behavior.
 * Public-key derivation and signatures stay inside bittensor-core Rust, while
 * full keystore lifecycle APIs such as PKCS#8, JSON export, locking, and VRF
 * deliberately remain unsupported.
 */
export interface PolkadotCompatibleKeypair {
  readonly address: string
  readonly addressRaw: Uint8Array
  readonly publicKey: Uint8Array
  readonly type: SubstrateKeyType
  readonly meta: KeypairMetadata
  readonly isLocked: boolean
  sign(message: string | Uint8Array, options?: KeypairSignOptions): Uint8Array
  verify(
    message: string | Uint8Array,
    signature: Uint8Array,
    signerPublic?: string | Uint8Array,
  ): boolean
  derive(suri: string, meta?: KeypairMetadata): PolkadotCompatibleKeypair
  setMeta(meta: KeypairMetadata): void
  encodePkcs8(passphrase?: string): Uint8Array
  decodePkcs8(passphrase?: string, encoded?: Uint8Array): void
  lock(): void
  unlock(passphrase?: string): void
  toJson(passphrase?: string): unknown
  vrfSign(
    message: string | Uint8Array,
    context?: string | Uint8Array,
    extra?: string | Uint8Array,
  ): Uint8Array
  vrfVerify(
    message: string | Uint8Array,
    vrfResult: Uint8Array,
    signerPublic: string | Uint8Array,
    context?: string | Uint8Array,
    extra?: string | Uint8Array,
  ): boolean
}

function unsupported(operation: string): never {
  throw new Error(
    `${operation} is not part of bittensor-core; use the native keyfile APIs instead`,
  )
}

function normalizeWriteKeyfileOptions(
  passwordOrOptions?: string | null | WriteKeyfileOptions,
  overwrite = false,
  allowPlaintext = false,
): Required<WriteKeyfileOptions> {
  if (typeof passwordOrOptions === 'object' && passwordOrOptions != null) {
    return {
      password: passwordOrOptions.password ?? null,
      overwrite: passwordOrOptions.overwrite ?? false,
      allowPlaintext: passwordOrOptions.allowPlaintext === true,
    }
  }
  return {
    password: passwordOrOptions ?? null,
    overwrite,
    allowPlaintext,
  }
}

/**
 * Keep mutable compatibility metadata outside the JavaScript object shape.
 * Mnemonics, passwords, and secret URIs stay exclusively in NativeKeypair.
 */
const metadataStore = new WeakMap<object, KeypairMetadata>()

function sanitizeMetadata(metadata: KeypairMetadata): KeypairMetadata {
  const sanitized = { ...metadata }
  // `suri` may contain a mnemonic, seed, password, and derivation path.
  delete sanitized.suri
  return sanitized
}

export function cryptoTypeForKeyType(type: SubstrateKeyType): number {
  return type === 'ed25519' ? CRYPTO_ED25519 : CRYPTO_SR25519
}

export function keyTypeForCryptoType(cryptoType: number): SubstrateKeyType {
  if (cryptoType === CRYPTO_ED25519) return 'ed25519'
  if (cryptoType === CRYPTO_SR25519) return 'sr25519'
  throw new RangeError(`unsupported crypto type ${cryptoType}`)
}

export class Keypair implements PolkadotCompatibleKeypair {
  private handle!: NativeKeypairHandle

  constructor(
    ss58Address?: string | null,
    publicKey?: ByteLike | null,
    cryptoType = CRYPTO_SR25519,
    ss58Format = DEFAULT_SS58_FORMAT,
  ) {
    this.handle = nativeCall(() =>
      native.keypairNew(
        ss58Address ?? undefined,
        publicKey == null ? undefined : toBuffer(publicKey, 'publicKey'),
        cryptoType,
        ss58Format,
      ),
    )
    metadataStore.set(this, {})
  }

  private static wrap(
    handle: NativeKeypairHandle,
    metadata: KeypairMetadata = {},
  ): Keypair {
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
      nativeCall(() => native.keypairFromMnemonic(mnemonic, cryptoType, password ?? undefined)),
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

  static from_mnemonic(
    mnemonic: string,
    cryptoType = CRYPTO_SR25519,
    password?: string | null,
  ): Keypair {
    return Keypair.fromMnemonic(mnemonic, cryptoType, password)
  }

  static create_from_mnemonic(
    mnemonic: string,
    cryptoType = CRYPTO_SR25519,
    password?: string | null,
  ): Keypair {
    return Keypair.fromMnemonic(mnemonic, cryptoType, password)
  }

  static fromSeed(seed: ByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      nativeCall(() => native.keypairFromSeed(toBuffer(seed, 'seed'), cryptoType)),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromSeed(seed: ByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromSeed(seed, cryptoType)
  }

  static from_seed(seed: ByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromSeed(seed, cryptoType)
  }

  static create_from_seed(seed: ByteLike, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromSeed(seed, cryptoType)
  }

  static fromUri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      nativeCall(() => native.keypairFromUri(uri, cryptoType)),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromUri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromUri(uri, cryptoType)
  }

  static from_uri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromUri(uri, cryptoType)
  }

  static create_from_uri(uri: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromUri(uri, cryptoType)
  }

  static fromPrivateKey(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.wrap(
      nativeCall(() => native.keypairFromPrivateKey(privateKey, cryptoType)),
      { type: keyTypeForCryptoType(cryptoType) },
    )
  }

  static createFromPrivateKey(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromPrivateKey(privateKey, cryptoType)
  }

  static from_private_key(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromPrivateKey(privateKey, cryptoType)
  }

  static create_from_private_key(privateKey: string, cryptoType = CRYPTO_SR25519): Keypair {
    return Keypair.fromPrivateKey(privateKey, cryptoType)
  }

  static async fromEncryptedJson(jsonData: string, passphrase: string): Promise<Keypair> {
    return Keypair.wrap(
      await nativeAsync(() => native.keypairFromEncryptedJson(jsonData, passphrase)),
    )
  }

  static createFromEncryptedJson(jsonData: string, passphrase: string): Promise<Keypair> {
    return Keypair.fromEncryptedJson(jsonData, passphrase)
  }

  static from_encrypted_json(jsonData: string, passphrase: string): Promise<Keypair> {
    return Keypair.fromEncryptedJson(jsonData, passphrase)
  }

  static create_from_encrypted_json(jsonData: string, passphrase: string): Promise<Keypair> {
    return Keypair.fromEncryptedJson(jsonData, passphrase)
  }

  static generateMnemonic(nWords = 12): string {
    return nativeCall(() => native.generateMnemonic(nWords))
  }

  static generate_mnemonic(nWords = 12): string {
    return Keypair.generateMnemonic(nWords)
  }

  static generate(
    cryptoType = CRYPTO_SR25519,
    nWords = 12,
    password?: string | null,
  ): Keypair {
    return Keypair.generateWithMnemonic(cryptoType, nWords, password).keypair
  }

  static generateWithMnemonic(
    cryptoType = CRYPTO_SR25519,
    nWords = 12,
    password?: string | null,
  ): GeneratedKeypair {
    const mnemonic = Keypair.generateMnemonic(nWords)
    return {
      keypair: Keypair.fromMnemonic(mnemonic, cryptoType, password),
      mnemonic,
    }
  }

  static writeKeyfilePair(
    privateKeypair: Keypair,
    privatePath: string,
    publicKeypair: Keypair,
    publicPath: string,
    options: WriteKeyfileOptions = {},
  ): Promise<void> {
    const normalized = normalizeWriteKeyfileOptions(options)
    return nativeAsync(() =>
      native.writeKeypairPairKeyfile(
        privateKeypair.handle,
        privatePath,
        normalized.password ?? undefined,
        publicKeypair.handle,
        publicPath,
        normalized.overwrite,
        normalized.allowPlaintext,
      ),
    )
  }

  static encryptFor(
    ss58Address: string,
    message: string | ByteLike,
    cryptoType = CRYPTO_ED25519,
  ): Buffer {
    return nativeCall(() => native.encryptFor(ss58Address, coerceMessage(message), cryptoType))
  }

  static encrypt_for(
    ss58Address: string,
    message: string | ByteLike,
    cryptoType = CRYPTO_ED25519,
  ): Buffer {
    return Keypair.encryptFor(ss58Address, message, cryptoType)
  }

  static deserialize(keyfileData: ByteLike): Keypair {
    return Keypair.wrap(
      nativeCall(() => native.deserializeKeypair(toBuffer(keyfileData, 'keyfileData'))),
    )
  }

  static async fromKeyfileData(keyfileData: ByteLike, password?: string | null): Promise<Keypair> {
    return Keypair.wrap(
      await nativeAsync(() =>
        native.deserializeKeypairFromKeyfile(
          toBuffer(keyfileData, 'keyfileData'),
          password ?? undefined,
        ),
      ),
    )
  }

  static async fromKeyfile(path: string, password?: string | null): Promise<Keypair> {
    return Keypair.wrap(
      await nativeAsync(() => native.readKeypairKeyfile(path, password ?? undefined)),
    )
  }

  get cryptoType(): number {
    return this.handle.cryptoType
  }

  get crypto_type(): number {
    return this.cryptoType
  }

  get kind(): KeypairKind {
    return this.handle.kind
  }

  get publicKey(): Buffer {
    return Buffer.from(this.handle.publicKey)
  }

  get public_key(): Buffer {
    return this.publicKey
  }

  get addressRaw(): Buffer {
    return this.publicKey
  }

  get ss58Address(): string {
    return this.handle.ss58Address
  }

  get ss58_address(): string {
    return this.ss58Address
  }

  /** Polkadot.js-compatible alias used by Moonwall and PAPI signer helpers. */
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

  get ss58_format(): number {
    return this.ss58Format
  }

  get meta(): KeypairMetadata {
    return { ...(metadataStore.get(this) ?? {}) }
  }

  get isLocked(): boolean {
    return false
  }

  sign(message: string | ByteLike, options: KeypairSignOptions = {}): Buffer {
    const signature = nativeCall(() => this.handle.sign(coerceMessage(message)))
    if (!options.withType) return Buffer.from(signature)
    return Buffer.concat([Buffer.from([this.cryptoType]), Buffer.from(signature)])
  }

  verify(
    message: string | ByteLike,
    signature: ByteLike,
    signerPublic?: string | ByteLike,
  ): boolean {
    const suppliedSignature = toBuffer(signature, 'signature')
    const hasType = suppliedSignature.length === 65 && suppliedSignature[0] <= CRYPTO_SR25519
    const cryptoType = hasType ? suppliedSignature[0] : this.cryptoType
    const rawSignature = hasType ? suppliedSignature.subarray(1) : suppliedSignature

    if (signerPublic == null) {
      if (hasType && cryptoType !== this.cryptoType) return false
      return nativeCall(() => this.handle.verify(coerceMessage(message), rawSignature))
    }
    if (typeof signerPublic === 'string' && !signerPublic.startsWith('0x')) {
      return verifySignature(message, rawSignature, signerPublic, cryptoType)
    }
    const publicKey =
      typeof signerPublic === 'string'
        ? Buffer.from(signerPublic.slice(2), 'hex')
        : toBuffer(signerPublic, 'signerPublic')
    return new Keypair(undefined, publicKey, cryptoType, this.ss58Format).verify(
      message,
      rawSignature,
    )
  }

  derive(suri: string, meta: KeypairMetadata = {}): Keypair {
    const derived = Keypair.wrap(
      nativeCall(() => this.handle.derive(suri)),
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

  encodePkcs8(_passphrase?: string): Uint8Array {
    return unsupported('PKCS#8 encoding')
  }

  decodePkcs8(_passphrase?: string, _encoded?: Uint8Array): void {
    unsupported('PKCS#8 decoding')
  }

  lock(): void {
    unsupported('in-memory key locking')
  }

  unlock(_passphrase?: string): void {
    unsupported('in-memory key unlocking')
  }

  toJson(_passphrase?: string): never {
    return unsupported('Polkadot.js JSON export')
  }

  vrfSign(
    _message: string | Uint8Array,
    _context?: string | Uint8Array,
    _extra?: string | Uint8Array,
  ): never {
    return unsupported('VRF signing')
  }

  vrfVerify(
    _message: string | Uint8Array,
    _vrfResult: Uint8Array,
    _signerPublic: string | Uint8Array,
    _context?: string | Uint8Array,
    _extra?: string | Uint8Array,
  ): never {
    return unsupported('VRF verification')
  }

  encrypt(message: string | ByteLike): Buffer {
    return nativeCall(() => this.handle.encrypt(coerceMessage(message)))
  }

  decrypt(ciphertext: ByteLike): Buffer {
    return nativeCall(() => this.handle.decrypt(toBuffer(ciphertext, 'ciphertext')))
  }

  serialize(): Buffer {
    return nativeCall(() => native.serializeKeypair(this.handle))
  }

  toKeyfileData(password?: string | null): Promise<Buffer> {
    return nativeAsync(() =>
      native.keypairToKeyfileData(this.handle, password ?? undefined),
    )
  }

  writeKeyfile(path: string, options?: WriteKeyfileOptions): Promise<void>
  writeKeyfile(
    path: string,
    password?: string | null,
    overwrite?: boolean,
    allowPlaintext?: boolean,
  ): Promise<void>
  writeKeyfile(
    path: string,
    passwordOrOptions?: string | null | WriteKeyfileOptions,
    overwrite = false,
    allowPlaintext = false,
  ): Promise<void> {
    const options = normalizeWriteKeyfileOptions(passwordOrOptions, overwrite, allowPlaintext)
    return nativeAsync(() =>
      native.writeKeypairKeyfile(
        this.handle,
        path,
        options.password ?? undefined,
        options.overwrite,
        options.allowPlaintext,
      ),
    )
  }
}

export function createKeyringPairFromUri(
  uri: string,
  type: SubstrateKeyType = 'sr25519',
  meta: KeypairMetadata = {},
): Keypair {
  const pair = Keypair.fromUri(uri, cryptoTypeForKeyType(type))
  pair.setMeta(meta)
  return pair
}

export function createKeyringPairFromMnemonic(
  mnemonic: string,
  type: SubstrateKeyType = 'sr25519',
  password?: string | null,
  meta: KeypairMetadata = {},
): Keypair {
  const pair = Keypair.fromMnemonic(mnemonic, cryptoTypeForKeyType(type), password)
  pair.setMeta(meta)
  return pair
}

export function generateKeyringPair(
  type: SubstrateKeyType = 'sr25519',
  nWords = 12,
  meta: KeypairMetadata = {},
): Keypair {
  const pair = Keypair.generate(cryptoTypeForKeyType(type), nWords)
  pair.setMeta(meta)
  return pair
}

export function generateKeypairWithMnemonic(
  cryptoType = CRYPTO_SR25519,
  nWords = 12,
  password?: string | null,
): GeneratedKeypair {
  return Keypair.generateWithMnemonic(cryptoType, nWords, password)
}

export function generateMnemonic(nWords = 12): string {
  return Keypair.generateMnemonic(nWords)
}

export const generate_mnemonic = generateMnemonic

export function verifySignature(
  message: string | ByteLike,
  signature: ByteLike,
  ss58Address: string,
  cryptoType = CRYPTO_SR25519,
): boolean {
  return nativeCall(() =>
    native.verifySignature(
      coerceMessage(message),
      toBuffer(signature, 'signature'),
      ss58Address,
      cryptoType,
    ),
  )
}

export const verify = verifySignature
export const verify_signature = verifySignature

export function publicKeyFromSs58(ss58Address: string): Buffer {
  return nativeCall(() => native.publicKeyFromSs58(ss58Address))
}

export const ss58Decode = publicKeyFromSs58
export const decodeSs58 = publicKeyFromSs58
export const ss58_decode = publicKeyFromSs58
export const decode_ss58 = publicKeyFromSs58

export function ss58FromPublic(
  publicKey: ByteLike,
  ss58Format = DEFAULT_SS58_FORMAT,
): string {
  return nativeCall(() => native.ss58FromPublic(toBuffer(publicKey, 'publicKey'), ss58Format))
}

export const ss58Encode = ss58FromPublic
export const encodeSs58 = ss58FromPublic
export const ss58_encode = ss58FromPublic
export const encode_ss58 = ss58FromPublic

export function encryptFor(
  ss58Address: string,
  message: string | ByteLike,
  cryptoType = CRYPTO_ED25519,
): Buffer {
  return Keypair.encryptFor(ss58Address, message, cryptoType)
}

export const encrypt_for = encryptFor

export function serializeKeypair(keypair: Keypair): Buffer {
  return keypair.serialize()
}

export const serializedKeypairToKeyfileData = serializeKeypair
export const serialized_keypair_to_keyfile_data = serializedKeypairToKeyfileData

export function deserializeKeypair(keyfileData: ByteLike): Keypair {
  return Keypair.deserialize(keyfileData)
}

export const deserializeKeypairFromKeyfileData = deserializeKeypair
export const deserialize_keypair_from_keyfile_data = deserializeKeypairFromKeyfileData

export function keypairToKeyfileData(
  keypair: Keypair,
  password?: string | null,
): Promise<Buffer> {
  return keypair.toKeyfileData(password)
}

export const keypair_to_keyfile_data = keypairToKeyfileData

export function deserializeKeypairFromKeyfile(
  keyfileData: ByteLike,
  password?: string | null,
): Promise<Keypair> {
  return Keypair.fromKeyfileData(keyfileData, password)
}

export const deserialize_keypair_from_keyfile = deserializeKeypairFromKeyfile

export function readKeypairKeyfile(path: string, password?: string | null): Promise<Keypair> {
  return Keypair.fromKeyfile(path, password)
}

export const read_keypair_keyfile = readKeypairKeyfile

export function writeKeypairPairKeyfile(
  privateKeypair: Keypair,
  privatePath: string,
  publicKeypair: Keypair,
  publicPath: string,
  options: WriteKeyfileOptions = {},
): Promise<void> {
  return Keypair.writeKeyfilePair(privateKeypair, privatePath, publicKeypair, publicPath, options)
}

export const write_keypair_pair_keyfile = writeKeypairPairKeyfile

export function encryptKeyfileData(keyfileData: ByteLike, password: string): Promise<Buffer> {
  return nativeAsync(() =>
    native.encryptKeyfileData(toBuffer(keyfileData, 'keyfileData'), password),
  )
}

export const encrypt_keyfile_data = encryptKeyfileData

export function dangerouslyDecryptKeyfileData(
  keyfileData: ByteLike,
  password?: string | null,
): Promise<Buffer> {
  return nativeAsync(() =>
    native.decryptKeyfileData(toBuffer(keyfileData, 'keyfileData'), password ?? undefined),
  )
}

export const dangerously_decrypt_keyfile_data = dangerouslyDecryptKeyfileData

export function keyfileDataIsEncrypted(keyfileData: ByteLike): boolean {
  return native.keyfileDataIsEncrypted(toBuffer(keyfileData, 'keyfileData'))
}

export const keyfile_data_is_encrypted = keyfileDataIsEncrypted

export function keyfileDataIsEncryptedNacl(keyfileData: ByteLike): boolean {
  return native.keyfileDataIsEncryptedNacl(toBuffer(keyfileData, 'keyfileData'))
}

export const keyfile_data_is_encrypted_nacl = keyfileDataIsEncryptedNacl

export function keyfileDataIsEncryptedAnsible(keyfileData: ByteLike): boolean {
  return native.keyfileDataIsEncryptedAnsible(toBuffer(keyfileData, 'keyfileData'))
}

export const keyfile_data_is_encrypted_ansible = keyfileDataIsEncryptedAnsible

export function keyfileDataIsEncryptedLegacy(keyfileData: ByteLike): boolean {
  return native.keyfileDataIsEncryptedLegacy(toBuffer(keyfileData, 'keyfileData'))
}

export const keyfile_data_is_encrypted_legacy = keyfileDataIsEncryptedLegacy

export function keyfileDataEncryptionMethod(keyfileData: ByteLike): string {
  return native.keyfileDataEncryptionMethod(toBuffer(keyfileData, 'keyfileData'))
}

export const keyfile_data_encryption_method = keyfileDataEncryptionMethod

export function legacyGetPasswordFromEnvironment(name: string): string | null {
  return nativeCall(() => native.getPasswordFromEnvironment(name) ?? null)
}

export const legacy_get_password_from_environment = legacyGetPasswordFromEnvironment

export function legacySavePasswordToEnvironment(name: string, password: string): string {
  return nativeCall(() => native.savePasswordToEnvironment(name, password))
}

export const legacy_save_password_to_environment = legacySavePasswordToEnvironment

export const dangerousKeyfiles = Object.freeze({
  dangerouslyDecryptKeyfileData,
  dangerously_decrypt_keyfile_data,
  legacySavePasswordToEnvironment,
  legacy_save_password_to_environment,
  legacyGetPasswordFromEnvironment,
  legacy_get_password_from_environment,
})
