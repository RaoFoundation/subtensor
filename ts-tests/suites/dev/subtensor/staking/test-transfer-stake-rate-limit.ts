import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { generateKeyringPair } from "../../../../utils/account";

const TAO = 1_000_000_000n; // 10^9 RAO per TAO
const tao = (value: number): bigint => TAO * BigInt(value);

async function devForceSetBalance(
    polkadotJs: ApiPromise,
    context: any,
    address: string,
    amount: bigint
): Promise<void> {
    await context.createBlock([
        await polkadotJs.tx.sudo
            .sudo(polkadotJs.tx.balances.forceSetBalance(address, amount))
            .signAsync(context.keyring.alice),
    ]);
}

async function devSudoSetLockReductionInterval(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    interval: number
): Promise<void> {
    await context.createBlock([await polkadotJs.tx.adminUtils.sudoSetLockReductionInterval(interval).signAsync(alice)]);
}

async function devRegisterSubnet(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    hotkey: KeyringPair
): Promise<number> {
    await context.createBlock([await polkadotJs.tx.subtensorModule.registerNetwork(hotkey.address).signAsync(alice)]);
    const events = (await polkadotJs.query.system.events()) as any;
    const netuid = (events as any[]).filter((e: any) => e.event.method === "NetworkAdded")[0].event.data[0].toNumber();
    return netuid;
}

async function devEnableSubtoken(
    polkadotJs: ApiPromise,
    context: any,
    alice: KeyringPair,
    netuid: number
): Promise<void> {
    await context.createBlock([
        await polkadotJs.tx.sudo.sudo(polkadotJs.tx.adminUtils.sudoSetSubtokenEnabled(netuid, true)).signAsync(alice),
    ]);
}

async function devAssociateHotKey(
    polkadotJs: ApiPromise,
    context: any,
    coldkey: KeyringPair,
    hotkey: string
): Promise<void> {
    await context.createBlock([await polkadotJs.tx.subtensorModule.tryAssociateHotkey(hotkey).signAsync(coldkey)]);
}

async function devGetAlphaStake(
    polkadotJs: ApiPromise,
    hotkey: string,
    coldkey: string,
    netuid: number
): Promise<bigint> {
    const value = (await polkadotJs.query.subtensorModule.alphaV2(hotkey, coldkey, netuid)) as any;
    const mantissa = value.mantissa;
    const exponent = value.exponent;
    if (exponent >= 0n) {
        return BigInt(mantissa) * BigInt(10) ** BigInt(exponent);
    }
    return BigInt(mantissa) / BigInt(10) ** BigInt(-exponent);
}

