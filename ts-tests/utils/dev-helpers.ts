/**
 * Polkadot.js (ApiPromise) compatible helpers for dev tests.
 * Uses ApiPromise, not PAPI TypedApi — keep them separate.
 */
import type { ApiPromise } from "@polkadot/api";
import { tao } from "./balance.ts";
import type { DevModeContext } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { SignedOrder } from "./index.js";

export async function devForceSetBalance(
    polkadotJs: ApiPromise,
    context: any,
    address: string,
    amount: bigint = tao(1e10)
): Promise<void> {
    await context.createBlock([
        await polkadotJs.tx.sudo
            .sudo(polkadotJs.tx.balances.forceSetBalance(address, amount))
            .signAsync(context.keyring.alice),
    ]);
}

export async function devSetWeightsTx(
    api: ApiPromise,
    context: DevModeContext,
    coldkey: KeyringPair,
    netuid: number,
    uids: number[],
    values: number[],
    versionKey: bigint
): Promise<void> {
    await context.createBlock([
        await api.tx.subtensorModule.setWeights(netuid, uids, values, versionKey).signAsync(coldkey),
    ]);
}

export async function devAddStake(
    polkadotJs: ApiPromise,
    context: any,
    coldkey: KeyringPair,
    hotkey: string,
    netuid: number,
    amount: bigint
): Promise<void> {
    await context.createBlock([
        await polkadotJs.tx.subtensorModule.addStake(hotkey, netuid, amount).signAsync(coldkey),
    ]);
}

export async function devAssociateHotKey(
    polkadotJs: ApiPromise,
    context: any,
    coldkey: KeyringPair,
    hotkey: string
): Promise<void> {
    await context.createBlock([await polkadotJs.tx.subtensorModule.tryAssociateHotkey(hotkey).signAsync(coldkey)]);
}

export async function devGetAlphaStake(
    polkadotJs: ApiPromise,
    hotkey: string,
    coldkey: string,
    netuid: number
): Promise<bigint> {
    const value = await polkadotJs.query.subtensorModule.alphaV2(hotkey, coldkey, netuid);

    const mantissa = value.mantissa;
    const exponent = value.exponent;

    let result: bigint;

    if (exponent >= 0n) {
        result = BigInt(mantissa) * BigInt(10) ** BigInt(exponent);
    } else {
        result = BigInt(mantissa) / BigInt(10) ** BigInt(-exponent);
    }

    return result;
}

export async function devSudoSetLockReductionInterval(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    interval: number
): Promise<void> {
    await context.createBlock([await polkadotJs.tx.adminUtils.sudoSetLockReductionInterval(interval).signAsync(alice)]);
}

export async function devRegisterSubnet(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    hotkey: KeyringPair
): Promise<number> {
    await context.createBlock([await polkadotJs.tx.subtensorModule.registerNetwork(hotkey.address).signAsync(alice)]);
    const events = (await polkadotJs.query.system.events()) as any;
    const netuid = (events as any[]).filter((e: any) => e.event.method === "NetworkAdded")[0].event.data[0].toNumber();
    return netuid;
}

export async function devEnableSubtoken(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    netuid: number
): Promise<void> {
    await context.createBlock([
        await polkadotJs.tx.sudo.sudo(polkadotJs.tx.adminUtils.sudoSetSubtokenEnabled(netuid, true)).signAsync(alice),
    ]);
}
export async function devExecuteOrders(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    orders: SignedOrder[],
    shouldFail = false
): Promise<void> {
    await context.createBlock([await polkadotJs.tx.limitOrders.executeOrders(orders, shouldFail).signAsync(alice)]);
}
