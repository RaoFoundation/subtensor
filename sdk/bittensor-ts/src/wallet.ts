import {
  access, lstat, open, readdir, readFile,
} from 'node:fs/promises'
import { constants as fsConstants } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'

import {
  CRYPTO_SR25519, CRYPTO_ED25519, DEFAULT_SS58_FORMAT,
  Keypair, decodeReceivingAddress, publicKeyFromSs58,
  keyfileDataIsEncrypted, legacyGetPasswordFromEnvironment, legacySavePasswordToEnvironment,
  writeKeypairPairKeyfile,
} from './keys'
import type { ByteLike } from './types'
import { toBuffer } from './wire'
import { WALLET_TYPES, accountCryptoType, isPqCrypto, isRotatingCrypto, u64 } from './wallet-types'
import { snapshotWalletIdentity, requireWalletIdentity, type WalletRegistrationOptions, type WalletRegistrationResult } from './wallet-registration'
import type { Balance } from './balance'

export const DEFAULT_WALLET_PATH = join(homedir(), '.bittensor', 'wallets')

export interface WalletOptions {
  name?: string
  hotkey?: string
  path?: string
}

export interface SaveKeyOptions {
  encrypt?: boolean
  overwrite?: boolean
  allowPlaintext?: boolean
  keyfilePassword?: string | null
  /** @deprecated Use keyfilePassword. */
  password?: string | null
}

export type PrivateKeySaveOptions =
  | {
      keyfilePassword: string
      allowPlaintext?: false
      overwrite?: boolean
      encrypt?: true
      password?: never
    }
  | {
      allowPlaintext: true
      keyfilePassword?: never
      overwrite?: boolean
      encrypt?: false
      password?: never
    }

export interface GenerateWalletKeyOptions {
  nWords?: number
  cryptoType?: number
  accountType?: 'standard' | 'hashed'
  mnemonicPassword?: string | null
}

export type CreateWalletKeyOptions = GenerateWalletKeyOptions & PrivateKeySaveOptions

export type RegenerateKeyOptions = PrivateKeySaveOptions & {
  cryptoType?: number
  accountType?: 'standard' | 'hashed'
  mnemonicPassword?: string | null
}

export interface GeneratedWalletKey {
  keypair: Keypair
  mnemonic: string
}

export interface CreatedWalletKey {
  wallet: Wallet
  keypair: Keypair
  mnemonic: string
}

export interface CreateWalletOptions {
  coldkey: CreateWalletKeyOptions
  hotkey: CreateWalletKeyOptions
  /** One overwrite decision applies to all four files. */
  overwrite?: boolean
}
export interface CreatedWallet {
  wallet: Wallet
  coldkey: GeneratedWalletKey
  hotkey: GeneratedWalletKey
}
export interface WalletRegistrationClient {
  registerWallet(wallet: unknown, options?: WalletRegistrationOptions & WalletPasswordOptions): Promise<WalletRegistrationResult>
  registrationDepositForTypes(types: readonly number[]): Promise<Balance>
}

export type WalletKeySource =
  | { mnemonic: string }
  | { seed: ByteLike }
  | { privateKey: string }
  | { encryptedJson: string; passphrase: string }

export interface PublicWalletKeyOptions {
  descriptor?: ByteLike
  ss58Address?: string
  publicKey?: ByteLike
  cryptoType?: number
  accountType?: 'standard' | 'hashed'
  overwrite?: boolean
}

export interface WalletPasswordOptions {
  password?: string | null
  passwordFile?: string
  /** Optional application-owned keychain or UI callback; the SDK never prompts implicitly. */
  passwordProvider?: (wallet: Wallet, role: 'coldkey' | 'hotkey') => Promise<string | null>
}

export class Keyfile {
  readonly path: string
  readonly name: string

  constructor(path: string, name = basename(path)) {
    this.path = path
    this.name = name
  }

  envVarName(): string { return `BT_PW_${this.path.replaceAll(sep, '_').replaceAll('.', '_').toUpperCase()}` }

  async isReadable(): Promise<boolean> {
    try { await access(this.path, fsConstants.R_OK); return true } catch { return false }
  }

