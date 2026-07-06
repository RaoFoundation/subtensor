import { expect, beforeAll } from "vitest";
import {
    addNewSubnetwork,
    addStake,
    burnedRegister,
    forceSetBalance,
    generateKeyringPair,
    getRootClaimable,
    startCall,
    sudoSetAdminFreezeWindow,
    sudoSetEmaPriceHalvingPeriod,
    sudoSetLockReductionInterval,
    sudoSetRootClaimThreshold,
    sudoSetSubnetMovingAlpha,
    sudoSetSubtokenEnabled,
    sudoSetTempo,
    tao,
    waitForBlocks,
} from "../../utils";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import { swapHotkey } from "../../utils/swap.ts";
import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";

// Shared setup: creates two subnets, registers oldHotkey on both,
// stakes on ROOT and both subnets, waits for RootClaimable to accumulate.
async function setupTwoSubnetsWithClaimable(
    api: TypedApi<typeof subtensor>,
    ROOT_NETUID: number,
    log: (msg: string) => void
): Promise<{
    oldHotkey: KeyringPair;
    oldHotkeyColdkey: KeyringPair;
    newHotkey: KeyringPair;
    netuid1: number;
    netuid2: number;
}> {
    const oldHotkey = generateKeyringPair("sr25519");
    const oldHotkeyColdkey = generateKeyringPair("sr25519");
    const newHotkey = generateKeyringPair("sr25519");
    const owner1Hotkey = generateKeyringPair("sr25519");
    const owner1Coldkey = generateKeyringPair("sr25519");
    const owner2Hotkey = generateKeyringPair("sr25519");
    const owner2Coldkey = generateKeyringPair("sr25519");

    for (const kp of [
        oldHotkey,
        oldHotkeyColdkey,
        newHotkey,
        owner1Hotkey,
        owner1Coldkey,
        owner2Hotkey,
        owner2Coldkey,
    ]) {
        await forceSetBalance(api, kp.address);
    }

    await sudoSetAdminFreezeWindow(api, 0);
    await sudoSetSubtokenEnabled(api, ROOT_NETUID, true);

    const netuid1 = await addNewSubnetwork(api, owner1Hotkey, owner1Coldkey);
    await startCall(api, netuid1, owner1Coldkey);
    log(`Created netuid1: ${netuid1}`);

    const netuid2 = await addNewSubnetwork(api, owner2Hotkey, owner2Coldkey);
    await startCall(api, netuid2, owner2Coldkey);
    log(`Created netuid2: ${netuid2}`);

    for (const netuid of [netuid1, netuid2]) {
        await sudoSetTempo(api, netuid, 5);
        await sudoSetEmaPriceHalvingPeriod(api, netuid, 1);
        await sudoSetRootClaimThreshold(api, netuid, 0n);
    }
    await sudoSetSubnetMovingAlpha(api, BigInt(4294967296));

    // Register oldHotkey on both subnets so it appears in epoch hotkey_emission
    // and receives root_alpha_dividends → RootClaimable on both netuids
    await burnedRegister(api, netuid1, oldHotkey.address, oldHotkeyColdkey);
    log("oldHotkey registered on netuid1");
    await burnedRegister(api, netuid2, oldHotkey.address, oldHotkeyColdkey);
    log("oldHotkey registered on netuid2");

    // ROOT stake drives root_alpha_dividends for oldHotkey
    await addStake(api, oldHotkeyColdkey, oldHotkey.address, ROOT_NETUID, tao(100));
    log("Added ROOT stake for oldHotkey");

    await addStake(api, oldHotkeyColdkey, oldHotkey.address, netuid1, tao(50));
    await addStake(api, oldHotkeyColdkey, oldHotkey.address, netuid2, tao(50));

    await addStake(api, owner1Coldkey, owner1Hotkey.address, netuid1, tao(50));
    await addStake(api, owner2Coldkey, owner2Hotkey.address, netuid2, tao(50));

    const waitBlocks = 90;
    log(`Waiting ${waitBlocks} blocks for RootClaimable to accumulate on both subnets...`);
    await waitForBlocks(api, waitBlocks);

    return { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1, netuid2 };
}

