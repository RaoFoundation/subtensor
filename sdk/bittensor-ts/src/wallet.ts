import {
  access,
} from 'node:fs/promises'
import { homedir } from 'node:os'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'

import {
  CRYPTO_SR25519,
  Keypair,
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

export interface RegenerateKeyOptions extends SaveKeyOptions {
  cryptoType?: number
  mnemonicPassword?: string | null
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
    const { encrypt = false, overwrite = false } = options
    const password = keyfilePassword(options)
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

  async setColdkey(keypair: Keypair, options: SaveKeyOptions = {}): Promise<this> {
    const coldkeypub = publicOnly(keypair)
    await this.coldkeyFile.setKeypair(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    await this.coldkeypubFile.setKeypair(coldkeypub, { overwrite: options.overwrite ?? true })
    this.coldkeyCache = Promise.resolve(keypair)
    this.coldkeypubCache = Promise.resolve(coldkeypub)
    return this
  }

  async setHotkey(keypair: Keypair, options: SaveKeyOptions = {}): Promise<this> {
    const hotkeypub = publicOnly(keypair)
    await this.hotkeyFile.setKeypair(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    await this.hotkeypubFile.setKeypair(hotkeypub, { overwrite: options.overwrite ?? true })
    this.hotkeyCache = Promise.resolve(keypair)
    this.hotkeypubCache = Promise.resolve(hotkeypub)
    return this
  }

  async createNewColdkey(
    options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {},
  ): Promise<CreatedWalletKey> {
    const mnemonic = Keypair.generateMnemonic(options.nWords ?? 12)
    const keypair = Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519)
    await this.setColdkey(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    return { wallet: this, keypair, mnemonic }
  }

  async createNewHotkey(
    options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {},
  ): Promise<CreatedWalletKey> {
    const mnemonic = Keypair.generateMnemonic(options.nWords ?? 12)
    const keypair = Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519)
    await this.setHotkey(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    return { wallet: this, keypair, mnemonic }
  }

  async regenerateColdkey(
    mnemonic: string,
    options: RegenerateKeyOptions = {},
  ): Promise<this> {
    return this.setColdkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      {
        ...options,
        encrypt: options.encrypt ?? keyfilePassword(options) != null,
      },
    )
  }

  async regenerateHotkey(
    mnemonic: string,
    options: RegenerateKeyOptions = {},
  ): Promise<this> {
    return this.setHotkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      {
        ...options,
        encrypt: options.encrypt ?? keyfilePassword(options) != null,
      },
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

function publicOnly(keypair: Keypair): Keypair {
  return new Keypair(keypair.ss58Address, keypair.publicKey, keypair.cryptoType, keypair.ss58Format)
}
