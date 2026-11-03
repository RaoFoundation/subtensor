import type { IntentContext, IntentDescriptor, IntentReader } from './intents'
import type { EpochScheduleState } from './types'

const U16_MAX = 65535
const GLOBAL_MAX_SUBNET_COUNT = 4096
export const DEFAULT_COMMIT_REVEAL_VERSION = 4

export interface EncryptWeightsV2Options {
  uids: number[]
  values: number[]
  versionKey: bigint
  state: EpochScheduleState
  subnetRevealPeriodEpochs: bigint
  /** Seconds per block, including fractional fast-block intervals. */
  blockTime: number
  hotkey: Uint8Array
}

export class WeightPreflightError extends Error {
  constructor(message: string, readonly code: 'NOT_REGISTERED' | 'NOT_AUTHORIZED' | 'RATE_LIMITED' | 'INVALID_ARGUMENT') {
    super(message)
    this.name = 'WeightPreflightError'
  }
}

function integer(value: unknown, name: string, max = U16_MAX): number {
  const number = Number(value)
  if (!Number.isSafeInteger(number) || number < 0 || number > max) throw new RangeError(`${name} must be an integer in 0..${max}`)
  return number
}

function unsigned(value: unknown, name: string): bigint {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError(`${name} must be lossless`)
  if (!['bigint', 'number', 'string'].includes(typeof value)) throw new TypeError(`${name} must be an integer`)
  const n = BigInt(value as string | number | bigint)
  if (n < 0n || n > 0xffffffffffffffffn) throw new RangeError(`${name} must fit u64`)
  return n
}

// Python's canonical normalize uses round-to-even rather than Math.round's tie rule.
function roundEven(value: number): number {
  const floor = Math.floor(value)
  return value - floor === 0.5 ? floor + (floor % 2) : Math.round(value)
}

export function normalizeValidatorWeights(uids: readonly number[], weights: readonly number[]): [number[], number[]] {
  if (uids.length !== weights.length) throw new RangeError('uids and weights must be equal length')
  if (weights.some(w => !Number.isFinite(w) || w < 0)) throw new RangeError('weights must be finite and non-negative')
  for (const uid of uids) integer(uid, 'uid')
  const top = weights.reduce((maximum, weight) => Math.max(maximum, weight), 0)
  if (top === 0) return [[], []]
  const result: [number[], number[]] = [[], []]
  weights.forEach((weight, i) => {
    const value = roundEven((weight / top) * U16_MAX)
    if (value !== 0) { result[0].push(uids[i]); result[1].push(value) }
  })
  return result
}

export function clipToMaxWeight(weights: readonly number[], limit: number): number[] {
  if (!Number.isFinite(limit) || limit < 0 || limit > 1) throw new RangeError('max weight limit must be in 0..1')
  if (weights.some(w => !Number.isFinite(w) || w < 0)) throw new RangeError('weights must be finite and non-negative')
  const total = weights.reduce((sum, w) => sum + w, 0), n = weights.length
  if (n === 0 || total <= 0 || !Number.isFinite(total)) throw new RangeError('weights require a finite positive total')
  if (n * limit <= 1) return weights.map(() => 1 / n)
  const estimation = weights.map(w => w / total).sort((a, b) => a - b)
  if (estimation[n - 1] <= limit) return weights.map(w => w / total)
  let cumulative = 0
  const cumsum = estimation.map(w => { cumulative += w; return cumulative })
  const epsilon = 1e-7
  const count = estimation.filter((w, i) => w / ((n - i - 1) * w + cumsum[i] + epsilon) < limit).length
  const cutoff = ((limit * cumsum[count - 1] - epsilon) / (1 - limit * (n - count))) * total
  const clipped = weights.map(w => Math.min(w, cutoff))
  const clippedTotal = clipped.reduce((sum, w) => sum + w, 0)
  return clipped.map(w => w / clippedTotal)
}

