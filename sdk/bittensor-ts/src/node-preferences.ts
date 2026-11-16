import { mkdir, readFile, rename, unlink, open } from 'node:fs/promises'
import { setTimeout as wait } from 'node:timers/promises'
import { homedir } from 'node:os'
import { join, dirname } from 'node:path'
import { randomUUID } from 'node:crypto'
import { SdkPreferences, type JsonStore } from './preferences'

const FILES: Record<string, string> = { config: 'btcli.json', addresses: 'btcli_addresses.json', proxies: 'btcli_proxies.json', multisigs: 'btcli_multisigs.json', multisig_cache: 'btcli_multisig_cache.json', subnet_names: 'btcli_subnet_names.json', token_symbols: 'btcli_token_symbols.json', protected: 'btcli_protected.json', protected_setups: 'btcli_protected_setups.json', proxy_announcements: 'btcli_proxy_announcements.json' }
/** Atomic public configuration files compatible with Python's books. Corrupt files are never overwritten implicitly. */
export class FileJsonStore implements JsonStore {
  constructor(readonly directory = join(homedir(), '.bittensor'), readonly paths: Partial<Record<keyof typeof FILES, string>> = {}) {}
  path(key: string): string { if (!Object.hasOwn(FILES, key)) throw new TypeError('unknown preference file'); return this.paths[key] ?? join(this.directory, FILES[key]) }
  async read(key: string): Promise<unknown> { try { return JSON.parse(await readFile(this.path(key), 'utf8')) } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined; throw error } }
  async write(key: string, value: unknown): Promise<void> {
    await this.locked(key, () => this.publish(key, value))
  }
  async update<T>(key: string, action: (current: unknown) => { value: unknown; result: T }): Promise<T> {
    return this.locked(key, async () => {
      const { value, result } = action(await this.read(key))
      await this.publish(key, value)
      return result
    })
  }
  private async locked<T>(key: string, action: () => Promise<T>): Promise<T> {
    const path = this.path(key), lock = `${path}.lock`, deadline = Date.now() + 5000
    await mkdir(dirname(path), { recursive: true, mode: 0o700 })
    let handle
    for (;;) {
      try { handle = await open(lock, 'wx', 0o600); break }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
        if (Date.now() >= deadline) throw new Error(`public configuration is locked: ${lock}; inspect the owning process before removing an abandoned lock`)
        await wait(10)
      }
    }
    try {
      await handle.writeFile(JSON.stringify({ pid: process.pid, created_at: Date.now() }) + '\n')
      return await action()
    } finally {
      await handle.close()
      await unlink(lock)
    }
  }
  private async publish(key: string, value: unknown): Promise<void> {
    const path = this.path(key), temporary = `${path}.${randomUUID()}.tmp`
    await mkdir(dirname(path), { recursive: true, mode: 0o700 })
    try {
      const file = await open(temporary, 'wx', 0o600)
      try { await file.writeFile(JSON.stringify(value, null, 2) + '\n'); await file.sync() }
      finally { await file.close() }
      await rename(temporary, path)
      if (process.platform !== 'win32') {
        const directory = await open(dirname(path), 'r')
        try { await directory.sync() } finally { await directory.close() }
      }
    }
    finally { await unlink(temporary).catch((error: NodeJS.ErrnoException) => { if (error.code !== 'ENOENT') throw error }) }
  }
}
export function filePreferences(directory?: string): SdkPreferences { return new SdkPreferences(new FileJsonStore(directory)) }
