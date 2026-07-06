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
    buildSignedOrder,
    FAR_FUTURE,
    fetchChainId,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";
import { encryptTransaction } from "../../../../utils/shield_helpers.js";
import { u8aToHex } from "@polkadot/util";

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_MEVSHIELD",
    title: "execute_orders via MEVShield submit_encrypted",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();

            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            // Create 3+ blocks so PendingKey is populated (needs 2 blocks for the
            // AuthorKeys → NextKey → PendingKey pipeline to fill). The subsequent setup
            // transactions each create additional blocks, so 2 here is sufficient.
            await context.createBlock([]);
            await context.createBlock([]);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
        });

        it({
            id: "T01",
            title: "LimitBuy submitted via MEVShield submit_encrypted is decrypted and executed in the same block",
            test: async () => {
                // Use PendingKey — this is the key the current block's proposer checks against.
                // NextKey is one rotation ahead; encrypting with it would require waiting an extra
                // block for it to advance to PendingKey, which doesn't happen automatically in
                // manual-seal mode.
                const pendingKeyRaw = await polkadotJs.query.mevShield.pendingKey();
                if ((pendingKeyRaw as any).isNone)
                    throw new Error("MEVShield PendingKey not available — create more blocks first");
                const nextKeyBytes = (pendingKeyRaw as any).unwrap().toU8a(true);

                const signedOrder = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    relayer: null,
                    chainId,
                });

                // Get alice's current nonce so we can pre-sign the inner tx at nonce+1
                const aliceNonce = (
                    (await polkadotJs.query.system.account(alice.address)) as any
                ).nonce.toNumber() as number;

                // Sign the inner execute_orders tx at nonce+1, then get its raw bytes
                const innerTx = await polkadotJs.tx.limitOrders
                    .executeOrders([signedOrder], false)
                    .signAsync(alice, { nonce: aliceNonce + 1 });
                const innerTxBytes = innerTx.toU8a();

                // Encrypt the inner tx with the MEVShield NextKey
                const ciphertext = await encryptTransaction(innerTxBytes, nextKeyBytes);

                // submit_encrypted requires a mortal era — immortal is rejected by CheckMortality.
                // Anchor to the PARENT block, not the current best block.
                //
                // try_decode_shielded_tx is a runtime API call executed at parent_hash (block B's
                // state). CheckMortality::implicit() looks up BlockHash[birth]. In block B's state,
                // only blocks 0..B-1 are stored — BlockHash[B] is populated when block B+1
                // initializes. If we sign with { current: B }, birth = B and the lookup fails
                // (AncientBirthBlock), check() returns Err, and try_decode_shielded_tx returns None,
                // so the outer tx is included as a plain tx with no inner tx extracted.
                // Anchoring to B-1 (the parent) means birth = B-1, which IS in BlockHash at block
                // B's state, so implicit() succeeds and the signature verifies correctly.
                const header = await polkadotJs.rpc.chain.getHeader();
                const blockNumber = header.number.toNumber() - 1;
                const blockHash = header.parentHash;
                const era = polkadotJs.createType("ExtrinsicEra", { current: blockNumber, period: 8 });

                // Submit the wrapper directly to the pool (not via createBlock) so the proposer
                // scans the pool naturally and runs shielded-tx detection.
                const signedWrapper = await polkadotJs.tx.mevShield
                    .submitEncrypted(u8aToHex(ciphertext))
                    .signAsync(alice, { nonce: aliceNonce, era, blockHash });
                await polkadotJs.rpc.author.submitExtrinsic(signedWrapper.toHex());

                // Seal a block — the proposer detects the shielded tx in the pool, decrypts the
                // inner execute_orders, and includes both in the same block.
                await context.createBlock([]);

                // Assert the order is Fulfilled
                const id = orderId(polkadotJs, signedOrder.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");
            },
        });

        it({
            id: "T02",
            title: "LimitBuy with a designated relayer is executed when the relayer submits via MEVShield",
            test: async () => {
                const relayer = generateKeyringPair("sr25519");
                await devForceSetBalance(polkadotJs, context, relayer.address, tao(100));

                const pendingKeyRaw = await polkadotJs.query.mevShield.pendingKey();
                if ((pendingKeyRaw as any).isNone)
                    throw new Error("MEVShield PendingKey not available — create more blocks first");
                const pendingKeyBytes = (pendingKeyRaw as any).unwrap().toU8a(true);

                const signedOrder = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    relayer: [relayer.address],
                    chainId,
                });

                // The relayer submits the encrypted execute_orders tx on Alice's behalf.
                // relayerNonce+0 = outer submit_encrypted, relayerNonce+1 = inner execute_orders.
                const relayerNonce = (
                    (await polkadotJs.query.system.account(relayer.address)) as any
                ).nonce.toNumber() as number;

                const innerTx = await polkadotJs.tx.limitOrders
                    .executeOrders([signedOrder], false)
                    .signAsync(relayer, { nonce: relayerNonce + 1 });
                const innerTxBytes = innerTx.toU8a();

                const ciphertext = await encryptTransaction(innerTxBytes, pendingKeyBytes);

                const header = await polkadotJs.rpc.chain.getHeader();
                const blockNumber = header.number.toNumber() - 1;
                const blockHash = header.parentHash;
                const era = polkadotJs.createType("ExtrinsicEra", { current: blockNumber, period: 8 });

                const signedWrapper = await polkadotJs.tx.mevShield
                    .submitEncrypted(u8aToHex(ciphertext))
                    .signAsync(relayer, { nonce: relayerNonce, era, blockHash });
                await polkadotJs.rpc.author.submitExtrinsic(signedWrapper.toHex());

                await context.createBlock([]);

                const id = orderId(polkadotJs, signedOrder.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");
            },
        });
    },
});