function pairs(args: Record<string, unknown>): [number[], number[]] {
  let uids: number[], weights: number[]
  if (args.weights != null && typeof args.weights === 'object' && !Array.isArray(args.weights)) {
    if (args.uids != null) throw new TypeError('provide a weight mapping or parallel lists, not both')
    const entries = Object.entries(args.weights).map(([uid, weight]) => [integer(uid, 'uid'), Number(weight)]).sort((a, b) => a[0] - b[0])
    uids = entries.map(e => e[0]); weights = entries.map(e => e[1])
  } else {
    if (!Array.isArray(args.uids) || !Array.isArray(args.weights)) throw new TypeError('provide a weight mapping or parallel uids/weights lists')
    uids = args.uids.map(uid => integer(uid, 'uid')); weights = args.weights.map(Number)
  }
  if (new Set(uids).size !== uids.length) throw new RangeError('duplicate UIDs in weights')
  if (uids.length !== weights.length || weights.some(w => !Number.isFinite(w) || w < 0)) throw new RangeError('weights must be finite, non-negative, and parallel to uids')
  return [uids, weights]
}

function nullConsensus(value: unknown): boolean {
  return value === 'Null' || value === 1 || (value != null && typeof value === 'object' && Object.prototype.hasOwnProperty.call(value, 'Null'))
}

async function pinnedBlock(reader: IntentReader): Promise<{ current: number; hash: string }> {
  if (!reader.blockNumber || !reader.blockHash) throw new TypeError('weight preparation requires blockNumber and blockHash')
  const current = await reader.blockNumber()
  return { current, hash: await reader.blockHash(current) }
}

