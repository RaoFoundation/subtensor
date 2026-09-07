import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import type { SubmittableExtrinsic } from "@polkadot/api/types";
import { Keyring } from "@polkadot/keyring";
import type { Bytes, Option } from "@polkadot/types";
import { randomAsU8a } from "@polkadot/util-crypto";

const generateKeyringPair = () => new Keyring({ type: "sr25519" }).addFromSeed(randomAsU8a(32));

const TAO = 1_000_000_000n;

describeSuite({
    id: "DEV_SUB_DISASSOCIATE",
    title: "disassociate_hotkey — ownership, cleanup and dispatch protections",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let coldkey: KeyringPair;

        async function submit(call: SubmittableExtrinsic<"promise">, signer = coldkey) {
            const { result } = await context.createBlock([await call.signAsync(signer)]);
            return result[0];
        }

        async function associate() {
            const hotkey = generateKeyringPair();
            expect((await submit(api.tx.subtensorModule.tryAssociateHotkey(hotkey.address))).successful).toBe(true);
            return hotkey;
        }

        // Pin work estimates to one block; count distinct netuid buckets per map.
        async function release(hotkey: string, signer = coldkey, underestimate = false) {
            const at = await api.at(await api.rpc.chain.getBlockHash());
            const module = at.query.subtensorModule;
            const [owned, staking, destinations, ...buckets] = await Promise.all([
                module.ownedHotkeys(signer.address),
                module.stakingHotkeys(signer.address),
                module.autoStakeDestinationColdkeys.entries(hotkey),
                module.subnetOwnerHotkey.keys(),
                module.pendingChildKeys.keys(),
                module.uids.keys(),
                module.lockingColdkeys.keys(),
                module.minerCollateral.keys(),
                module.rootClaimed.keys(),
            ]);
            const hotkeyCount = (owned as any).length + (staking as any).length;
            const autoStakeCount = (destinations as any[]).reduce((total, [, keys]) => total + keys.length, 0);
            const subnetCount = (buckets as any[][]).reduce(
                (total, keys) => total + new Set(keys.map((key) => key.args[0].toString())).size,
                (destinations as any[]).length
            );
            const maxItems = hotkeyCount + autoStakeCount + subnetCount;
            return api.tx.subtensorModule.disassociateHotkey(hotkey, underestimate ? maxItems - 1 : maxItems);
        }

        async function ownerExists(hotkey: string) {
            return (await api.rpc.state.getStorage<Option<Bytes>>(api.query.subtensorModule.owner.key(hotkey))).isSome;
        }

        async function expectFailure(call: SubmittableExtrinsic<"promise">, name: string, signer = coldkey) {
            const attempt = await submit(call, signer);
            expect(attempt.successful).toBe(false);
            const failed = attempt.events.find(({ event }) => api.events.system.ExtrinsicFailed.is(event));
            expect(failed).toBeDefined();
            const error = failed.event.data[0] as any;
            expect(api.registry.findMetaError(error.asModule).name).toBe(name);
        }

        async function seedStorage(entries: [string, string][]) {
            const attempt = await submit(api.tx.sudo.sudo(api.tx.system.setStorage(entries)), context.keyring.alice);
            expect(attempt.successful).toBe(true);
            const sudo = attempt.events.find(({ event }) => api.events.sudo.Sudid.is(event));
            expect((sudo.event.data[0] as any).isOk).toBe(true);
        }

        beforeAll(async () => {
            api = context.polkadotJs();
            coldkey = generateKeyringPair();
            await seedStorage([
                [api.query.subtensorModule.subtokenEnabled.key(0), api.createType("bool", true).toHex()],
                [api.query.subtensorModule.registrationsThisInterval.key(0), api.createType("u16", 0).toHex()],
            ]);
            expect(
                (
                    await submit(
                        api.tx.sudo.sudo(api.tx.balances.forceSetBalance(coldkey.address, 10_000n * TAO)),
                        context.keyring.alice
                    )
                ).successful
            ).toBe(true);
        });

        it({
            id: "T01",
            title: "releases ownership, emits its event, charges fees and allows reassociation",
            test: async () => {
                const hotkey = await associate();
                const before = (await api.query.system.account(coldkey.address)) as any;
                const attempt = await submit(await release(hotkey.address));
                expect(attempt.successful).toBe(true);
                const event = attempt.events.find(({ event }) =>
                    api.events.subtensorModule.HotkeyDisassociated.is(event)
                );
                expect(event.event.data.map((value) => value.toString())).toEqual([coldkey.address, hotkey.address]);
                expect(await ownerExists(hotkey.address)).toBe(false);
                for (const query of [
                    api.query.subtensorModule.ownedHotkeys,
                    api.query.subtensorModule.stakingHotkeys,
                ]) {
                    expect((await query(coldkey.address)).toJSON()).not.toContain(hotkey.address);
                }
                const after = (await api.query.system.account(coldkey.address)) as any;
                expect(after.data.free.toBigInt()).toBeLessThan(before.data.free.toBigInt());
                await expectFailure(await release(hotkey.address), "HotKeyAccountNotExists");
                expect(
                    (await submit(api.tx.subtensorModule.tryAssociateHotkey(hotkey.address), context.keyring.alice))
                        .successful
                ).toBe(true);
                expect((await api.query.subtensorModule.owner(hotkey.address)).toString()).toBe(
                    context.keyring.alice.address
                );
                await expectFailure(await release(hotkey.address), "NonAssociatedColdKey");
            },
        });

        it({
            id: "T02",
            title: "rejects a different coldkey and an insufficient work limit",
            test: async () => {
                const hotkey = await associate();
                await expectFailure(
                    await release(hotkey.address, context.keyring.alice),
                    "NonAssociatedColdKey",
                    context.keyring.alice
                );
                await expectFailure(await release(hotkey.address, coldkey, true), "InvalidDisassociationWitness");
                expect((await api.query.subtensorModule.owner(hotkey.address)).toString()).toBe(coldkey.address);
                expect((await submit(await release(hotkey.address))).successful).toBe(true);
            },
        });

        it({
            id: "T03",
            title: "rejects a hotkey registered on root through a real extrinsic",
            test: async () => {
                const hotkey = await associate();
                expect((await submit(api.tx.subtensorModule.rootRegister(hotkey.address))).successful).toBe(true);
                await expectFailure(await release(hotkey.address), "HotkeyIsStillRegistered");
                expect(await ownerExists(hotkey.address)).toBe(true);
            },
        });

        it({
            id: "T04",
            title: "preserves a third party's stake on an unregistered hotkey",
            test: async () => {
                const hotkey = await associate();
                const deposit = await submit(
                    api.tx.subtensorModule.addStake(hotkey.address, 0, 10n * TAO),
                    context.keyring.alice
                );
                expect(deposit.successful, JSON.stringify(deposit.error)).toBe(true);
                const key = api.query.subtensorModule.alphaV2.key(hotkey.address, context.keyring.alice.address, 0);
                const before = (await api.rpc.state.getStorage<Option<Bytes>>(key)).toHex();
                await expectFailure(await release(hotkey.address), "HotkeyHasOutstandingStake");
                expect((await api.rpc.state.getStorage<Option<Bytes>>(key)).toHex()).toBe(before);
            },
        });

        it({
            id: "T05",
            title: "cleans every autostake destination, preserving a retargeted stale index",
            test: async () => {
                const hotkey = await associate();
                const staker = generateKeyringPair();
                const retargeted = generateKeyringPair();
                const other = generateKeyringPair();
                const entries: [string, string][] = [];
                for (const netuid of [1, 65535]) {
                    entries.push(
                        [
                            api.query.subtensorModule.autoStakeDestination.key(coldkey.address, netuid),
                            api.createType("AccountId", hotkey.address).toHex(),
                        ],
                        [
                            api.query.subtensorModule.autoStakeDestination.key(staker.address, netuid),
                            api.createType("AccountId", hotkey.address).toHex(),
                        ],
                        [
                            api.query.subtensorModule.autoStakeDestination.key(retargeted.address, netuid),
                            api.createType("AccountId", other.address).toHex(),
                        ],
                        [
                            api.query.subtensorModule.autoStakeDestinationColdkeys.key(hotkey.address, netuid),
                            api
                                .createType("Vec<AccountId>", [coldkey.address, staker.address, retargeted.address])
                                .toHex(),
                        ]
                    );
                }
                await seedStorage(entries);
                await expectFailure(await release(hotkey.address, coldkey, true), "InvalidDisassociationWitness");
                expect((await api.query.subtensorModule.autoStakeDestination(coldkey.address, 1)).toString()).toBe(
                    hotkey.address
                );
                expect((await submit(await release(hotkey.address))).successful).toBe(true);
                expect(await api.query.subtensorModule.autoStakeDestinationColdkeys.keys(hotkey.address)).toHaveLength(
                    0
                );
                for (const netuid of [1, 65535]) {
                    expect(
                        (await api.query.subtensorModule.autoStakeDestination(coldkey.address, netuid)).toJSON()
                    ).toBeNull();
                    expect(
                        (await api.query.subtensorModule.autoStakeDestination(staker.address, netuid)).toJSON()
                    ).toBeNull();
                    expect(
                        (await api.query.subtensorModule.autoStakeDestination(retargeted.address, netuid)).toString()
                    ).toBe(other.address);
                }
            },
        });

        it({
            id: "T06",
            title: "rejects a basket entitlement even after root stake has gone",
            test: async () => {
                const hotkey = await associate();
                const staker = generateKeyringPair();
                const key = api.query.subtensorModule.basketClaimed.key(hotkey.address, staker.address);
                await seedStorage([[key, api.createType("i128", -1).toHex()]]);
                await expectFailure(await release(hotkey.address), "HotkeyHasOutstandingRewards");
                expect((await api.rpc.state.getStorage<Option<Bytes>>(key)).toHex()).toBe(
                    api.createType("i128", -1).toHex()
                );
            },
        });

        it({
            id: "T07",
            title: "NonTransfer proxy is filtered; Any proxy can release the real owner's hotkey",
            test: async () => {
                const hotkey = await associate();
                const delegate = context.keyring.alice;
                expect((await submit(api.tx.proxy.addProxy(delegate.address, "NonTransfer", 0))).successful).toBe(true);
                const filtered = await submit(
                    api.tx.proxy.proxy(coldkey.address, "NonTransfer", await release(hotkey.address)),
                    delegate
                );
                const failed = filtered.events.find(({ event }) => api.events.proxy.ProxyExecuted.is(event));
                const error = (failed.event.data[0] as any).asErr;
                expect(api.registry.findMetaError(error.asModule).name).toBe("CallFiltered");
                expect(await ownerExists(hotkey.address)).toBe(true);
                expect((await submit(api.tx.proxy.addProxy(delegate.address, "Any", 0))).successful).toBe(true);
                const released = await submit(
                    api.tx.proxy.proxy(coldkey.address, "Any", await release(hotkey.address)),
                    delegate
                );
                const executed = released.events.find(({ event }) => api.events.proxy.ProxyExecuted.is(event));
                expect((executed.event.data[0] as any).isOk).toBe(true);
                expect(await ownerExists(hotkey.address)).toBe(false);
            },
        });
    },
});
