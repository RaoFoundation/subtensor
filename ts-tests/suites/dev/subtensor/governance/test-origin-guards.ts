import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    bootstrapMembership,
    DEV_TRACK,
    fundAccounts,
    type GovernanceMembership,
    inBlock,
    lastModuleError,
    submitOnTrack,
} from "../../../../utils/governance";

/**
 * Comprehensive proof that every privileged extrinsic in the governance
 * surface rejects non-Root callers with `BadOrigin`. Each test exercises a
 * single extrinsic so a regression localizes immediately. This is the most
 * security-critical file in the suite: governance is the only path to Root
 * dispatch, and a leaky origin check would erase that guarantee.
 */
describeSuite({
    id: "DEV_SUB_GOV_ORIGIN_GUARDS_01",
    title: "Governance — origin guards on privileged extrinsics",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const attacker = generateKeyringPair("sr25519");
        const victim = generateKeyringPair("sr25519");
        const accomplice = generateKeyringPair("sr25519");

        const expectBadOrigin = async () => {
            const err = await lastModuleError(api);
            expect(err, "ExtrinsicFailed").to.exist;
            expect((err as { kind: string }).kind).to.equal("BadOrigin");
        };

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            // Bootstrap a referendum so `kill`, `advance_referendum`, and
            // `enact` have a real index to target. Seating Triumvirate also
            // means `attacker` is a strict outsider.
            gov = await bootstrapMembership(api, context, sudoer, {
                triumvirate: 3,
                economic: 1,
                building: 1,
            });
            await fundAccounts(api, context, sudoer, [attacker.address, victim.address, accomplice.address]);
        });

        it({
            id: "T01",
            title: "multiCollective.add_member from a signed non-Root caller → BadOrigin",
            test: async () => {
                await inBlock(context, attacker, api.tx.multiCollective.addMember("Triumvirate", attacker.address));
                await expectBadOrigin();
            },
        });

        it({
            id: "T02",
            title: "multiCollective.remove_member from non-Root → BadOrigin",
            test: async () => {
                await inBlock(
                    context,
                    attacker,
                    api.tx.multiCollective.removeMember("Triumvirate", gov.triumvirate[0].address)
                );
                await expectBadOrigin();
            },
        });

        it({
            id: "T03",
            title: "multiCollective.swap_member from non-Root → BadOrigin",
            test: async () => {
                await inBlock(
                    context,
                    attacker,
                    api.tx.multiCollective.swapMember("Triumvirate", gov.triumvirate[0].address, accomplice.address)
                );
                await expectBadOrigin();
            },
        });

        it({
            id: "T04",
            title: "multiCollective.set_members from non-Root → BadOrigin",
            test: async () => {
                await inBlock(
                    context,
                    attacker,
                    api.tx.multiCollective.setMembers("Triumvirate", [
                        attacker.address,
                        accomplice.address,
                        victim.address,
                    ])
                );
                await expectBadOrigin();
            },
        });

        it({
            id: "T05",
            title: "multiCollective.force_rotate from non-Root → BadOrigin",
            test: async () => {
                await inBlock(context, attacker, api.tx.multiCollective.forceRotate("Economic"));
                await expectBadOrigin();
            },
        });

        it({
            id: "T06",
            title: "referenda.kill from non-Root → BadOrigin",
            test: async () => {
                const index = await submitOnTrack(
                    api,
                    context,
                    gov.proposer,
                    DEV_TRACK.TRIUMVIRATE,
                    api.tx.system.remark("victim-call")
                );
                await inBlock(context, attacker, api.tx.referenda.kill(index));
                await expectBadOrigin();
            },
        });

        it({
            id: "T07",
            title: "referenda.advance_referendum from non-Root → BadOrigin",
            test: async () => {
                const index = await submitOnTrack(
                    api,
                    context,
                    gov.proposer,
                    DEV_TRACK.TRIUMVIRATE,
                    api.tx.system.remark("advance-target")
                );
                await inBlock(context, attacker, api.tx.referenda.advanceReferendum(index));
                await expectBadOrigin();
            },
        });

        it({
            id: "T08",
            title: "referenda.enact from non-Root → BadOrigin",
            test: async () => {
                const phantomCall = api.tx.system.remark("hijack-attempt");
                await inBlock(context, attacker, api.tx.referenda.enact(0, phantomCall));
                await expectBadOrigin();
            },
        });

        it({
            id: "T09",
            title: "sudo.sudo from a non-sudo caller is rejected before runtime (pool-level)",
            test: async () => {
                // Defense in depth: the sudo pallet pre-validates the caller
                // via a signed extension, so a non-sudo signer never even
                // reaches runtime dispatch. Any other behavior would let an
                // attacker probe sudo'd calls cheaply.
                let rejected = false;
                try {
                    await context.createBlock([
                        await api.tx.sudo
                            .sudo(api.tx.multiCollective.addMember("Triumvirate", attacker.address))
                            .signAsync(attacker, { era: 0 }),
                    ]);
                } catch (e) {
                    rejected = true;
                    expect(String(e)).to.match(/Invalid signing address|RequireSudo|BadOrigin/i);
                }
                expect(rejected, "transaction must be rejected").to.be.true;

                // The Triumvirate membership remains untouched.
                const members = (await api.query.multiCollective.members("Triumvirate")).toJSON() as string[];
                expect(members).to.not.include(attacker.address);
            },
        });
    },
});
