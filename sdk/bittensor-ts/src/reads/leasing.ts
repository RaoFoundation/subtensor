import { type ReadView, type ChainRecord, st, num, integer, bal, keyOf } from './base'
const leaseRecord = (view: ReadView, id: any, value: ChainRecord) => ({ lease_id: num(id), beneficiary: String(value.beneficiary), coldkey: String(value.coldkey), hotkey: String(value.hotkey), emissions_share: num(value.emissions_share), end_block: value.end_block == null ? null : num(value.end_block), netuid: num(value.netuid), cost: bal(view, value.cost) })
export const leasingReads = {
  lease: async (view: ReadView, p: { lease_id: number }) => { const value = await view.query(st('SubnetLeases'), [p.lease_id]); return value ? leaseRecord(view, p.lease_id, value) : null },
  leases: async (view: ReadView) => (await view.queryMap(st('SubnetLeases'))).map(([key, value]) => leaseRecord(view, keyOf(key), value)),
  crowdloan: async (view: ReadView, p: { crowdloan_id: number }) => {
    const value = await view.query(['Crowdloan', 'Crowdloans'], [p.crowdloan_id])
    return value ? { creator: String(value.creator), deposit: bal(view, value.deposit), min_contribution: bal(view, value.min_contribution), cap: bal(view, value.cap), raised: bal(view, value.raised), end: num(value.end), finalized: Boolean(value.finalized), target_address: value.target_address ? String(value.target_address) : null, funds_account: value.funds_account ? String(value.funds_account) : null } : null
  },
  crowdloans: async (view: ReadView) => (await view.queryMap(['Crowdloan', 'Crowdloans'])).filter(([, value]) => value).map(([id, v]) => ({ id: num(keyOf(id)), creator: String(v.creator), raised_tao: Number(integer(v.raised)) / 1e9, cap_tao: Number(integer(v.cap)) / 1e9, finalized: Boolean(v.finalized) })).sort((a, b) => a.id - b.id),
  crowdloan_contributors: async (view: ReadView, p: { crowdloan_id: number }) => (await view.queryMap(['Crowdloan', 'Contributions'], [p.crowdloan_id])).map(([key, value]) => ({ contributor: String(keyOf(key)), amount_tao: Number(integer(value)) / 1e9 })),
}
