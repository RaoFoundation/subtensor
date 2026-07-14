import * as fs from "node:fs";
import * as path from "node:path";
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import { referendumCount, systemEvents } from "../../../../utils/governance";

const UPGRADED_WASM_PATH = path.resolve(process.cwd(), "tmp/upgraded-runtime.wasm");

describeSuite({
    id: "DEV_SUB_GOV_UPGRADE_01",
    title: "Governance — runtime upgrade via setCode",
    foundationMethods: "dev",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;

        const proposer = generateKeyringPair("sr25519");
        const triumvirate1 = generateKeyringPair("sr25519");
        const triumvirate2 = generateKeyringPair("sr25519");
        const triumvirate3 = generateKeyringPair("sr25519");
        const economic1 = generateKeyringPair("sr25519");
        const economic2 = generateKeyringPair("sr25519");
        const building1 = generateKeyringPair("sr25519");
        const building2 = generateKeyringPair("sr25519");

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;

            if (!fs.existsSync(UPGRADED_WASM_PATH)) {
                throw new Error(
                    `Upgraded runtime WASM not found at ${UPGRADED_WASM_PATH}. Run ts-tests/scripts/build-upgrade-runtime.sh first (moonwall should run it automatically via runScripts).`
                );
            }

            const minimumPeriod = (api.consts.timestamp.minimumPeriod as unknown as { toNumber(): number }).toNumber();
            if (minimumPeriod !== 6000) {
                throw new Error(
                    `node-subtensor binary appears to be built with --features fast-runtime (timestamp.minimumPeriod=${minimumPeriod}, expected 6000). The upgrade WASM is built without fast-runtime; mixing them bricks block production after setCode. Rebuild the node binary without --features fast-runtime: cargo build --release -p node-subtensor`
                );
            }

            const fund = 1_000_000_000_000n;
            for (const inner of [
                api.tx.balances.forceSetBalance(proposer.address, fund),
                api.tx.balances.forceSetBalance(triumvirate1.address, fund),
                api.tx.balances.forceSetBalance(triumvirate2.address, fund),
                api.tx.balances.forceSetBalance(triumvirate3.address, fund),
                api.tx.balances.forceSetBalance(economic1.address, fund),
                api.tx.balances.forceSetBalance(economic2.address, fund),
                api.tx.balances.forceSetBalance(building1.address, fund),
                api.tx.balances.forceSetBalance(building2.address, fund),
                api.tx.multiCollective.addMember("Proposers", proposer.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate1.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate2.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate3.address),
                api.tx.multiCollective.addMember("Economic", economic1.address),
                api.tx.multiCollective.addMember("Economic", economic2.address),
                api.tx.multiCollective.addMember("Building", building1.address),
                api.tx.multiCollective.addMember("Building", building2.address),
            ]) {
                await context.createBlock([await api.tx.sudo.sudo(inner).signAsync(sudoer)]);
            }
        });

        it({
            id: "T01",
            title: "setCode passes governance and bumps specVersion",
            test: async () => {
                const wasmBytes = fs.readFileSync(UPGRADED_WASM_PATH);
                const wasmHex = `0x${wasmBytes.toString("hex")}`;
                log(`upgraded runtime size: ${wasmBytes.length} bytes`);

                const versionBefore = await api.rpc.state.getRuntimeVersion();
                const specBefore = versionBefore.specVersion.toNumber();
                log(`specVersion before: ${specBefore}`);

                const setCodePayload = api.tx.system.setCode(wasmHex);

                const countBefore = await referendumCount(api);

                await context.createBlock([await api.tx.referenda.submit(0, setCodePayload).signAsync(proposer)]);
                const outerPoll = countBefore;

                await context.createBlock([await api.tx.signedVoting.vote(outerPoll, true).signAsync(triumvirate1)]);
                await context.createBlock([await api.tx.signedVoting.vote(outerPoll, true).signAsync(triumvirate2)]);

                await context.createBlock([]);

                const delegatedEvent = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                expect(delegatedEvent, "outer Delegated").to.exist;
                const innerPoll = outerPoll + 1;

                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(economic1)]);
                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(economic2)]);
                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(building1)]);

                await context.createBlock([]);

                const fastTracked = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "FastTracked"
                );
                expect(fastTracked, "inner FastTracked").to.exist;

                await context.createBlock([]);

                const enactmentEvents = await systemEvents(api);
                const codeUpdated = enactmentEvents.find(
                    (e) => e.event.section === "system" && e.event.method === "CodeUpdated"
                );
                expect(codeUpdated, "system.CodeUpdated").to.exist;

                await context.createBlock([]);

                const versionAfter = await api.rpc.state.getRuntimeVersion();
                const specAfter = versionAfter.specVersion.toNumber();
                log(`specVersion after: ${specAfter}`);
                expect(specAfter).to.equal(specBefore + 1);
            },
        });
    },
});
