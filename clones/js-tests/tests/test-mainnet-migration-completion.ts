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
const POST_MIGRATION_DIVIDEND_TIMEOUT_MS = 30 * 60 * 1000;
const POLL_INTERVAL_MS = 2_000;
const PAGE_SIZE = 1_000;
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
  upgradeTimestampMs?: string;
  upgradedObservedAtMs?: number;
  completion?: MigrationCompletion;
}

type MigrationPhase =
  | "convert"
  | "clear_shares"
  | "reconcile_claimants"
  | "clear_claimed"
  | "clear_principal";

interface MigrationCompletion {
  block: number;
  hash: string;
  migrationBlocks: number;
  phaseBlocks: Record<MigrationPhase, number>;
  chainElapsedMs: string;
  sawDeferredRootDividend?: boolean;
}

interface SerializedSourceAudit {
  hotkeys: string[];
  nonzeroRateHotkeys: string[];
  valuedRateHotkeys?: string[];
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
  valuedRateHotkeys: Set<string>;
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
  claimedColdkeys: Set<string>;
}

interface DissolutionState {
  queue: string | null;
  status: string | null;
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
    await storagePrefixHasAny(api, "BasketRate", header.hash),
    false,
    "pre-upgrade mainnet already contains BasketRate rows"
  );
  assert.equal(
    await storagePrefixHasAny(api, "BasketShares", header.hash),
    false,
    "pre-upgrade mainnet already contains BasketShares rows"
  );
  assert.equal(
    await storagePrefixHasAny(api, "BasketClaimed", header.hash),
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
  const upgradeHash = await api.rpc.chain.getBlockHash(upgradeBlock);
  const upgraded = await api.at(upgradeHash);
  snapshot.upgradeTimestampMs = codecToBigInt(await upgraded.query.timestamp.now()).toString();
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
  assert.notEqual(snapshot.upgradeTimestampMs, undefined, "missing upgrade timestamp from upgraded probe");
  assert.notEqual(snapshot.upgradedObservedAtMs, undefined, "missing upgraded observation time");

  const cursorKey = storagePrefix("SeedBetaBasketV2Migration");
  const deadline = Date.now() + WAIT_TIMEOUT_MS;
  let lastReportedBlock = -1;
  let observedCompletionBlock = -1;
  let lastCursorBlock = -1;
  let sawCursor = false;
  let lastDeferredAuditBlock = -1;
  let previousPendingRoot = new Map<string, bigint>();
  let previousEpochIndex = new Map<string, bigint>();
  let sawDeferredRootDividend = false;
  let dissolutionBaseline: DissolutionState | undefined;
  let currentPhase: MigrationPhase | undefined;
  let currentPhaseStartBlock = snapshot.upgradeBlock!;
  const phaseBlocks: Record<MigrationPhase, number> = {
    convert: 0,
    clear_shares: 0,
    reconcile_claimants: 0,
    clear_claimed: 0,
    clear_principal: 0,
  };

  while (Date.now() < deadline) {
    const header = await api.rpc.chain.getHeader();
    const block = header.number.toNumber();
    const cursorExists = await storageExistsAt(api, cursorKey, header.hash);
    if (cursorExists) {
      const phase = await readMigrationPhaseAt(api, cursorKey, header.hash);
      if (currentPhase === undefined) {
        currentPhase = phase;
        assert.equal(phase, "convert", "migration did not begin in the convert phase");
      } else if (phase !== currentPhase) {
        phaseBlocks[currentPhase] += block - currentPhaseStartBlock;
        console.log(
          "migration phase transition:",
          `from=${currentPhase}`,
          `to=${phase}`,
          `block=${block}`,
          `completed_phase_blocks=${phaseBlocks[currentPhase]}`
        );
        currentPhase = phase;
        currentPhaseStartBlock = block;
      }
      sawCursor = true;
      lastCursorBlock = block;
      const dissolutionNow = await readDissolutionState(api, header.hash);
      if (dissolutionBaseline === undefined) {
        dissolutionBaseline = dissolutionNow;
      } else {
        assert.deepEqual(
          dissolutionNow,
          dissolutionBaseline,
          `dissolution cleanup advanced while basket seed was running at block ${block}`
        );
      }
      if (block !== lastDeferredAuditBlock) {
        const at = await api.at(header.hash);
        const pendingRoot = await fetchStorageMap(at.query.subtensorModule.pendingRootAlphaDivs);
        const epochIndex = await fetchStorageMap(at.query.subtensorModule.subnetEpochIndex);
        if (lastDeferredAuditBlock !== -1) {
          for (const [netuid, previousPending] of previousPendingRoot) {
            const currentPending = pendingRoot.get(netuid) ?? 0n;
            assert.equal(
              currentPending >= previousPending,
              true,
              `PendingRootAlphaDivs decreased during basket seed for netuid ${netuid}: ` +
                `before=${previousPending} after=${currentPending}`
            );
            if (
              previousPending > 0n &&
              (epochIndex.get(netuid) ?? 0n) > (previousEpochIndex.get(netuid) ?? 0n)
            ) {
              sawDeferredRootDividend = true;
            }
          }
        }
        previousPendingRoot = pendingRoot;
        previousEpochIndex = epochIndex;
        lastDeferredAuditBlock = block;
      }
    } else if (sawCursor) {
      assert.deepEqual(
        await readDissolutionState(api, header.hash),
        dissolutionBaseline,
        `dissolution cleanup advanced in the basket seed completion block ${block}`
      );
      observedCompletionBlock = block;
      break;
    } else if (
      (
        await api.query.subtensorModule.hasMigrationRun([
          ...Buffer.from("migrate_seed_beta_basket_v2"),
        ])
      ).toString() === "true"
    ) {
      // Support resuming the audit after the node/test process was restarted post-completion.
      observedCompletionBlock = block;
      break;
    }
    if (sawCursor && block >= lastReportedBlock + 50) {
      console.log("migration still running:", `block=${block}`);
      lastReportedBlock = block;
    }
    await delay(POLL_INTERVAL_MS);
  }
  assert.notEqual(observedCompletionBlock, -1, `migration cursor remained present for ${WAIT_TIMEOUT_MS}ms`);

  const completionBlock = sawCursor
    ? await findExactCompletionBlock(api, cursorKey, lastCursorBlock, observedCompletionBlock)
    : snapshot.completion?.block ??
      (await findRecentCompletionBlock(api, cursorKey, observedCompletionBlock));
  const completionHash = await api.rpc.chain.getBlockHash(completionBlock);
  const completed = await api.at(completionHash);
  const migrationBlocks = completionBlock - snapshot.upgradeBlock!;
  const upgradeTimestamp = BigInt(snapshot.upgradeTimestampMs!);
  const completionTimestamp = codecToBigInt(await completed.query.timestamp.now());
  const chainElapsedMs = completionTimestamp - upgradeTimestamp;
  assert.equal(
    chainElapsedMs,
    BigInt(migrationBlocks * EXPECTED_BLOCK_TIME_MS),
    `clone did not preserve 12-second mainnet cadence: elapsed_ms=${chainElapsedMs} blocks=${migrationBlocks}`
  );
  const normalCadenceSeconds = migrationBlocks * 12;
  const observedWallMs = Date.now() - snapshot.upgradedObservedAtMs!;
  if (!sawCursor && snapshot.completion !== undefined) {
    Object.assign(phaseBlocks, snapshot.completion.phaseBlocks);
  } else if (currentPhase !== undefined) {
    phaseBlocks[currentPhase] += completionBlock - currentPhaseStartBlock;
  }
  assert.equal(
    Object.values(phaseBlocks).reduce((sum, blocks) => sum + blocks, 0),
    migrationBlocks,
    "per-phase block counts do not sum to the total migration blocks"
  );
  const deferredRootDividendObserved = sawCursor
    ? sawDeferredRootDividend
    : snapshot.completion?.sawDeferredRootDividend ??
      (await auditDeferredRootDividendsDuringMigration(
        api,
        snapshot.upgradeBlock!,
        completionBlock
      ));
  snapshot.completion = {
    block: completionBlock,
    hash: completionHash.toHex(),
    migrationBlocks,
    phaseBlocks,
    chainElapsedMs: chainElapsedMs.toString(),
    sawDeferredRootDividend: deferredRootDividendObserved,
  };
  fs.writeFileSync(SNAPSHOT_URL, `${JSON.stringify(snapshot, null, 2)}\n`);
  console.log(
    "migration completion:",
    `upgrade_block=${snapshot.upgradeBlock}`,
    `completion_block=${completionBlock}`,
    `completion_hash=${completionHash.toHex()}`,
    `migration_blocks=${migrationBlocks}`,
    `chain_elapsed_ms=${chainElapsedMs}`,
    `block_time_ms=${EXPECTED_BLOCK_TIME_MS}`,
    `normal_12s_estimate_seconds=${normalCadenceSeconds}`,
    `observed_wall_ms=${observedWallMs}`,
    `convert_blocks=${phaseBlocks.convert}`,
    `clear_shares_blocks=${phaseBlocks.clear_shares}`,
    `reconcile_claimants_blocks=${phaseBlocks.reconcile_claimants}`,
    `clear_claimed_blocks=${phaseBlocks.clear_claimed}`,
    `clear_principal_blocks=${phaseBlocks.clear_principal}`
  );

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

  await assertDeferredRootDividendsReleased(
    api,
    completed,
    completionBlock,
    deferredRootDividendObserved
  );

  const source = deserializeSourceAudit(snapshot.source);
  const destination = await auditDestinationState(completed);

  assertSetSubset(
    destination.rateByHotkey.keys(),
    source.nonzeroRateHotkeys,
    "BasketRate contains a hotkey absent from nonzero RootClaimable"
  );
  const roundedZeroSourceHotkeys = [...source.nonzeroRateHotkeys].filter(
    (hotkey) => !destination.rateByHotkey.has(hotkey)
  );
  // A positive subnet price does not guarantee a nonzero fixed-point rate contribution:
  // sufficiently small `rate * price` products legitimately round to zero. Verify every
  // omitted source using the migration's actual conversion-block pricing instead.
  await assertOmittedSourcesRoundToZero(
    api,
    roundedZeroSourceHotkeys,
    snapshot.upgradeBlock!,
    completionBlock
  );
  for (const hotkey of roundedZeroSourceHotkeys) {
    assert.equal(
      destination.sharesByHotkey.has(hotkey),
      false,
      `source hotkey without BasketRate unexpectedly has BasketShares: ${hotkey}`
    );
    assert.equal(
      destination.claimedSumByHotkey.has(hotkey),
      false,
      `source hotkey without BasketRate unexpectedly has BasketClaimed: ${hotkey}`
    );
  }
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
  let rateFundsAudited = 0;
  let worstConservationPpm = 0n;
  let worstConservationHotkey = "";
  let aggregateFloorMismatches = 0;

  for (const [hotkey, rateRaw] of destination.rateByHotkey) {
    assert.equal(rateRaw >= 0n, true, `BasketRate is negative for ${hotkey}`);
    const rootStake = totalRootByHotkey.get(hotkey) ?? 0n;
    const claimed = destination.claimedSumByHotkey.get(hotkey) ?? 0n;
    const shares = destination.sharesByHotkey.get(hotkey) ?? 0n;
    const aggregateOwed = (rateRaw * rootStake) / FIXED_SCALE - claimed;
    const payableOwed = aggregateOwed < 0n ? 0n : aggregateOwed;
    const difference = absolute(payableOwed - shares);
    const sourceRows = source.claimedRowCounts.get(hotkey) ?? 0n;
    const sourceSlots = source.slotCounts.get(hotkey) ?? 0n;

    // The migration floors fixed-point values once per source slot/claim row. Permit that
    // unavoidable dust plus one basis point, while still rejecting any material loss or mint.
    const roundingAllowance = sourceRows + sourceSlots + 100n;
    const proportionalAllowance = payableOwed / 10_000n;
    const allowedDifference =
      roundingAllowance > proportionalAllowance ? roundingAllowance : proportionalAllowance;
    if (difference > allowedDifference) {
      aggregateFloorMismatches += 1;
    }

    const denominator = payableOwed || 1n;
    const differencePpm = (difference * 1_000_000n) / denominator;
    if (differencePpm > worstConservationPpm) {
      worstConservationPpm = differencePpm;
      worstConservationHotkey = hotkey;
    }
    rateFundsAudited += 1;
  }

  const claimantAudit = await auditClaimantConservation(
    api,
    completed,
    completionHash.toHex(),
    source,
    destination
  );

  const rootWeightKeys = await completed.query.subtensorModule.weights.keys(ROOT_NETUID);
  assert.equal(rootWeightKeys.length, 0, "legacy root weight vectors were not fully cleared");

  const minAllowedWeights = await completed.query.subtensorModule.minAllowedWeights(ROOT_NETUID);
  assert.equal(
    Number(minAllowedWeights.toString()),
    EXPECTED_MIN_ROOT_WEIGHTS,
    "root MinAllowedWeights was not migrated to the basket minimum"
  );

  console.log(
    "migration source audit:",
    `RootClaimable_entries=${source.claimableEntries}`,
    `RootClaimable_slots=${source.claimableSlots}`,
    `RootClaimable_nonzero_rate_hotkeys=${source.nonzeroRateHotkeys.size}`,
    `RootClaimed_entries=${source.claimedEntries}`,
    `RootClaimed_zero_entries=${source.zeroClaimedEntries}`,
    `RootClaimed_unique_pairs=${source.claimedPairs.size}`
  );
  console.log(
    "migration destination audit:",
    `BasketRate_entries=${destination.rateByHotkey.size}`,
    `BasketShares_entries=${destination.sharesByHotkey.size}`,
    `BasketClaimed_entries=${destination.claimedPairs.size}`,
    `rounded_zero_source_hotkeys=${roundedZeroSourceHotkeys.length}`,
    `rate_funds_audited=${rateFundsAudited}`,
    `aggregate_floor_mismatches=${aggregateFloorMismatches}`,
    `claimant_conservation_failures=${claimantAudit.failures.length}`,
    `claimant_positions=${claimantAudit.positions}`,
    `worst_conservation_ppm=${worstConservationPpm}`,
    `worst_conservation_hotkey=${worstConservationHotkey || "none"}`
  );
  console.log(
    "migration structural invariants: ok",
    "legacy RootClaimable/RootClaimed/BasketPrincipal empty;",
    "all TAO-valued source rates transferred; omitted zero-valued sources have zero destinations;",
    "all destination shares/claims trace to legacy source keys;",
    "dissolution cleanup remained paused through the completion block;",
    "root weights empty;",
    `MinAllowedWeights[root]=${EXPECTED_MIN_ROOT_WEIGHTS};`,
    "all HasMigrationRun flags set"
  );
  assert.equal(
    claimantAudit.failures.length,
    0,
    `claimant conservation failed for ${claimantAudit.failures.length} validators; ` +
      `worst=${claimantAudit.worstHotkey} shares=${claimantAudit.worstShares} ` +
      `owed=${claimantAudit.worstOwed} difference=${claimantAudit.worstDifference}`
  );
}