  async isWritable(): Promise<boolean> {
    try { await access(await this.exists() ? this.path : dirname(this.path), fsConstants.W_OK); return true } catch { return false }
  }

  async isEncrypted(): Promise<boolean> {
    let handle
    try {
      handle = await open(this.path, 'r')
      const header = Buffer.alloc(16)
      const { bytesRead } = await handle.read(header, 0, header.length, 0)
      return keyfileDataIsEncrypted(header.subarray(0, bytesRead))
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false
      throw error
    } finally { await handle?.close() }
  }

  savePasswordToEnv(password: string): string {
    if (!password) throw new Error('password must not be empty')
    return legacySavePasswordToEnvironment(this.envVarName(), password)
  }

  removePasswordFromEnv(): void { delete process.env[this.envVarName()] }

  async exists(): Promise<boolean> {
    try {
      await access(this.path)
      return true
    } catch {
      return false
    }
  }

  getKeypair(password?: string | null): Promise<Keypair> {
    return Keypair.fromKeyfile(this.path, password ?? legacyGetPasswordFromEnvironment(this.envVarName()) ?? undefined)
  }

  async setKeypair(keypair: Keypair, options: SaveKeyOptions = {}): Promise<void> {
    const password = keyfilePassword(options)
    const encrypt = options.encrypt ?? password != null
    const overwrite = options.overwrite ?? false
    if (encrypt && password == null) {
      throw new Error(`Password is required to encrypt ${this.path}`)
    }
    if (!encrypt && keypair.kind !== 'PublicOnly' && options.allowPlaintext !== true) {
      throw new Error(
        `Refusing to write plaintext private keyfile ${this.path}; pass allowPlaintext: true or provide keyfilePassword`,
      )
    }
    await keypair.writeKeyfile(this.path, {
      password: encrypt ? password : undefined,
      overwrite,
      allowPlaintext: options.allowPlaintext === true,
    })
  }
}

export class Wallet {
  readonly name: string
  readonly hotkeyName: string
  readonly path: string
  readonly coldkeyFile: Keyfile
  readonly coldkeypubFile: Keyfile
  readonly hotkeyFile: Keyfile
  readonly hotkeypubFile: Keyfile

  private coldkeyCache?: Promise<Keypair>
  private coldkeypubCache?: Promise<Keypair>
  private hotkeyCache?: Promise<Keypair>
  private hotkeypubCache?: Promise<Keypair>

  constructor(options: WalletOptions = {}) {
    this.name = validateWalletComponent(options.name ?? 'default', 'wallet name')
    this.hotkeyName = validateWalletComponent(options.hotkey ?? 'default', 'hotkey name')
    this.path = resolve(options.path ?? DEFAULT_WALLET_PATH)
    containedPath(this.path, this.name, 'wallet directory')
    this.coldkeyFile = new Keyfile(
      containedPath(this.path, this.name, 'coldkey', 'coldkey'),
      'coldkey',
    )
    this.coldkeypubFile = new Keyfile(
      containedPath(this.path, this.name, 'coldkeypub.txt', 'coldkeypub.txt'),
      'coldkeypub.txt',
    )
    this.hotkeyFile = new Keyfile(
      containedPath(this.path, this.name, 'hotkeys', this.hotkeyName, 'hotkey'),
      this.hotkeyName,
    )
    this.hotkeypubFile = new Keyfile(
      containedPath(
        this.path,
        this.name,
        'hotkeys',
        `${this.hotkeyName}pub.txt`,
        'hotkey public key',
      ),
      `${this.hotkeyName}pub.txt`,
    )
  }

  get coldkey(): Promise<Keypair> {
    this.coldkeyCache ??= this.coldkeyFile.getKeypair()
    return this.coldkeyCache
  }

  get coldkeypub(): Promise<Keypair> {
    this.coldkeypubCache ??= this.coldkeypubFile.getKeypair()
    return this.coldkeypubCache
  }

  get hotkey(): Promise<Keypair> {
    this.hotkeyCache ??= this.hotkeyFile.getKeypair()
    return this.hotkeyCache
  }

