import { expect, beforeAll } from "vitest";
import { describeSuite } from "@moonwall/cli";
import {
    addNewSubnetwork,
    addStake,
    claimRoot,
    forceSetBalance,
    generateKeyringPair,
    getBasketClaimed,
    getBasketRate,
    getBasketShares,
    getPendingRootAlphaDivs,
    getStake,
    getSubnetAlphaIn,
    getSubnetMovingPrice,
    getSubnetTAO,
    getTaoWeight,
    getTotalHotkeyAlpha,
    isSubtokenEnabled,
    setRootWeights,
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

describeSuite({
    id: "0203_claim_root",
    title: "▶ claim_root extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;
        const ROOT_NETUID = 0;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await sudoSetLockReductionInterval(api, 1);
        });

        it({
            id: "T0401",
            title: "should redeem the basket fund to ROOT stake via claim_root",
            test: async () => {
                // Setup accounts
                // - owner1Hotkey/owner1Coldkey: subnet 1 owner
                // - owner2Hotkey/owner2Coldkey: subnet 2 owner (needed for root_sell_flag)
                // - stakerColdkey: the coldkey that will stake on root and claim dividends
                const owner1Hotkey = generateKeyringPair("sr25519");
                const owner1Coldkey = generateKeyringPair("sr25519");
                const owner2Hotkey = generateKeyringPair("sr25519");
                const owner2Coldkey = generateKeyringPair("sr25519");
                const stakerColdkey = generateKeyringPair("sr25519");
                const owner1HotkeyAddress = owner1Hotkey.address;
                const owner1ColdkeyAddress = owner1Coldkey.address;
                const owner2HotkeyAddress = owner2Hotkey.address;
                const owner2ColdkeyAddress = owner2Coldkey.address;
                const stakerColdkeyAddress = stakerColdkey.address;

                // Fund all accounts
                await forceSetBalance(api, owner1HotkeyAddress);
                await forceSetBalance(api, owner1ColdkeyAddress);
                await forceSetBalance(api, owner2HotkeyAddress);
                await forceSetBalance(api, owner2ColdkeyAddress);
                await forceSetBalance(api, stakerColdkeyAddress);

                // Disable admin freeze window to allow enabling subtoken for ROOT
                await sudoSetAdminFreezeWindow(api, 0);
                log("Admin freeze window set to 0");

                // Enable subtoken for ROOT subnet (required for staking on root)
                const subtokenEnabledBefore = await isSubtokenEnabled(api, ROOT_NETUID);
                if (!subtokenEnabledBefore) {
                    await sudoSetSubtokenEnabled(api, ROOT_NETUID, true);
                    const subtokenEnabledAfter = await isSubtokenEnabled(api, ROOT_NETUID);
                    log(`ROOT subtoken enabled: ${subtokenEnabledAfter}`);
                    expect(subtokenEnabledAfter).toBe(true);
                }

                // Create TWO dynamic subnets - needed for root_sell_flag to become true
                // root_sell_flag = sum(moving_prices) > 1.0
                // Each subnet's moving price approaches 1.0 via EMA, so 2 subnets can exceed threshold
                const netuid1 = await addNewSubnetwork(api, owner1Hotkey, owner1Coldkey);
                await startCall(api, netuid1, owner1Coldkey);
                log(`Created subnet 1 with netuid: ${netuid1}`);

                const netuid2 = await addNewSubnetwork(api, owner2Hotkey, owner2Coldkey);
                await startCall(api, netuid2, owner2Coldkey);
                log(`Created subnet 2 with netuid: ${netuid2}`);

                // Set short tempo for faster emission distribution
                await sudoSetTempo(api, netuid1, 1);
                await sudoSetTempo(api, netuid2, 1);
                log("Set tempo to 1 for both subnets");

                // Set EMA price halving period to 1 for fast moving price convergence
                // Formula: alpha = SubnetMovingAlpha * blocks/(blocks + halving_time)
                // With halving_time=1: after 10 blocks, alpha ≈ 0.91, moving price ≈ 0.91
                // With 2 subnets at ~0.9 each, total > 1.0 enabling root_sell_flag
                await sudoSetEmaPriceHalvingPeriod(api, netuid1, 1);
                await sudoSetEmaPriceHalvingPeriod(api, netuid2, 1);
                log("Set EMA halving period to 1 for fast price convergence");

                // Set SubnetMovingAlpha to 1.0 (default is 0.000003 which is way too slow)
                // I96F32 encoding: 1.0 * 2^32 = 4294967296
                const movingAlpha = BigInt(4294967296); // 1.0 in I96F32
                await sudoSetSubnetMovingAlpha(api, movingAlpha);
                log("Set SubnetMovingAlpha to 1.0 for fast EMA convergence");

                // Set threshold to 0 to allow claiming any amount (claims consult the ROOT entry)
                await sudoSetRootClaimThreshold(api, ROOT_NETUID, 0n);

                // Add stake to ROOT subnet for the staker (makes them eligible for root dividends)
                const rootStakeAmount = tao(100);
                await addStake(api, stakerColdkey, owner1HotkeyAddress, ROOT_NETUID, rootStakeAmount);
                log(`Added ${rootStakeAmount} stake to root subnet for staker`);

                // Verify root stake was added
                const rootStake = await getStake(api, owner1HotkeyAddress, stakerColdkeyAddress, ROOT_NETUID);
                log(`Root stake: ${rootStake}`);
                expect(rootStake, "Should have stake on root subnet").toBeGreaterThan(0n);

                // The validator must set its basket weight vector for dividends to be deposited
                // into the fund (otherwise they are recycled). Route them into subnet 1.
                await setRootWeights(api, owner1Hotkey, [netuid1], [65535]);
                log("Set root weights: 100% to netuid1");

                // Add stake to both dynamic subnets (owner stake to enable emissions flow)
                const subnetStakeAmount = tao(50);
                await addStake(api, owner1Coldkey, owner1HotkeyAddress, netuid1, subnetStakeAmount);
                await addStake(api, owner2Coldkey, owner2HotkeyAddress, netuid2, subnetStakeAmount);
                log(`Added ${subnetStakeAmount} owner stake to subnets ${netuid1} and ${netuid2}`);

                // Snapshot the staker's ROOT stake before the claim (redemption pays to root).
                const stakerRootStakeBefore = await getStake(
                    api,
                    owner1HotkeyAddress,
                    stakerColdkeyAddress,
                    ROOT_NETUID
                );
                log(`Staker root stake before claim: ${stakerRootStakeBefore}`);

                // Wait for blocks to:
                // 1. Allow moving prices to converge (need sum > 1.0 for root_sell_flag)
                // 2. Accumulate PendingRootAlphaDivs
                // 3. Distribute emissions at tempo boundary
                const blocksToWait = 25;
                log(`Waiting for ${blocksToWait} blocks for moving prices to converge and emissions to accumulate...`);
                await waitForBlocks(api, blocksToWait);

                // Debug: Check key storage values
                const subnetTaoRoot = await getSubnetTAO(api, ROOT_NETUID);
                const subnetTao1 = await getSubnetTAO(api, netuid1);
                const subnetTao2 = await getSubnetTAO(api, netuid2);
                log(`SubnetTAO - ROOT: ${subnetTaoRoot}, netuid1: ${subnetTao1}, netuid2: ${subnetTao2}`);

                const movingPrice1 = await getSubnetMovingPrice(api, netuid1);
                const movingPrice2 = await getSubnetMovingPrice(api, netuid2);
                log(`SubnetMovingPrice - netuid1: ${movingPrice1}, netuid2: ${movingPrice2}`);
                // Note: Moving price is I96F32, so divide by 2^32 to get actual value
                const mp1Float = Number(movingPrice1) / 2 ** 32;
                const mp2Float = Number(movingPrice2) / 2 ** 32;
                log(
                    `SubnetMovingPrice (float) - netuid1: ${mp1Float}, netuid2: ${mp2Float}, sum: ${mp1Float + mp2Float}`
                );

                const pendingDivs1 = await getPendingRootAlphaDivs(api, netuid1);
                const pendingDivs2 = await getPendingRootAlphaDivs(api, netuid2);
                log(`PendingRootAlphaDivs - netuid1: ${pendingDivs1}, netuid2: ${pendingDivs2}`);

                const taoWeight = await getTaoWeight(api);
                log(`TaoWeight: ${taoWeight}`);

                const alphaIn1 = await getSubnetAlphaIn(api, netuid1);
                const alphaIn2 = await getSubnetAlphaIn(api, netuid2);
                log(`SubnetAlphaIn - netuid1: ${alphaIn1}, netuid2: ${alphaIn2}`);

                const totalHotkeyAlpha1 = await getTotalHotkeyAlpha(api, owner1HotkeyAddress, netuid1);
                log(`TotalHotkeyAlpha for hotkey1 on netuid1: ${totalHotkeyAlpha1}`);

                // Check the validator's basket fund state
                const basketRate = await getBasketRate(api, owner1HotkeyAddress);
                const basketShares = await getBasketShares(api, owner1HotkeyAddress);
                log(`BasketRate: ${basketRate}, BasketShares: ${basketShares}`);

                // Call claim_root: redeems the staker's owed fund shares to ROOT stake.
                await claimRoot(api, stakerColdkey);
                log("Called claim_root");

                // Get ROOT stake after claim
                const stakerRootStakeAfter = await getStake(
                    api,
                    owner1HotkeyAddress,
                    stakerColdkeyAddress,
                    ROOT_NETUID
                );
                log(`Staker root stake after claim: ${stakerRootStakeAfter}`);

                // Check the claimed-shares watermark
                const basketClaimed = await getBasketClaimed(api, owner1HotkeyAddress, stakerColdkeyAddress);
                log(`BasketClaimed value: ${basketClaimed}`);

                // Verify dividends were claimed
                expect(
                    stakerRootStakeAfter,
                    "ROOT stake should increase after claiming root dividends"
                ).toBeGreaterThan(stakerRootStakeBefore);
                log(
                    `✅ Root claim successful: root stake increased from ${stakerRootStakeBefore} to ${stakerRootStakeAfter}`
                );
            },
        });

        it({
            id: "T0402",
            title: "should claim root dividends with Swap type (swap to TAO on ROOT)",
            test: async () => {
                // Setup accounts
                // - owner1Hotkey/owner1Coldkey: subnet 1 owner
                // - owner2Hotkey/owner2Coldkey: subnet 2 owner (needed for root_sell_flag)
                // - stakerColdkey: the coldkey that will stake on root and claim dividends
                const owner1Hotkey = generateKeyringPair("sr25519");
                const owner1Coldkey = generateKeyringPair("sr25519");
                const owner2Hotkey = generateKeyringPair("sr25519");
                const owner2Coldkey = generateKeyringPair("sr25519");
                const stakerColdkey = generateKeyringPair("sr25519");
                const owner1HotkeyAddress = owner1Hotkey.address;
                const owner1ColdkeyAddress = owner1Coldkey.address;
                const owner2HotkeyAddress = owner2Hotkey.address;
                const owner2ColdkeyAddress = owner2Coldkey.address;
                const stakerColdkeyAddress = stakerColdkey.address;

                // Fund all accounts
                await forceSetBalance(api, owner1HotkeyAddress);
                await forceSetBalance(api, owner1ColdkeyAddress);
                await forceSetBalance(api, owner2HotkeyAddress);
                await forceSetBalance(api, owner2ColdkeyAddress);
                await forceSetBalance(api, stakerColdkeyAddress);

                // Disable admin freeze window to allow enabling subtoken for ROOT
                await sudoSetAdminFreezeWindow(api, 0);
                log("Admin freeze window set to 0");

                // Create TWO dynamic subnets
                const netuid1 = await addNewSubnetwork(api, owner1Hotkey, owner1Coldkey);
                await startCall(api, netuid1, owner1Coldkey);
                log(`Created subnet 1 with netuid: ${netuid1}`);

                const netuid2 = await addNewSubnetwork(api, owner2Hotkey, owner2Coldkey);
                await startCall(api, netuid2, owner2Coldkey);
                log(`Created subnet 2 with netuid: ${netuid2}`);

                // Set short tempo for faster emission distribution
                await sudoSetTempo(api, netuid1, 1);
                await sudoSetTempo(api, netuid2, 1);
                log("Set tempo to 1 for both subnets");

                // Set EMA price halving period to 1 for fast moving price convergence
                await sudoSetEmaPriceHalvingPeriod(api, netuid1, 1);
                await sudoSetEmaPriceHalvingPeriod(api, netuid2, 1);
                log("Set EMA halving period to 1 for fast price convergence");

                // Set SubnetMovingAlpha to 1.0 (default is 0.000003 which is way too slow)
                // I96F32 encoding: 1.0 * 2^32 = 4294967296
                const movingAlpha = BigInt(4294967296); // 1.0 in I96F32
                await sudoSetSubnetMovingAlpha(api, movingAlpha);
                log("Set SubnetMovingAlpha to 1.0 for fast EMA convergence");

                // Set threshold to 0 to allow claiming any amount (claims consult the ROOT entry)
                await sudoSetRootClaimThreshold(api, ROOT_NETUID, 0n);

                // Add stake to ROOT subnet for the staker
                const rootStakeAmount = tao(100);
                await addStake(api, stakerColdkey, owner1HotkeyAddress, ROOT_NETUID, rootStakeAmount);
                log(`Added ${rootStakeAmount} stake to root subnet for staker`);

                // Get initial ROOT stake
                const rootStakeBefore = await getStake(api, owner1HotkeyAddress, stakerColdkeyAddress, ROOT_NETUID);
                log(`Root stake before: ${rootStakeBefore}`);

                // Route the validator's basket into subnet 1 so dividends are deposited.
                await setRootWeights(api, owner1Hotkey, [netuid1], [65535]);
                log("Set root weights: 100% to netuid1");

                // Add stake to both dynamic subnets (owner stake to enable emissions flow)
                const subnetStakeAmount = tao(50);
                await addStake(api, owner1Coldkey, owner1HotkeyAddress, netuid1, subnetStakeAmount);
                await addStake(api, owner2Coldkey, owner2HotkeyAddress, netuid2, subnetStakeAmount);
                log(`Added ${subnetStakeAmount} owner stake to subnets ${netuid1} and ${netuid2}`);

                // Wait for blocks
                const blocksToWait = 25;
                log(`Waiting for ${blocksToWait} blocks for emissions to accumulate...`);
                await waitForBlocks(api, blocksToWait);

                // Debug: Check moving prices
                const movingPrice1 = await getSubnetMovingPrice(api, netuid1);
                const movingPrice2 = await getSubnetMovingPrice(api, netuid2);
                const mp1Float = Number(movingPrice1) / 2 ** 32;
                const mp2Float = Number(movingPrice2) / 2 ** 32;
                log(
                    `SubnetMovingPrice (float) - netuid1: ${mp1Float}, netuid2: ${mp2Float}, sum: ${mp1Float + mp2Float}`
                );

                const pendingDivs1 = await getPendingRootAlphaDivs(api, netuid1);
                log(`PendingRootAlphaDivs netuid1: ${pendingDivs1}`);

                // Check the validator's basket fund state
                const basketRate = await getBasketRate(api, owner1HotkeyAddress);
                const basketShares = await getBasketShares(api, owner1HotkeyAddress);
                log(`BasketRate: ${basketRate}, BasketShares: ${basketShares}`);

                // Call claim_root - the fund is redeemed to TAO and added to ROOT stake
                await claimRoot(api, stakerColdkey);
                log("Called claim_root");

                // Get ROOT stake after claim
                const rootStakeAfter = await getStake(api, owner1HotkeyAddress, stakerColdkeyAddress, ROOT_NETUID);
                log(`Root stake after claim: ${rootStakeAfter}`);

                // Check the claimed-shares watermark
                const basketClaimed = await getBasketClaimed(api, owner1HotkeyAddress, stakerColdkeyAddress);
                log(`BasketClaimed value: ${basketClaimed}`);

                // With Swap type, ROOT stake should increase (not dynamic subnet stake)
                expect(rootStakeAfter, "ROOT stake should increase after claiming with Swap type").toBeGreaterThan(
                    rootStakeBefore
                );
                log(
                    `✅ Root claim with Swap successful: ROOT stake increased from ${rootStakeBefore} to ${rootStakeAfter}`
                );
            },
        });

        it({
            id: "T0403",
            title: "should handle claim_root when no dividends are available",
            test: async () => {
                // Setup accounts
                const coldkey = generateKeyringPair("sr25519");
                const coldkeyAddress = coldkey.address;

                await forceSetBalance(api, coldkeyAddress);

                // Claim with no basket accrued (should succeed but be a no-op)
                await claimRoot(api, coldkey);

                log("✅ claim_root with no dividends executed successfully (no-op).");
            },
        });
    },
});