async function auditClaimantConservation(
  api: ApiPromise,
  completed,
  completionHash: string,
  source: SourceAudit,
  destination: DestinationAudit
) {
  const basketHotkeys = new Set([
    ...destination.rateByHotkey.keys(),
    ...destination.sharesByHotkey.keys(),
  ]);
  const indexedColdkeys = await auditDenseStakingColdkeyIndex(completed);
  const coldkeys = new Set(destination.claimedColdkeys);
  let startKey;
  let stakingPage = 0;
  let stakingEntries = 0;

  for (;;) {
    const entries = await completed.query.subtensorModule.stakingHotkeys.entriesPaged({
      args: [],
      pageSize: PAGE_SIZE,
      startKey,
    });
    if (entries.length === 0) {
      break;
    }
    stakingPage += 1;
    stakingEntries += entries.length;
    for (const [storageKey, stakedHotkeys] of entries) {
      if ([...stakedHotkeys].some((hotkey) => basketHotkeys.has(hotkey.toString()))) {
        coldkeys.add(storageKey.args[0].toString());
      }
    }
    startKey = entries.at(-1)[0];
    if (stakingPage === 1 || stakingPage % 25 === 0) {
      console.log(
        "claimant discovery scan:",
        `page=${stakingPage}`,
        `staking_entries=${stakingEntries}`,
        `candidate_coldkeys=${coldkeys.size}`
      );
    }
  }

  const coldkeyList = [...coldkeys];
  const owedByHotkey = new Map<string, bigint>();
  let positions = 0;
  const rpcBatchSize = 25;
  for (let offset = 0; offset < coldkeyList.length; offset += rpcBatchSize) {
    const batch = coldkeyList.slice(offset, offset + rpcBatchSize);
    const results = await Promise.all(
      batch.map(async (coldkey) => {
        const encoded = await (api as any)._rpcCore.provider.send("betaBasket_getStakerPositions", [
          coldkey,
          completionHash,
        ]);
        return {
          coldkey,
          positions: api.createType(
            "Vec<(AccountId32,u64,u64)>",
            rpcBytes(api, encoded)
          ),
        };
      })
    );
    for (const result of results) {
      for (const [hotkey, owedShares] of result.positions) {
        const key = hotkey.toString();
        if (!basketHotkeys.has(key)) {
          continue;
        }
        assert.equal(
          indexedColdkeys.has(result.coldkey),
          true,
          `nonzero migrated claimant ${result.coldkey} was absent from the reconciliation index`
        );
        owedByHotkey.set(key, (owedByHotkey.get(key) ?? 0n) + codecToBigInt(owedShares));
        positions += 1;
      }
    }
    if (offset === 0 || offset % 1_000 === 0) {
      console.log(
        "claimant position audit:",
        `queried=${Math.min(offset + batch.length, coldkeyList.length)}`,
        `total=${coldkeyList.length}`,
        `positions=${positions}`
      );
    }
  }

  const failures: Array<{
    hotkey: string;
    shares: bigint;
    owed: bigint;
    difference: bigint;
  }> = [];
  let worstHotkey = "none";
  let worstShares = 0n;
  let worstOwed = 0n;
  let worstDifference = 0n;
  let underbackedValidators = 0;
  let overbackedValidators = 0;
  let totalClaimantShortfall = 0n;
  let totalStrandedShares = 0n;
  for (const hotkey of basketHotkeys) {
    const shares = destination.sharesByHotkey.get(hotkey) ?? 0n;
    const owed = owedByHotkey.get(hotkey) ?? 0n;
    const difference = absolute(owed - shares);
    if (difference !== 0n) {
      failures.push({ hotkey, shares, owed, difference });
      if (owed > shares) {
        underbackedValidators += 1;
        totalClaimantShortfall += owed - shares;
      } else {
        overbackedValidators += 1;
        totalStrandedShares += shares - owed;
      }
      if (difference > worstDifference) {
        worstHotkey = hotkey;
        worstShares = shares;
        worstOwed = owed;
        worstDifference = difference;
      }
    }
  }
  failures.sort((left, right) =>
    left.difference === right.difference ? 0 : left.difference > right.difference ? -1 : 1
  );

  console.log(
    "claimant conservation audit:",
    `candidate_coldkeys=${coldkeyList.length}`,
    `positions=${positions}`,
    `validators=${basketHotkeys.size}`,
    `failures=${failures.length}`,
    `underbacked_validators=${underbackedValidators}`,
    `overbacked_validators=${overbackedValidators}`,
    `total_claimant_shortfall=${totalClaimantShortfall}`,
    `total_stranded_shares=${totalStrandedShares}`,
    `worst_hotkey=${worstHotkey}`,
    `worst_shares=${worstShares}`,
    `worst_owed=${worstOwed}`,
    `worst_difference=${worstDifference}`
  );
  for (const failure of failures) {
    console.log(
      "claimant conservation failure:",
      `hotkey=${failure.hotkey}`,
      `shares=${failure.shares}`,
      `owed=${failure.owed}`,
      `difference=${failure.difference}`,
      `direction=${failure.owed > failure.shares ? "underbacked" : "stranded"}`
    );
  }
  return {
    failures,
    positions,
    worstHotkey,
    worstShares,
    worstOwed,
    worstDifference,
    underbackedValidators,
    overbackedValidators,
    totalClaimantShortfall,
    totalStrandedShares,
  };
}

