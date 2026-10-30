/** Explicit application persistence. Values are never loaded or applied merely by importing the SDK. */
export interface JsonStore { read(key: string): Promise<unknown | undefined>; write(key: string, value: unknown): Promise<void> }
export class MemoryJsonStore implements JsonStore {
  private values = new Map<string, unknown>()
  async read(key: string) { return structuredClone(this.values.get(key)) }
  async write(key: string, value: unknown) { this.values.set(key, structuredClone(value)) }
}
export class BrowserJsonStore implements JsonStore {
  constructor(private readonly storage: Pick<Storage, 'getItem' | 'setItem'>, private readonly prefix = 'bittensor:') {}
  async read(key: string) { const value = this.storage.getItem(this.prefix + key); return value == null ? undefined : JSON.parse(value) }
  async write(key: string, value: unknown) { this.storage.setItem(this.prefix + key, JSON.stringify(value)) }
}
export interface AddressEntry { name: string; address: string; [key: string]: unknown }
export interface ProxyEntry { name: string; [key: string]: unknown }
export interface MultisigEntry { name: string; threshold: number; signatories: string[]; address?: string; [key: string]: unknown }
export class NamedBook<T extends { name: string }> {
  private queue: Promise<unknown> = Promise.resolve()
  constructor(private readonly store: JsonStore, readonly key: string, private readonly validate: (entry: T) => void = () => {}) {}
  async list(): Promise<T[]> {
    const value = await this.store.read(this.key)
    if (value == null) return []
    if (!Array.isArray(value) || value.some(item => !item || typeof item.name !== 'string' || !item.name.trim())) throw new TypeError(`invalid ${this.key} book`)
    for (const entry of value) this.validate(entry)
    return value as T[]
  }
  async get(name: string): Promise<T | undefined> { return (await this.list()).find(entry => entry.name === name) }
  private async update<R>(fn: (entries: T[]) => { entries: T[]; result: R }): Promise<R> {
    const next = this.queue.catch(() => {}).then(async () => { const { entries, result } = fn(await this.list()); await this.store.write(this.key, entries); return result })
    this.queue = next
    return next
  }
  put(entry: T): Promise<T> {
    const saved = structuredClone(entry)
    if (typeof saved.name !== 'string' || !saved.name.trim()) throw new TypeError('book entry requires a nonempty name')
    this.validate(saved)
    return this.update(entries => ({ entries: [...entries.filter(item => item.name !== saved.name), saved], result: structuredClone(saved) }))
  }
  patch(name: string, fields: Partial<T>): Promise<T | undefined> {
    const saved = structuredClone(fields)
    return this.update(entries => {
      const old = entries.find(item => item.name === name)
      if (!old) return { entries, result: undefined }
      const value = { ...old, ...saved, name }; this.validate(value)
      return { entries: entries.map(item => item.name === name ? value : item), result: value }
    })
  }
  remove(name: string): Promise<boolean> { return this.update(entries => ({ entries: entries.filter(item => item.name !== name), result: entries.some(item => item.name === name) })) }
  clear(): Promise<number> { return this.update(entries => ({ entries: [], result: entries.length })) }
}
export const CONFIG_KEYS = Object.freeze({ network: 'string', wallet: 'string', wallet_hotkey: 'string', wallet_path: 'string', json: 'boolean', quiet: 'boolean', signer_address: 'string', extension_browser: 'string', fallback_endpoints: 'string', archive_endpoints: 'string', retry_forever: 'boolean', proxy_for: 'string', mev_shield: 'boolean' } as const)
export type ConfigKey = keyof typeof CONFIG_KEYS
export class SdkPreferences {
  readonly addresses: NamedBook<AddressEntry>
  readonly proxies: NamedBook<ProxyEntry>
  readonly multisigs: NamedBook<MultisigEntry>
  private queue: Promise<unknown> = Promise.resolve()
  constructor(readonly store: JsonStore) {
    this.addresses = new NamedBook(store, 'addresses', entry => { if (typeof entry.address !== 'string' || !entry.address) throw new TypeError('address entry requires an address') })
    this.proxies = new NamedBook(store, 'proxies')
    this.multisigs = new NamedBook(store, 'multisigs', entry => { if (!Number.isInteger(entry.threshold) || entry.threshold < 1 || entry.threshold > 65535 || !Array.isArray(entry.signatories) || !entry.signatories.length || entry.signatories.some(address => typeof address !== 'string' || !address.trim()) || new Set(entry.signatories).size !== entry.signatories.length || entry.threshold > entry.signatories.length) throw new TypeError('invalid multisig signer set or threshold') })
  }
  async settings(): Promise<Partial<Record<ConfigKey, string | boolean>>> {
    const value = record(await this.store.read('config'))
    return Object.fromEntries(Object.entries(value).filter(([key, value]) => Object.hasOwn(CONFIG_KEYS, key) && typeof value === CONFIG_KEYS[key as ConfigKey]))
  }
  async get(key: ConfigKey): Promise<string | boolean | undefined> { requireKey(key); return (await this.settings())[key] }
  private async change<R>(action: () => Promise<R>): Promise<R> { const next = this.queue.catch(() => {}).then(action); this.queue = next; return next }
  set(key: ConfigKey, value: string | boolean): Promise<string | boolean> {
    requireKey(key)
    let normalized: string | boolean = value
    if (CONFIG_KEYS[key] === 'boolean' && typeof value === 'string') {
      if (/^(true|1|yes|on)$/i.test(value.trim())) normalized = true
      else if (/^(false|0|no|off)$/i.test(value.trim())) normalized = false
      else throw new TypeError(`${key} requires a boolean`)
    }
    if (typeof normalized !== CONFIG_KEYS[key]) throw new TypeError(`${key} requires ${CONFIG_KEYS[key]}`)
    return this.change(async () => { await this.store.write('config', { ...await this.settings(), [key]: normalized }); return normalized })
  }
  unset(key: ConfigKey): Promise<boolean> { requireKey(key); return this.change(async () => { const settings = await this.settings(), existed = Object.hasOwn(settings, key); delete settings[key]; await this.store.write('config', settings); return existed }) }
  clear(): Promise<void> { return this.change(() => this.store.write('config', {})) }
  async resolveAddress(nameOrAddress: string): Promise<string> { return (await this.addresses.list()).find(entry => entry.name === nameOrAddress || entry.address === nameOrAddress)?.address ?? nameOrAddress }
  async cachedMultisig(callHash: string): Promise<unknown> { const entries = record(await this.store.read('multisig_cache')), key = hashKey(callHash); return Object.hasOwn(entries, key) ? entries[key] : undefined }
  cacheMultisig(callHash: string, entry: unknown): Promise<void> { return this.change(async () => this.store.write('multisig_cache', { ...record(await this.store.read('multisig_cache')), [hashKey(callHash)]: entry })) }
  async labels(kind: 'subnet_names' | 'token_symbols', network: string): Promise<{ values: Record<number, string>; fresh: boolean }> {
    const cache = record(await this.store.read(kind)), row = Object.hasOwn(cache, network) ? record(cache[network]) : {}, values = record(row[kind === 'subnet_names' ? 'names' : 'symbols'])
    return { values: Object.fromEntries(Object.entries(values).filter(([key, value]) => /^\d+$/.test(key) && typeof value === 'string')), fresh: typeof row.fetched_at === 'number' && Date.now() / 1000 - row.fetched_at < (kind === 'subnet_names' ? 86400 : 604800) }
  }
  saveLabels(kind: 'subnet_names' | 'token_symbols', network: string, labels: Record<number, string>): Promise<void> { return this.change(async () => this.store.write(kind, { ...record(await this.store.read(kind)), [network]: { fetched_at: Math.floor(Date.now() / 1000), [kind === 'subnet_names' ? 'names' : 'symbols']: labels } })) }
}
function record(value: unknown): Record<string, any> { if (value == null) return {}; if (typeof value !== 'object' || Array.isArray(value)) throw new TypeError('expected a JSON object'); return value as Record<string, any> }
function requireKey(key: string): asserts key is ConfigKey { if (!Object.hasOwn(CONFIG_KEYS, key)) throw new TypeError(`unknown config key ${key}`) }
function hashKey(value: string): string { const text = value.startsWith('0x') ? value : '0x' + value; if (!/^0x[0-9a-fA-F]{64}$/.test(text)) throw new TypeError('expected a 32-byte call hash'); return text.toLowerCase() }
