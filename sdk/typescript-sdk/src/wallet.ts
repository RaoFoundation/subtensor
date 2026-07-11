import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'

import {
  CRYPTO_SR25519,
  Keypair,
  decryptKeyfileData,
  deserializeKeypair,
  encryptKeyfileData,
  keyfileDataIsEncrypted,
  serializeKeypair,
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
  password?: string | null
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
    const data = readFileSync(this.path)
    const decoded = keyfileDataIsEncrypted(data)
      ? decryptKeyfileData(data, password ?? undefined)
      : data
    return deserializeKeypair(decoded)
  }

  setKeypair(keypair: Keypair, options: SaveKeyOptions = {}): void {
    const { encrypt = false, overwrite = false, password } = options
    if (encrypt && password == null) {
      throw new Error(`Password is required to encrypt ${this.path}`)
    }
    if (this.exists() && !overwrite) {
      throw new Error(`Keyfile ${this.path} already exists`)
    }
    mkdirSync(dirname(this.path), { recursive: true })
    const serialized = serializeKeypair(keypair)
    const data = encrypt ? encryptKeyfileData(serialized, password as string) : serialized
    writeFileSync(this.path, data, { mode: 0o600 })
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
    this.coldkeyCache = keypair
    this.coldkeyFile.setKeypair(keypair, options)
    this.coldkeypubCache = publicOnly(keypair)
    this.coldkeypubFile.setKeypair(this.coldkeypubCache, { overwrite: options.overwrite ?? true })
    return this
  }

  setHotkey(keypair: Keypair, options: SaveKeyOptions = {}): this {
    this.hotkeyCache = keypair
    this.hotkeyFile.setKeypair(keypair, options)
    this.hotkeypubCache = publicOnly(keypair)
    this.hotkeypubFile.setKeypair(this.hotkeypubCache, { overwrite: options.overwrite ?? true })
    return this
  }

  createNewColdkey(options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {}): this {
    const keypair = Keypair.generate(options.cryptoType ?? CRYPTO_SR25519, options.nWords ?? 12)
    return this.setColdkey(keypair, { ...options, encrypt: options.encrypt ?? options.password != null })
  }

  createNewHotkey(options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {}): this {
    const keypair = Keypair.generate(options.cryptoType ?? CRYPTO_SR25519, options.nWords ?? 12)
    return this.setHotkey(keypair, { ...options, encrypt: options.encrypt ?? options.password != null })
  }

  regenerateColdkey(
    mnemonic: string,
    options: SaveKeyOptions & { cryptoType?: number; password?: string | null } = {},
  ): this {
    return this.setColdkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519),
      { ...options, encrypt: options.encrypt ?? options.password != null },
    )
  }

  regenerateHotkey(
    mnemonic: string,
    options: SaveKeyOptions & { cryptoType?: number; password?: string | null } = {},
  ): this {
    return this.setHotkey(
      Keypair.fromMnemonic(mnemonic, options.cryptoType ?? CRYPTO_SR25519),
      { ...options, encrypt: options.encrypt ?? options.password != null },
    )
  }
}

function publicOnly(keypair: Keypair): Keypair {
  return new Keypair(keypair.ss58Address, keypair.publicKey, keypair.cryptoType, keypair.ss58Format)
}
