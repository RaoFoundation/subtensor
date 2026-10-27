import { expect, beforeAll } from "vitest";
import { describeSuite } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    addStake,
    announceColdkeySwap,
    burnedRegister,
    disputeColdkeySwap,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    getColdkeySwapAnnouncement,
    getColdkeySwapDispute,
    getHotkeyOwner,
    getOwnedHotkeys,
    getStake,
    getStakingHotkeys,
    getSubnetOwner,
    startCall,
    sudoResetColdkeySwap,
    sudoResetColdkeySwapKeepLocked,
    sudoSwapColdkey,
    tao,
} from "../../utils";

describeSuite({
    id: "01_coldkey_swap_sudo",
    title: "▶ coldkey swap sudo operations",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
        });

        it({
            id: "T01",
            title: "reset as root: clears announcement and dispute",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                // Announce and dispute
                await announceColdkeySwap(api, oldColdkey, newColdkey);
                await disputeColdkeySwap(api, oldColdkey);
                log("Announced + disputed");

                // Verify storage before reset
                const annBefore = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annBefore, "announcement should exist before reset").not.toBeNull();

                // Reset via sudo
                await sudoResetColdkeySwap(api, oldColdkey.address);
                log("Reset via sudo");

                // Verify storage cleared
                const annAfter = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annAfter, "announcement should be cleared").toBeNull();
                const dispAfter = await getColdkeySwapDispute(api, oldColdkey.address);
                expect(dispAfter, "dispute should be cleared").toBeNull();
                log("Storage cleared");

                // Re-announce should succeed
                await announceColdkeySwap(api, oldColdkey, newColdkey);
                log("Re-announce after reset succeeded");
            },
        });

        it({
            id: "T04",
            title: "reset as root: clears announcement and dispute while retaining the recovery lock",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                // Announce and dispute
                await announceColdkeySwap(api, oldColdkey, newColdkey);
                await disputeColdkeySwap(api, oldColdkey);
                log("Announced + disputed");

                // Verify storage before reset
                const annBefore = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annBefore, "announcement should exist before reset").not.toBeNull();

                // Reset via sudo
                await sudoResetColdkeySwapKeepLocked(api, oldColdkey.address);
                log("Reset via sudo");

                // Zero replaces the active dispute timestamp and retains the recovery lock.
                const annAfter = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annAfter, "announcement should be cleared").toBeNull();
                const dispAfter = await getColdkeySwapDispute(api, oldColdkey.address);
                expect(dispAfter, "recovery lock should remain without an active dispute").toBe(0);
                log("Announcement and active dispute cleared; recovery lock retained");

                // Re-announce should succeed
                await announceColdkeySwap(api, oldColdkey, newColdkey);
                log("Re-announce after reset succeeded");
                await disputeColdkeySwap(api, oldColdkey);
                expect(await getColdkeySwapDispute(api, oldColdkey.address)).toBeGreaterThan(0);
            },
        });

        it({
            id: "T02",
            title: "instant swap as root: transfers stake and ownership across multiple subnets",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const hotkey1 = generateKeyringPair("sr25519");
                const hotkey2 = generateKeyringPair("sr25519");

                await forceSetBalance(api, oldColdkey.address);
                await forceSetBalance(api, hotkey1.address);
                await forceSetBalance(api, hotkey2.address);

                // Create two subnets
                const netuid1 = await addNewSubnetwork(api, hotkey1, oldColdkey);
                await startCall(api, netuid1, oldColdkey);
                const netuid2 = await addNewSubnetwork(api, hotkey2, oldColdkey);
                await startCall(api, netuid2, oldColdkey);
                log(`Created subnets ${netuid1} and ${netuid2}`);

                // Register hotkey1 on subnet2 and stake on both
                await burnedRegister(api, netuid2, hotkey1.address, oldColdkey);
                await addStake(api, oldColdkey, hotkey1.address, netuid1, tao(100));
                await addStake(api, oldColdkey, hotkey1.address, netuid2, tao(50));

                const stake1Before = await getStake(api, hotkey1.address, oldColdkey.address, netuid1);
                const stake2Before = await getStake(api, hotkey1.address, oldColdkey.address, netuid2);
                expect(stake1Before, "should have stake on subnet1").toBeGreaterThan(0n);
                expect(stake2Before, "should have stake on subnet2").toBeGreaterThan(0n);
                log(`Before — subnet1 stake: ${stake1Before}, subnet2 stake: ${stake2Before}`);

                // Sudo swap
                await sudoSwapColdkey(api, oldColdkey.address, newColdkey.address, 0n);
                log("Sudo swap executed");

                // Verify both subnets' stake migrated
                expect(
                    await getStake(api, hotkey1.address, oldColdkey.address, netuid1),
                    "old coldkey stake on subnet1 should be 0"
                ).toBe(0n);
                expect(
                    await getStake(api, hotkey1.address, newColdkey.address, netuid1),
                    "new coldkey should have stake on subnet1"
                ).toBeGreaterThan(0n);
                expect(
                    await getStake(api, hotkey1.address, oldColdkey.address, netuid2),
                    "old coldkey stake on subnet2 should be 0"
                ).toBe(0n);
                expect(
                    await getStake(api, hotkey1.address, newColdkey.address, netuid2),
                    "new coldkey should have stake on subnet2"
                ).toBeGreaterThan(0n);
                log("Stake migrated on both subnets");

                // Verify subnet ownership transferred
                expect(await getSubnetOwner(api, netuid1), "new coldkey should own subnet1").toBe(newColdkey.address);
                expect(await getSubnetOwner(api, netuid2), "new coldkey should own subnet2").toBe(newColdkey.address);

                // Verify hotkey ownership transferred
                expect(await getHotkeyOwner(api, hotkey1.address), "hotkey1 owner").toBe(newColdkey.address);
                expect(await getHotkeyOwner(api, hotkey2.address), "hotkey2 owner").toBe(newColdkey.address);

                // Verify old coldkey is fully empty
                expect(
                    (await getOwnedHotkeys(api, oldColdkey.address)).length,
                    "old coldkey should own no hotkeys"
                ).toBe(0);
                expect(
                    (await getStakingHotkeys(api, oldColdkey.address)).length,
                    "old coldkey should have no staking hotkeys"
                ).toBe(0);
                expect(await getBalance(api, oldColdkey.address), "old coldkey balance should be 0").toBe(0n);

                log("All state migrated across both subnets");
            },
        });

        it({
            id: "T03",
            title: "instant swap as root: clears pending announcement",
            test: async () => {
                const oldColdkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                const decoy = generateKeyringPair("sr25519");
                await forceSetBalance(api, oldColdkey.address);

                // Announce for decoy
                await announceColdkeySwap(api, oldColdkey, decoy);
                const annBefore = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annBefore, "announcement should exist").not.toBeNull();
                log("Pending announcement exists");

                // Sudo swap with different key
                await sudoSwapColdkey(api, oldColdkey.address, newColdkey.address, 0n);

                // Announcement should be cleared
                const annAfter = await getColdkeySwapAnnouncement(api, oldColdkey.address);
                expect(annAfter, "announcement should be cleared after root swap").toBeNull();
                log("Announcement cleared by root swap");
            },
        });
    },
});
