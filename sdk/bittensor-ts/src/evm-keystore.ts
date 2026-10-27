/** Node-only Ethereum V3 keystores; private EVM keys are always encrypted on disk. */
import { constants } from 'node:fs'
import { link, lstat, mkdir, open, readFile, readdir, rename, unlink } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'
import { basename, dirname, join, parse, resolve, sep } from 'node:path'
import { HDNodeWallet, Wallet as EthereumWallet } from 'ethers'
import { blake2_256 } from './crypto'
import { ss58FromPublic } from './keys'
import { Wallet, resolveWalletPassword, type WalletPasswordOptions } from './wallet'
import { ETH_DERIVATION_PATH, normalizeH160, type EvmAccount } from './evm'
import { concatBytes, hexBytes } from './portable'

export interface EvmKeyInfo { name: string; address: string; ss58Mirror: string; path: string }
export type EvmKeySource =
  | { privateKey: string }
  | { mnemonic: string; derivationPath?: string; mnemonicPassword?: string }
  | { keystoreJson: string; keystorePassword?: string }
export interface EvmKeySaveOptions extends WalletPasswordOptions { overwrite?: boolean }

function mirror(address: string): string { return ss58FromPublic(blake2_256(concatBytes(new TextEncoder().encode('evm:'), hexBytes(address)))) }
function keyName(value: string): string {
  if (!value || value === '.' || value === '..' || /[\\/\0]/.test(value)) throw new TypeError('EVM key name must be a single path component')
  return value
}
async function secureDirectory(directory: string): Promise<void> {
  const absolute = resolve(directory), root = parse(absolute).root
  let current = root
  for (const component of absolute.slice(root.length).split(sep).filter(Boolean)) {
    current = join(current, component)
    try { await mkdir(current, { mode: 0o700 }) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
    const stat = await lstat(current)
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('EVM keystore directories must not be symlinks')
  }
}
async function readSecure(path: string): Promise<string> {
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const stat = await file.stat()
    if (!stat.isFile() || stat.size > 1024 * 1024) throw new Error('invalid EVM keystore file')
    return await file.readFile('utf8')
  } finally { await file.close() }
}
async function writeSecure(path: string, json: string, overwrite: boolean): Promise<void> {
  await secureDirectory(dirname(path))
  try {
    const existing = await lstat(path)
    if (existing.isSymbolicLink() || !existing.isFile()) throw new Error('EVM keystore target must be a regular file')
    if (!overwrite) throw new Error('EVM key already exists; pass overwrite to replace it')
  } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
  const temporary = join(dirname(path), `.${basename(path)}.${randomUUID()}.tmp`)
  const file = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
  try {
    await file.writeFile(json + '\n'); await file.sync(); await file.close()
    if (overwrite) await rename(temporary, path)
    else { await link(temporary, path); await unlink(temporary) }
  } finally {
    await file.close().catch(() => undefined)
    await unlink(temporary).catch(error => { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error })
  }
}
function validateKeystore(text: string): { address: string; payload: Record<string, unknown> } {
  const data = JSON.parse(text) as Record<string, unknown>
  if (data.version !== 3 || typeof data.address !== 'string' || !(data.crypto ?? data.Crypto)) throw new TypeError('expected an encrypted Ethereum V3 keystore')
  return { address: normalizeH160(data.address), payload: data }
}

export class EvmKeystore {
  readonly directory: string
  constructor(readonly wallet: Wallet = new Wallet()) { this.directory = join(wallet.path, wallet.name, 'evmkeys') }
  keyfilePath(name = 'default'): string { return join(this.directory, keyName(name)) }
  private async password(options: WalletPasswordOptions): Promise<string> {
    const value = await resolveWalletPassword(this.wallet, options)
    if (!value) throw new Error('EVM keystores require a non-empty password or password provider')
    return value
  }
  async info(name = 'default'): Promise<EvmKeyInfo> {
    const path = this.keyfilePath(name), { address } = validateKeystore(await readSecure(path))
    return { name, address, ss58Mirror: mirror(address), path }
  }
  async list(): Promise<EvmKeyInfo[]> {
    let files
    try { files = await readdir(this.directory, { withFileTypes: true }) } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []; throw error }
    const result: EvmKeyInfo[] = []
    for (const file of files.filter(file => file.isFile() && !file.name.startsWith('.')).sort((a, b) => a.name.localeCompare(b.name))) {
      try { result.push(await this.info(file.name)) } catch { /* Ignore unrelated or malformed files. */ }
    }
    return result
  }
  private async store(account: EvmAccount, name: string, options: EvmKeySaveOptions): Promise<EvmKeyInfo> {
    const path = this.keyfilePath(name), password = await this.password(options)
    // EthereumWallet deliberately drops mnemonic metadata before persistence.
    const encrypted = await new EthereumWallet(account.privateKey).encrypt(password)
    validateKeystore(encrypted)
    await writeSecure(path, encrypted, options.overwrite === true)
    return this.info(name)
  }
  create(name = 'default', options: EvmKeySaveOptions = {}): Promise<EvmKeyInfo> {
    return this.store(EthereumWallet.createRandom(), name, options)
  }
  async import(source: EvmKeySource, name = 'default', options: EvmKeySaveOptions = {}): Promise<EvmKeyInfo> {
    if (['privateKey', 'mnemonic', 'keystoreJson'].filter(key => key in source).length !== 1) throw new TypeError('provide exactly one privateKey, mnemonic, or keystoreJson')
    let account: EvmAccount
    if ('privateKey' in source) account = new EthereumWallet(source.privateKey.trim())
    else if ('mnemonic' in source) account = HDNodeWallet.fromPhrase(source.mnemonic.trim(), source.mnemonicPassword, source.derivationPath ?? ETH_DERIVATION_PATH)
    else {
      validateKeystore(source.keystoreJson)
      account = await EthereumWallet.fromEncryptedJson(source.keystoreJson, source.keystorePassword ?? await this.password(options))
    }
    return this.store(account, name, options)
  }
  async unlock(name = 'default', options: WalletPasswordOptions = {}): Promise<EvmAccount> {
    const json = await readSecure(this.keyfilePath(name)), metadata = validateKeystore(json)
    const account = await EthereumWallet.fromEncryptedJson(json, await this.password(options))
    if (normalizeH160(account.address) !== metadata.address) throw new Error('decrypted key differs from keystore address')
    return account
  }
  /** Export the encrypted standard V3 object; no decryption occurs. */
  async export(name = 'default'): Promise<Record<string, unknown>> { return validateKeystore(await readSecure(this.keyfilePath(name))).payload }
}

export async function loadContractArtifact(path: string) {
  const { parseContractArtifact } = await import('./evm')
  return parseContractArtifact(await readFile(path, 'utf8'))
}
