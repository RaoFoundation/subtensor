import {
  existsSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

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

  exists(): boolean {
    return existsSync(this.path)
  }

  getKeypair(password?: string | null): Keypair {
    return Keypair.fromKeyfile(this.path, password ?? undefined)
  }

  setKeypair(keypair: Keypair, options: SaveKeyOptions = {}): void {
    const { encrypt = false, overwrite = false } = options
    const password = keyfilePassword(options)
    if (encrypt && password == null) {
      throw new Error(`Password is required to encrypt ${this.path}`)
    }
    if (!encrypt && keypair.kind !== 'PublicOnly' && options.allowPlaintext !== true) {
      throw new Error(`Refusing to write plaintext private keyfile ${this.path}; pass allowPlaintext: true or provide keyfilePassword`)
    }
    keypair.writeKeyfile(this.path, encrypt ? password : undefined, overwrite)
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

  private coldkeyCache?: Keypair
  private coldkeypubCache?: Keypair
  private hotkeyCache?: Keypair
  private hotkeypubCache?: Keypair

  constructor(options: WalletOptions = {}) {
    this.name = options.name ?? 'default'
    this.hotkeyName = options.hotkey ?? 'default'
    this.path = options.path ?? DEFAULT_WALLET_PATH
    const walletDir = join(this.path, this.name)
    this.coldkeyFile = new Keyfile(join(walletDir, 'coldkey'), 'coldkey')
    this.coldkeypubFile = new Keyfile(join(walletDir, 'coldkeypub.txt'), 'coldkeypub.txt')
    this.hotkeyFile = new Keyfile(join(walletDir, 'hotkeys', this.hotkeyName), this.hotkeyName)
    this.hotkeypubFile = new Keyfile(
      join(walletDir, 'hotkeys', `${this.hotkeyName}pub.txt`),
      `${this.hotkeyName}pub.txt`,
    )
  }

  get coldkey(): Keypair {
    this.coldkeyCache ??= this.coldkeyFile.getKeypair()
    return this.coldkeyCache
  }

  get coldkeypub(): Keypair {
    this.coldkeypubCache ??= this.coldkeypubFile.getKeypair()
    return this.coldkeypubCache
  }

  get hotkey(): Keypair {
    this.hotkeyCache ??= this.hotkeyFile.getKeypair()
    return this.hotkeyCache
  }

  get hotkeypub(): Keypair {
    this.hotkeypubCache ??= this.hotkeypubFile.getKeypair()
    return this.hotkeypubCache
  }

  getColdkey(password?: string | null): Keypair {
    this.coldkeyCache = this.coldkeyFile.getKeypair(password)
    return this.coldkeyCache
  }

  getHotkey(password?: string | null): Keypair {
    this.hotkeyCache = this.hotkeyFile.getKeypair(password)
    return this.hotkeyCache
  }

  setColdkey(keypair: Keypair, options: SaveKeyOptions = {}): this {
    const coldkeypub = publicOnly(keypair)
    this.coldkeyFile.setKeypair(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    this.coldkeypubFile.setKeypair(coldkeypub, { overwrite: options.overwrite ?? true })
    this.coldkeyCache = keypair
    this.coldkeypubCache = coldkeypub
    return this
  }

  setHotkey(keypair: Keypair, options: SaveKeyOptions = {}): this {
    const hotkeypub = publicOnly(keypair)
    this.hotkeyFile.setKeypair(keypair, {
      ...options,
      encrypt: options.encrypt ?? keyfilePassword(options) != null,
    })
    this.hotkeypubFile.setKeypair(hotkeypub, { overwrite: options.overwrite ?? true })
    this.hotkeyCache = keypair
    this.hotkeypubCache = hotkeypub
    return this
  }

  createNewColdkey(options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {}): CreatedWalletKey {
    const mnemonic = Keypair.generateMnemonic(options.nWords ?? 12)
    const keypair = Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519)
    this.setColdkey(keypair, { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null })
    return { wallet: this, keypair, mnemonic }
  }

  createNewHotkey(options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {}): CreatedWalletKey {
    const mnemonic = Keypair.generateMnemonic(options.nWords ?? 12)
    const keypair = Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519)
    this.setHotkey(keypair, { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null })
    return { wallet: this, keypair, mnemonic }
  }

  regenerateColdkey(
    mnemonic: string,
    options: RegenerateKeyOptions = {},
  ): this {
    return this.setColdkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null },
    )
  }

  regenerateHotkey(
    mnemonic: string,
    options: RegenerateKeyOptions = {},
  ): this {
    return this.setHotkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519, options.mnemonicPassword),
      { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null },
    )
  }
}

function keyfilePassword(options: SaveKeyOptions): string | null | undefined {
  return options.keyfilePassword ?? options.password
}

function publicOnly(keypair: Keypair): Keypair {
  return new Keypair(keypair.ss58Address, keypair.publicKey, keypair.cryptoType, keypair.ss58Format)
}