async function auditDenseStakingColdkeyIndex(completed): Promise<Set<string>> {
  const expected = Number(codecToBigInt(await completed.query.subtensorModule.numStakingColdkeys()));
  const byIndex = new Map<number, string>();
  let startKey;

  for (;;) {
    const entries = await completed.query.subtensorModule.stakingColdkeysByIndex.entriesPaged({
      args: [],
      pageSize: PAGE_SIZE,
      startKey,
    });
    if (entries.length === 0) {
      break;
    }
    for (const [storageKey, coldkey] of entries) {
      byIndex.set(Number(storageKey.args[0].toString()), coldkey.toString());
    }
    startKey = entries.at(-1)[0];
  }

  assert.equal(
    byIndex.size,
    expected,
    `StakingColdkeysByIndex is not dense: entries=${byIndex.size} NumStakingColdkeys=${expected}`
  );
  const orderedColdkeys: string[] = [];
  for (let index = 0; index < expected; index++) {
    const coldkey = byIndex.get(index);
    assert.notEqual(coldkey, undefined, `StakingColdkeysByIndex is missing dense index ${index}`);
    orderedColdkeys.push(coldkey!);
  }
  assert.equal(
    new Set(orderedColdkeys).size,
    expected,
    "StakingColdkeysByIndex contains duplicate coldkeys"
  );

  for (let offset = 0; offset < orderedColdkeys.length; offset += QUERY_MULTI_PAGE_SIZE) {
    const page = orderedColdkeys.slice(offset, offset + QUERY_MULTI_PAGE_SIZE);
    const reverse = await completed.query.subtensorModule.stakingColdkeys.multi(page);
    for (let position = 0; position < page.length; position++) {
      assert.equal(
        reverse[position].isSome,
        true,
        `StakingColdkeys reverse index is missing ${page[position]}`
      );
      assert.equal(
        Number(reverse[position].unwrap().toString()),
        offset + position,
        `StakingColdkeys reverse index disagrees for ${page[position]}`
      );
    }
  }

  console.log("dense staking-coldkey index audit: ok", `entries=${expected}`);
  return new Set(orderedColdkeys);
}

