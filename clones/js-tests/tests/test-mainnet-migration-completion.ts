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
const EXPECTED_BLOCK_TIME_MS = 12_000;
const WAIT_TIMEOUT_MS = 3 * 60 * 60 * 1000;
const POLL_INTERVAL_MS = 2_000;
const PAGE_SIZE = 10_000;
const QUERY_MULTI_PAGE_SIZE = 1_000;
const FIXED_SCALE = 1n << 32n;

const MIGRATIONS = [
  "migrate_seed_beta_basket_v2",
  "clear_root_basket_weights_v2",
  "set_root_min_allowed_weights_8",
  "reset_emission_gate_bar_rank_32",
] as const;

interface MigrationSnapshot {
  preUpgradeBlock: number;
  preUpgradeHash: string;
  preUpgradeSpecVersion: number;
  preUpgradeObservedAtMs: number;
  source: SerializedSourceAudit;
  upgradeBlock?: number;
  upgradedObservedAtMs?: number;
}

interface SerializedSourceAudit {
  hotkeys: string[];
  nonzeroRateHotkeys: string[];
  claimedPairs: string[];
  slotCounts: Array<[string, string]>;
  claimedRowCounts: Array<[string, string]>;
  claimableEntries: string;
  claimableSlots: string;
  claimedEntries: string;
  zeroClaimedEntries: string;
}

interface SourceAudit {
  hotkeys: Set<string>;
  nonzeroRateHotkeys: Set<string>;
  claimedPairs: Set<string>;
  slotCounts: Map<string, bigint>;
  claimedRowCounts: Map<string, bigint>;
  claimableEntries: bigint;
  claimableSlots: bigint;
  claimedEntries: bigint;
  zeroClaimedEntries: bigint;
}

interface DestinationAudit {
  rateByHotkey: Map<string, bigint>;
  sharesByHotkey: Map<string, bigint>;
  claimedSumByHotkey: Map<string, bigint>;
  claimedPairs: Set<string>;
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
  const at = await api.at(header.hash);
  const preUpgradeSpecVersion = runtime.specVersion.toNumber();
  const preUpgradeObservedAtMs = Date.now();

  assert.equal(
    preUpgradeSpecVersion < EXPECTED_SPEC_VERSION,
    true,
    `expected pre-upgrade spec below ${EXPECTED_SPEC_VERSION}, got ${preUpgradeSpecVersion}`
  );
  assert.equal(
    await storageQueryHasAny(at.query.subtensorModule.rootClaimable),
    true,
    "mainnet snapshot has no RootClaimable state to migrate"
  );
  assert.equal(
    await storageQueryHasAny(at.query.subtensorModule.rootClaimed),
    true,
    "mainnet snapshot has no RootClaimed state to migrate"
  );
  assert.equal(
    await storageQueryHasAny(at.query.subtensorModule.basketRate),
    false,
    "pre-upgrade mainnet already contains BasketRate rows"
  );
  assert.equal(
    await storageQueryHasAny(at.query.subtensorModule.basketShares),
    false,
    "pre-upgrade mainnet already contains BasketShares rows"
  );
  assert.equal(
    await storageQueryHasAny(at.query.subtensorModule.basketClaimed),
    false,
    "pre-upgrade mainnet already contains BasketClaimed rows"
  );

