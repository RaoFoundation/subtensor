import { beforeAll, expect } from "vitest";
import { describeSuite } from "@moonwall/cli";
import { generateKeyringPair, tao } from "../../../../utils";
import type { ApiPromise } from "@polkadot/api";
import { devAssociateHotKey, devForceSetBalance, devSetWeightsTx } from "../../../../utils/dev-helpers.ts";

describeSuite({
    id: "00_transaction_payment_wrapper_dev",
    title: "Transaction payment wrapper",
    foundationMethods: "dev",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;

        beforeAll(() => {
            api = context.polkadotJs();
        });

        it({
            id: "T01",
            title: "Fees for set_weights charged by coldkey instead of origin(hotkey)",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");

                log(`coldkey: ${coldkey.address}`);
                log(`hotkey: ${hotkey.address}`);

                const initialBalance = tao(1e10);

                log("Set Up");
                const existentialDeposit = await api.consts.balances.existentialDeposit.toBigInt();
                // Hotkey should "exist", even if coldkey is paying for tx
                await devForceSetBalance(api, context, hotkey.address, existentialDeposit);
                await devForceSetBalance(api, context, coldkey.address, initialBalance);
                await devAssociateHotKey(api, context, coldkey, hotkey.address);

                const coldkeyBalanceBefore = (await api.query.system.account(coldkey.address)).data.free.toBigInt();

                log("Execute the tx from hotkey, but coldkey will pay");
                await devSetWeightsTx(api, context, hotkey, 0, [], [], 0n);

                const events = await api.query.system.events();
                const feeEvent = events.filter((a) => {
                    return a.event.method.toString() === "TransactionFeePaid";
                });

                const hotkeyBalance = (await api.query.system.account(hotkey.address)).data.free.toBigInt();
                const coldkeyBalanceAfter = (await api.query.system.account(coldkey.address)).data.free.toBigInt();
                // Fees paid by the hotkey
                const txFee = feeEvent[0].event.data.actualFee.toBigInt();
                expect(txFee).toBeGreaterThan(0n);
                expect(coldkeyBalanceAfter).toEqual(coldkeyBalanceBefore - txFee);
                expect(hotkeyBalance).toEqual(existentialDeposit);
            },
        });

        it({
            id: "T02",
            title: "Fees for set_weights charged from hotkey if no association",
            test: async () => {
                const coldkey = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");

                log(`coldkey: ${coldkey.address}`);
                log(`hotkey: ${hotkey.address}`);

                const initialBalance = tao(1e10);

                log("Set Up");
                const existentialDeposit = await api.consts.balances.existentialDeposit.toBigInt();
                // Hotkey should "exist", even if coldkey is paying for tx
                await devForceSetBalance(api, context, hotkey.address, initialBalance);

                const hotkeyBalanceBefore = (await api.query.system.account(hotkey.address)).data.free.toBigInt();

                log("Execute the tx from hotkey, no association, hotkey will pay");
                await devSetWeightsTx(api, context, hotkey, 0, [], [], 0n);

                const events = await api.query.system.events();
                const feeEvent = events.filter((a) => {
                    return a.event.method.toString() === "TransactionFeePaid";
                });

                const hotkeyBalanceAfter = (await api.query.system.account(hotkey.address)).data.free.toBigInt();
                const coldkeyBalanceAfter = (await api.query.system.account(coldkey.address)).data.free.toBigInt();
                // Fees paid by the hotkey
                const txFee = feeEvent[0].event.data.actualFee.toBigInt();
                expect(txFee).toBeGreaterThan(0n);
                expect(coldkeyBalanceAfter).toEqual(0n);
                expect(hotkeyBalanceBefore - txFee).toEqual(hotkeyBalanceAfter);
            },
        });
    },
});
