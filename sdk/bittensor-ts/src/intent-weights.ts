import { evaluatePolicy } from './policy'
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

export function normalizeValidatorWeights(uids: readonly number[], weights: readonly number[]): [number[], number[]] {
  if (weights.some(w => !Number.isFinite(w))) throw new RangeError('weights must be finite and non-negative')
  return evaluatePolicy('validation.weights.normalize', { uids, weights })
}

export function clipToMaxWeight(weights: readonly number[], limit: number): number[] {
  if (!Number.isFinite(limit)) throw new RangeError('max weight limit must be in 0..1')
  if (weights.some(w => !Number.isFinite(w))) throw new RangeError('weights must be finite and non-negative')
  return evaluatePolicy('validation.weights.clip', { weights, limit })
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
  const permits = nullConsensus(mode) && uidRaw != null ? await query('ValidatorPermit') : []
  const uid = uidRaw == null ? null : integer(uidRaw, 'registered uid')
  const admission = evaluatePolicy<{ code: WeightPreflightError['code']; wait?: string } | null>('validation.weights.admission', {
    uid, null: nullConsensus(mode), permit: uid != null && Array.isArray(permits) && Boolean(permits[uid]),
    current, rate: unsigned(rateRaw ?? 0, 'rate limit'), last: uid != null && Array.isArray(last) ? unsigned(last[uid] ?? 0, 'last update') : 0n,
  })
  if (admission?.code === 'NOT_REGISTERED') throw new WeightPreflightError(`Hotkey ${context.hotkey} is not registered on netuid ${netuid}.`, admission.code)
  if (admission?.code === 'NOT_AUTHORIZED') throw new WeightPreflightError(`Only the Null validator permit holder may set weights on netuid ${netuid}.`, admission.code)
  if (admission) throw new WeightPreflightError(`Weights are rate limited; retry in ${admission.wait} blocks.`, admission.code)
  const min = integer(minRaw ?? 0, 'minimum weight count'), max = integer(maxRaw ?? U16_MAX, 'maximum weight limit')
  const selected = evaluatePolicy<{ uids: number[]; values: number[]; insufficient: boolean }>('validation.weights.finalize', {
    uid, uids, weights, minimum: min, maximum: max, null: nullConsensus(mode), raw: Boolean(args.raw_u16 ?? args.rawU16 ?? false),
  })
  uids = selected.uids; weights = selected.values
  if (selected.insufficient) throw new WeightPreflightError(`Subnet ${netuid} requires at least ${min} nonzero weights.`, 'INVALID_ARGUMENT')
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
