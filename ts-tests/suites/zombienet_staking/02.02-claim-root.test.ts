import { expect, beforeAll } from "vitest";
import { describeSuite } from "@moonwall/cli";
import {
    addNewSubnetwork,
    forceSetBalance,
    generateKeyringPair,
    getRootClaimThreshold,
    startCall,
    sudoSetLockReductionInterval,
    sudoSetRootClaimThreshold,
} from "../../utils";
import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";

describeSuite({
    id: "0202_sudo_set_root_claim_threshold",
    title: "▶ sudo_set_root_claim_threshold extrinsic",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let api: TypedApi<typeof subtensor>;
        const ROOT_NETUID = 0;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await sudoSetLockReductionInterval(api, 1);
        });

        it({
            id: "T0301",
            title: "should set the ROOT root-claim threshold and reject other netuids",
            test: async () => {
                // Get initial threshold
                const thresholdBefore = await getRootClaimThreshold(api, ROOT_NETUID);
                log(`Root claim threshold before: ${thresholdBefore}`);

                // Set new threshold value (MAX_ROOT_CLAIM_THRESHOLD is 10_000_000)
                // The value is stored as I96F32 fixed-point with 32 fractional bits
                const newThreshold = 1_000_000n;
                await sudoSetRootClaimThreshold(api, ROOT_NETUID, newThreshold);

                // Verify threshold changed
                // I96F32 encoding: newThreshold * 2^32 = 1_000_000 * 4294967296 = 4294967296000000
                const thresholdAfter = await getRootClaimThreshold(api, ROOT_NETUID);
                log(`Root claim threshold after: ${thresholdAfter}`);

                const expectedStoredValue = newThreshold * (1n << 32n); // I96F32 encoding
                expect(thresholdAfter).toBe(expectedStoredValue);

                // Claims only consult the ROOT entry, so setting any other netuid is rejected
                // rather than silently storing an inert value.
                const hotkey = generateKeyringPair("sr25519");
                const coldkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, hotkey.address);
                await forceSetBalance(api, coldkey.address);
                const netuid = await addNewSubnetwork(api, hotkey, coldkey);
                await startCall(api, netuid, coldkey);

                await expect(sudoSetRootClaimThreshold(api, netuid, newThreshold)).rejects.toThrow();

                log("✅ ROOT threshold set; non-ROOT netuid rejected.");
            },
        });
    },
});