  get hotkeypub(): Promise<Keypair> {
    return this.publicHotkey()
  }

  private publicHotkey(legacyPassword?: () => Promise<string | null>): Promise<Keypair> {
    if (this.hotkeypubCache) return this.hotkeypubCache
    const pending = (async () => {
      try { await lstat(this.hotkeypubFile.path) }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
        // Legacy wallets may have only a plaintext hotkey. Snapshot its public
        // identity before invoking any application password provider. An encrypted
        // legacy file needs a password already supplied by the caller/environment.
        return (await this.hotkeyFile.getKeypair(await legacyPassword?.())).publicOnly()
      }
      return this.hotkeypubFile.getKeypair()
    })()
    this.hotkeypubCache = pending
    void pending.catch(() => { if (this.hotkeypubCache === pending) this.hotkeypubCache = undefined })
    return pending
  }

  getColdkey(password?: string | null): Promise<Keypair> {
    this.coldkeyCache = this.coldkeyFile.getKeypair(password)
    return this.coldkeyCache
  }

  getHotkey(password?: string | null): Promise<Keypair> {
    this.hotkeyCache = this.hotkeyFile.getKeypair(password)
    return this.hotkeyCache
  }

  async setColdkey(keypair: Keypair, options: PrivateKeySaveOptions): Promise<this> {
    const saveOptions = requirePrivateKeySaveOptions(options, 'setColdkey')
    const coldkeypub = publicOnly(keypair)
    await writeKeypairPairKeyfile(keypair, this.coldkeyFile.path, coldkeypub, this.coldkeypubFile.path, {
      password: keyfilePassword(saveOptions) ?? undefined,
      overwrite: saveOptions.overwrite ?? false,
      allowPlaintext: saveOptions.allowPlaintext === true,
    })
    this.coldkeyCache = Promise.resolve(keypair)
    this.coldkeypubCache = Promise.resolve(coldkeypub)
    return this
  }

  async setHotkey(keypair: Keypair, options: PrivateKeySaveOptions): Promise<this> {
    const saveOptions = requirePrivateKeySaveOptions(options, 'setHotkey')
    const hotkeypub = publicOnly(keypair)
    await writeKeypairPairKeyfile(keypair, this.hotkeyFile.path, hotkeypub, this.hotkeypubFile.path, {
      password: keyfilePassword(saveOptions) ?? undefined,
      overwrite: saveOptions.overwrite ?? false,
      allowPlaintext: saveOptions.allowPlaintext === true,
    })
    this.hotkeyCache = Promise.resolve(keypair)
    this.hotkeypubCache = Promise.resolve(hotkeypub)
    return this
  }

  static generateColdkey(options: GenerateWalletKeyOptions = {}): GeneratedWalletKey {
    return generateWalletKey(options)
  }

  static generateHotkey(options: GenerateWalletKeyOptions = {}): GeneratedWalletKey {
    return generateWalletKey(options)
  }

  generateColdkey(options: GenerateWalletKeyOptions = {}): GeneratedWalletKey {
    return Wallet.generateColdkey(options)
  }

  generateHotkey(options: GenerateWalletKeyOptions = {}): GeneratedWalletKey {
    return Wallet.generateHotkey(options)
  }

  async createNewColdkey(options: CreateWalletKeyOptions): Promise<CreatedWalletKey> {
    const generated = Wallet.generateColdkey(options)
    await this.setColdkey(generated.keypair, options)
    return { wallet: this, ...generated }
  }

  async createNewHotkey(options: CreateWalletKeyOptions): Promise<CreatedWalletKey> {
    const generated = Wallet.generateHotkey(options)
    await this.setHotkey(generated.keypair, options)
    return { wallet: this, ...generated }
  }

  /** Generate both roles, then atomically publish all four keyfiles. */
  async create(options: CreateWalletOptions): Promise<CreatedWallet> {
    const coldOptions = requirePrivateKeySaveOptions(options.coldkey, 'create coldkey')
    const hotOptions = requirePrivateKeySaveOptions(options.hotkey, 'create hotkey')
    const overwrite = options.overwrite ?? false
    if ((coldOptions.overwrite != null && coldOptions.overwrite !== overwrite) || (hotOptions.overwrite != null && hotOptions.overwrite !== overwrite)) throw new Error('set overwrite once for the complete wallet')
    const coldkey = Wallet.generateColdkey(options.coldkey), hotkey = Wallet.generateHotkey(options.hotkey)
    const coldPublic = publicOnly(coldkey.keypair), hotPublic = publicOnly(hotkey.keypair)
    await Keypair.writeWalletKeyfiles(coldkey.keypair, coldPublic, hotkey.keypair, hotPublic,
      [this.coldkeyFile.path, this.coldkeypubFile.path, this.hotkeyFile.path, this.hotkeypubFile.path],
      { password: keyfilePassword(coldOptions), allowPlaintext: coldOptions.allowPlaintext },
      { password: keyfilePassword(hotOptions), allowPlaintext: hotOptions.allowPlaintext }, overwrite)
    this.coldkeyCache = Promise.resolve(coldkey.keypair); this.coldkeypubCache = Promise.resolve(coldPublic)
    this.hotkeyCache = Promise.resolve(hotkey.keypair); this.hotkeypubCache = Promise.resolve(hotPublic)
    return { wallet: this, coldkey, hotkey }
  }

  register(client: WalletRegistrationClient, options: WalletRegistrationOptions & WalletPasswordOptions = {}): Promise<WalletRegistrationResult> {
    return client.registerWallet(this, options)
  }

  /** Registration failure preserves the newly persisted keys for a later retry. */
  async createAndRegister(client: WalletRegistrationClient, options: CreateWalletOptions, registrationOptions: WalletRegistrationOptions & WalletPasswordOptions = {}): Promise<CreatedWallet & { registration: WalletRegistrationResult }> {
    const roles = registrationOptions.roles ?? ['coldkey', 'hotkey']
    if (!Array.isArray(roles) || roles.some(role => role !== 'coldkey' && role !== 'hotkey')) throw new TypeError('roles must contain coldkey and/or hotkey')
    const types = [...new Set<'coldkey' | 'hotkey'>(roles)].map(role => {
      const key = options[role]
      return accountCryptoType(key.cryptoType ?? CRYPTO_SR25519, key.accountType)
    })
    if (types.some(isRotatingCrypto)) {
      if (registrationOptions.payer == null) throw new Error('hashed wallet creation and registration requires an existing funded payer')
      if (registrationOptions.maxRegistrationDepositRao == null && !registrationOptions.onRegistrationSponsorship) throw new Error('wallet registration requires reserve approval')
      const reserve = await client.registrationDepositForTypes(types)
      if (registrationOptions.maxRegistrationDepositRao != null && reserve.rao > u64(registrationOptions.maxRegistrationDepositRao)) throw new Error('wallet registration reserve exceeds the approved maximum')
    }
    const created = await this.create(options)
    return { ...created, registration: await this.register(client, registrationOptions) }
  }

  async regenerateColdkey(
    source: string | WalletKeySource,
    options: RegenerateKeyOptions,
  ): Promise<this> {
    return this.setColdkey(await keyFromSource(source, options), options)
  }

  async regenerateHotkey(
    source: string | WalletKeySource,
    options: RegenerateKeyOptions,
  ): Promise<this> {
    return this.setHotkey(await keyFromSource(source, options), options)
  }

  async regenerateColdkeypub(options: PublicWalletKeyOptions): Promise<this> {
    const key = publicKeyFromOptions(options)
    await this.coldkeypubFile.setKeypair(key, { encrypt: false, overwrite: options.overwrite })
    this.coldkeypubCache = Promise.resolve(key)
    return this
  }

  async regenerateHotkeypub(options: PublicWalletKeyOptions): Promise<this> {
    const key = publicKeyFromOptions(options)
    await this.hotkeypubFile.setKeypair(key, { encrypt: false, overwrite: options.overwrite })
    this.hotkeypubCache = Promise.resolve(key)
    return this
  }

  unlockColdkey(password?: string | null): Promise<Keypair> { return password == null ? this.coldkey : this.getColdkey(password) }
  unlockHotkey(password?: string | null): Promise<Keypair> { return password == null ? this.hotkey : this.getHotkey(password) }

  async signer(role: 'coldkey' | 'hotkey', options: WalletPasswordOptions = {}): Promise<Keypair> {
    if (role !== 'coldkey' && role !== 'hotkey') throw new TypeError('signer must be coldkey or hotkey')
    if (role === 'hotkey' && !this.hotkeypubCache) {
      let companionMissing = false
      try { await lstat(this.hotkeypubFile.path) }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
        companionMissing = true
      }
      if (companionMissing) {
        const captured = await readOriginalKeyfile(this.hotkeyFile.path)
        try {
          if (keyfileDataIsEncrypted(captured)) {
            // A legacy encrypted hotkey has no public identity until unlocked.
            // Pin its original ciphertext before the provider runs, then decrypt
            // those bytes rather than a path the provider could have replaced.
            const password = await resolveWalletPassword(this, options, role)
            const key = await Keypair.fromKeyfileData(captured, password)
            this.hotkeyCache = Promise.resolve(key)
            this.hotkeypubCache = Promise.resolve(key.publicOnly())
            return key
          }
        } finally { captured.fill(0) }
      }
    }
    const expected = snapshotWalletIdentity(await (role === 'coldkey' ? this.coldkeypub : this.publicHotkey(() => resolveWalletPassword(this, { ...options, passwordProvider: undefined }, 'hotkey'))))
    const password = await resolveWalletPassword(this, options, role)
    const key = await (role === 'coldkey' ? this.getColdkey(password) : this.getHotkey(password))
    requireWalletIdentity(key, expected, publicKeyFromSs58)
    return key
  }

  /** Drop cached handles so subsequent access must unlock the keyfiles again. */
  lock(): void {
    this.coldkeyCache = this.hotkeyCache = undefined
  }
}

