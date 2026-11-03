import { type ReadView, st, num, integer, keyOf } from './base'
import { commitmentRevealTime } from '../metagraph'
const mechanism = (netuid: number, mechid = 0) => {
  if (!Number.isInteger(netuid) || netuid < 0 || netuid >= 4096 || !Number.isInteger(mechid) || mechid < 0 || mechid >= 16) throw new RangeError('invalid netuid or mechanism index')
  return mechid * 4096 + netuid
}
export const weightReads = {
  weights: async (view: ReadView, p: { netuid: number; mechid?: number }): Promise<Record<number, Record<number, number>>> => Object.fromEntries((await view.queryMap(st('Weights'), [mechanism(p.netuid, p.mechid)])).map(([uid, row]) => {
    const total = (row ?? []).reduce((sum: bigint, pair: any[]) => sum + integer(pair[1]), 0n)
    return [num(keyOf(uid)), total ? Object.fromEntries(row.map(([key, value]: any[]) => [num(key), Number(integer(value)) / Number(total)])) : {}]
  })),
  bonds: async (view: ReadView, p: { netuid: number; mechid?: number }): Promise<Record<number, Record<number, number>>> => Object.fromEntries((await view.queryMap(st('Bonds'), [mechanism(p.netuid, p.mechid)])).map(([uid, row]) => [num(keyOf(uid)), Object.fromEntries((row ?? []).map(([key, value]: any[]) => [num(key), num(value) / 65535]))])),
  timelocked_weight_commits: async (view: ReadView, p: { netuid: number; mechid?: number }): Promise<Record<number, Array<{ hotkey: string; commit_block: number; reveal_round: bigint; reveals_at: Date; ciphertext: string }>>> => Object.fromEntries((await view.queryMap(st('TimelockedWeightCommits'), [mechanism(p.netuid, p.mechid)])).map(([epoch, values]) => [num(keyOf(epoch)), (values ?? []).map(([hotkey, block, ciphertext, round]: any[]) => ({ hotkey: String(hotkey), commit_block: num(block), reveal_round: integer(round), reveals_at: commitmentRevealTime(integer(round)), ciphertext: String(ciphertext) }))])),
}
