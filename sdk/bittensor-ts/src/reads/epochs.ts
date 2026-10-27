import { type ReadView, st, api, num, integer, blockOf } from './base'
const next = async (view: ReadView, netuid: number): Promise<number | null> => { const value = await view.runtime(api('SubnetInfoRuntimeApi', 'get_next_epoch_start_block'), [netuid]); return value == null ? null : num(value) }
const scalar = (name: string) => async (view: ReadView, p: { netuid: number }) => num(await view.query(st(name), [p.netuid]))
export const epochReads = {
  next_epoch_start_block: (view: ReadView, p: { netuid: number }) => next(view, p.netuid),
  blocks_until_next_epoch: async (view: ReadView, p: { netuid: number }) => { view = await view.at(); const value = await next(view, p.netuid); return value == null ? null : Math.max(0, value - blockOf(view)) },
  blocks_since_last_step: scalar('BlocksSinceLastStep'),
  blocks_since_last_update: async (view: ReadView, p: { netuid: number; uid: number }) => { view = await view.at(); const values = await view.query(st('LastUpdate'), [p.netuid]); return !values || p.uid < 0 || p.uid >= values.length ? null : blockOf(view) - num(values[p.uid]) },
  epoch_status: async (view: ReadView, p: { netuid: number }) => {
    view = await view.at()
    const [tempo, last, since, pending, index, start, seconds] = await Promise.all([view.query(st('Tempo'), [p.netuid]), view.query(st('LastEpochBlock'), [p.netuid]), view.query(st('BlocksSinceLastStep'), [p.netuid]), view.query(st('PendingEpochAt'), [p.netuid]), view.query(st('SubnetEpochIndex'), [p.netuid]), next(view, p.netuid), view.blockTime()])
    const remaining = start == null ? null : Math.max(0, start - blockOf(view))
    return { netuid: p.netuid, block: blockOf(view), tempo: num(tempo), epoch_index: num(index), last_epoch_block: num(last), blocks_since_last_step: num(since), pending_epoch_at: pending ? num(pending) : null, next_epoch_start_block: start, blocks_remaining: remaining, seconds_remaining: remaining == null ? null : remaining * seconds }
  },
  weights_rate_limit: scalar('WeightsSetRateLimit'), difficulty: async (view: ReadView, p: { netuid: number }) => integer(await view.query(st('Difficulty'), [p.netuid])), min_allowed_weights: scalar('MinAllowedWeights'), max_weight_limit: scalar('MaxWeightsLimit'), immunity_period: scalar('ImmunityPeriod'), reveal_period: scalar('RevealPeriodEpochs'), subnet_emission_enabled: scalar('SubnetEmissionEnabled'),
}
