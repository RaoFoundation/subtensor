import { expect, beforeAll } from "vitest";
import { describeSuite } from "@moonwall/cli";
import {
    addNewSubnetwork,
    depositGamma,
    enableGamma,
    feedTaoUsdPrice,
    forceSetBalance,
    gamma,
    generateKeyringPair,
    getGammaBalance,
    getGammaDeposit,
    mintGamma,
    sendTransaction,
    startCall,
    sudoSetLockReductionInterval,
    tao,
    transferGamma,
    withdrawGamma,
} from "../../utils";
import { subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@moonwall/util";
import type { TypedApi } from "polkadot-api";

describeSuite({
    id: "12_transfer_gamma",
    title: "▶ deposit_gamma, withdraw_gamma and transfer_gamma extrinsics",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;
        let netuid: number;
        let ownerHotkey: KeyringPair;
        let coldkey: KeyringPair;
        let receiver: KeyringPair;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);

            ownerHotkey = generateKeyringPair("sr25519");
            const ownerColdkey = generateKeyringPair("sr25519");
            coldkey = generateKeyringPair("sr25519");
            receiver = generateKeyringPair("sr25519");
            await forceSetBalance(api, ownerHotkey.address);
            await forceSetBalance(api, ownerColdkey.address);
            await forceSetBalance(api, coldkey.address);
            await forceSetBalance(api, receiver.address);

            await sudoSetLockReductionInterval(api, 1);
            netuid = await addNewSubnetwork(api, ownerHotkey, ownerColdkey);
            await startCall(api, netuid, ownerColdkey);
            await enableGamma(api, netuid);
        }, 600000);

        it({
            id: "T01",
            title: "should withdraw minted gamma into a coldkey's balance",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const balanceBefore = await getGammaBalance(api, netuid, coldkey.address);

                await feedTaoUsdPrice(api);
                await mintGamma(api, coldkey, hotkey, netuid, tao(10));
                const minted = (await getGammaDeposit(api, netuid, hotkey))?.balance ?? 0n;
                log(`Minted ${minted} gamma units into the deposit`);
                expect(minted, "the deposit should hold the minted gamma").toBeGreaterThan(gamma(1));

                await withdrawGamma(api, coldkey, hotkey, netuid, minted);

                expect(await getGammaDeposit(api, netuid, hotkey), "emptying the deposit closes it").toBeUndefined();
                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(balanceBefore + minted);
            },
        });

        it({
            id: "T02",
            title: "should deposit gamma from the balance at a hotkey and withdraw it back",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const balanceBefore = await getGammaBalance(api, netuid, coldkey.address);
                const amount = balanceBefore / 2n;

                await depositGamma(api, coldkey, hotkey, netuid, amount);

                const deposit = await getGammaDeposit(api, netuid, hotkey);
                expect(deposit?.coldkey, "the deposit should belong to the coldkey").toEqual(coldkey.address);
                expect(deposit?.balance, "the deposit should hold the amount").toEqual(amount);
                expect(deposit?.daily_spend_cap, "a deposit opened from the balance has no cap").toBeUndefined();
                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(balanceBefore - amount);

                await withdrawGamma(api, coldkey, hotkey, netuid, amount);

                expect(await getGammaDeposit(api, netuid, hotkey), "emptying the deposit closes it").toBeUndefined();
                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(balanceBefore);
            },
        });

        it({
            id: "T03",
            title: "should transfer gamma to another coldkey's balance",
            test: async () => {
                const senderBefore = await getGammaBalance(api, netuid, coldkey.address);
                const receiverBefore = await getGammaBalance(api, netuid, receiver.address);

                await transferGamma(api, coldkey, receiver.address, undefined, undefined, netuid, gamma(1));

                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(senderBefore - gamma(1));
                expect(await getGammaBalance(api, netuid, receiver.address)).toEqual(receiverBefore + gamma(1));
            },
        });

        it({
            id: "T04",
            title: "should transfer gamma into a deposit it opens for another coldkey",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const senderBefore = await getGammaBalance(api, netuid, coldkey.address);

                await transferGamma(api, coldkey, receiver.address, undefined, hotkey, netuid, gamma(1));

                const deposit = await getGammaDeposit(api, netuid, hotkey);
                expect(deposit?.coldkey, "the deposit should belong to the receiver").toEqual(receiver.address);
                expect(deposit?.balance).toEqual(gamma(1));
                expect(deposit?.daily_spend_cap, "a deposit a transfer opens has no cap").toBeUndefined();
                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(senderBefore - gamma(1));
            },
        });

        it({
            id: "T05",
            title: "should transfer gamma from a deposit to another coldkey's balance",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                await depositGamma(api, coldkey, hotkey, netuid, gamma(2));
                const receiverBefore = await getGammaBalance(api, netuid, receiver.address);

                await transferGamma(api, coldkey, receiver.address, hotkey, undefined, netuid, gamma(1));

                expect((await getGammaDeposit(api, netuid, hotkey))?.balance).toEqual(gamma(1));
                expect(await getGammaBalance(api, netuid, receiver.address)).toEqual(receiverBefore + gamma(1));
            },
        });

        it({
            id: "T06",
            title: "should refuse a transfer below 0.1 gamma or into another coldkey's hotkey",
            test: async () => {
                const senderBefore = await getGammaBalance(api, netuid, coldkey.address);
                const receiverBefore = await getGammaBalance(api, netuid, receiver.address);

                const dust = api.tx.SubtensorModule.transfer_gamma({
                    destination_coldkey: receiver.address,
                    origin_hotkey: undefined,
                    destination_hotkey: undefined,
                    netuid: netuid,
                    amount: gamma(1) / 10n - 1n,
                });
                const dustResult = await sendTransaction(dust, coldkey);
                expect(dustResult.success, "a transfer below 0.1 gamma should fail").toBe(false);
                expect(dustResult.errorMessage).toContain("AmountTooLow");

                // The subnet owner's coldkey owns its hotkey, so the receiver cannot hold a deposit there.
                const owned = api.tx.SubtensorModule.transfer_gamma({
                    destination_coldkey: receiver.address,
                    origin_hotkey: undefined,
                    destination_hotkey: ownerHotkey.address,
                    netuid: netuid,
                    amount: gamma(1),
                });
                const ownedResult = await sendTransaction(owned, coldkey);
                expect(ownedResult.success, "a transfer into another coldkey's hotkey should fail").toBe(false);
                expect(ownedResult.errorMessage).toContain("NonAssociatedColdKey");

                expect(await getGammaBalance(api, netuid, coldkey.address)).toEqual(senderBefore);
                expect(await getGammaBalance(api, netuid, receiver.address)).toEqual(receiverBefore);
                expect(await getGammaDeposit(api, netuid, ownerHotkey.address)).toBeUndefined();
            },
        });
    },
});
