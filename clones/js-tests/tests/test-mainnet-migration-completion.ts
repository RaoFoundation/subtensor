import assert from "node:assert/strict";
import fs from "node:fs";
import { ApiPromise, WsProvider } from "@polkadot/api";
import { xxhashAsHex } from "@polkadot/util-crypto";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const SNAPSHOT_URL = new URL("../temp/mainnet-migration-snapshot.json", import.meta.url);
const MODE = process.argv[2];
const EXPECTED_SPEC_VERSION = 441;
const ROOT_NETUID = 0;
const EXPECTED_MIN_ROOT_WEIGHTS = 8;
const WAIT_TIMEOUT_MS = 2 * 60 * 60 * 1000;
const POLL_INTERVAL_MS = 2_000;

const MIGRATIONS = [
  "migrate_seed_beta_basket_v2",
  "clear_root_basket_weights_v2",
  "set_root_min_allowed_weights_8",
  "reset_emission_gate_bar_rank_32",
] as const;

interface MigrationSnapshot {
  preUpgradeBlock: number;
  preUpgradeSpecVersion: number;
  preUpgradeObservedAtMs: number;
  rootClaimablePresent: boolean;
  rootClaimedPresent: boolean;
  upgradeBlock?: number;
  upgradedObservedAtMs?: number;
}

const logger = createTempLogger("test-mainnet-migration-completion.log");

async function main() {
  await logger.start();
  logger.captureConsole();
  assert.ok(["before", "upgraded", "after"].includes(MODE), `unknown mode: ${MODE}`);

  console.log(`Connecting to ${WS_ENDPOINT} for migration probe mode=${MODE} ...`);
  const provider = new WsProvider(WS_ENDPOINT);
  const api = await ApiPromise.create({ provider });
  await api.isReady;

  try {
    if (MODE === "before") {
      await captureBefore(api);
    } else if (MODE === "upgraded") {
      await captureUpgrade(api);
    } else {
      await waitForCompletionAndAssert(api);
    }
  } finally {
    await api.disconnect();
    await logger.flush();
  }
}

async function captureBefore(api: ApiPromise) {
  const header = await api.rpc.chain.getHeader();
  const runtime = await api.rpc.state.getRuntimeVersion(header.hash);
  const snapshot: MigrationSnapshot = {
    preUpgradeBlock: header.number.toNumber(),
    preUpgradeSpecVersion: runtime.specVersion.toNumber(),
    preUpgradeObservedAtMs: Date.now(),
    rootClaimablePresent: await storagePrefixHasAny(api, "RootClaimable"),
    rootClaimedPresent: await storagePrefixHasAny(api, "RootClaimed"),
  };

  assert.equal(
    snapshot.preUpgradeSpecVersion < EXPECTED_SPEC_VERSION,
    true,
    `expected pre-upgrade spec below ${EXPECTED_SPEC_VERSION}, got ${snapshot.preUpgradeSpecVersion}`
  );
  assert.equal(snapshot.rootClaimablePresent, true, "mainnet snapshot has no RootClaimable state to migrate");
  assert.equal(snapshot.rootClaimedPresent, true, "mainnet snapshot has no RootClaimed state to migrate");

  fs.writeFileSync(SNAPSHOT_URL, `${JSON.stringify(snapshot, null, 2)}\n`);
  console.log(
    "migration pre-state captured:",
    `block=${snapshot.preUpgradeBlock}`,
    `spec=${snapshot.preUpgradeSpecVersion}`,
    "RootClaimable=present",
    "RootClaimed=present"
  );
}

async function captureUpgrade(api: ApiPromise) {
  const snapshot = readSnapshot();
  const current = await api.rpc.chain.getHeader();
  const currentBlock = current.number.toNumber();
  const currentRuntime = await api.rpc.state.getRuntimeVersion(current.hash);
  assert.equal(
    currentRuntime.specVersion.toNumber(),
    EXPECTED_SPEC_VERSION,
    `runtime upgrade did not reach spec ${EXPECTED_SPEC_VERSION}`
  );

  let upgradeBlock: number | undefined;
  for (let block = snapshot.preUpgradeBlock + 1; block <= currentBlock; block++) {
    const hash = await api.rpc.chain.getBlockHash(block);
    const runtime = await api.rpc.state.getRuntimeVersion(hash);
    if (runtime.specVersion.toNumber() === EXPECTED_SPEC_VERSION) {
      upgradeBlock = block;
      break;
    }
  }
  assert.notEqual(upgradeBlock, undefined, "could not locate the first upgraded block");

  snapshot.upgradeBlock = upgradeBlock;
  snapshot.upgradedObservedAtMs = Date.now();
  fs.writeFileSync(SNAPSHOT_URL, `${JSON.stringify(snapshot, null, 2)}\n`);
  console.log(
    "runtime upgrade located:",
    `upgrade_block=${upgradeBlock}`,
    `observed_block=${currentBlock}`,
    `spec=${EXPECTED_SPEC_VERSION}`
  );
}

