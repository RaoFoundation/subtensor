import { open, readFile, mkdir, unlink } from 'node:fs/promises'
import { dirname } from 'node:path'
import type { PruningJournal, PruningRecord } from './pruning'
/** Pass an explicit per-chain/subnet path. Exclusive create prevents concurrent broadcasts. */
export class FilePruningJournal implements PruningJournal {
  constructor(readonly path: string) {}
  async pending(): Promise<PruningRecord | undefined> { try { return JSON.parse(await readFile(this.path, 'utf8')) } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined; throw error } }
  async reserve(record: PruningRecord): Promise<void> { await mkdir(dirname(this.path), { recursive: true, mode: 0o700 }); const file = await open(this.path, 'wx', 0o600); try { await file.writeFile(JSON.stringify(record)); await file.sync() } finally { await file.close() } }
  async clear(): Promise<void> { await unlink(this.path) }
}