async function assertOmittedSourcesRoundToZero(
  api: ApiPromise,
  hotkeys: string[],
  upgradeBlock: number,
  completionBlock: number
) {
  for (const hotkey of hotkeys) {
    let low = upgradeBlock;
    let high = completionBlock;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      const at = await api.at(await api.rpc.chain.getBlockHash(middle));
      const claimable: any = await at.query.subtensorModule.rootClaimable(hotkey);
      if (claimable.size === 0) {
        high = middle;
      } else {
        low = middle + 1;
      }
    }
    const conversionBlock = low;
    const before = await api.at(await api.rpc.chain.getBlockHash(conversionBlock - 1));
    const converted = await api.at(await api.rpc.chain.getBlockHash(conversionBlock));
    const claimable: any = await before.query.subtensorModule.rootClaimable(hotkey);
    let totalRateContribution = 0n;
    for (const [netuid, rate] of claimable.entries()) {
      const netuidNumber = Number(netuid.toString());
      let priceRaw = FIXED_SCALE;
      if (netuidNumber !== ROOT_NETUID) {
        const moving = codecToBigInt(
          await converted.query.subtensorModule.subnetMovingPrice(netuidNumber)
        );
        if (moving > 0n) {
          priceRaw = moving;
        } else {
          const [tao, alpha] = await Promise.all([
            converted.query.subtensorModule.subnetTAO(netuidNumber),
            converted.query.subtensorModule.subnetAlphaIn(netuidNumber),
          ]);
          const alphaRaw = codecToBigInt(alpha);
          priceRaw = alphaRaw === 0n ? 0n : (codecToBigInt(tao) * FIXED_SCALE) / alphaRaw;
        }
      }
      totalRateContribution += (codecToBigInt(rate) * priceRaw) / FIXED_SCALE;
    }
    assert.equal(
      totalRateContribution,
      0n,
      `source hotkey omitted from BasketRate had a nonzero converted rate: ${hotkey}`
    );
    console.log(
      "rounded-zero source verified:",
      `hotkey=${hotkey}`,
      `conversion_block=${conversionBlock}`,
      `rate_contribution_bits=${totalRateContribution}`
    );
  }
}