/** Capture one regular keyfile without following a symlink or accepting an inode swap. */
async function readOriginalKeyfile(path: string): Promise<Buffer> {
  const before = await lstat(path, { bigint: true })
  if (!before.isFile() || before.isSymbolicLink()) throw new Error('wallet keyfile must be a regular file, not a symlink')
  const handle = await open(path, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0))
  try {
    const opened = await handle.stat({ bigint: true })
    if (!opened.isFile() || opened.dev !== before.dev || opened.ino !== before.ino) throw new Error('wallet keyfile changed before reading')
    const data = await handle.readFile()
    const after = await handle.stat({ bigint: true })
    if (after.size !== opened.size || after.mtimeNs !== opened.mtimeNs || after.ctimeNs !== opened.ctimeNs) {
      data.fill(0)
      throw new Error('wallet keyfile changed while reading')
    }
    return data
  } finally { await handle.close() }
}

async function keyFromSource(source: string | WalletKeySource, options: RegenerateKeyOptions): Promise<Keypair> {
  const cryptoType = accountCryptoType(options.cryptoType ?? CRYPTO_SR25519, options.accountType)
  if (typeof source === 'string') return Keypair.fromMnemonic(source, cryptoType, options.mnemonicPassword)
  const present = ['mnemonic', 'seed', 'privateKey', 'encryptedJson'].filter(key => key in source)
  if (present.length !== 1) throw new TypeError('provide exactly one mnemonic, seed, privateKey, or encryptedJson')
  if ('mnemonic' in source) return Keypair.fromMnemonic(source.mnemonic, cryptoType, options.mnemonicPassword)
  if ('seed' in source) return Keypair.fromSeed(source.seed, cryptoType)
  if ('privateKey' in source) return Keypair.fromPrivateKey(source.privateKey, cryptoType)
  if (!source.encryptedJson || !source.passphrase) throw new TypeError('encrypted JSON import requires data and passphrase')
  const key = await Keypair.fromEncryptedJson(source.encryptedJson, source.passphrase)
  if ((options.cryptoType != null || options.accountType != null) && key.cryptoType !== accountCryptoType(options.cryptoType ?? key.cryptoType, options.accountType)) throw new Error('imported key identity differs from the requested crypto type or account mode')
  return key
}

