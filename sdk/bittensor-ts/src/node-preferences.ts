import { mkdir, readFile, writeFile, rename, unlink } from 'node:fs/promises'
import { homedir } from 'node:os'
import { join, dirname } from 'node:path'
import { randomUUID } from 'node:crypto'
import { SdkPreferences, type JsonStore } from './preferences'

const FILES: Record<string, string> = { config: 'btcli.json', addresses: 'btcli_addresses.json', proxies: 'btcli_proxies.json', multisigs: 'btcli_multisigs.json', multisig_cache: 'btcli_multisig_cache.json', subnet_names: 'btcli_subnet_names.json', token_symbols: 'btcli_token_symbols.json' }
/** Atomic public configuration files compatible with Python's books. Corrupt files are never overwritten implicitly. */
export class FileJsonStore implements JsonStore {
  constructor(readonly directory = join(homedir(), '.bittensor'), readonly paths: Partial<Record<keyof typeof FILES, string>> = {}) {}
  path(key: string): string { if (!Object.hasOwn(FILES, key)) throw new TypeError('unknown preference file'); return this.paths[key] ?? join(this.directory, FILES[key]) }
  async read(key: string): Promise<unknown> { try { return JSON.parse(await readFile(this.path(key), 'utf8')) } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined; throw error } }
  async write(key: string, value: unknown): Promise<void> {
    const path = this.path(key), temporary = `${path}.${randomUUID()}.tmp`
    await mkdir(dirname(path), { recursive: true, mode: 0o700 })
    try { await writeFile(temporary, JSON.stringify(value, null, 2) + '\n', { flag: 'wx', mode: 0o600 }); await rename(temporary, path) }
    finally { await unlink(temporary).catch((error: NodeJS.ErrnoException) => { if (error.code !== 'ENOENT') throw error }) }
  }
}
export function filePreferences(directory?: string): SdkPreferences { return new SdkPreferences(new FileJsonStore(directory)) }