async function auditDeferredRootDividendsDuringMigration(
  api: ApiPromise,
  upgradeBlock: number,
  completionBlock: number
): Promise<boolean> {
  let previousPendingRoot = new Map<string, bigint>();
  let previousEpochIndex = new Map<string, bigint>();
  let sawDeferredRootDividend = false;

  for (let block = upgradeBlock; block < completionBlock; block++) {
    const hash = await api.rpc.chain.getBlockHash(block);
    const at = await api.at(hash);
    const [pendingRoot, epochIndex] = await Promise.all([
      fetchStorageMap(at.query.subtensorModule.pendingRootAlphaDivs),
      fetchStorageMap(at.query.subtensorModule.subnetEpochIndex),
    ]);

    if (block !== upgradeBlock) {
      for (const [netuid, previousPending] of previousPendingRoot) {
        const currentPending = pendingRoot.get(netuid) ?? 0n;
        assert.equal(
          currentPending >= previousPending,
          true,
          `PendingRootAlphaDivs decreased during basket seed for netuid ${netuid} at block ${block}: ` +
            `before=${previousPending} after=${currentPending}`
        );
        if (
          previousPending > 0n &&
          (epochIndex.get(netuid) ?? 0n) > (previousEpochIndex.get(netuid) ?? 0n)
        ) {
          sawDeferredRootDividend = true;
        }
      }
    }

    previousPendingRoot = pendingRoot;
    previousEpochIndex = epochIndex;
    if (block === upgradeBlock || (block - upgradeBlock) % 50 === 0) {
      console.log(
        "historical deferred-dividend audit:",
        `block=${block}`,
        `completion_block=${completionBlock}`,
        `observed_due_epoch=${sawDeferredRootDividend}`
      );
    }
  }

  return sawDeferredRootDividend;
}

