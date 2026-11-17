import { type ReadView, type ChainRecord, st, api, num, integer, bal, keyOf } from './base'
const delegateInfo = (view: ReadView, d: ChainRecord) => ({ hotkey: String(d.delegate_ss58), owner: String(d.owner_ss58), take: num(d.take) / 65535, nominators: (d.nominators ?? []).length as number, registrations: (d.registrations ?? []).map(num) as number[], validator_permits: (d.validator_permits ?? []).map(num) as number[], return_per_1000: bal(view, d.return_per_1000), total_daily_return: bal(view, d.total_daily_return) })
const edges = (rows: Array<[any, any]>): Array<[bigint, string]> => (rows ?? []).map(([p, key]) => [integer(p), String(key)])
export const delegationReads = {
  delegates: async (view: ReadView) => ((await view.runtime(api('DelegateInfoRuntimeApi', 'get_delegates'), []) ?? []) as ChainRecord[]).map((d: ChainRecord) => delegateInfo(view, d)),
  delegate: async (view: ReadView, p: { hotkey_ss58: string }) => { const value = await view.runtime(api('DelegateInfoRuntimeApi', 'get_delegate'), [p.hotkey_ss58]); return value ? delegateInfo(view, value) : null },
  is_delegate: async (view: ReadView, p: { hotkey_ss58: string }) => (await view.runtime(api('DelegateInfoRuntimeApi', 'get_delegate'), [p.hotkey_ss58])) != null,
  delegate_take: async (view: ReadView, p: { hotkey_ss58: string }) => {
    const [raw, lo, hi] = await Promise.all([view.query(st('Delegates'), [p.hotkey_ss58]), view.query(st('MinDelegateTake')), view.query(st('MaxDelegateTake'))])
    return { hotkey: p.hotkey_ss58, take: num(raw) / 65535, take_u16: num(raw), min: num(lo) / 65535, max: num(hi) / 65535 }
  },
  delegated: async (view: ReadView, p: { coldkey_ss58: string }) => ((await view.runtime(api('DelegateInfoRuntimeApi', 'get_delegated'), [p.coldkey_ss58]) ?? []) as Array<[ChainRecord, [unknown, unknown]]>).map(([info, [netuid, value]]: [ChainRecord, [any, any]]) => ({ delegate: delegateInfo(view, info), netuid: num(netuid), stake: bal(view, value, num(netuid)) })),
  children: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => edges(await view.query(st('ChildKeys'), [p.hotkey_ss58, p.netuid])),
  parents: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => edges(await view.query(st('ParentKeys'), [p.hotkey_ss58, p.netuid])),
  pending_children: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => { const [rows, block] = await view.query(st('PendingChildKeys'), [p.netuid, p.hotkey_ss58]) ?? [[], 0]; return { children: edges(rows), cooldown_block: num(block) } },
  childkey_threshold_suspended: async (view: ReadView, p: { hotkey_ss58: string }) => (await view.query(st('ChildkeyThresholdSuspended'), [p.hotkey_ss58])) != null,
  neurons: async (view: ReadView, p: { netuid: number; lite?: boolean }) => {
    const rows: ChainRecord[] = await view.runtime(api('NeuronInfoRuntimeApi', p.lite === false ? 'get_neurons' : 'get_neurons_lite'), [p.netuid]) ?? []
    return rows.map(r => ({ uid: num(r.uid), hotkey: String(r.hotkey), coldkey: String(r.coldkey), active: Boolean(r.active), validator_permit: Boolean(r.validator_permit), last_update: num(r.last_update), total_stake: bal(view, (r.stake ?? []).reduce((sum: bigint, pair: any[]) => sum + integer(pair[1]), 0n), p.netuid), raw: r })).sort((a, b) => a.uid - b.uid)
  },
  hotkey_owner: async (view: ReadView, p: { hotkey_ss58: string }): Promise<string | null> => { const value = await view.query(st('Owner'), [p.hotkey_ss58]); return value ? String(value) : null },
  owned_hotkeys: async (view: ReadView, p: { coldkey_ss58: string }): Promise<string[]> => (await view.query(st('OwnedHotkeys'), [p.coldkey_ss58]) ?? []).map(String),
  netuids_for_hotkey: async (view: ReadView, p: { hotkey_ss58: string }) => (await view.queryMap(st('IsNetworkMember'), [p.hotkey_ss58])).filter(([, value]) => value).map(([key]) => num(keyOf(key))).sort((a, b) => a - b),
  uid: async (view: ReadView, p: { hotkey_ss58: string; netuid: number }) => { const value = await view.query(st('Uids'), [p.netuid, p.hotkey_ss58]); return value == null ? null : num(value) },
  associated_evm_key: async (view: ReadView, p: { netuid: number; uid: number }) => { const value = await view.query(st('AssociatedEvmAddress'), [p.netuid, p.uid]); return value ? { evm_address: String(value[0]), block: num(value[1]) } : null },
}
