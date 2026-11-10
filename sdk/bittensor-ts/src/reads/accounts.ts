import { type ReadView, type ChainRecord, st, num, bal, toHex, blockOf } from './base'
export const accountReads = {
  balance: async (view: ReadView, p: { coldkey_ss58: string }) => bal(view, (await view.query(['System', 'Account'], [p.coldkey_ss58]))?.data?.free),
  balances: async (view: ReadView, p: { coldkey_ss58s: string[] }) => {
    const rows = await view.queryBatch(['System', 'Account'], p.coldkey_ss58s.map(key => [key]))
    return Object.fromEntries(p.coldkey_ss58s.map((key, i) => [key, bal(view, rows[i]?.data?.free)]))
  },
  existential_deposit: async (view: ReadView) => bal(view, await view.constant(['Balances', 'ExistentialDeposit'])),
  proxies: async (view: ReadView, p: { coldkey_ss58: string }) => {
    const [rows, deposit] = await view.query(['Proxy', 'Proxies'], [p.coldkey_ss58]) ?? [[], 0n]
    return { proxies: (rows as ChainRecord[]).map((d: ChainRecord) => ({ delegate: String(d.delegate), proxy_type: typeof d.proxy_type === 'string' ? d.proxy_type : Object.keys(d.proxy_type)[0], delay: num(d.delay) })), deposit: bal(view, deposit) }
  },
  coldkey_swap_announcement: async (view: ReadView, p: { coldkey_ss58: string }) => {
    const [value, dispute] = await Promise.all([view.query(st('ColdkeySwapAnnouncements'), [p.coldkey_ss58]), view.query(st('ColdkeySwapDisputes'), [p.coldkey_ss58])])
    const recovery = dispute != null && num(dispute) === 0
    if (!value && !recovery) return null
    return { execute_block: value ? num(value[0]) : null, new_coldkey_hash: value ? String(value[1]) : null, disputed: Boolean(dispute), dispute_block: dispute ? num(dispute) : null, recovery_locked: recovery }
  },
  multisig: async (view: ReadView, p: { account_ss58: string; call_hash: string }) => {
    const hash = p.call_hash.startsWith('0x') ? p.call_hash : `0x${p.call_hash}`
    const value = await view.query(['Multisig', 'Multisigs'], [p.account_ss58, hash])
    return value ? { timepoint: { height: num(value.when?.height), index: num(value.when?.index) }, deposit: bal(view, value.deposit), depositor: String(value.depositor), approvals: (value.approvals ?? []).map(String) as string[] } : null
  },
  timestamp: async (view: ReadView) => { view = await view.at(); const date = await view.timestamp(); return { block: blockOf(view), unix: date.getTime() / 1000, iso: date.toISOString() } },
  block_time: (view: ReadView) => view.blockTime(),
  is_fast_blocks: (view: ReadView) => view.isFastBlocks(),
  block_info: async (view: ReadView, p: { block: number }) => {
    const info = await view.blockInfo(p.block)
    if (!info) return null
    const stamp: unknown = info.timestamp
    return { number: num(info.number), hash: String(info.hash), timestamp: stamp == null ? null : stamp instanceof Date ? stamp.toISOString() : String(stamp), extrinsics: ((info.decodedExtrinsics ?? info.extrinsics ?? []) as ChainRecord[]).map(value => value?.call ? `${value.call.call_module}.${value.call.call_function}` : null), explorer_url: info.explorer_url == null && info.explorerUrl == null ? null : String(info.explorer_url ?? info.explorerUrl) }
  },
  mev_shield_next_key: async (view: ReadView): Promise<string | null> => { const value = await view.query(['MevShield', 'NextKey']); return value ? (typeof value === 'string' ? value : toHex(Uint8Array.from(value))) : null },
  tx_rate_limit: async (view: ReadView) => num(await view.query(st('TxRateLimit'))),
}
