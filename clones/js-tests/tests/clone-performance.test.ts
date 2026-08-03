import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import type { ApiPromise } from "@polkadot/api";
import {
  evaluateMigrationReadiness,
  waitForMigrationReadiness,
  type MigrationReadinessSnapshot,
} from "../lib/clone-readiness.js";
import {
  BestHeadLiveness,
  NodeLogTail,
  bestHeadFailureReasons,
  blockLatencyFailureReasons,
  computeEpochCoverageBudget,
  evaluateEpochCoverage,
  parsePreparedBlockChunk,
  remainingRequiredBlockSamples,
  sampleDrainFailureReason,
  summarizeBlockSamples,
  type EpochBaseline,
} from "../lib/clone-performance.js";

test("parses complete and partial proposer log lines without duplicating samples", () => {
  const first = parsePreparedBlockChunk(
    "",
    "noise\n2026 Prepared block for proposing at 100 (1999 ms)\nPrepared block for proposing at 101 (20",
  );
  assert.deepEqual(first.samples, [{ block: 100, durationMs: 1999 }]);
  assert.equal(first.remainder, "Prepared block for proposing at 101 (20");

  const second = parsePreparedBlockChunk(first.remainder, "00 ms) hash=0x1\nmalformed (5000 ms)\n");
  assert.deepEqual(second.samples, [{ block: 101, durationMs: 2000 }]);
  assert.equal(second.remainder, "");
});

test("flushes a final unterminated proposer line", () => {
  const parsed = parsePreparedBlockChunk(
    "Prepared block for proposing at #9 (4000",
    " ms)",
    true,
  );
  assert.deepEqual(parsed.samples, [{ block: 9, durationMs: 4000 }]);
});

test("tails from the post-upgrade byte offset and handles appended partial lines", (context) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "clone-block-tail-"));
  context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const filename = path.join(directory, "clone-node.log");
  fs.writeFileSync(filename, "Prepared block for proposing at 1 (9000 ms)\n");
  const postUpgradeOffset = fs.statSync(filename).size;
  const tail = new NodeLogTail(filename, postUpgradeOffset);

  fs.appendFileSync(filename, "Prepared block for proposing at 2 (250");
  assert.deepEqual(tail.read(), []);
  fs.appendFileSync(filename, " ms)\n");
  assert.deepEqual(tail.read(), [{ block: 2, durationMs: 250 }]);
});

test("classifies warning and hard-failure boundaries and calculates nearest-rank percentiles", () => {
  const summary = summarizeBlockSamples([
    { block: 1, durationMs: 100 },
    { block: 2, durationMs: 1999 },
    { block: 3, durationMs: 2000 },
    { block: 4, durationMs: 3999 },
    { block: 5, durationMs: 4000 },
  ]);

  assert.equal(summary.sampleCount, 5);
  assert.equal(summary.meanMs, 2419.6);
  assert.equal(summary.maximumMs, 4000);
  assert.equal(summary.p50Ms, 2000);
  assert.equal(summary.p95Ms, 4000);
  assert.deepEqual(summary.warnings.map(({ block }) => block), [3, 4]);
  assert.deepEqual(summary.violations.map(({ block }) => block), [5]);
  const empty = summarizeBlockSamples([]);
  assert.equal(empty.meanMs, null);
  assert.equal(empty.maximumMs, null);
  assert.match(blockLatencyFailureReasons(empty, 20)[0], /observed 0 proposer samples/);
  assert.match(blockLatencyFailureReasons(summary, 5)[0], /1 block\(s\)/);
});

test("graceful monitor shutdown drains exactly to the minimum sample count", () => {
  assert.equal(remainingRequiredBlockSamples(0), 20);
  assert.equal(remainingRequiredBlockSamples(19), 1);
  assert.equal(remainingRequiredBlockSamples(20), 0);
  assert.equal(remainingRequiredBlockSamples(25), 0);
  assert.throws(() => remainingRequiredBlockSamples(-1), /invalid observed/);
  assert.equal(sampleDrainFailureReason(19, 119_999), undefined);
  assert.match(sampleDrainFailureReason(19, 120_000) ?? "", /expected proposer log format/);
  assert.equal(sampleDrainFailureReason(20, 120_000), undefined);
});

test("flags the 4725ms proposer duration observed on PR 3019", () => {
  const parsed = parsePreparedBlockChunk(
    "",
    "Prepared block for proposing at 491 (4725 ms) hash=0xabc\n",
  );
  const summary = summarizeBlockSamples(parsed.samples);
  assert.deepEqual(summary.violations, [{ block: 491, durationMs: 4725 }]);
});

