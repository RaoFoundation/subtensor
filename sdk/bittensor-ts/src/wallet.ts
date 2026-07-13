import {
  access,
} from 'node:fs/promises'
import { homedir } from 'node:os'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'

import {
  CRYPTO_SR25519,
  Keypair,
  writeKeypairPairKeyfile,
} from './keys'

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

export class Keyfile {
  readonly path: string
  readonly name: string

  constructor(path: string, name: string) {
    this.path = path
    this.name = name
  }

  async exists(): Promise<boolean> {
    try {
      await access(this.path)
      return true
    } catch {
      return false
    }
  }

  getKeypair(password?: string | null): Promise<Keypair> {
    return Keypair.fromKeyfile(this.path, password ?? undefined)
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
    mnemonic: string,
    options: RegenerateKeyOptions,
  ): Promise<this> {
    return this.setColdkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      options,
    )
  }

  async regenerateHotkey(
    mnemonic: string,
    options: RegenerateKeyOptions,
  ): Promise<this> {
    return this.setHotkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      options,
    )
  }
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