async function assertDeferredRootDividendsReleased(
  api: ApiPromise,
  completed,
  completionBlock: number,
  sawDeferredRootDividend: boolean
) {
  assert.equal(
    sawDeferredRootDividend,
    true,
    "mainnet clone did not observe a due epoch with nonzero deferred root alpha during migration"
  );

  const pendingAtCompletion = await fetchStorageMap(
    completed.query.subtensorModule.pendingRootAlphaDivs
  );
  const epochAtCompletion = await fetchStorageMap(completed.query.subtensorModule.subnetEpochIndex);
  const depositedAtCompletion = sumMapValues(
    await fetchStorageMap(completed.query.subtensorModule.basketDepositedTao)
  );
  const candidates = new Map(
    [...pendingAtCompletion].filter(([, pending]) => pending > 0n)
  );
  assert.equal(
    candidates.size > 0,
    true,
    "no deferred PendingRootAlphaDivs remained at migration completion"
  );

  const deadline = Date.now() + POST_MIGRATION_DIVIDEND_TIMEOUT_MS;
  let lastBlock = completionBlock;
  while (Date.now() < deadline) {
    const header = await api.rpc.chain.getHeader();
    const block = header.number.toNumber();
    if (block <= lastBlock) {
      await delay(POLL_INTERVAL_MS);
      continue;
    }
    lastBlock = block;
    const at = await api.at(header.hash);
    const pendingNow = await fetchStorageMap(at.query.subtensorModule.pendingRootAlphaDivs);
    const epochNow = await fetchStorageMap(at.query.subtensorModule.subnetEpochIndex);

    for (const [netuid, pendingBefore] of candidates) {
      const pendingAfter = pendingNow.get(netuid) ?? 0n;
      const epochAdvanced =
        (epochNow.get(netuid) ?? 0n) > (epochAtCompletion.get(netuid) ?? 0n);
      if (!epochAdvanced || pendingAfter >= pendingBefore) {
        continue;
      }

      const distributed = await at.query.subtensorModule.rootAlphaDividendsPerSubnet.entries(netuid);
      const distributedRootAlpha = distributed.reduce(
        (total, [, value]) => total + codecToBigInt(value),
        0n
      );
      const depositedNow = sumMapValues(
        await fetchStorageMap(at.query.subtensorModule.basketDepositedTao)
      );
      assert.equal(
        distributedRootAlpha > 0n,
        true,
        `deferred root alpha drained for netuid ${netuid} without recorded root dividends`
      );
      assert.equal(
        depositedNow > depositedAtCompletion,
        true,
        `deferred root alpha drained for netuid ${netuid} without increasing basket deposits`
      );
      console.log(
        "deferred root dividends: ok",
        `netuid=${netuid}`,
        `completion_pending=${pendingBefore}`,
        `post_epoch_pending=${pendingAfter}`,
        `distributed_root_alpha=${distributedRootAlpha}`,
        `basket_deposited_delta=${depositedNow - depositedAtCompletion}`,
        `release_block=${block}`
      );
      return;
    }
    await delay(POLL_INTERVAL_MS);
  }

  assert.fail(
    `deferred root dividends were not released within ${POST_MIGRATION_DIVIDEND_TIMEOUT_MS}ms`
  );
}