test("warns, recovers, and aborts best-head stalls at distinct thresholds", () => {
  const liveness = new BestHeadLiveness(4_000, 12_000);
  liveness.observe(10, 1_000);
  assert.equal(liveness.tick(4_999).kind, "healthy");
  assert.equal(liveness.tick(5_000).kind, "warning");
  assert.equal(liveness.tick(8_000).kind, "healthy");

  const recovered = liveness.observe(11, 8_500);
  assert.equal(recovered?.durationMs, 7_500);
  assert.equal(recovered?.outcome, "recovered");
  assert.equal(recovered?.endedAtMs, 8_500);
  assert.equal(liveness.getStalls().length, 1);

  assert.equal(liveness.tick(20_499).kind, "warning");
  const aborted = liveness.tick(20_500);
  assert.equal(aborted.kind, "abort");
  if (aborted.kind === "abort") {
    assert.equal(aborted.stall.afterBlock, 11);
    assert.equal(aborted.stall.durationMs, 12_000);
    assert.equal(aborted.stall.outcome, "aborted");
    assert.equal(aborted.stall.endedAtMs, 20_500);
  }
});

test("rejects a best-head regression", () => {
  const liveness = new BestHeadLiveness();
  liveness.observe(20, 0);
  assert.throws(() => liveness.observe(19, 1), /regressed/);
});

test("records a recoverable soak stall as a violation before its hard timeout", () => {
  const liveness = new BestHeadLiveness(4_000, 12_000, 120_000);
  liveness.observe(50, 0);
  assert.equal(liveness.tick(4_000).kind, "warning");
  const violation = liveness.tick(12_000);
  assert.equal(violation.kind, "violation");
  assert.equal(liveness.tick(30_000).kind, "healthy");

  const recovered = liveness.observe(51, 30_660);
  assert.equal(recovered?.durationMs, 30_660);
  assert.equal(recovered?.violatedAtMs, 12_000);
  assert.deepEqual(bestHeadFailureReasons(liveness.getStalls()), [
    "1 best-head stall(s) met or exceeded 12000ms",
  ]);

  liveness.tick(34_660);
  liveness.tick(42_660);
  const aborted = liveness.tick(150_660);
  assert.equal(aborted.kind, "abort");
});

test("tracks complete epoch cycles and exposes removal, missing, and regression failures", () => {
  const baseline: EpochBaseline[] = [
    { netuid: 1, tempo: 360, epochIndex: 4n },
    { netuid: 2, tempo: 1800, epochIndex: 10n },
  ];
  const complete = evaluateEpochCoverage(
    baseline,
    new Map([
      [1, 6n],
      [2, 12n],
    ]),
    new Set([1, 2]),
    2,
  );
  assert.equal(complete.complete, true);

  const incomplete = evaluateEpochCoverage(
    baseline,
    new Map([
      [1, 3n],
      [2, 11n],
    ]),
    new Set([1]),
    2,
  );
  assert.equal(incomplete.complete, false);
  assert.deepEqual(incomplete.removedNetuids, [2]);
  assert.deepEqual(incomplete.regressedNetuids, [1]);

  const missing = evaluateEpochCoverage(baseline, new Map([[1, 6n]]), new Set([1, 2]), 2);
  assert.deepEqual(missing.missingNetuids, [2]);
});