export async function prepareWeights(op: string, args: Record<string, unknown>, reader: IntentReader, context: IntentContext): Promise<{ call: IntentDescriptor; extras?: Record<string, unknown> }> {
  const netuid = integer(args.netuid, 'netuid'), mechid = integer(args.mechid ?? 0, 'mechid')
  const versionKey = unsigned(args.version_key ?? args.versionKey ?? 0, 'version_key')
  let [uids, weights] = pairs(args)
  if (op === 'reveal_weights') {
    const [dests, values] = normalizeValidatorWeights(uids, weights)
    if (!values.length) throw new RangeError('all weights are zero; nothing to reveal')
    if (!Array.isArray(args.salt)) throw new TypeError('salt must be a u16 list')
    return { call: { pallet: 'SubtensorModule', call: 'reveal_weights', params: {
      netuid, uids: dests, values, salt: args.salt.map(value => integer(value, 'salt')), version_key: versionKey,
    } } }
  }
  if (op !== 'set_weights' && op !== 'commit_weights') throw new TypeError(`unsupported weights operation ${op}`)
  if (!context.hotkey) throw new TypeError('weight preparation requires a hotkey')
  const { current, hash } = await pinnedBlock(reader)
  const query = (item: string, params: unknown[] = [netuid]) => reader.query('SubtensorModule', item, params, hash)
  const [uidRaw, cr, rateRaw, last, minRaw, maxRaw, mode] = await Promise.all([
    query('Uids', [netuid, context.hotkey]), query('CommitRevealWeightsEnabled'), query('WeightsSetRateLimit'),
    query('LastUpdate', [mechid * GLOBAL_MAX_SUBNET_COUNT + netuid]), query('MinAllowedWeights'), query('MaxWeightsLimit'), query('SubnetEpochConsensus'),
  ])
  if (uidRaw == null) throw new WeightPreflightError(`Hotkey ${context.hotkey} is not registered on netuid ${netuid}.`, 'NOT_REGISTERED')
  const uid = integer(uidRaw, 'registered uid')
  if (nullConsensus(mode)) {
    const permits = await query('ValidatorPermit')
    if (!Array.isArray(permits) || !permits[uid]) throw new WeightPreflightError(`Only the Null validator permit holder may set weights on netuid ${netuid}.`, 'NOT_AUTHORIZED')
  }
  const rate = unsigned(rateRaw ?? 0, 'rate limit')
  if (rate && Array.isArray(last) && last[uid] != null) {
    const previous = unsigned(last[uid], 'last update'), since = BigInt(current) - previous
    if (previous > 0n && since < rate) throw new WeightPreflightError(`Weights are rate limited; retry in ${rate - since} blocks.`, 'RATE_LIMITED')
  }
  const min = integer(minRaw ?? 0, 'minimum weight count'), max = integer(maxRaw ?? U16_MAX, 'maximum weight limit')
  const selfWeight = uids.length === 1 && uids[0] === uid
  const raw = Boolean(args.raw_u16 ?? args.rawU16 ?? false)
  if (raw) {
    if (!nullConsensus(mode)) throw new TypeError('raw_u16 requires a subnet running Null consensus')
    if (weights.some(w => !Number.isInteger(w) || w > U16_MAX)) throw new RangeError('raw weights must be integers in 0..65535')
    const selected = uids.map((uid, i) => [uid, weights[i]]).filter(pair => pair[1] > 0)
    uids = selected.map(pair => pair[0]); weights = selected.map(pair => pair[1])
    const top = weights.reduce((maximum, value) => Math.max(maximum, value), 0)
    if (!selfWeight && BigInt(top) * BigInt(U16_MAX) > BigInt(max) * weights.reduce((sum, value) => sum + BigInt(value), 0n)) throw new RangeError('raw weights exceed max_weight_limit')
  } else {
    if (!selfWeight && max < U16_MAX) weights = clipToMaxWeight(weights, max / U16_MAX)
    ;[uids, weights] = normalizeValidatorWeights(uids, weights)
  }
  if (!uids.length) throw new RangeError('all weights are zero; nothing to set')
  if (!selfWeight && uids.length < min) throw new WeightPreflightError(`Subnet ${netuid} requires at least ${min} nonzero weights.`, 'INVALID_ARGUMENT')
  if (op === 'set_weights' && !cr) return { call: { pallet: 'SubtensorModule', call: 'set_mechanism_weights', params: { netuid, mecid: mechid, dests: uids, weights, version_key: versionKey } } }
  if (!reader.encryptWeightsV2 || !reader.blockTime) throw new TypeError('timelocked weights require encryptWeightsV2 and blockTime')
  if (context.hotkeyPublicKey?.length !== 32) throw new TypeError('timelocked weights require a 32-byte hotkey public key')
  const schedule = await pinnedBlock(reader)
  const scheduleItems = ['Tempo', 'RevealPeriodEpochs', 'LastEpochBlock', 'PendingEpochAt', 'SubnetEpochIndex', 'BlocksSinceLastStep']
  const [tempo, reveal, epoch, pending, index, since] = await Promise.all(scheduleItems.map(item => reader.query('SubtensorModule', item, [netuid], schedule.hash)))
  const blockTime = await reader.blockTime()
  if (!Number.isFinite(blockTime) || blockTime <= 0) throw new RangeError('block time must be positive seconds')
  const options: EncryptWeightsV2Options = {
    uids, values: weights, versionKey, state: {
      lastEpochBlock: unsigned(epoch ?? 0, 'last epoch block'), pendingEpochAt: unsigned(pending ?? 0, 'pending epoch'),
      subnetEpochIndex: unsigned(index ?? 0, 'subnet epoch index'), tempo: integer(tempo, 'tempo'),
      blocksSinceLastStep: unsigned(since ?? 0, 'blocks since last step'), currentBlock: BigInt(schedule.current),
    }, subnetRevealPeriodEpochs: unsigned(reveal, 'reveal period'), blockTime, hotkey: context.hotkeyPublicKey,
  }
  const encrypted = await reader.encryptWeightsV2({ ...options })
  return { call: { pallet: 'SubtensorModule', call: 'commit_timelocked_mechanism_weights', params: {
    netuid, mecid: mechid, commit: encrypted.commit, reveal_round: encrypted.revealRound,
    commit_reveal_version: unsigned(args.commit_reveal_version ?? DEFAULT_COMMIT_REVEAL_VERSION, 'commit reveal version'),
  } }, extras: { reveal_round: encrypted.revealRound } }
}
