import { buildIntent, intentBuilders, type IntentJSON } from '../../dist/intents'
import { Balance } from '../../dist/balance'

const transfer = buildIntent('transfer', { dest_ss58: 'dest', amount_tao: Balance.fromTao('1.5') })
buildIntent('set_weights', { netuid: 1, uids: [0, 1], weights: [0.25, 0.75], version_key: 9007199254740993n })
buildIntent('remove_proxies')
intentBuilders.batch({ intents: [transfer, { op: 'transfer', dest_ss58: 'dest', amount_tao: '2' }] })
intentBuilders.multisig_execute({ threshold: 2, other_signatories: ['other'], call: transfer })
const spec: IntentJSON = { op: 'set_hyperparameter', netuid: 1, name: 'tempo', value: 360 }
const noArgs: IntentJSON = { op: 'clear_coldkey_swap_announcement' }
void spec
void noArgs
// @ts-expect-error amount_tao is required
buildIntent('transfer', { dest_ss58: 'dest' })
// @ts-expect-error misspelled fields are rejected
intentBuilders.add_stake({ netuid: 1, amount_tao: '1', hotkey: 'wrong-field' })
// @ts-expect-error operation does not exist
buildIntent('stake_everything')
// @ts-expect-error money inputs require explicit decimal amounts or Balance, not raw bigint
buildIntent('transfer', { dest_ss58: 'dest', amount_tao: 1000000000n })
// @ts-expect-error recursive specs still validate required child fields
intentBuilders.batch({ intents: [{ op: 'transfer', amount_tao: '1' }] })
// @ts-expect-error operation with required arguments cannot omit the argument object
intentBuilders.add_collateral()

buildIntent('mint_gamma', { netuid: 1, hotkey_ss58: 'api-key', amount_tao: Balance.fromTao('1'), daily_spend_cap_gamma: '0' })
intentBuilders.mint_gamma_with_stake({ netuid: 1, origin_hotkey_ss58: 'validator', hotkey_ss58: 'api-key', amount_alpha: 'all' })
intentBuilders.spend_gamma({ netuid: 1, spends: [{ hotkey_ss58: 'api-key', amount_gamma: '1.000000001', reference: 'ab'.repeat(32) }] })
intentBuilders.set_service({ netuid: 1, url: 'https://example.test/services' })
// @ts-expect-error Gamma credits are not a TAO/alpha Balance
intentBuilders.move_gamma({ netuid: 1, origin_hotkey_ss58: 'a', destination_hotkey_ss58: 'b', amount_gamma: Balance.fromTao('1') })
// @ts-expect-error Gamma spend entries require decimal amounts, not raw units
intentBuilders.spend_gamma({ netuid: 1, spends: [{ hotkey_ss58: 'api-key', amount_gamma: 1n }] })
// @ts-expect-error Every Gamma spend entry names its hotkey
intentBuilders.spend_gamma({ netuid: 1, spends: [{ amount_gamma: '1' }] })