  const source = await auditSourceState(at);
  const snapshot: MigrationSnapshot = {
    preUpgradeBlock: header.number.toNumber(),
    preUpgradeHash: header.hash.toHex(),
    preUpgradeSpecVersion,
    preUpgradeObservedAtMs,
    source: serializeSourceAudit(source),
  };
  fs.writeFileSync(SNAPSHOT_URL, `${JSON.stringify(snapshot, null, 2)}\n`);
  console.log(
    "migration pre-state captured:",
    `block=${snapshot.preUpgradeBlock}`,
    `hash=${snapshot.preUpgradeHash}`,
    `spec=${snapshot.preUpgradeSpecVersion}`,
    "legacy_sources=present",
    "basket_destinations=empty",
    `RootClaimable_entries=${source.claimableEntries}`,
    `RootClaimable_slots=${source.claimableSlots}`,
    `RootClaimed_entries=${source.claimedEntries}`,
    `RootClaimed_unique_pairs=${source.claimedPairs.size}`
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
  let observedCompletionBlock = -1;

  while (Date.now() < deadline) {
    const header = await api.rpc.chain.getHeader();
    const block = header.number.toNumber();
    if (!(await storageExistsAt(api, cursorKey, header.hash))) {
      observedCompletionBlock = block;
      break;
    }
    if (block >= lastReportedBlock + 50) {
      console.log("migration still running:", `block=${block}`);
      lastReportedBlock = block;
    }
    await delay(POLL_INTERVAL_MS);
  }
  assert.notEqual(observedCompletionBlock, -1, `migration cursor remained present for ${WAIT_TIMEOUT_MS}ms`);

  const completionBlock = await findExactCompletionBlock(
    api,
    cursorKey,
    snapshot.upgradeBlock!,
    observedCompletionBlock
  );
  const completionHash = await api.rpc.chain.getBlockHash(completionBlock);
  const upgradeHash = await api.rpc.chain.getBlockHash(snapshot.upgradeBlock!);
  const upgraded = await api.at(upgradeHash);
  const completed = await api.at(completionHash);

  for (const migration of MIGRATIONS) {
    const ran = await completed.query.subtensorModule.hasMigrationRun([...Buffer.from(migration)]);
    assert.equal(ran.toString(), "true", `HasMigrationRun is false for ${migration}`);
  }

  assert.equal(
    await storageQueryHasAny(completed.query.subtensorModule.rootClaimable),
    false,
    "RootClaimable still contains legacy entries"
  );
  assert.equal(
    await storageQueryHasAny(completed.query.subtensorModule.rootClaimed),
    false,
    "RootClaimed still contains legacy entries"
  );
  assert.equal(
    await storagePrefixHasAny(api, "BasketPrincipal", completionHash),
    false,
    "deprecated BasketPrincipal still contains entries"
  );

  const source = deserializeSourceAudit(snapshot.source);
  const destination = await auditDestinationState(completed);

  assertSetEqual(
    destination.rateByHotkey.keys(),
    source.nonzeroRateHotkeys,
    "BasketRate hotkeys do not exactly match nonzero RootClaimable hotkeys"
  );
  assertSetSubset(
    destination.sharesByHotkey.keys(),
    source.hotkeys,
    "BasketShares contains a hotkey that was absent from RootClaimable"
  );
  assertSetSubset(
    destination.claimedPairs,
    source.claimedPairs,
    "BasketClaimed contains a (hotkey, coldkey) pair absent from RootClaimed"
  );
  assert.equal(destination.rateByHotkey.size > 0, true, "BasketRate was not populated");
  assert.equal(destination.sharesByHotkey.size > 0, true, "BasketShares was not populated");
  assert.equal(destination.claimedPairs.size > 0, true, "BasketClaimed was not populated");

  const totalRootByHotkey = await queryTotalRootStake(completed, destination.rateByHotkey.keys());
  let conservedFunds = 0;
  let worstConservationPpm = 0n;
  let worstConservationHotkey = "";

  for (const [hotkey, rateRaw] of destination.rateByHotkey) {
    assert.equal(rateRaw >= 0n, true, `BasketRate is negative for ${hotkey}`);
    const rootStake = totalRootByHotkey.get(hotkey) ?? 0n;
    const claimed = destination.claimedSumByHotkey.get(hotkey) ?? 0n;
    const shares = destination.sharesByHotkey.get(hotkey) ?? 0n;
    const aggregateOwed = (rateRaw * rootStake) / FIXED_SCALE - claimed;
    const difference = absolute(aggregateOwed - shares);
    const sourceRows = source.claimedRowCounts.get(hotkey) ?? 0n;
    const sourceSlots = source.slotCounts.get(hotkey) ?? 0n;

    // The migration floors fixed-point values once per source slot/claim row. Permit that
    // unavoidable dust plus one basis point, while still rejecting any material loss or mint.
    const roundingAllowance = sourceRows + sourceSlots + 100n;
    const proportionalAllowance = absolute(aggregateOwed) / 10_000n;
    const allowedDifference =
      roundingAllowance > proportionalAllowance ? roundingAllowance : proportionalAllowance;
    assert.equal(
      difference <= allowedDifference,
      true,
      `basket conservation failed for ${hotkey}: aggregate_owed=${aggregateOwed} shares=${shares} ` +
        `difference=${difference} allowed=${allowedDifference}`
    );

    const denominator = absolute(aggregateOwed) || 1n;
    const differencePpm = (difference * 1_000_000n) / denominator;
    if (differencePpm > worstConservationPpm) {
      worstConservationPpm = differencePpm;
      worstConservationHotkey = hotkey;
    }
    conservedFunds += 1;
  }

  const rootWeightKeys = await completed.query.subtensorModule.weights.keys(ROOT_NETUID);
  assert.equal(rootWeightKeys.length, 0, "legacy root weight vectors were not fully cleared");

  const minAllowedWeights = await completed.query.subtensorModule.minAllowedWeights(ROOT_NETUID);
  assert.equal(
    Number(minAllowedWeights.toString()),
    EXPECTED_MIN_ROOT_WEIGHTS,
    "root MinAllowedWeights was not migrated to the basket minimum"
  );

  const migrationBlocks = completionBlock - snapshot.upgradeBlock!;
  const upgradeTimestamp = codecToBigInt(await upgraded.query.timestamp.now());
  const completionTimestamp = codecToBigInt(await completed.query.timestamp.now());
  const chainElapsedMs = completionTimestamp - upgradeTimestamp;
  assert.equal(
    chainElapsedMs,
    BigInt(migrationBlocks * EXPECTED_BLOCK_TIME_MS),
    `clone did not preserve 12-second mainnet cadence: elapsed_ms=${chainElapsedMs} blocks=${migrationBlocks}`
  );
  const normalCadenceSeconds = migrationBlocks * 12;
  const observedWallMs = Date.now() - snapshot.upgradedObservedAtMs!;
  console.log(
    "migration completion:",
    `upgrade_block=${snapshot.upgradeBlock}`,
    `completion_block=${completionBlock}`,
    `completion_hash=${completionHash.toHex()}`,
    `migration_blocks=${migrationBlocks}`,
    `chain_elapsed_ms=${chainElapsedMs}`,
    `block_time_ms=${EXPECTED_BLOCK_TIME_MS}`,
    `normal_12s_estimate_seconds=${normalCadenceSeconds}`,
    `observed_wall_ms=${observedWallMs}`
  );
  console.log(
    "migration source audit:",
    `RootClaimable_entries=${source.claimableEntries}`,
    `RootClaimable_slots=${source.claimableSlots}`,
    `RootClaimed_entries=${source.claimedEntries}`,
    `RootClaimed_zero_entries=${source.zeroClaimedEntries}`,
    `RootClaimed_unique_pairs=${source.claimedPairs.size}`
  );
  console.log(
    "migration destination audit:",
    `BasketRate_entries=${destination.rateByHotkey.size}`,
    `BasketShares_entries=${destination.sharesByHotkey.size}`,
    `BasketClaimed_entries=${destination.claimedPairs.size}`,
    `conserved_funds=${conservedFunds}`,
    `worst_conservation_ppm=${worstConservationPpm}`,
    `worst_conservation_hotkey=${worstConservationHotkey || "none"}`
  );
  console.log(
    "migration invariants: ok",
    "legacy RootClaimable/RootClaimed/BasketPrincipal empty;",
    "all nonzero source-rate hotkeys transferred to BasketRate;",
    "all destination shares/claims trace to legacy source keys;",
    "every basket conserved within fixed-point rounding tolerance;",
    "root weights empty;",
    `MinAllowedWeights[root]=${EXPECTED_MIN_ROOT_WEIGHTS};`,
    "all HasMigrationRun flags set"
  );
}

async function findExactCompletionBlock(
  api: ApiPromise,
  cursorKey: string,
  upgradeBlock: number,
  observedCompletionBlock: number
): Promise<number> {
  let sawCursor = false;
  for (let block = upgradeBlock; block <= observedCompletionBlock; block++) {
    const hash = await api.rpc.chain.getBlockHash(block);
    const exists = await storageExistsAt(api, cursorKey, hash);
    if (exists) {
      sawCursor = true;
    } else if (sawCursor) {
      return block;
    }
  }
  assert.fail(
    `could not find cursor transition between upgrade block ${upgradeBlock} and ${observedCompletionBlock}`
  );
}

async function auditSourceState(before): Promise<SourceAudit> {
  const hotkeys = new Set<string>();
  const nonzeroRateHotkeys = new Set<string>();
  const claimedPairs = new Set<string>();
  const slotCounts = new Map<string, bigint>();
  const claimedRowCounts = new Map<string, bigint>();
  let claimableEntries = 0n;
  let claimableSlots = 0n;
  let claimedEntries = 0n;
  let zeroClaimedEntries = 0n;

  let startKey;
  let page = 0;
  for (;;) {
    const entries = await before.query.subtensorModule.rootClaimable.entriesPaged({
      args: [],
      pageSize: PAGE_SIZE,
      startKey,
    });
    if (entries.length === 0) {
      break;
    }
    page += 1;
    for (const [storageKey, claimable] of entries) {
      const hotkey = storageKey.args[0].toString();
      hotkeys.add(hotkey);
      claimableEntries += 1n;
      let slots = 0n;
      let hasNonzeroRate = false;
      for (const [, rate] of claimable.entries()) {
        const raw = codecToBigInt(rate);
        assert.equal(raw >= 0n, true, `legacy RootClaimable rate is negative for ${hotkey}`);
        hasNonzeroRate ||= raw !== 0n;
        slots += 1n;
      }
      if (hasNonzeroRate) {
        nonzeroRateHotkeys.add(hotkey);
      }
      slotCounts.set(hotkey, slots);
      claimableSlots += slots;
    }
    startKey = entries.at(-1)[0];
    console.log(
      "source RootClaimable scan:",
      `page=${page}`,
      `entries=${claimableEntries}`,
      `slots=${claimableSlots}`
    );
  }

  startKey = undefined;
  page = 0;
  for (;;) {
    const entries = await before.query.subtensorModule.rootClaimed.entriesPaged({
      args: [],
      pageSize: PAGE_SIZE,
      startKey,
    });
    if (entries.length === 0) {
      break;
    }
    page += 1;
    for (const [storageKey, claimed] of entries) {
      const [, hotkeyArg, coldkeyArg] = storageKey.args;
      const hotkey = hotkeyArg.toString();
      const coldkey = coldkeyArg.toString();
      const claimedRaw = codecToBigInt(claimed);
      assert.equal(claimedRaw >= 0n, true, `legacy RootClaimed value is negative for ${hotkey}`);
      if (claimedRaw === 0n) {
        zeroClaimedEntries += 1n;
      } else {
        claimedPairs.add(pairKey(hotkey, coldkey));
      }
      incrementBigIntMap(claimedRowCounts, hotkey);
      claimedEntries += 1n;
    }
    startKey = entries.at(-1)[0];
    if (page === 1 || page % 25 === 0) {
      console.log(
        "source RootClaimed scan:",
        `page=${page}`,
        `entries=${claimedEntries}`,
        `unique_positive_pairs=${claimedPairs.size}`
      );
    }
  }

  return {
    hotkeys,
    nonzeroRateHotkeys,
    claimedPairs,
    slotCounts,
    claimedRowCounts,
    claimableEntries,
    claimableSlots,
    claimedEntries,
    zeroClaimedEntries,
  };
}

async function auditDestinationState(completed): Promise<DestinationAudit> {
  const rateByHotkey = await fetchStorageMap(completed.query.subtensorModule.basketRate);
  const sharesByHotkey = await fetchStorageMap(completed.query.subtensorModule.basketShares);
  const claimedSumByHotkey = new Map<string, bigint>();
  const claimedPairs = new Set<string>();
  let startKey;
  let page = 0;

  for (;;) {
    const entries = await completed.query.subtensorModule.basketClaimed.entriesPaged({
      args: [],
      pageSize: PAGE_SIZE,
      startKey,
    });
    if (entries.length === 0) {
      break;
    }
    page += 1;
    for (const [storageKey, claimed] of entries) {
      const [hotkeyArg, coldkeyArg] = storageKey.args;
      const hotkey = hotkeyArg.toString();
      const coldkey = coldkeyArg.toString();
      const claimedRaw = codecToBigInt(claimed);
      assert.notEqual(claimedRaw, 0n, `BasketClaimed stored an explicit zero for ${hotkey}/${coldkey}`);
      claimedPairs.add(pairKey(hotkey, coldkey));
      claimedSumByHotkey.set(hotkey, (claimedSumByHotkey.get(hotkey) ?? 0n) + claimedRaw);
    }
    startKey = entries.at(-1)[0];
    if (page === 1 || page % 25 === 0) {
      console.log("destination BasketClaimed scan:", `page=${page}`, `entries=${claimedPairs.size}`);
    }
  }

  return { rateByHotkey, sharesByHotkey, claimedSumByHotkey, claimedPairs };
}

async function fetchStorageMap(query): Promise<Map<string, bigint>> {
  const result = new Map<string, bigint>();
  let startKey;
  for (;;) {
    const entries = await query.entriesPaged({ args: [], pageSize: PAGE_SIZE, startKey });
    if (entries.length === 0) {
      break;
    }
    for (const [storageKey, value] of entries) {
      result.set(storageKey.args[0].toString(), codecToBigInt(value));
    }
    startKey = entries.at(-1)[0];
  }
  return result;
}

async function queryTotalRootStake(completed, hotkeys: Iterable<string>): Promise<Map<string, bigint>> {
  const hotkeyList = [...hotkeys];
  const result = new Map<string, bigint>();
  for (let offset = 0; offset < hotkeyList.length; offset += QUERY_MULTI_PAGE_SIZE) {
    const page = hotkeyList.slice(offset, offset + QUERY_MULTI_PAGE_SIZE);
    const values = await completed.query.subtensorModule.totalHotkeyAlpha.multi(
      page.map((hotkey) => [hotkey, ROOT_NETUID])
    );
    for (let index = 0; index < page.length; index++) {
      result.set(page[index], codecToBigInt(values[index]));
    }
    if (offset === 0 || offset % (QUERY_MULTI_PAGE_SIZE * 25) === 0) {
      console.log(
        "destination root stake scan:",
        `queried=${Math.min(offset + page.length, hotkeyList.length)}`,
        `total=${hotkeyList.length}`
      );
    }
  }
  return result;
}

function readSnapshot(): MigrationSnapshot {
  return JSON.parse(fs.readFileSync(SNAPSHOT_URL, "utf8")) as MigrationSnapshot;
}

function serializeSourceAudit(source: SourceAudit): SerializedSourceAudit {
  return {
    hotkeys: [...source.hotkeys],
    nonzeroRateHotkeys: [...source.nonzeroRateHotkeys],
    claimedPairs: [...source.claimedPairs],
    slotCounts: [...source.slotCounts].map(([key, value]) => [key, value.toString()]),
    claimedRowCounts: [...source.claimedRowCounts].map(([key, value]) => [key, value.toString()]),
    claimableEntries: source.claimableEntries.toString(),
    claimableSlots: source.claimableSlots.toString(),
    claimedEntries: source.claimedEntries.toString(),
    zeroClaimedEntries: source.zeroClaimedEntries.toString(),
  };
}

function deserializeSourceAudit(source: SerializedSourceAudit): SourceAudit {
  assert.ok(source, "migration snapshot is missing its pre-upgrade source audit");
  return {
    hotkeys: new Set(source.hotkeys),
    nonzeroRateHotkeys: new Set(source.nonzeroRateHotkeys),
    claimedPairs: new Set(source.claimedPairs),
    slotCounts: new Map(source.slotCounts.map(([key, value]) => [key, BigInt(value)])),
    claimedRowCounts: new Map(source.claimedRowCounts.map(([key, value]) => [key, BigInt(value)])),
    claimableEntries: BigInt(source.claimableEntries),
    claimableSlots: BigInt(source.claimableSlots),
    claimedEntries: BigInt(source.claimedEntries),
    zeroClaimedEntries: BigInt(source.zeroClaimedEntries),
  };
}

function storagePrefix(item: string): string {
  return `${xxhashAsHex("SubtensorModule", 128)}${xxhashAsHex(item, 128).slice(2)}`;
}

async function storagePrefixHasAny(api: ApiPromise, item: string, atHash?): Promise<boolean> {
  const keys = await api.rpc.state.getKeysPaged(storagePrefix(item), 1, undefined, atHash);
  return keys.length > 0;
}

async function storageQueryHasAny(query): Promise<boolean> {
  const keys = await query.keysPaged({ args: [], pageSize: 1 });
  return keys.length > 0;
}

async function storageExistsAt(api: ApiPromise, key: string, atHash): Promise<boolean> {
  const value = (await api.rpc.state.getStorage(key, atHash)) as unknown as { isSome: boolean };
  return value.isSome;
}

function codecToBigInt(codec): bigint {
  if (typeof codec.toBigInt === "function") {
    return codec.toBigInt();
  }
  const json = typeof codec.toJSON === "function" ? codec.toJSON() : codec;
  if (json && typeof json === "object" && "bits" in json) {
    return BigInt(json.bits);
  }
  return BigInt(codec.toString());
}

function incrementBigIntMap(map: Map<string, bigint>, key: string) {
  map.set(key, (map.get(key) ?? 0n) + 1n);
}

function pairKey(hotkey: string, coldkey: string): string {
  return `${hotkey}|${coldkey}`;
}

function assertSetEqual(actualValues: Iterable<string>, expectedValues: Iterable<string>, message: string) {
  const actual = new Set(actualValues);
  const expected = new Set(expectedValues);
  const missing = [...expected].filter((value) => !actual.has(value));
  const unexpected = [...actual].filter((value) => !expected.has(value));
  assert.equal(
    missing.length + unexpected.length,
    0,
    `${message}: missing=${missing.length} unexpected=${unexpected.length} ` +
      `first_missing=${missing[0] ?? "none"} first_unexpected=${unexpected[0] ?? "none"}`
  );
}

function assertSetSubset(actualValues: Iterable<string>, expectedValues: Iterable<string>, message: string) {
  const expected = new Set(expectedValues);
  const unexpected = [...actualValues].filter((value) => !expected.has(value));
  assert.equal(
    unexpected.length,
    0,
    `${message}: unexpected=${unexpected.length} first_unexpected=${unexpected[0] ?? "none"}`
  );
}

function absolute(value: bigint): bigint {
  return value < 0n ? -value : value;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
