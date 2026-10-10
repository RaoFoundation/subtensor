import type { Vec } from "@polkadot/types-codec";
import type { EventRecord } from "@polkadot/types/interfaces";
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { BN } from "@polkadot/util";

describeSuite({
    id: "DEV_SUB_STAKING_ADD_STAKING_01",
    title: "Add staking test suite",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let netuid1: number;

        let alice: KeyringPair;
        let bob: KeyringPair;

        beforeAll(() => {
            polkadotJs = context.polkadotJs();

            alice = context.keyring.alice;
            bob = context.keyring.bob;
        });

        it({
            id: "T01",
            title: "Add stake payable",
            test: async () => {
                const alice = context.keyring.alice;
                const bob = context.keyring.bob;
                const appFees = new BN(100_000);

                // Register network
                let tx = polkadotJs.tx.subtensorModule.registerNetwork(bob.address);
                await context.createBlock([await tx.signAsync(alice)]);

                let events = await polkadotJs.query.system.events<Vec<EventRecord>>();
                const event = events.filter((a) => {
                    return a.event.method === "NetworkAdded";
                });
                expect(event.length).to.be.equal(1);
                netuid1 = Number(event[0].event.data[0].toString());

                // Enabling subtokens
                const tx1 = polkadotJs.tx.adminUtils.sudoSetSubtokenEnabled(netuid1, true);
                await context.createBlock([await polkadotJs.tx.sudo.sudo(tx1).signAsync(alice)]);

                // Adding stake
                tx = polkadotJs.tx.subtensorModule.addStake(bob.address, netuid1, 1000_000_000);
                await context.createBlock([await tx.signAsync(alice)]);

                events = await polkadotJs.query.system.events<Vec<EventRecord>>();
                const stakeAddedEvent = events.filter((a) => {
                    return a.event.method === "StakeAdded";
                });

                expect(stakeAddedEvent.length).to.be.equal(1);
            },
        });

        it({
            id: "T02",
            title: "Remove stake payable",
            test: async () => {
                // Removing stake
                const tx = polkadotJs.tx.subtensorModule.removeStake(bob.address, netuid1, 500_000_000);
                await context.createBlock([await tx.signAsync(alice)]);

                const events = await polkadotJs.query.system.events<Vec<EventRecord>>();
                const removeAddedEvent = events.filter((a) => {
                    return a.event.method === "StakeRemoved";
                });

                expect(removeAddedEvent.length).to.be.equal(1);
            },
        });
    },
});