function publicKeyFromOptions(options: PublicWalletKeyOptions): Keypair {
  const receiving = options.ss58Address?.startsWith('tao_') || options.ss58Address?.startsWith('bth2_')
  if (options.descriptor != null || receiving) {
    const descriptor = options.descriptor == null ? decodeReceivingAddress(options.ss58Address!) : toBuffer(options.descriptor)
    if (receiving && !descriptor.equals(decodeReceivingAddress(options.ss58Address!))) throw new Error('receiving address and descriptor differ')
    const key = Keypair.fromDescriptor(descriptor)
    if ((options.cryptoType != null || options.accountType != null) && key.cryptoType !== accountCryptoType(options.cryptoType ?? key.cryptoType, options.accountType) || options.publicKey != null && !key.publicKey.equals(toBuffer(options.publicKey)) || options.ss58Address && !receiving && !key.publicKey.equals(publicKeyFromSs58(options.ss58Address))) throw new Error('public wallet identity differs from descriptor')
    return key
  }
  if (options.ss58Address == null && options.publicKey == null) throw new TypeError('provide ss58Address or publicKey')
  return new Keypair(options.ss58Address, options.publicKey, accountCryptoType(options.cryptoType ?? CRYPTO_SR25519, options.accountType))
}

export async function resolveWalletPassword(wallet: Wallet, options: WalletPasswordOptions = {}, role: 'coldkey' | 'hotkey' = 'coldkey'): Promise<string | null> {
  if (options.password) return options.password
  if (process.env.BT_WALLET_PASSWORD) return process.env.BT_WALLET_PASSWORD
  const keyfile = role === 'coldkey' ? wallet.coldkeyFile : wallet.hotkeyFile
  const saved = legacyGetPasswordFromEnvironment(keyfile.envVarName())
  if (saved) return saved
  const file = options.passwordFile ?? process.env.BT_WALLET_PASSWORD_FILE
  if (file) {
    const password = (await readFile(file.startsWith('~/') ? join(homedir(), file.slice(2)) : file, 'utf8')).trim()
    if (!password) throw new Error('wallet password file is empty')
    return password
  }
  return options.passwordProvider ? options.passwordProvider(wallet, role) : null
}

