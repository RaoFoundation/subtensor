import {
  access, open, readdir, readFile,
} from 'node:fs/promises'
import { constants as fsConstants } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'

import {
  CRYPTO_SR25519, CRYPTO_ED25519, DEFAULT_SS58_FORMAT,
  Keypair,
  keyfileDataIsEncrypted, legacyGetPasswordFromEnvironment, legacySavePasswordToEnvironment,
  writeKeypairPairKeyfile,
} from './keys'
import type { ByteLike } from './types'

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
  mnemonicPassword?: string | null
}

export type CreateWalletKeyOptions = GenerateWalletKeyOptions & PrivateKeySaveOptions

export type RegenerateKeyOptions = PrivateKeySaveOptions & {
  cryptoType?: number
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

export type WalletKeySource =
  | { mnemonic: string }
  | { seed: ByteLike }
  | { privateKey: string }
  | { encryptedJson: string; passphrase: string }

export interface PublicWalletKeyOptions {
  ss58Address?: string
  publicKey?: ByteLike
  cryptoType?: number
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
    this.hotkeypubCache ??= this.hotkeypubFile.getKeypair()
    return this.hotkeypubCache
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
    const password = await resolveWalletPassword(this, options, role)
    return role === 'coldkey' ? this.getColdkey(password) : this.getHotkey(password)
  }

  /** Drop cached handles so subsequent access must unlock the keyfiles again. */
  lock(): void {
    this.coldkeyCache = this.hotkeyCache = undefined
  }
}

async function keyFromSource(source: string | WalletKeySource, options: RegenerateKeyOptions): Promise<Keypair> {
  if (typeof source === 'string') return Keypair.fromMnemonic(source, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword)
  const present = ['mnemonic', 'seed', 'privateKey', 'encryptedJson'].filter(key => key in source)
  if (present.length !== 1) throw new TypeError('provide exactly one mnemonic, seed, privateKey, or encryptedJson')
  if ('mnemonic' in source) return Keypair.fromMnemonic(source.mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword)
  if ('seed' in source) return Keypair.fromSeed(source.seed, options.cryptoType ?? CRYPTO_SR25519)
  if ('privateKey' in source) return Keypair.fromPrivateKey(source.privateKey, options.cryptoType ?? CRYPTO_SR25519)
  if (!source.encryptedJson || !source.passphrase) throw new TypeError('encrypted JSON import requires data and passphrase')
  return Keypair.fromEncryptedJson(source.encryptedJson, source.passphrase)
}

function publicKeyFromOptions(options: PublicWalletKeyOptions): Keypair {
  if (options.ss58Address == null && options.publicKey == null) throw new TypeError('provide ss58Address or publicKey')
  return new Keypair(options.ss58Address, options.publicKey, options.cryptoType ?? CRYPTO_SR25519)
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

export function parseCryptoType(value: string | number): number {
  const text = String(value).trim().toLowerCase()
  if (text === 'ed25519' || text === '0') return CRYPTO_ED25519
  if (text === 'sr25519' || text === '1') return CRYPTO_SR25519
  throw new RangeError('crypto type must be ed25519 or sr25519')
}
export function formatCryptoType(value?: number | null): string {
  return value == null || value === CRYPTO_SR25519 ? 'sr25519' : value === CRYPTO_ED25519 ? 'ed25519' : `unknown(${value})`
}
export function isBittensorAddress(value: string): boolean {
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
  const mnemonic = Keypair.generateMnemonic(options.nWords ?? 12)
  const keypair = Keypair.fromMnemonic(
    mnemonic,
    options.cryptoType ?? CRYPTO_SR25519,
    options.mnemonicPassword,
  )
  return { keypair, mnemonic }
}

function publicOnly(keypair: Keypair): Keypair {
  return new Keypair(keypair.ss58Address, keypair.publicKey, keypair.cryptoType, keypair.ss58Format)
}
