import { randomBytes } from 'node:crypto'
import {
  chmodSync,
  closeSync,
  constants,
  existsSync,
  fsyncSync,
  linkSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, join } from 'node:path'

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
  keyfilePassword?: string | null
  /** @deprecated Use keyfilePassword. */
  password?: string | null
}

export interface RegenerateKeyOptions extends SaveKeyOptions {
  cryptoType?: number
  mnemonicPassword?: string | null
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
    const { encrypt = false, overwrite = false } = options
    const password = keyfilePassword(options)
    if (encrypt && password == null) {
      throw new Error(`Password is required to encrypt ${this.path}`)
    }
    validateKeyfileTarget(this.path, overwrite)
    ensurePrivateDirectory(dirname(this.path))
    const serialized = serializeKeypair(keypair)
    const data = encrypt ? encryptKeyfileData(serialized, password as string) : serialized
    atomicWriteKeyfile(this.path, data, overwrite)
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
    return this.setColdkey(keypair, { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null })
  }

  createNewHotkey(options: SaveKeyOptions & { nWords?: number; cryptoType?: number } = {}): this {
    const keypair = Keypair.generate(options.cryptoType ?? CRYPTO_SR25519, options.nWords ?? 12)
    return this.setHotkey(keypair, { ...options, encrypt: options.encrypt ?? keyfilePassword(options) != null })
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

function ensurePrivateDirectory(path: string): void {
  mkdirSync(path, { recursive: true, mode: 0o700 })
  const stat = lstatSync(path)
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw new Error(`Wallet path ${path} must be a real directory`)
  }
  chmodSync(path, 0o700)
}

function validateKeyfileTarget(path: string, overwrite: boolean): void {
  try {
    const stat = lstatSync(path)
    if (stat.isSymbolicLink()) throw new Error(`Refusing to write keyfile through symlink ${path}`)
    if (!stat.isFile()) throw new Error(`Refusing to overwrite non-file keyfile path ${path}`)
    if (!overwrite) throw new Error(`Keyfile ${path} already exists`)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return
    throw error
  }
}

function atomicWriteKeyfile(path: string, data: Uint8Array, overwrite: boolean): void {
  const dir = dirname(path)
  const temp = join(dir, `.${basename(path)}.${process.pid}.${randomBytes(8).toString('hex')}.tmp`)
  let fd: number | undefined
  try {
    fd = openSync(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o600)
    let offset = 0
    while (offset < data.length) {
      offset += writeSync(fd, data, offset, data.length - offset)
    }
    fsyncSync(fd)
    closeSync(fd)
    fd = undefined
    if (overwrite) renameSync(temp, path)
    else {
      linkSync(temp, path)
      unlinkSync(temp)
    }
    chmodSync(path, 0o600)
    fsyncDirectory(dir)
  } catch (error) {
    if (fd != null) {
      try {
        closeSync(fd)
      } catch {
        // Best effort cleanup.
      }
    }
    try {
      unlinkSync(temp)
    } catch {
      // Best effort cleanup.
    }
    throw error
  }
}

function fsyncDirectory(path: string): void {
  let fd: number | undefined
  try {
    fd = openSync(path, constants.O_RDONLY)
    fsyncSync(fd)
  } catch {
    // Directory fsync is best-effort across platforms and filesystems.
  } finally {
    if (fd != null) closeSync(fd)
  }
}
