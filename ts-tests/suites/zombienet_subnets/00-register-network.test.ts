import { beforeAll, expect } from "vitest";
import { describeSuite } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    addStake,
    burnedRegister,
    forceSetBalance,
    generateKeyringPair,
    rootRegister,
    startCall,
    sudoSetLockReductionInterval,
    waitForBlocks,
} from "../../utils";
import { sudoSetStakeThreshold } from "../../utils/admin_utils.ts";
import { getChildren, setAutoParentDelegationEnabled, sudoSetPendingChildKeyCooldown } from "../../utils/children.ts";
import { Keyring } from "@polkadot/keyring";

describeSuite({
    id: "00_register_network",
    title: "▶ register_network extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
        });

        it({
            id: "T01",
            title: "auto-delegation: validator with flag=true gets child, validator with flag=false does not",
            test: async () => {
                const keyring = new Keyring({ type: "sr25519" });
                const rootSubnetOwner = keyring.addFromUri("//Alice");

                const rootVal1Coldkey = generateKeyringPair("sr25519"); // will opt-OUT
                const rootVal1Hotkey = generateKeyringPair("sr25519");
                const rootVal2Coldkey = generateKeyringPair("sr25519"); // default opt-IN
                const rootVal2Hotkey = generateKeyringPair("sr25519");
                const subnetOwnerColdkey = generateKeyringPair("sr25519");
                const subnetOwnerHotkey = generateKeyringPair("sr25519");

                log(`rootVal1Coldkey: ${rootVal1Coldkey.address}`);
                log(`rootVal1Hotkey: ${rootVal1Hotkey.address}`);
                log(`rootVal2Coldkey: ${rootVal2Coldkey.address}`);
                log(`rootVal2Hotkey: ${rootVal2Hotkey.address}`);
                log(`subnetOwnerColdkey: ${subnetOwnerColdkey.address}`);
                log(`subnetOwnerHotkey: ${subnetOwnerHotkey.address}`);

                await sudoSetLockReductionInterval(api, 1);

                for (const addr of [
                    rootVal1Coldkey.address,
                    rootVal1Hotkey.address,
                    rootVal2Coldkey.address,
                    rootVal2Hotkey.address,
                    subnetOwnerColdkey.address,
                    subnetOwnerHotkey.address,
                ]) {
                    await forceSetBalance(api, addr);
                }

                await sudoSetPendingChildKeyCooldown(api, 0n);

                await sudoSetStakeThreshold(api, 0n);

                const bootstrapNetuid = await addNewSubnetwork(api, rootVal1Hotkey, rootVal1Coldkey);
                log(`Bootstrap netuid: ${bootstrapNetuid}`);

                await rootRegister(api, rootVal1Coldkey, rootVal1Hotkey.address);

                await startCall(api, 0, rootSubnetOwner);

                await burnedRegister(api, bootstrapNetuid, rootVal2Hotkey.address, rootVal2Coldkey);

                await rootRegister(api, rootVal2Coldkey, rootVal2Hotkey.address);

                const stake = 1_000_000_000n;
                await addStake(api, rootVal1Coldkey, rootVal1Hotkey.address, 0, stake);
                await addStake(api, rootVal2Coldkey, rootVal2Hotkey.address, 0, stake);

                await setAutoParentDelegationEnabled(api, rootVal1Coldkey, rootVal1Hotkey.address, false);
                log("val1 opted out of auto parent delegation");

                const newNetuid = await addNewSubnetwork(api, subnetOwnerHotkey, subnetOwnerColdkey);
                log(`New subnet netuid: ${newNetuid}`);

                await waitForBlocks(api, 2);

                const children1 = await getChildren(api, rootVal1Hotkey.address, newNetuid);
                expect(children1.length, "val1 opted out — should have no children on the new subnet").toBe(0);

                const children2 = await getChildren(api, rootVal2Hotkey.address, newNetuid);
                expect(children2.length, "val2 did not opt out — should have exactly one child").toBe(1);
                expect(children2[0].child, "val2's child should be the subnet owner hotkey").toBe(
                    subnetOwnerHotkey.address
                );
                expect(children2[0].proportion, "proportion should be u64::MAX").toBe(18446744073709551615n); // u64::MAX

                log("✅ Auto parent delegation flag correctly controls child assignment on subnet registration.");
            },
        });
    },
});
