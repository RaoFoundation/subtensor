import assert from "node:assert/strict";
import fs from "node:fs";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = "ws://127.0.0.1:9944";
const PHASE = process.env.UPGRADE_PHASE;
const SNAPSHOT_URL = new URL("../temp/stable2606-mainnet-before.json", import.meta.url);
const logger = createTempLogger(`stable2606-mainnet-upgrade-${PHASE}.log`);
logger.captureConsole();

interface Snapshot {
  chain: string;
  genesisHash: string;
  specVersion: number;
  block: number;
  sudoKey: string;
  totalNetworks: number;
  lastStoredRound: bigint;
  oldestStoredRound: bigint;
}

async function main() {
  assert.ok(PHASE === "before" || PHASE === "after", "set UPGRADE_PHASE to before or after");
  await logger.start();
  const api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    assert.ok(api.query.drand?.lastStoredRound, "Drand.LastStoredRound is unavailable");
    assert.ok(api.query.drand?.pulses, "Drand.Pulses is unavailable");

    const snapshot = await readSnapshot(api);
    console.log("phase:", PHASE);
    console.log("chain:", snapshot.chain);
    console.log("runtime:", snapshot.specVersion);
    console.log("block:", snapshot.block);
    console.log("last stored drand round:", snapshot.lastStoredRound);
    console.log("oldest stored drand round:", snapshot.oldestStoredRound);

    await assertBlockProduction(api, snapshot.block);
    await assertPulseExists(api, snapshot.lastStoredRound);

    if (PHASE === "before") {
      fs.writeFileSync(SNAPSHOT_URL, JSON.stringify(snapshot, bigintReplacer, 2));
      console.log("before-upgrade snapshot saved");
      return;
    }

    const before = JSON.parse(fs.readFileSync(SNAPSHOT_URL, "utf8"), bigintReviver) as Snapshot;
    assert.equal(snapshot.chain, before.chain, "chain changed during upgrade");
    assert.equal(snapshot.genesisHash, before.genesisHash, "genesis hash changed during upgrade");
    assert.equal(snapshot.sudoKey, before.sudoKey, "sudo key changed during upgrade");
    assert.equal(snapshot.totalNetworks, before.totalNetworks, "network count changed during upgrade");
    assert.equal(snapshot.specVersion, 433, "stable2606 runtime was not installed");
    assert.ok(snapshot.specVersion > before.specVersion, "runtime version did not increase");
    assert.ok(snapshot.block > before.block, "chain did not advance across the upgrade");

    await assertPulseExists(api, before.lastStoredRound);
    const newRound = await waitForNewDrandRound(api, snapshot.lastStoredRound);
    await assertPulseExists(api, newRound);
    console.log("new drand round inserted after upgrade:", newRound);
    console.log("stable2606 mainnet upgrade assertions: ok");
  } finally {
    await api.disconnect();
    await logger.flush();
  }
}

async function readSnapshot(api): Promise<Snapshot> {
  const [chain, runtime, header, sudoKey, totalNetworks, lastStoredRound, oldestStoredRound] =
    await Promise.all([
      api.rpc.system.chain(),
      api.rpc.state.getRuntimeVersion(),
      api.rpc.chain.getHeader(),
      api.query.sudo.key(),
      api.query.subtensorModule.totalNetworks(),
      api.query.drand.lastStoredRound(),
      api.query.drand.oldestStoredRound(),
    ]);

  return {
    chain: chain.toString(),
    genesisHash: api.genesisHash.toHex(),
    specVersion: runtime.specVersion.toNumber(),
    block: header.number.toNumber(),
    sudoKey: sudoKey.toString(),
    totalNetworks: totalNetworks.toNumber(),
    lastStoredRound: lastStoredRound.toBigInt(),
    oldestStoredRound: oldestStoredRound.toBigInt(),
  };
}

async function assertBlockProduction(api, startBlock: number) {
  let previous = startBlock;
  let advances = 0;

  for (let attempt = 0; attempt < 30 && advances < 2; attempt += 1) {
    await delay(6_000);
    const current = (await api.rpc.chain.getHeader()).number.toNumber();
    if (current > previous) {
      advances += 1;
      previous = current;
      console.log("block advanced:", current);
    }
  }

  assert.equal(advances, 2, "block height did not increase twice");
}

async function waitForNewDrandRound(api, initialRound: bigint): Promise<bigint> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const current = (await api.query.drand.lastStoredRound()).toBigInt();
    if (current > initialRound) {
      return current;
    }
    await delay(6_000);
  }

  assert.fail(`drand round did not advance beyond ${initialRound}`);
}

async function assertPulseExists(api, round: bigint) {
  assert.ok(round > 0n, "LastStoredRound must be initialized");
  const pulse = await api.query.drand.pulses(round);
  assert.ok(pulse.isSome, `pulse ${round} is missing`);
}

function bigintReplacer(_key: string, value: unknown) {
  return typeof value === "bigint" ? `${value}n` : value;
}

function bigintReviver(_key: string, value: unknown) {
  return typeof value === "string" && /^\d+n$/.test(value) ? BigInt(value.slice(0, -1)) : value;
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