describeSuite({
    id: "0204_claim-root_hotkey_swap",
    title: "▶ swap_hotkey RootClaimable per-subnet transfer",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;
        const ROOT_NETUID = 0;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await sudoSetLockReductionInterval(api, 1);
        });

        it({
            id: "T01",
            title: "single-subnet swap doesn't move root claimable if it is not root",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1, netuid2 } = await setupTwoSubnetsWithClaimable(
                    api,
                    ROOT_NETUID,
                    log
                );

                const claimableMapBefore = await getRootClaimable(api, oldHotkey.address);
                log(
                    `RootClaimable[oldHotkey] before swap: ${
                        [...claimableMapBefore.entries()].map(([k, v]) => `netuid${k}=${v}`).join(", ") || "(none)"
                    }`
                );

                expect(
                    claimableMapBefore.get(netuid1) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid1 before swap"
                ).toBeGreaterThan(0n);
                expect(
                    claimableMapBefore.get(netuid2) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid2 before swap"
                ).toBeGreaterThan(0n);
                expect(
                    (await getRootClaimable(api, newHotkey.address)).size,
                    "newHotkey should have no RootClaimable before swap"
                ).toBe(0);

                // Swap oldHotkey → newHotkey on netuid1 ONLY
                log(`Swapping oldHotkey → newHotkey on netuid1=${netuid1} only...`);
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address, netuid1);
                log("Swap done");

                const oldAfter = await getRootClaimable(api, oldHotkey.address);
                const newAfter = await getRootClaimable(api, newHotkey.address);

                log(
                    `RootClaimable[oldHotkey] after swap: netuid1=${oldAfter.get(netuid1) ?? 0n}, netuid2=${oldAfter.get(netuid2) ?? 0n}`
                );
                log(
                    `RootClaimable[newHotkey] after swap: netuid1=${newAfter.get(netuid1) ?? 0n}, netuid2=${newAfter.get(netuid2) ?? 0n}`
                );

                expect(newAfter.get(netuid1) ?? 0n, "newHotkey should not have RootClaimable for netuid1").toEqual(0n);
                expect(
                    oldAfter.get(netuid1) ?? 0n,
                    "oldHotkey should retain RootClaimable for netuid1"
                ).toBeGreaterThan(0n);

                expect(
                    oldAfter.get(netuid2) ?? 0n,
                    "oldHotkey should retain RootClaimable for netuid2"
                ).toBeGreaterThan(0n);
                expect(newAfter.get(netuid2) ?? 0n, "newHotkey should have no RootClaimable for netuid2").toBe(0n);

                log(
                    "✅ Single-subnet swap doesn't transfer RootClaimable for the subnet if it was done for non-root subnet"
                );
            },
        });

        it({
            id: "T02",
            title: "full swap (no netuid) moves RootClaimable for all subnets to newHotkey",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1, netuid2 } = await setupTwoSubnetsWithClaimable(
                    api,
                    ROOT_NETUID,
                    log
                );

                const claimableMapBefore = await getRootClaimable(api, oldHotkey.address);
                log(
                    `RootClaimable[oldHotkey] before swap: ${
                        [...claimableMapBefore.entries()].map(([k, v]) => `netuid${k}=${v}`).join(", ") || "(none)"
                    }`
                );

                expect(
                    claimableMapBefore.get(netuid1) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid1 before swap"
                ).toBeGreaterThan(0n);
                expect(
                    claimableMapBefore.get(netuid2) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid2 before swap"
                ).toBeGreaterThan(0n);

                // Full swap — no netuid
                log("Swapping oldHotkey → newHotkey on ALL subnets...");
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address);
                log("Swap done");

                const oldAfter = await getRootClaimable(api, oldHotkey.address);
                const newAfter = await getRootClaimable(api, newHotkey.address);

                log(
                    `RootClaimable[oldHotkey] after swap: netuid1=${oldAfter.get(netuid1) ?? 0n}, netuid2=${oldAfter.get(netuid2) ?? 0n}`
                );
                log(
                    `RootClaimable[newHotkey] after swap: netuid1=${newAfter.get(netuid1) ?? 0n}, netuid2=${newAfter.get(netuid2) ?? 0n}`
                );

                expect(newAfter.get(netuid1) ?? 0n, "newHotkey should have RootClaimable for netuid1").toBeGreaterThan(
                    0n
                );
                expect(newAfter.get(netuid2) ?? 0n, "newHotkey should have RootClaimable for netuid2").toBeGreaterThan(
                    0n
                );

                expect(oldAfter.get(netuid1) ?? 0n, "oldHotkey should have no RootClaimable for netuid1").toBe(0n);
                expect(oldAfter.get(netuid2) ?? 0n, "oldHotkey should have no RootClaimable for netuid2").toBe(0n);

                log("✅ Full swap correctly transferred RootClaimable for both subnets to newHotkey");
            },
        });

        it({
            id: "T03",
            title: "single-subnet swap moves root claimable if it is root",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1, netuid2 } = await setupTwoSubnetsWithClaimable(
                    api,
                    ROOT_NETUID,
                    log
                );

                const claimableMapBefore = await getRootClaimable(api, oldHotkey.address);
                log(
                    `RootClaimable[oldHotkey] before swap: ${
                        [...claimableMapBefore.entries()].map(([k, v]) => `netuid${k}=${v}`).join(", ") || "(none)"
                    }`
                );

                expect(
                    claimableMapBefore.get(netuid1) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid1 before swap"
                ).toBeGreaterThan(0n);
                expect(
                    claimableMapBefore.get(netuid2) ?? 0n,
                    "oldHotkey should have RootClaimable on netuid2 before swap"
                ).toBeGreaterThan(0n);

                log("Swapping oldHotkey → newHotkey for root subnet...");
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address, 0);
                log("Swap done");

                const oldAfter = await getRootClaimable(api, oldHotkey.address);
                const newAfter = await getRootClaimable(api, newHotkey.address);

                log(
                    `RootClaimable[oldHotkey] after swap: netuid1=${oldAfter.get(netuid1) ?? 0n}, netuid2=${oldAfter.get(netuid2) ?? 0n}`
                );
                log(
                    `RootClaimable[newHotkey] after swap: netuid1=${newAfter.get(netuid1) ?? 0n}, netuid2=${newAfter.get(netuid2) ?? 0n}`
                );

                expect(newAfter.get(netuid1) ?? 0n, "newHotkey should have RootClaimable for netuid1").toBeGreaterThan(
                    0n
                );
                expect(newAfter.get(netuid2) ?? 0n, "newHotkey should have RootClaimable for netuid2").toBeGreaterThan(
                    0n
                );

                expect(oldAfter.get(netuid1) ?? 0n, "oldHotkey should have no RootClaimable for netuid1").toBe(0n);
                expect(oldAfter.get(netuid2) ?? 0n, "oldHotkey should have no RootClaimable for netuid2").toBe(0n);

                log("✅ Single swap correctly transferred RootClaimable if it is done for root subnet");
            },
        });
    },
});
