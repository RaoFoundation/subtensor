import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  BestHeadLiveness,
  NodeLogTail,
  blockLatencyFailureReasons,
  computeEpochCoverageBudget,
  evaluateEpochCoverage,
  evaluateMigrationGate,
  parsePreparedBlockChunk,
  remainingRequiredBlockSamples,
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
  assert.equal(summary.maximumMs, 4000);
  assert.equal(summary.p50Ms, 2000);
  assert.equal(summary.p95Ms, 4000);
  assert.deepEqual(summary.warnings.map(({ block }) => block), [3, 4]);
  assert.deepEqual(summary.violations.map(({ block }) => block), [5]);
  const empty = summarizeBlockSamples([]);
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

test("requires an internally consistent migration completion state", () => {
  assert.deepEqual(evaluateMigrationGate(false, false, false), {
    kind: "waiting",
    sawCursor: false,
  });
  assert.deepEqual(evaluateMigrationGate(true, false, false), {
    kind: "waiting",
    sawCursor: true,
  });
  assert.deepEqual(evaluateMigrationGate(false, true, true), {
    kind: "complete",
    sawCursor: true,
  });
  assert.equal(evaluateMigrationGate(true, true, true).kind, "invalid");
  assert.equal(evaluateMigrationGate(false, false, true).kind, "invalid");
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