async function findRecentCompletionBlock(
  api: ApiPromise,
  cursorKey: string,
  observedCompletionBlock: number
): Promise<number> {
  const oldestAvailable = Math.max(0, observedCompletionBlock - 255);
  for (let block = observedCompletionBlock - 1; block >= oldestAvailable; block--) {
    const hash = await api.rpc.chain.getBlockHash(block);
    if (await storageExistsAt(api, cursorKey, hash)) {
      return block + 1;
    }
  }
  assert.fail(
    `could not find a recent cursor transition before completed block ${observedCompletionBlock}`
  );
}

async function findExactCompletionBlock(
  api: ApiPromise,
  cursorKey: string,
  lastCursorBlock: number,
  observedCompletionBlock: number
): Promise<number> {
  assert.notEqual(lastCursorBlock, -1, "missing last block with a migration cursor");
  for (let block = lastCursorBlock + 1; block <= observedCompletionBlock; block++) {
    const hash = await api.rpc.chain.getBlockHash(block);
    const exists = await storageExistsAt(api, cursorKey, hash);
    if (!exists) {
      return block;
    }
  }
  assert.fail(
    `could not find cursor transition after block ${lastCursorBlock} and by ${observedCompletionBlock}`
  );
}

async function auditSourceState(before): Promise<SourceAudit> {
  const hotkeys = new Set<string>();
  const nonzeroRateHotkeys = new Set<string>();
  const nonzeroRateSlots = new Map<string, Set<number>>();
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
      for (const [netuid, rate] of claimable.entries()) {
        const raw = codecToBigInt(rate);
        assert.equal(raw >= 0n, true, `legacy RootClaimable rate is negative for ${hotkey}`);
        if (raw !== 0n) {
          hasNonzeroRate = true;
          const netuids = nonzeroRateSlots.get(hotkey) ?? new Set<number>();
          netuids.add(Number(netuid.toString()));
          nonzeroRateSlots.set(hotkey, netuids);
        }
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

  const positivePriceNetuids = await findPositiveConversionPriceNetuids(
    before,
    new Set([...nonzeroRateSlots.values()].flatMap((netuids) => [...netuids]))
  );
  const valuedRateHotkeys = new Set(
    [...nonzeroRateSlots]
      .filter(([, netuids]) => [...netuids].some((netuid) => positivePriceNetuids.has(netuid)))
      .map(([hotkey]) => hotkey)
  );

  return {
    hotkeys,
    nonzeroRateHotkeys,
    valuedRateHotkeys,
    claimedPairs,
    slotCounts,
    claimedRowCounts,
    claimableEntries,
    claimableSlots,
    claimedEntries,
    zeroClaimedEntries,
  };
}

async function findPositiveConversionPriceNetuids(
  before,
  netuids: Set<number>
): Promise<Set<number>> {
  const result = new Set<number>();
  const all = [...netuids];

  for (let offset = 0; offset < all.length; offset += QUERY_MULTI_PAGE_SIZE) {
    const page = all.slice(offset, offset + QUERY_MULTI_PAGE_SIZE);
    const [mechanisms, movingPrices, alphaReserves, taoReserves] = await Promise.all([
      before.query.subtensorModule.subnetMechanism.multi(page),
      before.query.subtensorModule.subnetMovingPrice.multi(page),
      before.query.subtensorModule.subnetAlphaIn.multi(page),
      before.query.subtensorModule.subnetTAO.multi(page),
    ]);

    for (let index = 0; index < page.length; index++) {
      const netuid = page[index];
      const isRootOrStable = netuid === ROOT_NETUID || codecToBigInt(mechanisms[index]) === 0n;
      const movingPriceIsPositive = codecToBigInt(movingPrices[index]) > 0n;
      const spotPriceIsPositive =
        codecToBigInt(alphaReserves[index]) > 0n && codecToBigInt(taoReserves[index]) > 0n;
      if (isRootOrStable || movingPriceIsPositive || spotPriceIsPositive) {
        result.add(netuid);
      }
    }
  }

  console.log(
    "source conversion-price audit:",
    `nonzero_rate_netuids=${all.length}`,
    `positive_price_netuids=${result.size}`
  );
  return result;
}

async function auditDestinationState(completed): Promise<DestinationAudit> {
  const rateByHotkey = await fetchStorageMap(completed.query.subtensorModule.basketRate);
  const sharesByHotkey = await fetchStorageMap(completed.query.subtensorModule.basketShares);
  const claimedSumByHotkey = new Map<string, bigint>();
  const claimedPairs = new Set<string>();
  const claimedColdkeys = new Set<string>();
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
      claimedColdkeys.add(coldkey);
      claimedSumByHotkey.set(hotkey, (claimedSumByHotkey.get(hotkey) ?? 0n) + claimedRaw);
    }
    startKey = entries.at(-1)[0];
    if (page === 1 || page % 25 === 0) {
      console.log("destination BasketClaimed scan:", `page=${page}`, `entries=${claimedPairs.size}`);
    }
  }

  return { rateByHotkey, sharesByHotkey, claimedSumByHotkey, claimedPairs, claimedColdkeys };
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

