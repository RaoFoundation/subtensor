import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair, tao } from "../../../../utils";
import {
    devAssociateHotKey,
    devEnableSubtoken,
    devExecuteOrders,
    devForceSetBalance,
    devGetAlphaStake,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    FAR_FUTURE,
    buildWrappedSignedOrder,
    fetchChainId,
    filterEvents,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// One subnet per file — this test submits a real buy order signed by an
// ed25519 key over the `<Bytes>`-wrapped order hash (the Ledger / signRaw
// form).  It exercises the runtime's alternative `is_order_valid` path:
//   signature.verify(b"<Bytes>" ++ blake2_256(SCALE(VersionedOrder)) ++ b"</Bytes>", signer)
// with an Ed25519 signature.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_ED25519_WRAPPED",
    title: "execute_orders — ed25519 + <Bytes>-wrapped LimitBuy execution",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let edSigner: KeyringPair;
        let edHotKey: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();

            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");

            // ed25519 coldkey/signer that signs the wrapped order hash, with an
            // sr25519 hotkey associated to it.
            edSigner = generateKeyringPair("ed25519");
            edHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, edSigner.address, tao(10_000));

            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            // Enable subtoken
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            // Associate hotkeys — the ed25519 signer associates its own hotkey.
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, edSigner, edHotKey.address);
        });

        it({
            id: "T01",
            title: "LimitBuy executes with an ed25519 <Bytes>-wrapped signature",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, edHotKey.address, edSigner.address, netuid);
                const taoBalanceBefore = (
                    (await polkadotJs.query.system.account(edSigner.address)) as any
                ).data.free.toBigInt();

                const signed = buildWrappedSignedOrder(polkadotJs, {
                    signer: edSigner,
                    hotkey: edHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: edSigner.address,
                    chainId,
                });

                // Alice relays/submits the ed25519-signed order.
                await devExecuteOrders(polkadotJs, context, alice, [signed]);

                const events = await polkadotJs.query.system.events();
                const executed = filterEvents(events, "OrderExecuted");
                expect(executed.length).toBe(1);

                // OrderId should be stored as Fulfilled
                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");

                // Alpha stake for the ed25519 signer's hotkey should have increased
                const stakeAfter = await devGetAlphaStake(polkadotJs, edHotKey.address, edSigner.address, netuid);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);

                // ed25519 signer's TAO balance should have decreased
                const taoBalanceAfter = (
                    (await polkadotJs.query.system.account(edSigner.address)) as any
                ).data.free.toBigInt();
                expect(taoBalanceAfter).toBeLessThan(taoBalanceBefore);
            },
        });
    },
});
