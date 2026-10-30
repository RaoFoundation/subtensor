import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import {
    addNewSubnetwork,
    addStake,
    forceSetBalance,
    generateKeyringPair,
    getStake,
    startCall,
    sudoSetLockReductionInterval,
    tao,
    waitForTransactionCompletion,
} from "../../utils";
import { subtensor } from "@polkadot-api/descriptors";
import { Binary, type TypedApi } from "polkadot-api";

describeSuite({
    id: "00_add_stake",
    title: "▶ add_stake extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;

        const hotkey = generateKeyringPair("sr25519");
        const coldkey = generateKeyringPair("sr25519");
        const hotkeyAddress = hotkey.address;
        const coldkeyAddress = coldkey.address;
        let netuid: number;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);

            // Set lock reduction interval to 1 block to make network registration lock cost decay instantly.
            // By default, the lock cost doubles with each subnet registration and decays over 14 days (100,800 blocks).
            // Without this, tests creating multiple subnets would fail with CannotAffordLockCost.
            await sudoSetLockReductionInterval(api, 1);

            await forceSetBalance(api, hotkeyAddress);
            await forceSetBalance(api, coldkeyAddress);
            netuid = await addNewSubnetwork(api, hotkey, coldkey);
            await startCall(api, netuid, coldkey);
        });

        it({
            id: "T00",
            title: "Legacy v4 transactions use only their v16 extension pipeline",
            test: async () => {
                // Moonwall's client and the directly imported signer must agree
                // on pipeline 0. Older PAPI decoders also read the hashed proof
                // from a v4 transaction, overrunning this deliberately tiny call.
                const nonce = (await api.query.System.Account.getValue(coldkeyAddress)).nonce;
                await waitForTransactionCompletion(
                    api.tx.System.remark({ remark: Binary.fromBytes(new Uint8Array()) }),
                    coldkey
                );
                expect((await api.query.System.Account.getValue(coldkeyAddress)).nonce).toBe(nonce + 1);
            },
        });

        it({
            id: "T01",
            title: "Add staking payable",
            test: async () => {
                // Get initial stake
                const stakeBefore = await getStake(api, hotkeyAddress, coldkeyAddress, netuid);

                // Add stake
                const stakeAmount = tao(100);
                await addStake(api, coldkey, hotkeyAddress, netuid, stakeAmount);

                // Verify stake increased
                const stakeAfter = await getStake(api, hotkeyAddress, coldkeyAddress, netuid);
                expect(stakeAfter, "Stake should increase after adding stake").toBeGreaterThan(stakeBefore);

                log("✅ Successfully added stake.");
            },
        });
    },
});
