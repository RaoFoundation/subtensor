import { describeSuite } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import { beforeAll, expect } from "vitest";
import {
    addNewSubnetwork,
    addStake,
    forceSetBalance,
    generateKeyringPair,
    getStake,
    moveStakeLimit,
    sendTransaction,
    startCall,
    sudoSetLockReductionInterval,
    tao,
} from "../../utils";

describeSuite({
    id: "03_01_move_stake_limit",
    title: "▶ move_stake_limit extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await sudoSetLockReductionInterval(api, 1);
        });

        it({
            id: "T01",
            title: "should move stake to another hotkey with a price limit (allow partial)",
            test: async () => {
                const originHotkey = generateKeyringPair("sr25519");
                const destinationHotkey = generateKeyringPair("sr25519");
                const coldkey = generateKeyringPair("sr25519");
                const originHotkeyAddress = originHotkey.address;
                const destinationHotkeyAddress = destinationHotkey.address;
                const coldkeyAddress = coldkey.address;

                await forceSetBalance(api, originHotkeyAddress);
                await forceSetBalance(api, destinationHotkeyAddress);
                await forceSetBalance(api, coldkeyAddress);

                const originNetuid = await addNewSubnetwork(api, originHotkey, coldkey);
                await startCall(api, originNetuid, coldkey);
                const destinationNetuid = await addNewSubnetwork(api, destinationHotkey, coldkey);
                await startCall(api, destinationNetuid, coldkey);

                await addStake(api, coldkey, originHotkeyAddress, originNetuid, tao(100));

                const originStakeBefore = await getStake(api, originHotkeyAddress, coldkeyAddress, originNetuid);
                const destinationStakeBefore = await getStake(
                    api,
                    destinationHotkeyAddress,
                    coldkeyAddress,
                    destinationNetuid
                );
                expect(originStakeBefore, "Origin hotkey should have stake before move").toBeGreaterThan(0n);

                const moveAmount = originStakeBefore / 2n;
                const limitPrice = (tao(1) * 99n) / 100n;
                await moveStakeLimit(
                    api,
                    coldkey,
                    originHotkeyAddress,
                    destinationHotkeyAddress,
                    originNetuid,
                    destinationNetuid,
                    moveAmount,
                    limitPrice,
                    true
                );

                const originStakeAfter = await getStake(api, originHotkeyAddress, coldkeyAddress, originNetuid);
                const destinationStakeAfter = await getStake(
                    api,
                    destinationHotkeyAddress,
                    coldkeyAddress,
                    destinationNetuid
                );

                log(
                    `Origin stake: ${originStakeBefore} -> ${originStakeAfter}, destination stake: ${destinationStakeBefore} -> ${destinationStakeAfter}`
                );
                expect(originStakeAfter, "Origin stake should decrease").toBeLessThan(originStakeBefore);
                expect(destinationStakeAfter, "Destination stake should increase").toBeGreaterThan(
                    destinationStakeBefore
                );
            },
        });

        it({
            id: "T02",
            title: "should reject fill-or-kill move when the price limit is exceeded",
            test: async () => {
                const originHotkey = generateKeyringPair("sr25519");
                const destinationHotkey = generateKeyringPair("sr25519");
                const coldkey = generateKeyringPair("sr25519");
                const originHotkeyAddress = originHotkey.address;
                const destinationHotkeyAddress = destinationHotkey.address;
                const coldkeyAddress = coldkey.address;

                await forceSetBalance(api, originHotkeyAddress);
                await forceSetBalance(api, destinationHotkeyAddress);
                await forceSetBalance(api, coldkeyAddress);

                const originNetuid = await addNewSubnetwork(api, originHotkey, coldkey);
                await startCall(api, originNetuid, coldkey);
                const destinationNetuid = await addNewSubnetwork(api, destinationHotkey, coldkey);
                await startCall(api, destinationNetuid, coldkey);

                await addStake(api, coldkey, originHotkeyAddress, originNetuid, tao(100));

                const originStakeBefore = await getStake(api, originHotkeyAddress, coldkeyAddress, originNetuid);
                const destinationStakeBefore = await getStake(
                    api,
                    destinationHotkeyAddress,
                    coldkeyAddress,
                    destinationNetuid
                );
                expect(originStakeBefore, "Origin hotkey should have stake before move").toBeGreaterThan(0n);

                // limit_price is dest-alpha per origin-alpha, scaled by 1e9. A floor above the
                // current relative price makes max executable amount 0, so fill-or-kill must abort.
                const originPrice = await api.apis.SwapRuntimeApi.current_alpha_price(originNetuid);
                const destinationPrice = await api.apis.SwapRuntimeApi.current_alpha_price(destinationNetuid);
                expect(destinationPrice, "Destination subnet should have a non-zero alpha price").toBeGreaterThan(0n);
                const limitPrice = (originPrice * tao(1)) / destinationPrice + 1n;
                const moveAmount = originStakeBefore / 2n;

                const tx = api.tx.SubtensorModule.move_stake_limit({
                    origin_hotkey: originHotkeyAddress,
                    destination_hotkey: destinationHotkeyAddress,
                    origin_netuid: originNetuid,
                    destination_netuid: destinationNetuid,
                    alpha_amount: moveAmount,
                    limit_price: limitPrice,
                    allow_partial: false,
                });
                const result = await sendTransaction(tx, coldkey);

                expect(result.success, "fill-or-kill should fail when the limit cannot be met").toBe(false);
                expect(result.errorMessage, "should fail with SlippageTooHigh").toContain("SlippageTooHigh");

                const originStakeAfter = await getStake(api, originHotkeyAddress, coldkeyAddress, originNetuid);
                const destinationStakeAfter = await getStake(
                    api,
                    destinationHotkeyAddress,
                    coldkeyAddress,
                    destinationNetuid
                );

                log(
                    `Origin stake: ${originStakeBefore} -> ${originStakeAfter}, destination stake: ${destinationStakeBefore} -> ${destinationStakeAfter}`
                );
                expect(originStakeAfter, "Origin stake should be unchanged").toBe(originStakeBefore);
                expect(destinationStakeAfter, "Destination stake should be unchanged").toBe(destinationStakeBefore);
            },
        });
    },
});