test("supports absent, cursorless instant, and multi-block migration states", () => {
  assert.deepEqual(
    evaluateMigrationReadiness({
      cursorExists: false,
      completionFlag: false,
      deferredEntriesExist: false,
    }),
    {
      kind: "not-observed",
      history: { sawCursor: false, sawDeferredEntries: false },
    },
  );

  const instant = evaluateMigrationReadiness({
    cursorExists: false,
    completionFlag: true,
    deferredEntriesExist: false,
  });
  assert.equal(instant.kind, "ready");

  const migrating = evaluateMigrationReadiness({
    cursorExists: true,
    completionFlag: false,
    deferredEntriesExist: true,
  });
  assert.equal(migrating.kind, "waiting");
  if (migrating.kind !== "waiting") {
    assert.fail("expected migration to be waiting");
  }
  assert.equal(migrating.stage, "migration");

  const draining = evaluateMigrationReadiness(
    { cursorExists: false, completionFlag: true, deferredEntriesExist: true },
    migrating.history,
  );
  assert.equal(draining.kind, "waiting");
  if (draining.kind !== "waiting") {
    assert.fail("expected deferred release to be waiting");
  }
  assert.equal(draining.stage, "deferred-release");

  const ready = evaluateMigrationReadiness(
    { cursorExists: false, completionFlag: true, deferredEntriesExist: false },
    draining.history,
  );
  assert.equal(ready.kind, "ready");
  assert.deepEqual(ready.history, { sawCursor: true, sawDeferredEntries: true });

  assert.equal(
    evaluateMigrationReadiness({
      cursorExists: true,
      completionFlag: true,
      deferredEntriesExist: false,
    }).kind,
    "invalid",
  );
  assert.equal(
    evaluateMigrationReadiness(
      { cursorExists: false, completionFlag: false, deferredEntriesExist: false },
      migrating.history,
    ).kind,
    "invalid",
  );
  assert.equal(
    evaluateMigrationReadiness({
      cursorExists: false,
      completionFlag: false,
      deferredEntriesExist: true,
    }).kind,
    "invalid",
  );
});

test("waits through cursor completion and deferred release before reporting readiness", async () => {
  const snapshots = [
    {
      block: 10,
      hash: "0x10",
      state: { cursorExists: true, completionFlag: false, deferredEntriesExist: true },
    },
    {
      block: 11,
      hash: "0x11",
      state: { cursorExists: false, completionFlag: true, deferredEntriesExist: true },
    },
    {
      block: 12,
      hash: "0x12",
      state: { cursorExists: false, completionFlag: true, deferredEntriesExist: false },
    },
  ];
  const api = mockReadinessApi(snapshots);
  const observation = await waitForMigrationReadiness(api, {
    deadlineEpochMs: Date.now() + 5_000,
    pollIntervalMs: 1,
  });

  assert.equal(observation.mode, "multi-block");
  assert.equal(observation.startBlock, 10);
  assert.equal(observation.cursorCompletionBlock, 11);
  assert.equal(observation.readinessBlock, 12);
  assert.equal(observation.observedBlocks, 2);
  assert.equal(observation.sawCursor, true);
  assert.equal(observation.sawDeferredEntries, true);
});

test("budgets tempo, two-per-block deferral, margin, and accelerated wall time", () => {
  const baseline: EpochBaseline[] = Array.from({ length: 128 }, (_, index) => ({
    netuid: index + 1,
    tempo: index === 0 ? 1800 : 360,
    epochIndex: 0n,
  }));
  const budget = computeEpochCoverageBudget(baseline, 2, 2);

  assert.equal(budget.maxTempo, 1800);
  assert.equal(budget.schedulingBlocksPerCycle, 64);
  assert.equal(budget.unpaddedBlocks, 3728);
  assert.equal(budget.blockBudget, 4101);
  assert.equal(budget.nominalWallMs, 1_025_250);
  assert.throws(
    () => computeEpochCoverageBudget([{ netuid: 1, tempo: 0, epochIndex: 0n }], 2, 2),
    /invalid tempo/,
  );
});

function mockReadinessApi(
  snapshots: ReadonlyArray<{
    block: number;
    hash: string;
    state: MigrationReadinessSnapshot;
  }>,
): ApiPromise {
  assert.ok(snapshots.length > 0);
  const byHash = new Map(snapshots.map((snapshot) => [snapshot.hash, snapshot.state]));
  let headerIndex = 0;
  const stateAt = (hash: string) => {
    const state = byHash.get(hash);
    assert.ok(state, `unknown mock block hash: ${hash}`);
    return state;
  };
  const api = {
    rpc: {
      chain: {
        getHeader: async () => {
          const snapshot = snapshots[Math.min(headerIndex, snapshots.length - 1)];
          headerIndex += 1;
          return {
            number: { toNumber: () => snapshot.block },
            hash: { toHex: () => snapshot.hash },
          };
        },
      },
      state: {
        getStorage: async (_key: string, hash: string) => ({
          isSome: stateAt(hash).cursorExists,
        }),
        getKeysPaged: async (_prefix: string, _count: number, _start: string, hash: string) =>
          stateAt(hash).deferredEntriesExist ? ["0xentry"] : [],
      },
    },
    at: async (hash: string) => ({
      query: {
        subtensorModule: {
          hasMigrationRun: async () => ({
            toString: () => String(stateAt(hash).completionFlag),
          }),
        },
      },
    }),
  };
  return api as unknown as ApiPromise;
}