export function parseCryptoType(value: string | number, accountType?: 'standard' | 'hashed'): number {
  const text = String(value).trim().toLowerCase()
  const found = Object.entries(WALLET_TYPES).find(([name,code]) => text === name || text === String(code))
  if (found) return accountCryptoType(found[1], accountType)
  throw new RangeError('unsupported wallet crypto type')
}
export function formatCryptoType(value?: number | null): string {
  return value == null ? 'sr25519' : Object.entries(WALLET_TYPES).find(([,code]) => code === value)?.[0] ?? `unknown(${value})`
}
export function isBittensorAddress(value: string): boolean {
  if (value.startsWith('tao_') || value.startsWith('bth2_')) { try { decodeReceivingAddress(value); return true } catch { return false } }
  try { const key = new Keypair(value); return new Keypair(null, key.publicKey, key.cryptoType, DEFAULT_SS58_FORMAT).ss58Address === value } catch { return false }
}

function naturalCompare(a: string, b: string): number {
  const left = a.toLowerCase().match(/\d+|\D+/g) ?? [], right = b.toLowerCase().match(/\d+|\D+/g) ?? []
  for (let i = 0; i < Math.min(left.length, right.length); i++) {
    const numericA = /^\d+$/.test(left[i]), numericB = /^\d+$/.test(right[i])
    if (numericA !== numericB) return numericA ? -1 : 1
    if (numericA) { const x = BigInt(left[i]), y = BigInt(right[i]); if (x !== y) return x < y ? -1 : 1 }
    else if (left[i] !== right[i]) return left[i] < right[i] ? -1 : 1
  }
  return left.length - right.length
}