function rpcBytes(api: ApiPromise, value): Uint8Array {
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return Uint8Array.from(value);
  if (typeof value === "string" && value.startsWith("0x")) {
    return api.createType("Bytes", value).toU8a(true);
  }
  return Uint8Array.from(value);
}

function serializeSourceAudit(source: SourceAudit): SerializedSourceAudit {
  return {
    hotkeys: [...source.hotkeys],
    nonzeroRateHotkeys: [...source.nonzeroRateHotkeys],
    valuedRateHotkeys: [...source.valuedRateHotkeys],
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
    valuedRateHotkeys: new Set(source.valuedRateHotkeys ?? []),
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

async function readMigrationPhaseAt(
  api: ApiPromise,
  key: string,
  atHash
): Promise<MigrationPhase> {
  const hex = await storageHexAt(api, key, atHash);
  assert.notEqual(hex, null, "migration cursor disappeared while reading its phase");
  const variant = Number.parseInt(hex!.slice(2, 4), 16);
  const phases: MigrationPhase[] = [
    "convert",
    "clear_shares",
    "reconcile_claimants",
    "clear_claimed",
    "clear_principal",
  ];
  assert.equal(
    variant < phases.length,
    true,
    `unknown SeedBetaBasketV2Progress variant ${variant}`
  );
  return phases[variant];
}

async function readDissolutionState(api: ApiPromise, atHash): Promise<DissolutionState> {
  return {
    queue: await storageHexAt(api, storagePrefix("DissolveCleanupQueue"), atHash),
    status: await storageHexAt(api, storagePrefix("CurrentDissolveCleanupStatus"), atHash),
  };
}

async function storageHexAt(api: ApiPromise, key: string, atHash): Promise<string | null> {
  const value = (await api.rpc.state.getStorage(key, atHash)) as unknown as {
    isSome: boolean;
    unwrap(): { toHex(): string };
  };
  return value.isSome ? value.unwrap().toHex() : null;
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

function sumMapValues(map: Map<string, bigint>): bigint {
  let total = 0n;
  for (const value of map.values()) {
    total += value;
  }
  return total;
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