describeSuite({
    id: "DEV_SUB_STAKING_TRANSFER_RATE_LIMIT",
    title: "staking — same-block add_stake / transfer_stake (no per-block rate limiter)",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let destinationColdkey: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");
            destinationColdkey = generateKeyringPair("sr25519");

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            // ensure destination coldkey can receive transferred stake
            await devForceSetBalance(polkadotJs, context, destinationColdkey.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            await context.createBlock([
                await polkadotJs.tx.sudo.sudo(polkadotJs.tx.adminUtils.sudoSetNetworkRateLimit(0)).signAsync(alice),
            ]);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
        });

        it({
            id: "T01",
            title: "add_stake + same-subnet transfer_stake in one block both succeed",
            test: async () => {
                // Both extrinsics are signed by alice, so use explicit incrementing
                // nonces to land them in the same block in submission order.
                const aliceNonce = ((await polkadotJs.query.system.account(alice.address)) as any).nonce.toNumber();

                // Stake a large amount so the same-block transfer has plenty of alpha
                // to move and clears the DefaultMinStake floor.
                const addTx = await polkadotJs.tx.subtensorModule
                    .addStake(aliceHotKey.address, netuid, tao(100))
                    .signAsync(alice, { nonce: aliceNonce });

                const transferAmount = 1_000_000_000n;
                const transferTx = await polkadotJs.tx.subtensorModule
                    .transferStake(destinationColdkey.address, aliceHotKey.address, netuid, netuid, transferAmount)
                    .signAsync(alice, { nonce: aliceNonce + 1 });

                const { result } = await context.createBlock([addTx, transferTx]);
                const [addAttempt, transferAttempt] = result;

                expect(addAttempt.successful).toEqual(true);
                expect(transferAttempt.successful).toEqual(true);
            },
        });

        it({
            id: "T02",
            title: "add_stake then transfer_stake across SEPARATE blocks both succeed",
            test: async () => {
                // add in its own block
                const {
                    result: [addAttempt2],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule
                        .addStake(aliceHotKey.address, netuid, tao(100))
                        .signAsync(alice),
                ]);
                expect(addAttempt2.successful).toEqual(true);

                const alphaStaked = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const transferAmount = alphaStaked / 2n;
                expect(transferAmount > 0n).toEqual(true);

                // transfer in the NEXT block — same triple, succeeds
                const {
                    result: [transferAttempt2],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule
                        .transferStake(destinationColdkey.address, aliceHotKey.address, netuid, netuid, transferAmount)
                        .signAsync(alice),
                ]);
                expect(transferAttempt2.successful).toEqual(true);
            },
        });

        it({
            id: "T03",
            title: "two add_stake on the IDENTICAL (coldkey, hotkey, netuid) in the SAME block both succeed",
            test: async () => {
                const aliceNonce = ((await polkadotJs.query.system.account(alice.address)) as any).nonce.toNumber();

                const addTx1 = await polkadotJs.tx.subtensorModule
                    .addStake(aliceHotKey.address, netuid, tao(10))
                    .signAsync(alice, { nonce: aliceNonce });

                const addTx2 = await polkadotJs.tx.subtensorModule
                    .addStake(aliceHotKey.address, netuid, tao(10))
                    .signAsync(alice, { nonce: aliceNonce + 1 });

                const { result } = await context.createBlock([addTx1, addTx2]);
                const [addAttempt1, addAttempt2] = result;

                expect(addAttempt1.successful).toEqual(true);
                expect(addAttempt2.successful).toEqual(true);
            },
        });

        it({
            id: "T04",
            title: "remove_stake then transfer_stake on the IDENTICAL (coldkey, hotkey, netuid) in the SAME block both succeed",
            test: async () => {
                const {
                    result: [seedAdd],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule
                        .addStake(aliceHotKey.address, netuid, tao(100))
                        .signAsync(alice),
                ]);
                expect(seedAdd.successful).toEqual(true);

                // Size both legs as a real fraction of available alpha so neither trips the
                // DefaultMinStake floor, and their sum stays below the available balance.
                const alphaStaked = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const legAmount = alphaStaked / 4n;
                expect(legAmount > 0n).toEqual(true);

                const aliceNonce = ((await polkadotJs.query.system.account(alice.address)) as any).nonce.toNumber();

                const removeTx = await polkadotJs.tx.subtensorModule
                    .removeStake(aliceHotKey.address, netuid, legAmount)
                    .signAsync(alice, { nonce: aliceNonce });

                const transferTx = await polkadotJs.tx.subtensorModule
                    .transferStake(destinationColdkey.address, aliceHotKey.address, netuid, netuid, legAmount)
                    .signAsync(alice, { nonce: aliceNonce + 1 });

                const { result } = await context.createBlock([removeTx, transferTx]);
                const [removeAttempt, transferAttempt] = result;

                expect(removeAttempt.successful).toEqual(true);
                expect(transferAttempt.successful).toEqual(true);
            },
        });

        it({
            id: "T05",
            title: "add_stake + CROSS-subnet transfer_stake in one block is no longer rate-limited (limiter removed) — it now falls through to the normal amount check",
            test: async () => {
                const netuid2 = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
                await devEnableSubtoken(polkadotJs, context, alice, netuid2);

                const aliceNonce = ((await polkadotJs.query.system.account(alice.address)) as any).nonce.toNumber();

                const addTx = await polkadotJs.tx.subtensorModule
                    .addStake(aliceHotKey.address, netuid, tao(100))
                    .signAsync(alice, { nonce: aliceNonce });

                const transferTx = await polkadotJs.tx.subtensorModule
                    .transferStake(destinationColdkey.address, aliceHotKey.address, netuid, netuid2, 1000n)
                    .signAsync(alice, { nonce: aliceNonce + 1 });

                const { result } = await context.createBlock([addTx, transferTx]);
                const [addAttempt, transferAttempt] = result;

                expect(addAttempt.successful).toEqual(true);
                expect(transferAttempt.successful).toEqual(false);
                expect(transferAttempt.error.name).not.toEqual("StakingOperationRateLimitExceeded");
                expect(transferAttempt.error.name).toEqual("AmountTooLow");
            },
        });
    },
});
