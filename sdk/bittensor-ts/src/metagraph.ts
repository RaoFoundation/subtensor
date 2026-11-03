import { Balance } from './balance'
import { type ReadView, type ChainRecord, st, api, integer, num, fixed, utf8Text, fromHex, toHex, blockOf, keyOf } from './reads/base'
/** Immutable Quicknet beacon schedule; also used by sdk/bittensor-core timelock constants. */
export function commitmentRevealTime(round: number | bigint): Date {
  const timestamp = (1692803367n + (BigInt(round) - 1n) * 3n) * 1000n
  if (timestamp < -8640000000000000n || timestamp > 8640000000000000n) throw new RangeError('drand round outside Date range')
  return new Date(Number(timestamp))
}
const fieldsOf = (field: any): Array<[string, any]> => typeof field === 'string' ? [[field, null]] : field && typeof field === 'object' ? Object.entries(field) : []
function roundOf(fields: any[], variant: string): bigint | null {
  const rounds = fields.flatMap(fieldsOf).filter(([name, value]) => name === variant && value && typeof value === 'object').map(([, value]) => integer(value.reveal_round))
  return rounds.length ? rounds.reduce((a, b) => a > b ? a : b) : null
}
export function decodeCommitmentFields(fields: any[], includeHashes = true): string {
  const raw: number[] = [], hashes: string[] = []
  for (const [name, value] of fields.flatMap(fieldsOf)) {
    if (name.startsWith('Raw') && typeof value === 'string') raw.push(...fromHex(value))
    else if (includeHashes && name !== 'TimelockEncrypted' && typeof value === 'string') hashes.push(`${name.toLowerCase()}:${value}`)
  }
  return [...(raw.length ? [utf8Text(Uint8Array.from(raw))] : []), ...hashes].join('\n')
}
function revealedEntry(entry: any[]): [number, string] {
  const [value, block] = entry
  let bytes = value instanceof Uint8Array ? value : Array.isArray(value) ? Uint8Array.from(value) : typeof value === 'string' && value.startsWith('0x') ? fromHex(value) : new TextEncoder().encode(String(value))
  if (bytes.length) {
    const mode = bytes[0] & 3, offset = mode === 3 ? 1 + (bytes[0] >> 2) + 4 : mode === 0 ? 1 : mode === 1 ? 2 : 4
    bytes = bytes.slice(offset)
  }
  return [num(block), String(utf8Text(bytes))]
}
export interface NeuronCommitmentData {
  hotkey: string; netuid: number; uid: number | null; block: number; queried_block: number; deposit: Balance; data: string;
  encrypted: boolean; reveal_round: bigint | null; revealed: Array<[number, string]>; fields: any[]; seconds_per_block: number
}
export class NeuronCommitment implements NeuronCommitmentData {
  hotkey!: string; netuid!: number; uid!: number | null; block!: number; queried_block!: number; deposit!: Balance; data!: string
  encrypted!: boolean; reveal_round!: bigint | null; revealed!: Array<[number, string]>; fields!: any[]; seconds_per_block!: number
  constructor(data: NeuronCommitmentData) { Object.assign(this, data) }
  get decrypted(): boolean { return this.revealed.some(([block]) => block >= this.block) }
  get status(): 'revealed' | 'failed' | 'sealed' | 'plain' { return this.decrypted ? 'revealed' : roundOf(this.fields, 'TimelockRevealFailed') != null ? 'failed' : this.encrypted ? 'sealed' : 'plain' }
  get is_revealed(): boolean { return this.status === 'plain' || this.status === 'revealed' }
  get age_blocks(): number { return Math.max(0, this.queried_block - this.block) }
  get duration(): number { return this.age_blocks * this.seconds_per_block }
  get value(): string | null {
    for (let i = this.revealed.length - 1; i >= 0; i--) if (this.revealed[i][0] >= this.block) return this.revealed[i][1]
    return this.data || null
  }
  get reveals_at(): Date | null { return this.reveal_round == null || this.decrypted ? null : commitmentRevealTime(this.reveal_round) }
}
export async function fetchCommitments(view: ReadView, netuid: number): Promise<Record<string, NeuronCommitment>> {
  view = await view.at()
  const [committed, revealed, uids, seconds] = await Promise.all([view.queryMap(['Commitments', 'CommitmentOf'], [netuid]), view.queryMap(['Commitments', 'RevealedCommitments'], [netuid]), view.queryMap(st('Uids'), [netuid]), view.blockTime()])
  const histories = new Map<string, Array<[number, string]>>(revealed.map(([key, entries]) => [String(keyOf(key)), (entries ?? []).map(revealedEntry).sort((a: [number, string], b: [number, string]) => a[0] - b[0])]))
  const uidMap = Object.fromEntries(uids.map(([key, value]) => [String(keyOf(key)), num(value)]))
  const records = new Map<string, NeuronCommitment>()
  for (const [key, row] of committed) {
    if (!row || typeof row !== 'object') continue
    const hotkey = String(keyOf(key)), fields = row.info?.fields ?? [], reveal = roundOf(fields, 'TimelockEncrypted')
    records.set(hotkey, new NeuronCommitment({ hotkey, netuid, uid: uidMap[hotkey] ?? null, block: num(row.block), queried_block: blockOf(view), deposit: view.balance(integer(row.deposit)), data: decodeCommitmentFields(fields), encrypted: reveal != null, reveal_round: reveal, revealed: histories.get(hotkey) ?? [], fields, seconds_per_block: seconds }))
  }
  for (const [hotkey, history] of histories) {
    if (records.has(hotkey) || !history.length) continue
    records.set(hotkey, new NeuronCommitment({ hotkey, netuid, uid: uidMap[hotkey] ?? null, block: history[history.length - 1][0], queried_block: blockOf(view), deposit: view.balance(0n), data: '', encrypted: true, reveal_round: null, revealed: history, fields: [], seconds_per_block: seconds }))
  }
  return Object.fromEntries([...records].sort(([, a], [, b]) => b.block - a.block))
}
export interface MetagraphNeuron {
  uid: number; hotkey: string; coldkey: string; active: boolean; validator_permit: boolean; last_update: number; block_at_registration: number;
  rank: number; trust: number; consensus: number; incentive: number; dividends: number; pruning_score: number;
  emission: Balance; alpha_stake: Balance; tao_stake: Balance; total_stake: Balance; axon: string | null; identity: ChainRecord | null;
  commitment: NeuronCommitment | null; collateral_locked: Balance | null; collateral_min: Balance | null; collateral_earned: Balance | null
}
function axonEndpoint(axon: ChainRecord | null): string | null {
  const ip = integer(axon?.ip)
  if (!ip) return null
  const port = num(axon?.port)
  if (num(axon?.ip_type ?? 4) === 6) {
    const groups = Array.from({ length: 8 }, (_, i) => ((ip >> BigInt((7 - i) * 16)) & 65535n).toString(16))
    let start = -1, length = 0
    for (let i = 0; i < groups.length;) { if (groups[i] !== '0') { i++; continue }; let j = i; while (j < groups.length && groups[j] === '0') j++; if (j - i > length) { start = i; length = j - i }; i = j }
    const address = length > 1 ? groups.slice(0, start).join(':') + '::' + groups.slice(start + length).join(':') : groups.join(':')
    return `[${address}]:${port}`
  }
  return `${[24n, 16n, 8n, 0n].map(shift => (ip >> shift) & 255n).join('.')}:${port}`
}
export class Metagraph implements Iterable<MetagraphNeuron> {
  readonly neurons: MetagraphNeuron[]
  readonly commitments: Record<number, NeuronCommitment> = {}
  readonly unregistered_commitments: Record<string, NeuronCommitment>
  readonly netuid: number; readonly mechid: number; readonly name: string; readonly symbol: string; readonly block: number; readonly tempo: number; readonly last_step: number; readonly blocks_since_last_step: number
  readonly owner_hotkey: string; readonly owner_coldkey: string; readonly network_registered_at: number; readonly num_uids: number; readonly max_uids: number; readonly price: number | null; readonly moving_price: number | null; readonly identity: ChainRecord | null
  constructor(netuid: number, readonly raw: ChainRecord, commitmentMap: Record<string, NeuronCommitment>) {
    this.netuid = netuid; this.mechid = Math.floor(num(raw.netuid ?? netuid) / 4096); this.name = String(utf8Text(raw.name) ?? ''); this.symbol = String(utf8Text(raw.symbol) ?? ''); this.block = num(raw.block); this.tempo = num(raw.tempo); this.last_step = num(raw.last_step); this.blocks_since_last_step = num(raw.blocks_since_last_step)
    this.owner_hotkey = String(raw.owner_hotkey); this.owner_coldkey = String(raw.owner_coldkey); this.network_registered_at = num(raw.network_registered_at); this.max_uids = num(raw.max_uids); this.identity = raw.identity && typeof raw.identity === 'object' ? raw.identity : null
    const alphaIn = integer(raw.alpha_in); this.price = alphaIn ? Number(integer(raw.tao_in)) / Number(alphaIn) : null; this.moving_price = raw.moving_price?.bits == null ? null : fixed(raw.moving_price, 32)
    this.unregistered_commitments = { ...commitmentMap }
    this.neurons = (raw.hotkeys ?? []).map((hotkey: any, uid: number): MetagraphNeuron => {
      const key = String(hotkey), commitment = this.unregistered_commitments[key] ?? null
      if (commitment) { commitment.uid = uid; this.commitments[uid] = commitment; delete this.unregistered_commitments[key] }
      const value = (name: string) => raw[name]?.[uid]
      const balance = (name: string, tao = false) => Balance.fromRao(integer(value(name)), tao ? 0 : netuid, tao ? undefined : this.symbol || undefined)
      const collateral = (name: string) => raw.collateral_locked == null ? null : balance(name)
      return { uid, hotkey: key, coldkey: String(value('coldkeys')), active: Boolean(value('active')), validator_permit: Boolean(value('validator_permit')), last_update: num(value('last_update')), block_at_registration: num(value('block_at_registration')), rank: num(value('rank')) / 65535, trust: num(value('trust')) / 65535, consensus: num(value('consensus')) / 65535, incentive: num(value('incentives')) / 65535, dividends: num(value('dividends')) / 65535, pruning_score: num(value('pruning_score')) / 65535, emission: balance('emission'), alpha_stake: balance('alpha_stake'), tao_stake: balance('tao_stake', true), total_stake: balance('total_stake'), axon: axonEndpoint(value('axons')), identity: value('identities') && typeof value('identities') === 'object' ? value('identities') : null, commitment, collateral_locked: collateral('collateral_locked'), collateral_min: collateral('collateral_min'), collateral_earned: collateral('collateral_earned') }
    })
    this.num_uids = num(raw.num_uids ?? this.neurons.length)
  }
  get hotkeys(): string[] { return this.neurons.map(n => n.hotkey) }
  get coldkeys(): string[] { return this.neurons.map(n => n.coldkey) }
  get validators(): MetagraphNeuron[] { return this.neurons.filter(n => n.validator_permit) }
  neuron(uid: number): MetagraphNeuron { if (!Number.isInteger(uid) || !this.neurons[uid]) throw new RangeError(`no uid ${uid} on netuid ${this.netuid}`); return this.neurons[uid] }
  byHotkey(hotkey: string): MetagraphNeuron | null { return this.neurons.find(n => n.hotkey === hotkey) ?? null }
  by_hotkey(hotkey: string): MetagraphNeuron | null { return this.byHotkey(hotkey) }
  [Symbol.iterator](): Iterator<MetagraphNeuron> { return this.neurons[Symbol.iterator]() }
}
export async function fetchMetagraph(view: ReadView, netuid: number, options: { commitments?: boolean } = {}): Promise<Metagraph | null> {
  view = await view.at()
  const [graph, commitments] = await Promise.all([view.runtime(api('SubnetInfoRuntimeApi', 'get_metagraph'), [netuid]), options.commitments === false ? Promise.resolve({}) : fetchCommitments(view, netuid)])
  return graph && typeof graph === 'object' ? new Metagraph(netuid, graph, commitments) : null
}
