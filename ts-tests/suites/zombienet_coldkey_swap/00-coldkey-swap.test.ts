import { expect, beforeAll } from "vitest";
import { describeSuite } from "@moonwall/cli";
import { subtensor, MultiAddress } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import {
    ANNOUNCEMENT_DELAY,
    REANNOUNCEMENT_DELAY,
    addNewSubnetwork,
    addStake,
    announceColdkeySwap,
    clearColdkeySwapAnnouncement,
    disputeColdkeySwap,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    getColdkeySwapAnnouncement,
    getHotkeyOwner,
    getOwnedHotkeys,
    getStake,
    getStakingHotkeys,
    getSubnetOwner,
    sendTransaction,
    startCall,
    sudoSetAnnouncementDelay,
    sudoSetReannouncementDelay,
    swapColdkeyAnnounced,
    tao,
    waitForBlocks,
} from "../../utils";

describeSuite({
    id: "00_coldkey_swap",
    title: "▶ coldkey swap extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await sudoSetAnnouncementDelay(api, ANNOUNCEMENT_DELAY);
            await sudoSetReannouncementDelay(api, REANNOUNCEMENT_DELAY);
        });

        it({
            id: "T01",
            title: "happy path: announce → wait → swap (verifies full state migration)",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");

                await forceSetBalance(api, oldColdkey.address);
                await forceSetBalance(api, hotkey.address);

                // Create a subnet (oldColdkey becomes subnet owner)
                const netuid = await addNewSubnetwork(api, hotkey, oldColdkey);
                await startCall(api, netuid, oldColdkey);
                log(`Created subnet ${netuid}`);

                // Add stake
                await addStake(api, oldColdkey, hotkey.address, netuid, tao(200));

                // Snapshot state before swap
                const stakeBefore = await getStake(api, hotkey.address, oldColdkey.address, netuid);
                expect(stakeBefore, "should have stake before swap").toBeGreaterThan(0n);
                expect(await getSubnetOwner(api, netuid), "old coldkey should own the subnet").toBe(oldColdkey.address);
                expect(await getHotkeyOwner(api, hotkey.address), "old coldkey should own the hotkey").toBe(
                    oldColdkey.address
                );
                expect(
                    await getOwnedHotkeys(api, oldColdkey.address),
                    "old coldkey should have owned hotkeys"
                ).toContain(hotkey.address);
                expect(
                    await getStakingHotkeys(api, oldColdkey.address),
                    "old coldkey should have staking hotkeys"
                ).toContain(hotkey.address);
                const balanceBefore = await getBalance(api, oldColdkey.address);
                log(`Before swap — stake: ${stakeBefore}, balance: ${balanceBefore}`);

                // Announce
                const announceTx = api.tx.SubtensorModule.announce_coldkey_swap_v2({
                    new_coldkey: newColdkey.address,
                });
                const announceResult = await sendTransaction(announceTx, oldColdkey);
                expect(announceResult.success, "announce should succeed").toBe(true);
                log("Announced coldkey swap");

                // Wait for delay
                await waitForBlocks(api, ANNOUNCEMENT_DELAY + 1);

                // Swap
                const swapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: newColdkey.address,
                });
                const swapResult = await sendTransaction(swapTx, oldColdkey);
                expect(swapResult.success, `swap should succeed: ${swapResult.errorMessage ?? "unknown error"}`).toBe(
                    true
                );
                log("Swap executed");

                // Verify stake migrated
                const stakeOldAfter = await getStake(api, hotkey.address, oldColdkey.address, netuid);
                expect(stakeOldAfter, "old coldkey should have no stake").toBe(0n);
                const stakeNewAfter = await getStake(api, hotkey.address, newColdkey.address, netuid);
                expect(stakeNewAfter, "new coldkey should have the stake").toBeGreaterThan(0n);
                log(`Stake migrated: old=${stakeOldAfter}, new=${stakeNewAfter}`);

                // Verify subnet ownership transferred
                expect(await getSubnetOwner(api, netuid), "new coldkey should own the subnet").toBe(newColdkey.address);
                log("Subnet ownership transferred");

                // Verify hotkey ownership transferred
                expect(await getHotkeyOwner(api, hotkey.address), "new coldkey should own the hotkey").toBe(
                    newColdkey.address
                );
                expect(
                    await getOwnedHotkeys(api, oldColdkey.address),
                    "old coldkey should have no owned hotkeys"
                ).not.toContain(hotkey.address);
                expect(await getOwnedHotkeys(api, newColdkey.address), "new coldkey should own the hotkey").toContain(
                    hotkey.address
                );
                log("Hotkey ownership transferred");

                // Verify staking hotkeys transferred
                expect(
                    await getStakingHotkeys(api, oldColdkey.address),
                    "old coldkey should have no staking hotkeys"
                ).not.toContain(hotkey.address);
                expect(
                    await getStakingHotkeys(api, newColdkey.address),
                    "new coldkey should have staking hotkeys"
                ).toContain(hotkey.address);
                log("Staking hotkeys transferred");

                // Verify balance transferred
                const balanceOldAfter = await getBalance(api, oldColdkey.address);
                expect(balanceOldAfter, "old coldkey balance should be 0").toBe(0n);
                const balanceNewAfter = await getBalance(api, newColdkey.address);
                expect(balanceNewAfter, "new coldkey should have balance").toBeGreaterThan(0n);
                log(`Balance: old=${balanceOldAfter}, new=${balanceNewAfter}`);
            },
        });

        it({
            id: "T02",
            title: "swap too early: rejected",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                await announceColdkeySwap(api, oldColdkey, newColdkey);

                // Immediately try swap without waiting
                const swapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: newColdkey.address,
                });
                const result = await sendTransaction(swapTx, oldColdkey);
                expect(result.success, "swap should be rejected (too early)").toBe(false);
                log("Correctly rejected early swap");
            },
        });

        it({
            id: "T03",
            title: "reannouncement: too early → wait → reannounce → swap",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey1 = generateKeyringPair("sr25519");
                const newColdkey2 = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                // First announcement
                await announceColdkeySwap(api, oldColdkey, newColdkey1);
                log("First announce ok");

                // Reannounce immediately (should fail)
                const earlyAnnounceTx = api.tx.SubtensorModule.announce_coldkey_swap_v2({
                    new_coldkey: newColdkey2.address,
                });
                const earlyResult = await sendTransaction(earlyAnnounceTx, oldColdkey);
                expect(earlyResult.success, "early reannounce should fail").toBe(false);
                log("Early reannounce rejected");

                // Wait for announcement delay + reannouncement delay
                await waitForBlocks(api, ANNOUNCEMENT_DELAY + REANNOUNCEMENT_DELAY + 1);

                // Reannounce (should succeed)
                await announceColdkeySwap(api, oldColdkey, newColdkey2);
                log("Reannounced to new key");

                // Wait for announcement delay
                await waitForBlocks(api, ANNOUNCEMENT_DELAY + 1);

                // Swap with old hash (should fail)
                const wrongSwapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: newColdkey1.address,
                });
                const wrongResult = await sendTransaction(wrongSwapTx, oldColdkey);
                expect(wrongResult.success, "swap with old hash should fail").toBe(false);
                log("Old hash rejected");

                // Swap with new hash (should succeed)
                await swapColdkeyAnnounced(api, oldColdkey, newColdkey2.address);
                log("Swap with reannounced key succeeded");
            },
        });

        it({
            id: "T04",
            title: "dispute blocks swap execution",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                await announceColdkeySwap(api, oldColdkey, newColdkey);

                // Dispute
                const disputeTx = api.tx.SubtensorModule.dispute_coldkey_swap();
                const disputeResult = await sendTransaction(disputeTx, oldColdkey);
                expect(disputeResult.success, "dispute should succeed").toBe(true);
                log("Disputed");

                await waitForBlocks(api, ANNOUNCEMENT_DELAY + 1);

                // Swap should fail (disputed)
                const swapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: newColdkey.address,
                });
                const swapResult = await sendTransaction(swapTx, oldColdkey);
                expect(swapResult.success, "swap should fail (disputed)").toBe(false);
                log("Swap blocked after dispute");
            },
        });

        it({
            id: "T05",
            title: "double dispute: second fails",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                await announceColdkeySwap(api, oldColdkey, newColdkey);
                await disputeColdkeySwap(api, oldColdkey);

                // Second dispute should fail
                const disputeTx = api.tx.SubtensorModule.dispute_coldkey_swap();
                const result = await sendTransaction(disputeTx, oldColdkey);
                expect(result.success, "second dispute should fail").toBe(false);
                log("Second dispute correctly rejected");
            },
        });

        it({
            id: "T06",
            title: "announce fails: insufficient balance",
            test: async () => {
                const poorKey = generateKeyringPair("sr25519");
                // Intentionally NOT funded

                const announceTx = api.tx.SubtensorModule.announce_coldkey_swap_v2({
                    new_coldkey: generateKeyringPair("sr25519").address,
                });
                const result = await sendTransaction(announceTx, poorKey);
                expect(result.success, "announce should fail (no balance)").toBe(false);
                log("Announce rejected for insufficient balance");
            },
        });

        it({
            id: "T07",
            title: "swap with wrong key: hash mismatch",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const wrongKey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                await announceColdkeySwap(api, oldColdkey, newColdkey);
                await waitForBlocks(api, ANNOUNCEMENT_DELAY + 1);

                // Swap with wrong address
                const swapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: wrongKey.address,
                });
                const result = await sendTransaction(swapTx, oldColdkey);
                expect(result.success, "swap should fail (hash mismatch)").toBe(false);
                log("Hash mismatch correctly rejected");
            },
        });

        it({
            id: "T08",
            title: "dispute without announcement: fails",
            test: async () => {
                const someKey = generateKeyringPair("sr25519");
                await forceSetBalance(api, someKey.address);

                const disputeTx = api.tx.SubtensorModule.dispute_coldkey_swap();
                const result = await sendTransaction(disputeTx, someKey);
                expect(result.success, "dispute should fail (no announcement)").toBe(false);
                log("Dispute without announcement rejected");
            },
        });

        it({
            id: "T09",
            title: "clear announcement: announce → wait → clear removes announcement",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, coldkey.address);

                await announceColdkeySwap(api, coldkey, newColdkey);
                expect(
                    await getColdkeySwapAnnouncement(api, coldkey.address),
                    "announcement should exist"
                ).not.toBeNull();
                log("Announced");

                // Wait for reannouncement delay (measured from execution block)
                await waitForBlocks(api, ANNOUNCEMENT_DELAY + REANNOUNCEMENT_DELAY + 1);

                await clearColdkeySwapAnnouncement(api, coldkey);

                expect(
                    await getColdkeySwapAnnouncement(api, coldkey.address),
                    "announcement should be removed"
                ).toBeNull();
                log("Announcement cleared");
            },
        });

        it({
            id: "T10",
            title: "clear announcement too early: rejected",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, coldkey.address);

                await announceColdkeySwap(api, coldkey, newColdkey);

                const clearTx = api.tx.SubtensorModule.clear_coldkey_swap_announcement();
                const result = await sendTransaction(clearTx, coldkey);
                expect(result.success, "clear should be rejected (too early)").toBe(false);

                expect(
                    await getColdkeySwapAnnouncement(api, coldkey.address),
                    "announcement should still exist"
                ).not.toBeNull();
                log("Correctly rejected early clear");
            },
        });

        it({
            id: "T11",
            title: "clear announcement without announcement: fails",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, coldkey.address);

                const clearTx = api.tx.SubtensorModule.clear_coldkey_swap_announcement();
                const result = await sendTransaction(clearTx, coldkey);
                expect(result.success, "clear should fail (no announcement)").toBe(false);
                log("Clear without announcement rejected");
            },
        });

        it({
            id: "T12",
            title: "clear announcement after dispute: blocked",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, coldkey.address);

                await announceColdkeySwap(api, coldkey, newColdkey);
                await disputeColdkeySwap(api, coldkey);
                log("Announced + disputed");

                await waitForBlocks(api, ANNOUNCEMENT_DELAY + REANNOUNCEMENT_DELAY + 1);

                const clearTx = api.tx.SubtensorModule.clear_coldkey_swap_announcement();
                const result = await sendTransaction(clearTx, coldkey);
                expect(result.success, "clear should fail (disputed)").toBe(false);
                log("Clear blocked after dispute");
            },
        });

        it({
            id: "T13",
            title: "dispatch guard: active announcement blocks staking and transfer but allows swap calls",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");
                const recipient = generateKeyringPair("sr25519");

                await forceSetBalance(api, coldkey.address);
                await forceSetBalance(api, hotkey.address);

                const netuid = await addNewSubnetwork(api, hotkey, coldkey);
                await startCall(api, netuid, coldkey);

                // Announce swap
                await announceColdkeySwap(api, coldkey, newColdkey);
                log("Announced coldkey swap");

                // add_stake should be blocked
                const stakeTx = api.tx.SubtensorModule.add_stake({
                    hotkey: hotkey.address,
                    netuid: netuid,
                    amount_staked: tao(10),
                });
                const stakeResult = await sendTransaction(stakeTx, coldkey);
                expect(stakeResult.success, "add_stake should be blocked by guard").toBe(false);
                log("add_stake blocked");

                // transfer_keep_alive should be blocked
                const transferTx = api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(recipient.address),
                    value: tao(1),
                });
                const transferResult = await sendTransaction(transferTx, coldkey);
                expect(transferResult.success, "transfer should be blocked by guard").toBe(false);
                log("transfer_keep_alive blocked");

                // swap-related calls should still go through the guard
                // (reannounce will fail because of reannouncement delay, but NOT because of the guard)
                const reannounceTx = api.tx.SubtensorModule.announce_coldkey_swap_v2({
                    new_coldkey: newColdkey.address,
                });
                const reannounceResult = await sendTransaction(reannounceTx, coldkey);
                // Fails with ReannounceBeforeDelay, not ColdkeySwapAnnounced — meaning the guard allowed it through
                expect(reannounceResult.success, "reannounce fails but not from the guard").toBe(false);
                expect(
                    reannounceResult.errorMessage,
                    "error should be reannouncement delay, not guard block"
                ).not.toContain("ColdkeySwapAnnounced");
                log("announce_coldkey_swap passed through guard (failed at pallet level as expected)");

                // dispute should succeed (allowed through the guard)
                await disputeColdkeySwap(api, coldkey);
                log("dispute_coldkey_swap allowed through guard");
            },
        });

        it({
            id: "T14",
            title: "dispatch guard: disputed swap blocks ALL calls including swap-related",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");
                const recipient = generateKeyringPair("sr25519");

                await forceSetBalance(api, coldkey.address);
                await forceSetBalance(api, hotkey.address);

                const netuid = await addNewSubnetwork(api, hotkey, coldkey);
                await startCall(api, netuid, coldkey);

                // Announce + dispute
                await announceColdkeySwap(api, coldkey, newColdkey);
                await disputeColdkeySwap(api, coldkey);
                log("Announced + disputed");

                // add_stake should be blocked
                const stakeTx = api.tx.SubtensorModule.add_stake({
                    hotkey: hotkey.address,
                    netuid: netuid,
                    amount_staked: tao(10),
                });
                const stakeResult = await sendTransaction(stakeTx, coldkey);
                expect(stakeResult.success, "add_stake should be blocked (disputed)").toBe(false);
                log("add_stake blocked");

                // transfer should be blocked
                const transferTx = api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(recipient.address),
                    value: tao(1),
                });
                const transferResult = await sendTransaction(transferTx, coldkey);
                expect(transferResult.success, "transfer should be blocked (disputed)").toBe(false);
                log("transfer_keep_alive blocked");

                // swap_coldkey_announced should also be blocked (unlike T13 where swap calls pass)
                const swapTx = api.tx.SubtensorModule.swap_coldkey_announced({
                    new_coldkey: newColdkey.address,
                });
                const swapResult = await sendTransaction(swapTx, coldkey);
                expect(swapResult.success, "swap should be blocked (disputed)").toBe(false);
                log("swap_coldkey_announced blocked");

                // announce should also be blocked
                const announceTx = api.tx.SubtensorModule.announce_coldkey_swap_v2({
                    new_coldkey: newColdkey.address,
                });
                const announceResult = await sendTransaction(announceTx, coldkey);
                expect(announceResult.success, "announce should be blocked (disputed)").toBe(false);
                log("announce_coldkey_swap blocked — all calls rejected under dispute");
            },
        });
    },
});
