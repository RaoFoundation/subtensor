import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import type { TypedApi } from "polkadot-api";
import { waitForTransactionWithRetry } from "./transactions.js";

export async function addStake(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.add_stake({
        hotkey: hotkey,
        netuid: netuid,
        amount_staked: amount,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "add_stake");
}

export async function addStakeLimit(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint,
    limitPrice: bigint,
    allowPartial: boolean
): Promise<void> {
    const tx = api.tx.SubtensorModule.add_stake_limit({
        hotkey: hotkey,
        netuid: netuid,
        amount_staked: amount,
        limit_price: limitPrice,
        allow_partial: allowPartial,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "add_stake_limit");
}

export async function removeStake(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.remove_stake({
        hotkey: hotkey,
        netuid: netuid,
        amount_unstaked: amount,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "remove_stake");
}

export async function removeStakeLimit(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint,
    limitPrice: bigint,
    allowPartial: boolean
): Promise<void> {
    const tx = api.tx.SubtensorModule.remove_stake_limit({
        hotkey: hotkey,
        netuid: netuid,
        amount_unstaked: amount,
        limit_price: limitPrice,
        allow_partial: allowPartial,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "remove_stake_limit");
}

export async function removeStakeFullLimit(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    limitPrice: bigint | undefined
): Promise<void> {
    const tx = api.tx.SubtensorModule.remove_stake_full_limit({
        hotkey: hotkey,
        netuid: netuid,
        limit_price: limitPrice,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "remove_stake_full_limit");
}

export async function unstakeAll(api: TypedApi<typeof subtensor>, coldkey: KeyringPair, hotkey: string): Promise<void> {
    const tx = api.tx.SubtensorModule.unstake_all({
        hotkey: hotkey,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "unstake_all");
}

export async function unstakeAllAlpha(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string
): Promise<void> {
    const tx = api.tx.SubtensorModule.unstake_all_alpha({
        hotkey: hotkey,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "unstake_all_alpha");
}

/**
 * Get stake shares (Alpha) for a hotkey/coldkey/netuid triplet.
 * Returns the integer part of the U64F64 value.
 */
export async function getStake(
    api: TypedApi<typeof subtensor>,
    hotkey: string,
    coldkey: string,
    netuid: number
): Promise<bigint> {
    const value = await api.query.SubtensorModule.AlphaV2.getValue(hotkey, coldkey, netuid);

    const mantissa = value.mantissa;
    const exponent = value.exponent;

    let result: bigint;

    if (exponent >= 0) {
        result = mantissa * BigInt(10) ** exponent;
    } else {
        result = mantissa / BigInt(10) ** -exponent;
    }

    return result;
}

/**
 * Get raw stake shares (Alpha) in U64F64 format.
 * Use this when you need the raw value for extrinsics like transfer_stake.
 */
export async function getStakeRaw(
    api: TypedApi<typeof subtensor>,
    hotkey: string,
    coldkey: string,
    netuid: number
): Promise<bigint> {
    return await api.query.SubtensorModule.Alpha.getValue(hotkey, coldkey, netuid);
}

export async function transferStake(
    api: TypedApi<typeof subtensor>,
    originColdkey: KeyringPair,
    destinationColdkey: string,
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.transfer_stake({
        destination_coldkey: destinationColdkey,
        hotkey: hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: amount,
    });
    await waitForTransactionWithRetry(api, tx, originColdkey, "transfer_stake");
}

export async function moveStake(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    originHotkey: string,
    destinationHotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.move_stake({
        origin_hotkey: originHotkey,
        destination_hotkey: destinationHotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: amount,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "move_stake");
}

export async function swapStake(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amount: bigint
): Promise<void> {
    const tx = api.tx.SubtensorModule.swap_stake({
        hotkey: hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: amount,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "swap_stake");
}

export async function swapStakeLimit(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    originNetuid: number,
    destinationNetuid: number,
    amount: bigint,
    limitPrice: bigint,
    allowPartial: boolean
): Promise<void> {
    const tx = api.tx.SubtensorModule.swap_stake_limit({
        hotkey: hotkey,
        origin_netuid: originNetuid,
        destination_netuid: destinationNetuid,
        alpha_amount: amount,
        limit_price: limitPrice,
        allow_partial: allowPartial,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "swap_stake_limit");
}

export type RootClaimType = "Swap" | "Keep" | { type: "KeepSubnets"; subnets: number[] };

export async function getRootClaimType(api: TypedApi<typeof subtensor>, coldkey: string): Promise<RootClaimType> {
    const result = await api.query.SubtensorModule.RootClaimType.getValue(coldkey);
    if (result.type === "KeepSubnets") {
        return { type: "KeepSubnets", subnets: result.value.subnets as number[] };
    }
    return result.type as "Swap" | "Keep";
}

export async function setRootClaimType(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    claimType: RootClaimType
): Promise<void> {
    let newRootClaimType;
    if (typeof claimType === "string") {
        newRootClaimType = { type: claimType, value: undefined };
    } else {
        newRootClaimType = { type: "KeepSubnets", value: { subnets: claimType.subnets } };
    }
    const tx = api.tx.SubtensorModule.set_root_claim_type({
        new_root_claim_type: newRootClaimType,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "set_root_claim_type");
}

export async function claimRoot(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    subnets: number[]
): Promise<void> {
    const tx = api.tx.SubtensorModule.claim_root({
        subnets: subnets,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "claim_root");
}

export async function getNumRootClaims(api: TypedApi<typeof subtensor>): Promise<bigint> {
    return await api.query.SubtensorModule.NumRootClaim.getValue();
}

export async function sudoSetNumRootClaims(api: TypedApi<typeof subtensor>, newValue: bigint): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.SubtensorModule.sudo_set_num_root_claims({
        new_value: newValue,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_num_root_claims");
}

export async function getRootClaimThreshold(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    return await api.query.SubtensorModule.RootClaimableThreshold.getValue(netuid);
}

export async function sudoSetRootClaimThreshold(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    newValue: bigint
): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.SubtensorModule.sudo_set_root_claim_threshold({
        netuid: netuid,
        new_value: newValue,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_root_claim_threshold");
}

export async function getTempo(api: TypedApi<typeof subtensor>, netuid: number): Promise<number> {
    return await api.query.SubtensorModule.Tempo.getValue(netuid);
}

export async function sudoSetTempo(api: TypedApi<typeof subtensor>, netuid: number, tempo: number): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_tempo({
        netuid: netuid,
        tempo: tempo,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_tempo");
}

export async function waitForBlocks(api: TypedApi<typeof subtensor>, numBlocks: number): Promise<void> {
    const startBlock = await api.query.System.Number.getValue();
    const targetBlock = startBlock + numBlocks;

    while (true) {
        const currentBlock = await api.query.System.Number.getValue();
        if (currentBlock >= targetBlock) {
            break;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
    }
}

export async function getRootClaimable(api: TypedApi<typeof subtensor>, hotkey: string): Promise<Map<number, bigint>> {
    const result = await api.query.SubtensorModule.RootClaimable.getValue(hotkey);
    const claimableMap = new Map<number, bigint>();
    for (const [netuid, amount] of result) {
        claimableMap.set(netuid, amount);
    }
    return claimableMap;
}

export async function getRootClaimed(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    hotkey: string,
    coldkey: string
): Promise<bigint> {
    return await api.query.SubtensorModule.RootClaimed.getValue(netuid, hotkey, coldkey);
}

export async function isSubtokenEnabled(api: TypedApi<typeof subtensor>, netuid: number): Promise<boolean> {
    return await api.query.SubtensorModule.SubtokenEnabled.getValue(netuid);
}

export async function sudoSetSubtokenEnabled(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    enabled: boolean
): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_subtoken_enabled({
        netuid: netuid,
        subtoken_enabled: enabled,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_subtoken_enabled");
}

export async function isNetworkAdded(api: TypedApi<typeof subtensor>, netuid: number): Promise<boolean> {
    return await api.query.SubtensorModule.NetworksAdded.getValue(netuid);
}

export async function getAdminFreezeWindow(api: TypedApi<typeof subtensor>): Promise<number> {
    return await api.query.SubtensorModule.AdminFreezeWindow.getValue();
}

export async function sudoSetAdminFreezeWindow(api: TypedApi<typeof subtensor>, window: number): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_admin_freeze_window({
        window: window,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_admin_freeze_window");
}

export async function sudoSetEmaPriceHalvingPeriod(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    emaPriceHalvingPeriod: number
): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_ema_price_halving_period({
        netuid: netuid,
        ema_halving: BigInt(emaPriceHalvingPeriod),
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_ema_price_halving_period");
}

export async function sudoSetLockReductionInterval(api: TypedApi<typeof subtensor>, interval: number): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_lock_reduction_interval({
        interval: BigInt(interval),
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_lock_reduction_interval");
}

export async function sudoSetSubnetMovingAlpha(api: TypedApi<typeof subtensor>, alpha: bigint): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_subnet_moving_alpha({
        alpha: alpha,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_subnet_moving_alpha");
}

// Debug helpers for claim_root investigation
export async function getSubnetTAO(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    return await api.query.SubtensorModule.SubnetTAO.getValue(netuid);
}

export async function getSubnetMovingPrice(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    return await api.query.SubtensorModule.SubnetMovingPrice.getValue(netuid);
}

export async function getPendingRootAlphaDivs(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    return await api.query.SubtensorModule.PendingRootAlphaDivs.getValue(netuid);
}

export async function getTaoWeight(api: TypedApi<typeof subtensor>): Promise<bigint> {
    return await api.query.SubtensorModule.TaoWeight.getValue();
}

export async function getSubnetAlphaIn(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    return await api.query.SubtensorModule.SubnetAlphaIn.getValue(netuid);
}

export async function getTotalHotkeyAlpha(
    api: TypedApi<typeof subtensor>,
    hotkey: string,
    netuid: number
): Promise<bigint> {
    return await api.query.SubtensorModule.TotalHotkeyAlpha.getValue(hotkey, netuid);
}

export async function getStakeInfoForHotkeyColdkeyNetuid(
    api: TypedApi<typeof subtensor>,
    hotkey: string,
    coldkey: string,
    netuid: number
): Promise<bigint | undefined> {
    return (await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(hotkey, coldkey, netuid))
        ?.stake;
}