export async function listWallets(path = DEFAULT_WALLET_PATH): Promise<Record<string, string[]>> {
  const result: Record<string, string[]> = Object.create(null)
  let directories
  try { directories = await readdir(path, { withFileTypes: true }) } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return result
    throw error
  }
  for (const entry of directories.filter(entry => entry.isDirectory()).sort((a, b) => naturalCompare(a.name, b.name))) {
    let hotkeys: string[] = []
    try { hotkeys = (await readdir(join(path, entry.name, 'hotkeys'), { withFileTypes: true }))
      .filter(key => key.isFile() && !key.name.endsWith('.lock') && !key.name.endsWith('pub.txt'))
      .map(key => key.name).sort(naturalCompare)
    } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
    result[entry.name] = hotkeys
  }
  return result
}

export interface WalletKeyInfo { name: string; ss58: string | null; cryptoType: number | null }
export interface WalletInfo extends WalletKeyInfo { hotkeys: WalletKeyInfo[] }
export async function listWalletsDetailed(path = DEFAULT_WALLET_PATH): Promise<WalletInfo[]> {
  const wallets = await listWallets(path)
  const info = async (file: Keyfile): Promise<{ ss58: string | null; cryptoType: number | null }> => {
    try { const key = await file.getKeypair(); return { ss58: key.ss58Address, cryptoType: key.cryptoType } } catch { return { ss58: null, cryptoType: null } }
  }
  return Promise.all(Object.entries(wallets).map(async ([name, hotkeys]) => {
    const wallet = new Wallet({ name, path })
    return { name, ...await info(wallet.coldkeypubFile), hotkeys: await Promise.all(hotkeys.map(async hotkey => {
      const pair = new Wallet({ name, path, hotkey })
      return { name: hotkey, ...await info(await pair.hotkeypubFile.exists() ? pair.hotkeypubFile : pair.hotkeyFile) }
    })) }
  }))
}

function validateWalletComponent(value: string, label: string): string {
  if (
    value.length === 0 ||
    value === '.' ||
    value === '..' ||
    value.includes('/') ||
    value.includes('\\') ||
    value.includes('\0') ||
    isAbsolute(value)
  ) {
    throw new Error(`${label} must be a single path component`)
  }
  return value
}

function containedPath(root: string, ...partsAndLabel: string[]): string {
  const label = partsAndLabel.pop() ?? 'wallet path'
  const path = resolve(root, ...partsAndLabel)
  const relativePath = relative(root, path)
  if (
    relativePath === '' ||
    relativePath === '..' ||
    relativePath.startsWith(`..${sep}`) ||
    isAbsolute(relativePath)
  ) {
    throw new Error(`${label} escapes wallet root`)
  }
  return path
}

function keyfilePassword(options: SaveKeyOptions): string | null | undefined {
  return options.keyfilePassword ?? options.password
}

function requirePrivateKeySaveOptions(
  options: SaveKeyOptions | undefined,
  operation: string,
): SaveKeyOptions {
  if (options == null) {
    throw new Error(`${operation} requires keyfilePassword or allowPlaintext: true`)
  }
  const password = keyfilePassword(options)
  if (password === '') {
    throw new Error(`${operation} requires a non-empty keyfilePassword`)
  }
  if (password != null && options.allowPlaintext === true) {
    throw new Error(`${operation} accepts either keyfilePassword or allowPlaintext: true, not both`)
  }
  if (password == null && options.allowPlaintext !== true) {
    throw new Error(`${operation} requires keyfilePassword or allowPlaintext: true`)
  }
  return options
}

function generateWalletKey(options: GenerateWalletKeyOptions = {}): GeneratedWalletKey {
  const cryptoType = accountCryptoType(options.cryptoType ?? CRYPTO_SR25519, options.accountType)
  const mnemonic = Keypair.generateMnemonic(options.nWords ?? (isPqCrypto(cryptoType) ? 24 : 12))
  const keypair = Keypair.fromMnemonic(
    mnemonic,
    cryptoType,
    options.mnemonicPassword,
  )
  return { keypair, mnemonic }
}

function publicOnly(keypair: Keypair): Keypair {
  return keypair.publicOnly()
}
