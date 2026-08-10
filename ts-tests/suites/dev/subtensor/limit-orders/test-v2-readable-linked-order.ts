import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAddStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devExecuteOrders,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    asV2,
    buildReadableSignedOrderV2,
    buildSignedOrderV2,
    buildWrappedSignedOrderV2,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    formatOrderMessageV2,
    getLinkedOutput,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// All three v2 signing forms must be accepted on chain, the readable one being the
// Ledger / clear-signing path:
//
//   raw       — signature over SCALE(VersionedOrder::V2)
//   wrapped   — signature over <Bytes> ++ blake2_256(SCALE(..)) ++ </Bytes>
//   readable  — signature over blake2_256(<Bytes> ++ utf8(render_order) ++ </Bytes>)
//
// The readable form is the one that can drift silently: the runtime recomputes the
// message from the payload, so a single byte of disagreement between
// `formatOrderMessageV2` here and `render_order` in Rust turns into
// `InvalidSignature`.  Executing a readable-signed linked order is therefore the real
// parity check; `test-v2-readable-message-format.ts` pins the string itself.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_V2_READABLE_SIGN",
    title: "limit-orders v2 — readable/wrapped/raw signing of linked orders",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let bob: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            bob = context.keyring.bob;
            aliceHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);

            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        // An `order_id` is `blake2_256(SCALE(payload))` and the signing form is NOT part
        // of the preimage — a wrapped-signed and a readable-signed order over the same
        // fields share one id.  Since every `it` here shares one chain, two tests
        // building the same provider fields would collide and the second execution would
        // be refused `OrderAlreadyProcessed`.  Salting the expiry keeps them distinct.
        let salt = 0n;
        function uniqueExpiry(): bigint {
            salt += 1n;
            return FAR_FUTURE - salt;
        }

        /**
         * Build a sell provider payload with the given signing form.
         *
         * Each call produces a distinct `order_id` (see `uniqueExpiry`), so a provider is
         * never accidentally shared between tests.
         */
        function providerOrder(build: typeof buildSignedOrderV2, amount: bigint) {
            return build(polkadotJs, {
                signer: alice,
                hotkey: aliceHotKey.address,
                netuid,
                orderType: "TakeProfit",
                amount: { Fixed: amount },
                limitPrice: 0n,
                expiry: uniqueExpiry(),
                feeRate: 0,
                feeRecipient: alice.address,
                chainId,
                hasLinkedOrder: true,
            });
        }

        /** Build a linked buy payload against `provider` with the given signing form. */
        function linkedOrder(build: typeof buildSignedOrderV2, provider: `0x${string}`) {
            return build(polkadotJs, {
                signer: alice,
                hotkey: aliceHotKey.address,
                netuid,
                orderType: "LimitBuy",
                amount: { LinkedPercentage: { provider, pct: 1_000_000_000 } },
                limitPrice: FAR_FUTURE,
                expiry: FAR_FUTURE,
                feeRate: 0,
                feeRecipient: alice.address,
                chainId,
            });
        }

        it({
            id: "T01",
            title: "a readable-signed provider and readable-signed linked order both execute",
            test: async () => {
                const provider = providerOrder(buildReadableSignedOrderV2, tao(100));
                const providerId = orderId(polkadotJs, provider.order);
                const linked = linkedOrder(buildReadableSignedOrderV2, providerId);
                const linkedId = orderId(polkadotJs, linked.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider, linked], true);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(0);
                expect(filterEvents(events, "OrderExecuted").length).toBe(2);

                expect(await getOrderStatus(polkadotJs, providerId)).toBe("Fulfilled");
                expect(await getOrderStatus(polkadotJs, linkedId)).toBe("Fulfilled");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();
            },
        });

        it({
            id: "T02",
            title: "wrapped-hash and raw signing forms are also accepted for v2",
            test: async () => {
                const provider = providerOrder(buildWrappedSignedOrderV2, tao(50));
                const providerId = orderId(polkadotJs, provider.order);
                const linked = linkedOrder(buildSignedOrderV2, providerId);
                const linkedId = orderId(polkadotJs, linked.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider, linked], true);

                expect(await getOrderStatus(polkadotJs, providerId)).toBe("Fulfilled");
                expect(await getOrderStatus(polkadotJs, linkedId)).toBe("Fulfilled");
            },
        });

        it({
            id: "T03",
            title: "flipping has_linked_order after readable signing is rejected",
            test: async () => {
                // The whole reason the flag appears in the readable message. Sign a
                // non-provider order, then set the flag: if the message omitted it, the
                // recomputed message would still match and the tampered order would
                // record a drawable output the user never authorised.
                const plain = buildReadableSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(10) },
                    limitPrice: 0n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: false,
                });

                const tampered = {
                    ...plain,
                    order: { V2: { ...asV2(plain.order), has_linked_order: true } },
                };

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([tampered], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("InvalidSignature");
            },
        });

        it({
            id: "T04",
            title: "swapping a linked order's provider after readable signing is rejected",
            test: async () => {
                const provider = providerOrder(buildReadableSignedOrderV2, tao(50));
                const providerId = orderId(polkadotJs, provider.order);
                await devExecuteOrders(polkadotJs, context, bob, [provider], true);
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();

                const linked = linkedOrder(buildReadableSignedOrderV2, providerId);
                // Repoint the link at another order. The full 64-hex provider id is in the
                // signed message precisely so this cannot be done.
                const other = `0x${"77".repeat(32)}` as `0x${string}`;
                const tampered = {
                    ...linked,
                    order: {
                        V2: {
                            ...asV2(linked.order),
                            amount: { LinkedPercentage: { provider: other, pct: 1_000_000_000 } },
                        },
                    },
                };

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([tampered], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("InvalidSignature");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T05",
            title: "the message a provider signs carries the has-linked-order tail",
            test: async () => {
                // Cross-check that what `providerOrder` builds — the same helper whose
                // output T01 submitted and had accepted on chain — renders the flag. A
                // guard against the tail silently disappearing from the formatter while
                // the on-chain tests keep passing via the raw signing form.
                const provider = providerOrder(buildReadableSignedOrderV2, tao(100));
                const msg = formatOrderMessageV2(asV2(provider.order));
                expect(msg.endsWith(", has-linked-order true")).toBe(true);
                expect(msg.startsWith("TAO.com order v2: Take-profit ")).toBe(true);

                const linked = linkedOrder(buildReadableSignedOrderV2, orderId(polkadotJs, provider.order));
                const linkedMsg = formatOrderMessageV2(asV2(linked.order));
                expect(linkedMsg.endsWith(", has-linked-order false")).toBe(true);
                expect(linkedMsg.includes(" ppb of order 0x")).toBe(true);
            },
        });
    },
});
