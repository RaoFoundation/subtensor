import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import type { TypedApi } from "polkadot-api";
import { sudoSetAdminFreezeWindow } from "./staking.js";
import { waitForTransactionWithRetry } from "./transactions.js";

export const GAMMA = BigInt(1000000000); // 10^9 units per gamma
// $400 per TAO, in gamma units per TAO.
export const TAO_USD_PRICE = BigInt(400) * GAMMA;

export function gamma(value: number): bigint {
    return GAMMA * BigInt(value);
}

export async function sudoSetGammaEnabled(api: TypedApi<typeof subtensor>, enabled: boolean): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_gamma_enabled({ enabled: enabled });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_gamma_enabled");
}

export async function sudoSetSubnetGammaEnabled(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    enabled: boolean
): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_subnet_gamma_enabled({ netuid: netuid, enabled: enabled });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_subnet_gamma_enabled");
}

export async function sudoSetTaoUsdPriceOracle(api: TypedApi<typeof subtensor>, oracle: string): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_tao_usd_price_oracle({ oracle: oracle });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_tao_usd_price_oracle");
}

/** Feeds the TAO/USD price as Alice, the oracle `enableGamma` sets. A mint needs a feed at
 * most `TAO_USD_PRICE_MAX_AGE` blocks old, so feed one shortly before minting. */
export async function feedTaoUsdPrice(api: TypedApi<typeof subtensor>, price: bigint = TAO_USD_PRICE): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const tx = api.tx.SubtensorModule.feed_tao_usd_price({ price: price });
    await waitForTransactionWithRetry(api, tx, alice, "feed_tao_usd_price");
}

/** Enables gamma network-wide and on `netuid`, makes Alice the TAO/USD oracle and feeds
 * `TAO_USD_PRICE`. */
export async function enableGamma(api: TypedApi<typeof subtensor>, netuid: number): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    await sudoSetAdminFreezeWindow(api, 0);
    await sudoSetGammaEnabled(api, true);
    await sudoSetSubnetGammaEnabled(api, netuid, true);
    await sudoSetTaoUsdPriceOracle(api, alice.address);
    await feedTaoUsdPrice(api);
}

export async function mintGamma(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.mint_gamma({
        hotkey: hotkey,
        netuid: netuid,
        amount: amount,
        min_amount_out: BigInt(0),
        daily_spend_cap: undefined,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "mint_gamma");
}

export async function depositGamma(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.deposit_gamma({ hotkey: hotkey, netuid: netuid, amount: amount });
    await waitForTransactionWithRetry(api, tx, coldkey, "deposit_gamma");
}

export async function withdrawGamma(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.withdraw_gamma({ hotkey: hotkey, netuid: netuid, amount: amount });
    await waitForTransactionWithRetry(api, tx, coldkey, "withdraw_gamma");
}

export async function transferGamma(
    api: TypedApi<typeof subtensor>,
    originColdkey: KeyringPair,
    destinationColdkey: string,
    originHotkey: string | undefined,
    destinationHotkey: string | undefined,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.transfer_gamma({
        destination_coldkey: destinationColdkey,
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        netuid: netuid,
        amount: amount,
    });
    await waitForTransactionWithRetry(api, tx, originColdkey, "transfer_gamma");
}

export async function getGammaBalance(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    coldkey: string
): Promise<bigint> {
    return await api.query.SubtensorModule.GammaBalances.getValue(netuid, coldkey);
}

export async function getGammaDeposit(api: TypedApi<typeof subtensor>, netuid: number, hotkey: string) {
    return await api.query.SubtensorModule.GammaDeposits.getValue(netuid, hotkey);
}