async function waitForCompletionAndAssert(api: ApiPromise) {
  const snapshot = readSnapshot();
  assert.notEqual(snapshot.upgradeBlock, undefined, "missing upgrade block from upgraded probe");
  assert.notEqual(snapshot.upgradedObservedAtMs, undefined, "missing upgraded observation time");

  const cursorKey = storagePrefix("SeedBetaBasketV2Migration");
  const deadline = Date.now() + WAIT_TIMEOUT_MS;
  let lastReportedBlock = -1;
  let completionBlock = -1;

  while (Date.now() < deadline) {
    const header = await api.rpc.chain.getHeader();
    const block = header.number.toNumber();
    const cursor = (await api.rpc.state.getStorage(cursorKey, header.hash)) as unknown as {
      isNone: boolean;
    };
    if (cursor.isNone) {
      completionBlock = block;
      break;
    }
    if (block >= lastReportedBlock + 50) {
      console.log("migration still running:", `block=${block}`);
      lastReportedBlock = block;
    }
    await delay(POLL_INTERVAL_MS);
  }
  assert.notEqual(completionBlock, -1, `migration cursor remained present for ${WAIT_TIMEOUT_MS}ms`);

  for (const migration of MIGRATIONS) {
    const ran = await api.query.subtensorModule.hasMigrationRun([...Buffer.from(migration)]);
    assert.equal(ran.toString(), "true", `HasMigrationRun is false for ${migration}`);
  }

  assert.equal(
    await storagePrefixHasAny(api, "RootClaimable"),
    false,
    "RootClaimable still contains legacy entries"
  );
  assert.equal(await storagePrefixHasAny(api, "RootClaimed"), false, "RootClaimed still contains legacy entries");
  assert.equal(
    await storagePrefixHasAny(api, "BasketPrincipal"),
    false,
    "deprecated BasketPrincipal still contains entries"
  );
  assert.equal(await storagePrefixHasAny(api, "BasketRate"), true, "BasketRate was not populated");
  assert.equal(await storagePrefixHasAny(api, "BasketShares"), true, "BasketShares was not populated");
  assert.equal(await storagePrefixHasAny(api, "BasketClaimed"), true, "BasketClaimed was not populated");

  const rootWeightKeys = await api.query.subtensorModule.weights.keys(ROOT_NETUID);
  assert.equal(rootWeightKeys.length, 0, "legacy root weight vectors were not fully cleared");

  const minAllowedWeights = await api.query.subtensorModule.minAllowedWeights(ROOT_NETUID);
  assert.equal(
    Number(minAllowedWeights.toString()),
    EXPECTED_MIN_ROOT_WEIGHTS,
    "root MinAllowedWeights was not migrated to the basket minimum"
  );

  const migrationBlocks = completionBlock - snapshot.upgradeBlock!;
  const normalCadenceSeconds = migrationBlocks * 12;
  const observedWallMs = Date.now() - snapshot.upgradedObservedAtMs!;
  console.log(
    "migration completion:",
    `upgrade_block=${snapshot.upgradeBlock}`,
    `completion_block=${completionBlock}`,
    `migration_blocks=${migrationBlocks}`,
    `normal_12s_estimate_seconds=${normalCadenceSeconds}`,
    `validator_accelerated_wall_ms=${observedWallMs}`
  );
  console.log(
    "migration invariants: ok",
    "legacy RootClaimable/RootClaimed/BasketPrincipal empty;",
    "BasketRate/BasketShares/BasketClaimed populated;",
    "root weights empty;",
    `MinAllowedWeights[root]=${EXPECTED_MIN_ROOT_WEIGHTS};`,
    "all HasMigrationRun flags set"
  );
}

function readSnapshot(): MigrationSnapshot {
  return JSON.parse(fs.readFileSync(SNAPSHOT_URL, "utf8")) as MigrationSnapshot;
}

function storagePrefix(item: string): string {
  return `${xxhashAsHex("SubtensorModule", 128)}${xxhashAsHex(item, 128).slice(2)}`;
}

async function storagePrefixHasAny(api: ApiPromise, item: string): Promise<boolean> {
  const keys = await api.rpc.state.getKeysPaged(storagePrefix(item), 1);
  return keys.length > 0;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
