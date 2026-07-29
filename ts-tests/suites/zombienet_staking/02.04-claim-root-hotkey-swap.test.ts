import { expect, beforeAll } from "vitest";
import {
    addNewSubnetwork,
    addStake,
    burnedRegister,
    forceSetBalance,
    generateKeyringPair,
    getBasketRate,
    getBasketShares,
    setRootWeights,
    startCall,
    sudoSetAdminFreezeWindow,
    sudoSetEmaPriceHalvingPeriod,
    sudoSetLockReductionInterval,
    sudoSetRootClaimThreshold,
    sudoSetSubnetMovingAlpha,
    sudoSetSubtokenEnabled,
    sudoSetTempo,
    sudoSetWeightsSetRateLimit,
    tao,
    waitForBlocks,
} from "../../utils";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import { rootRegister } from "../../utils/subnet.ts";
import { swapHotkey } from "../../utils/swap.ts";
import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";

// Shared setup: creates two subnets, registers oldHotkey on both (and on root), points its
// basket weight vector at the subnets, stakes on ROOT and both subnets, then waits for the
// unified basket fund (BasketRate / BasketShares) to accumulate.
async function setupTwoSubnetsWithBasket(
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
    }
    await sudoSetRootClaimThreshold(api, ROOT_NETUID, 0n);
    await sudoSetSubnetMovingAlpha(api, BigInt(4294967296));

    // Register oldHotkey on both subnets so it appears in epoch hotkey_emission
    // and receives root_alpha_dividends
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

    // Register oldHotkey on the root subnet and point its basket weight vector at both
    // subnets: without weights, root dividends are recycled and no fund accrues.
    await rootRegister(api, oldHotkeyColdkey, oldHotkey.address);
    log("oldHotkey registered on root");
    await sudoSetWeightsSetRateLimit(api, ROOT_NETUID, 0);
    await setRootWeights(api, oldHotkey, [netuid1, netuid2], [32768, 32768]);
    log("Set oldHotkey root weights: 50/50 across netuid1/netuid2");

    log("Waiting 30 blocks for the basket fund to accumulate...");
    await waitForBlocks(api, 30);

    return { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1, netuid2 };
}

describeSuite({
    id: "0204_swap_hotkey_basket_fund",
    title: "▶ swap_hotkey basket fund transfer",
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
            title: "single-subnet swap doesn't move the basket fund if it is not root",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey, netuid1 } = await setupTwoSubnetsWithBasket(
                    api,
                    ROOT_NETUID,
                    log
                );

                const rateBefore = await getBasketRate(api, oldHotkey.address);
                const sharesBefore = await getBasketShares(api, oldHotkey.address);
                log(`oldHotkey fund before swap: rate=${rateBefore}, shares=${sharesBefore}`);

                expect(rateBefore, "oldHotkey should have a basket fund before swap").toBeGreaterThan(0n);
                expect(sharesBefore, "oldHotkey should have fund shares before swap").toBeGreaterThan(0n);
                expect(
                    await getBasketRate(api, newHotkey.address),
                    "newHotkey should have no fund before swap"
                ).toBe(0n);

                // Swap oldHotkey → newHotkey on netuid1 ONLY
                log(`Swapping oldHotkey → newHotkey on netuid1=${netuid1} only...`);
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address, netuid1);
                log("Swap done");

                // The fund is tied to the validator's root identity: a non-root swap must not
                // move any of it.
                expect(
                    await getBasketRate(api, oldHotkey.address),
                    "oldHotkey must retain its fund rate"
                ).toBe(rateBefore);
                expect(
                    await getBasketRate(api, newHotkey.address),
                    "newHotkey must have no fund rate"
                ).toBe(0n);
                expect(
                    await getBasketShares(api, newHotkey.address),
                    "newHotkey must have no fund shares"
                ).toBe(0n);

                log("✅ Non-root single-subnet swap doesn't transfer the basket fund");
            },
        });

        it({
            id: "T02",
            title: "full swap (no netuid) moves the whole basket fund to newHotkey",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey } = await setupTwoSubnetsWithBasket(
                    api,
                    ROOT_NETUID,
                    log
                );

                const rateBefore = await getBasketRate(api, oldHotkey.address);
                const sharesBefore = await getBasketShares(api, oldHotkey.address);
                log(`oldHotkey fund before swap: rate=${rateBefore}, shares=${sharesBefore}`);

                expect(rateBefore, "oldHotkey should have a basket fund before swap").toBeGreaterThan(0n);
                expect(sharesBefore, "oldHotkey should have fund shares before swap").toBeGreaterThan(0n);

                // Full swap — no netuid
                log("Swapping oldHotkey → newHotkey on ALL subnets...");
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address);
                log("Swap done");

                expect(
                    await getBasketRate(api, newHotkey.address),
                    "newHotkey must have oldHotkey's fund rate"
                ).toBe(rateBefore);
                expect(
                    await getBasketShares(api, newHotkey.address),
                    "newHotkey must have oldHotkey's fund shares"
                ).toBe(sharesBefore);
                expect(await getBasketRate(api, oldHotkey.address), "oldHotkey must have no fund left").toBe(0n);
                expect(
                    await getBasketShares(api, oldHotkey.address),
                    "oldHotkey must have no shares left"
                ).toBe(0n);

                log("✅ Full swap correctly transferred the whole basket fund to newHotkey");
            },
        });

        it({
            id: "T03",
            title: "single-subnet swap moves the basket fund if it is root",
            test: async () => {
                const { oldHotkey, oldHotkeyColdkey, newHotkey } = await setupTwoSubnetsWithBasket(
                    api,
                    ROOT_NETUID,
                    log
                );

                const rateBefore = await getBasketRate(api, oldHotkey.address);
                const sharesBefore = await getBasketShares(api, oldHotkey.address);
                log(`oldHotkey fund before swap: rate=${rateBefore}, shares=${sharesBefore}`);

                expect(rateBefore, "oldHotkey should have a basket fund before swap").toBeGreaterThan(0n);
                expect(sharesBefore, "oldHotkey should have fund shares before swap").toBeGreaterThan(0n);

                log("Swapping oldHotkey → newHotkey for root subnet...");
                await swapHotkey(api, oldHotkeyColdkey, oldHotkey.address, newHotkey.address, 0);
                log("Swap done");

                expect(
                    await getBasketRate(api, newHotkey.address),
                    "newHotkey must have oldHotkey's fund rate"
                ).toBe(rateBefore);
                expect(
                    await getBasketShares(api, newHotkey.address),
                    "newHotkey must have oldHotkey's fund shares"
                ).toBe(sharesBefore);
                expect(await getBasketRate(api, oldHotkey.address), "oldHotkey must have no fund left").toBe(0n);
                expect(
                    await getBasketShares(api, oldHotkey.address),
                    "oldHotkey must have no shares left"
                ).toBe(0n);

                log("✅ Root swap correctly transferred the basket fund to newHotkey");
            },
        });
    },
});
