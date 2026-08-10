import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devGetAlphaStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildReadableSignedOrder,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// One subnet per file — this test submits real buy orders signed over the
// `<Bytes>`-wrapped canonical human-readable ("clear-signing") message, the
// form a hardware wallet (Ledger) displays field-by-field.  It exercises the
// runtime's `verify_readable` path:
//   signature.verify(b"<Bytes>" ++ utf8(render_order(order)) ++ b"</Bytes>", signer)
// for BOTH an ed25519 signer (the hardware/Ledger case) and an sr25519 signer.
// Both orders are relayed/submitted by Alice via execute_batched_orders.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_READABLE",
    title: "execute_batched_orders — human-readable (clear-signing) LimitBuy execution",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let edSigner: KeyringPair;
        let edHotKey: KeyringPair;
        let srSigner: KeyringPair;
        let srHotKey: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();

            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");

            // ed25519 coldkey/signer (hardware / Ledger case) with an sr25519 hotkey.
            edSigner = generateKeyringPair("ed25519");
            edHotKey = generateKeyringPair("sr25519");

            // sr25519 coldkey/signer with its own sr25519 hotkey.
            srSigner = generateKeyringPair("sr25519");
            srHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, edSigner.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, srSigner.address, tao(10_000));

            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);

            // Associate hotkeys — each signer associates its own hotkey.
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, edSigner, edHotKey.address);
            await devAssociateHotKey(polkadotJs, context, srSigner, srHotKey.address);
        });

        it({
            id: "T01",
            title: "LimitBuy executes with an ed25519 readable (clear-signing) signature",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, edHotKey.address, edSigner.address, netuid);
                const taoBalanceBefore = (await polkadotJs.query.system.account(edSigner.address)).data.free.toBigInt();

                const signed = buildReadableSignedOrder(polkadotJs, {
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

                // Alice relays/submits the ed25519 readable-signed order.
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(netuid, [signed]).signAsync(alice),
                ]);
                expect(attempt.successful).toEqual(true);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);

                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");

                // Alpha stake for the ed25519 signer's hotkey should have increased.
                const stakeAfter = await devGetAlphaStake(polkadotJs, edHotKey.address, edSigner.address, netuid);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);

                // ed25519 signer's TAO balance should have decreased.
                const taoBalanceAfter = (await polkadotJs.query.system.account(edSigner.address)).data.free.toBigInt();
                expect(taoBalanceAfter).toBeLessThan(taoBalanceBefore);
            },
        });

        it({
            id: "T02",
            title: "LimitBuy executes with an sr25519 readable (clear-signing) signature",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, srHotKey.address, srSigner.address, netuid);
                const taoBalanceBefore = (await polkadotJs.query.system.account(srSigner.address)).data.free.toBigInt();

                const signed = buildReadableSignedOrder(polkadotJs, {
                    signer: srSigner,
                    hotkey: srHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: srSigner.address,
                    chainId,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(netuid, [signed]).signAsync(alice),
                ]);
                expect(attempt.successful).toEqual(true);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);

                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");

                const stakeAfter = await devGetAlphaStake(polkadotJs, srHotKey.address, srSigner.address, netuid);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);

                const taoBalanceAfter = (await polkadotJs.query.system.account(srSigner.address)).data.free.toBigInt();
                expect(taoBalanceAfter).toBeLessThan(taoBalanceBefore);
            },
        });
    },
});
