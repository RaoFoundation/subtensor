import { gammaFromUnits, gammaUnits, resolveGammaQuote } from '../gamma'
import { type ReadView, api, st, integer, num, blockOf } from './base'

export interface GammaDepositRecord {
  netuid: number
  hotkey: string
  coldkey: string
  spender: string
  balance_gamma: string
  daily_spend_cap_gamma: string | null
  spendable_gamma: string
}
export interface GammaMintQuote {
  netuid: number
  alpha_burned: ReturnType<ReadView['balance']>
  tao_value: ReturnType<ReadView['balance']>
  gamma: string
}
export interface TaoUsdPrice {
  oracle: string | null
  /** Display values; use the decimal string companions for exact arithmetic. */
  accepted_usd: number | null
  feed_usd: number | null
  accepted_usd_string: string | null
  feed_usd_string: string | null
  fed_at: number | null
  blocks_since_last_feed: number | null
  stale: boolean
}

function depositRecord(info: any): GammaDepositRecord {
  if (!info?.deposit) throw new TypeError('invalid Gamma deposit record')
  const deposit = info.deposit
  return { netuid: num(info.netuid), hotkey: String(info.hotkey), coldkey: String(deposit.coldkey), spender: String(deposit.spender), balance_gamma: gammaFromUnits(deposit.balance), daily_spend_cap_gamma: deposit.daily_spend_cap == null ? null : gammaFromUnits(deposit.daily_spend_cap.cap), spendable_gamma: gammaFromUnits(info.spendable) }
}
async function mintQuote(view: ReadView, method: string, netuid: number, amount: number | string): Promise<GammaMintQuote> {
  const result = await view.runtime(api('GammaRuntimeApi', method), [netuid, gammaUnits(amount)])
  const [alpha, tao, gamma] = await resolveGammaQuote(result, view.moduleError?.bind(view))
  return { netuid, alpha_burned: view.balance(alpha, netuid), tao_value: view.balance(tao, 0), gamma: gammaFromUnits(gamma) }
}

/** Reads run on the block-pinned ReadView selected by dispatchRead. */
export const gammaReads = {
  gamma_deposit: async (view: ReadView, p: { netuid: number; hotkey_ss58: string }): Promise<GammaDepositRecord | null> => {
    const info = await view.runtime(api('GammaRuntimeApi', 'get_gamma_deposit'), [p.netuid, p.hotkey_ss58])
    return info == null ? null : depositRecord(info)
  },
  gamma_deposits: async (view: ReadView, p: { coldkey_ss58: string }): Promise<GammaDepositRecord[]> => {
    const infos = await view.runtime(api('GammaRuntimeApi', 'get_gamma_deposits_for_coldkey'), [p.coldkey_ss58])
    if (infos != null && !Array.isArray(infos)) throw new TypeError('invalid Gamma deposit list')
    return (infos ?? []).map(depositRecord)
  },
  quote_mint_gamma: (view: ReadView, p: { netuid: number; amount_tao: number | string }) => mintQuote(view, 'sim_mint_gamma', p.netuid, p.amount_tao),
  quote_mint_gamma_with_stake: (view: ReadView, p: { netuid: number; amount_alpha: number | string }) => mintQuote(view, 'sim_mint_gamma_with_stake', p.netuid, p.amount_alpha),
  tao_usd_price: async (view: ReadView): Promise<TaoUsdPrice> => {
    const [oracle, state] = await Promise.all([view.query(st('TaoUsdPriceOracle')), view.query(st('TaoUsdPrice'))])
    if (state == null) return { oracle: oracle == null ? null : String(oracle), accepted_usd: null, feed_usd: null, accepted_usd_string: null, feed_usd_string: null, fed_at: null, blocks_since_last_feed: null, stale: true }
    const fedAt = num(state.fed_at), blocks = Math.max(0, blockOf(view) - fedAt)
    const accepted = integer(state.accepted), feed = integer(state.feed)
    // Runtime Permill-equivalent rate is 66,000 per billion of the stored
    // accepted price, rounded UP once; it does not compound between feeds.
    const step = (accepted * 66_000n + 999_999_999n) / 1_000_000_000n
    const raised = accepted + step * BigInt(blocks)
    const acceptedText = gammaFromUnits(raised < feed ? raised : feed), feedText = gammaFromUnits(feed)
    return { oracle: oracle == null ? null : String(oracle), accepted_usd: Number(acceptedText), feed_usd: Number(feedText), accepted_usd_string: acceptedText, feed_usd_string: feedText, fed_at: fedAt, blocks_since_last_feed: blocks, stale: blocks > 300 }
  },
}
