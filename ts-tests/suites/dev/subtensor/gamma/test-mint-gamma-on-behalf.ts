import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account.js";
import { tao } from "../../../../utils/balance.js";
import {
    devAssociateHotKey,
    devEnableSubtoken,
    devForceSetBalance,
    devGetAlphaStake,
    devRegisterSubnet,
} from "../../../../utils/dev-helpers.js";

const U64_MAX = 18_446_744_073_709_551_615n;
/** 400 dollars per TAO, in gamma units (10^9 per dollar). */
const TAO_USD_PRICE = 400_000_000_000n;

describeSuite({
    id: "DEV_SUB_GAMMA_MINT_ON_BEHALF",
    title: "mint_gamma_on_behalf / mint_gamma_with_stake_on_behalf",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let payer: KeyringPair;
        let recipient: KeyringPair;
        let other: KeyringPair;
        let ownerHotkey: KeyringPair;
        let netuid: number;

        const deposit = async (hotkey: string) =>
            ((await polkadotJs.query.subtensorModule.gammaDeposits(netuid, hotkey)) as any).unwrapOr(null);
        const freeBalance = async (address: string): Promise<bigint> =>
            ((await polkadotJs.query.system.account(address)) as any).data.free.toBigInt();
        const sudo = async (call: any) => {
            const {
                result: [attempt],
            } = await context.createBlock([await polkadotJs.tx.sudo.sudo(call).signAsync(alice)]);
            expect(attempt.successful).toEqual(true);
        };
        const mintOnBehalf = async (hotkey: string, amount: bigint, minAmountOut: bigint) => {
            const {
                result: [attempt],
            } = await context.createBlock([
                await polkadotJs.tx.subtensorModule
                    .mintGammaOnBehalf(recipient.address, hotkey, netuid, amount, minAmountOut)
                    .signAsync(payer),
            ]);
            return attempt;
        };

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            payer = generateKeyringPair("sr25519");
            recipient = generateKeyringPair("sr25519");
            other = generateKeyringPair("sr25519");
            ownerHotkey = generateKeyringPair("sr25519");

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, payer.address, tao(1_000));
            await devForceSetBalance(polkadotJs, context, other.address, tao(1_000));
            await sudo(polkadotJs.tx.adminUtils.sudoSetNetworkRateLimit(0));
            netuid = await devRegisterSubnet(polkadotJs, context, alice, ownerHotkey);
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, ownerHotkey.address);

            await sudo(polkadotJs.tx.adminUtils.sudoSetGammaEnabled(true));
            await sudo(polkadotJs.tx.adminUtils.sudoSetSubnetGammaEnabled(netuid, true));
            await sudo(polkadotJs.tx.adminUtils.sudoSetTaoUsdPriceOracle(alice.address));
            const {
                result: [fed],
            } = await context.createBlock([
                await polkadotJs.tx.subtensorModule.feedTaoUsdPrice(TAO_USD_PRICE).signAsync(alice),
            ]);
            expect(fed.successful).toEqual(true);
        });

        it({
            id: "T01",
            title: "credits the recipient's new deposit and charges the signer",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const [payerBefore, recipientBefore] = [
                    await freeBalance(payer.address),
                    await freeBalance(recipient.address),
                ];

                const attempt = await mintOnBehalf(hotkey, tao(1), 0n);
                expect(attempt.successful).toEqual(true);

                const opened = await deposit(hotkey);
                expect(opened).not.toBeNull();
                expect(opened.coldkey.toString()).toEqual(recipient.address);
                expect(opened.balance.toBigInt() > 0n).toBe(true);
                expect(opened.dailySpendCap.isNone).toBe(true);
                expect(payerBefore - (await freeBalance(payer.address)) >= tao(1)).toBe(true);
                expect(await freeBalance(recipient.address)).toEqual(recipientBefore);
            },
        });

        it({
            id: "T02",
            title: "tops up the recipient's existing deposit",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                expect((await mintOnBehalf(hotkey, tao(1), 0n)).successful).toEqual(true);
                const before = (await deposit(hotkey)).balance.toBigInt();

                expect((await mintOnBehalf(hotkey, tao(1), 0n)).successful).toEqual(true);

                const after = await deposit(hotkey);
                expect(after.coldkey.toString()).toEqual(recipient.address);
                expect(after.balance.toBigInt() > before).toBe(true);
            },
        });

        it({
            id: "T03",
            title: "refuses a hotkey that holds another coldkey's deposit",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const {
                    result: [own],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule.mintGamma(hotkey, netuid, tao(1), 0n, null).signAsync(other),
                ]);
                expect(own.successful).toEqual(true);

                const attempt = await mintOnBehalf(hotkey, tao(1), 0n);
                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NonAssociatedColdKey");
                expect((await deposit(hotkey)).coldkey.toString()).toEqual(other.address);
            },
        });

        it({
            id: "T04",
            title: "refuses a mint below min_amount_out",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const attempt = await mintOnBehalf(hotkey, tao(1), U64_MAX);
                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("SlippageTooHigh");
                expect(await deposit(hotkey)).toBeNull();
            },
        });

        it({
            id: "T05",
            title: "mint_gamma_with_stake_on_behalf burns the signer's stake and credits the recipient",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519").address;
                const {
                    result: [staked],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule.addStake(ownerHotkey.address, netuid, tao(10)).signAsync(payer),
                ]);
                expect(staked.successful).toEqual(true);
                const stake = await devGetAlphaStake(polkadotJs, ownerHotkey.address, payer.address, netuid);
                expect(stake > 0n).toBe(true);

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.subtensorModule
                        .mintGammaWithStakeOnBehalf(recipient.address, ownerHotkey.address, hotkey, netuid, stake, 0n)
                        .signAsync(payer),
                ]);
                expect(attempt.successful).toEqual(true);

                expect(await devGetAlphaStake(polkadotJs, ownerHotkey.address, payer.address, netuid)).toEqual(0n);
                const opened = await deposit(hotkey);
                expect(opened.coldkey.toString()).toEqual(recipient.address);
                expect(opened.balance.toBigInt() > 0n).toBe(true);
            },
        });
    },
});
