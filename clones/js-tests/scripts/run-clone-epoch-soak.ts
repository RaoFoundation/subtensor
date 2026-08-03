import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import type { ApiPromise } from "@polkadot/api";
import { connectApi } from "../lib/api.js";
import {
  waitForBetaBasketV2ReleaseReadiness,
  type BetaBasketV2ReadinessObservation,
} from "../lib/clone-readiness.js";
import {
  assertIssuanceMirror,
  readIssuanceMirror,
  serializeIssuanceMirror,
} from "../lib/clone-invariants.js";
import {
  ACCELERATED_SEALING_MS,
  assertReliableEpochPollingGap,
  computeEpochCoverageBudget,
  SuccessfulEpochTracker,
  evaluateEpochCoverage,
  type EpochBaseline,
  type EpochCoverageBudget,
  type EpochCoverageEvaluation,
} from "../lib/clone-performance.js";
import { createTempLogger } from "../lib/file-log.js";

const ROOT_NETUID = 0;
const POLL_INTERVAL_MS = 1_000;
const COVERAGE_POLL_BLOCKS = 10;
const EXPECTED_CHAIN_BLOCK_TIME_MS = 12_000;
type ReleaseGate = "none" | "beta-basket-v2";

interface SoakArguments {
  epochCycles: number;
  releaseGate: ReleaseGate;
  upgradeBlock: number;
  minimumPostUpgradeBlocks: number;
  deadlineEpochMs: number;
  reportFile: string;
  logName: string;
}

interface EpochSoakReport {
  schemaVersion: 3;
  status: "running" | "passed" | "failed";
  startedAt: string;
  finishedAt?: string;
  requestedCycles: number;
  releaseGate: ReleaseGate;
  upgradeBlock: number;
  minimumPostUpgradeBlocks: number;
  deadlineEpochMs: number;
  betaBasketV2Readiness?: BetaBasketV2ReadinessObservation;
  baselineBlock?: number;
  completionBlock?: number;
  budget?: EpochCoverageBudget;
  coverageWindow?: {
    startedAtEpochMs: number;
    deadlineEpochMs: number;
    availableWallMs: number;
    nominalWallMs: number;
  };
  chainTiming?: {
    baselineTimestampMs: string;
    completionTimestampMs: string;
    elapsedMs: string;
    expectedElapsedMs: string;
    millisecondsPerBlock: number;
  };
  baseline?: Array<{
    netuid: number;
    tempo: number;
    epochIndex: string;
    lastSuccessfulEpochBlock: string;
  }>;
  coverage?: Array<{
    netuid: number;
    tempo: number;
    baselineEpochIndex: string;
    currentEpochIndex: string;
    baselineSuccessfulEpochBlock: string;
    currentSuccessfulEpochBlock: string;
    attemptedCycles: string;
    completedCycles: string;
  }>;
  invariants?: {
    baseline: ReturnType<typeof serializeIssuanceMirror>;
    completion?: ReturnType<typeof serializeIssuanceMirror>;
  };
  failure?: string;
}

type ChainHash = Awaited<ReturnType<typeof bestHeader>>["hash"];

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger(args.logName);
  await logger.start();
  logger.captureConsole();

  const report: EpochSoakReport = {
    schemaVersion: 3,
    status: "running",
    startedAt: new Date().toISOString(),
    requestedCycles: args.epochCycles,
    releaseGate: args.releaseGate,
    upgradeBlock: args.upgradeBlock,
    minimumPostUpgradeBlocks: args.minimumPostUpgradeBlocks,
    deadlineEpochMs: args.deadlineEpochMs,
  };
  let api: ApiPromise | undefined;

  try {
    api = await connectApi(process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944", {
      log: (message) => console.log(message),
    });
    if (args.releaseGate === "beta-basket-v2") {
      report.betaBasketV2Readiness = await waitForBetaBasketV2ReleaseReadiness(api, {
        deadlineEpochMs: args.deadlineEpochMs,
        log: (message) => console.log(message),
      });
      console.log(
        `Beta-basket v2 release gate resolved at block ${report.betaBasketV2Readiness.readinessBlock}; ` +
          `mode=${report.betaBasketV2Readiness.mode} ` +
          `saw_cursor=${report.betaBasketV2Readiness.sawCursor} ` +
          `saw_deferred=${report.betaBasketV2Readiness.sawDeferredEntries}`,
      );
    } else {
      console.log("Release-specific migration gate disabled for pre-upgrade baseline.");
    }

    const baselineHeader = await bestHeader(api);
    const baselineBlock = baselineHeader.number.toNumber();
    const baseline = await readEpochBaseline(api, baselineHeader.hash);
    const minimumTempo = Math.min(...baseline.map(({ tempo }) => tempo));
    if (minimumTempo <= COVERAGE_POLL_BLOCKS) {
      throw new Error(
        `minimum subnet tempo ${minimumTempo} is too short for ${COVERAGE_POLL_BLOCKS}-block ` +
          "successful-epoch polling",
      );
    }
    const successTracker = new SuccessfulEpochTracker(baseline);
    const atBaseline = await api.at(baselineHeader.hash);
    const maxEpochsPerBlock = Number(
      (await atBaseline.query.subtensorModule.maxEpochsPerBlock()).toString(),
    );
    const baselineTimestampMs = BigInt((await atBaseline.query.timestamp.now()).toString());
    const baselineIssuance = await readIssuanceMirror(api, baselineHeader.hash);
    assertIssuanceMirror(baselineIssuance, "epoch soak baseline");
    const budget = computeEpochCoverageBudget(baseline, args.epochCycles, maxEpochsPerBlock);
    const coverageStartedAtEpochMs = Date.now();
    const remainingWallMs = Math.max(0, args.deadlineEpochMs - coverageStartedAtEpochMs);
    const minimumCompletionBlock = args.upgradeBlock + args.minimumPostUpgradeBlocks;
    const remainingMinimumBlocks = Math.max(0, minimumCompletionBlock - baselineBlock);
    const requiredCoverageBlocks = Math.max(budget.blockBudget, remainingMinimumBlocks);
    const nominalRequiredWallMs = requiredCoverageBlocks * ACCELERATED_SEALING_MS;
    if (nominalRequiredWallMs > remainingWallMs) {
      throw new Error(
        `epoch coverage cannot fit: nominal ${formatDuration(nominalRequiredWallMs)} exceeds ` +
          `${formatDuration(Math.max(0, remainingWallMs))} remaining`,
      );
    }
    const coverageDeadlineEpochMs = args.deadlineEpochMs;

    report.baselineBlock = baselineBlock;
    report.budget = budget;
    report.coverageWindow = {
      startedAtEpochMs: coverageStartedAtEpochMs,
      deadlineEpochMs: coverageDeadlineEpochMs,
      availableWallMs: remainingWallMs,
      nominalWallMs: nominalRequiredWallMs,
    };
    report.baseline = baseline.map(({ netuid, tempo, epochIndex, lastSuccessfulEpochBlock }) => ({
      netuid,
      tempo,
      epochIndex: epochIndex.toString(),
      lastSuccessfulEpochBlock: lastSuccessfulEpochBlock.toString(),
    }));
    report.invariants = { baseline: serializeIssuanceMirror(baselineIssuance) };
    writeJson(args.reportFile, report);

    console.log(
      `Epoch baseline: block=${baselineBlock} active_non_root=${baseline.length} ` +
        `max_tempo=${budget.maxTempo} max_epochs_per_block=${maxEpochsPerBlock} ` +
        `cycles=${args.epochCycles} block_budget=${budget.blockBudget} ` +
        `minimum_post_upgrade_blocks=${args.minimumPostUpgradeBlocks} ` +
        `minimum_completion_block=${minimumCompletionBlock} ` +
        `nominal=${formatDuration(nominalRequiredWallMs)} ` +
        `operational_window=${formatDuration(remainingWallMs)}`,
    );

    const blockDeadline = baselineBlock + budget.blockBudget;
    let lastBlock = baselineBlock;
    let lastCoverageBlock = baselineBlock - COVERAGE_POLL_BLOCKS;
    let lastCompletedSubnets = -1;
    let lastReportBlock = baselineBlock - 100;

    for (;;) {
      ensureWallDeadline(coverageDeadlineEpochMs, "epoch coverage wall");
      const header = await bestHeader(api);
      const block = header.number.toNumber();
      if (block === lastBlock) {
        await delay(POLL_INTERVAL_MS);
        continue;
      }
      if (block < lastBlock) {
        throw new Error(`best block regressed from ${lastBlock} to ${block}`);
      }
      lastBlock = block;
      if (block < lastCoverageBlock + COVERAGE_POLL_BLOCKS) {
        await delay(POLL_INTERVAL_MS);
        continue;
      }
      assertReliableEpochPollingGap(lastCoverageBlock, block, minimumTempo);
      lastCoverageBlock = block;

      const snapshot = await readCoverageAt(api, header.hash, baseline);
      const successfulCycles = successTracker.observe(snapshot.currentSuccessfulEpochBlocks);
      const evaluation = evaluateEpochCoverage(
        baseline,
        snapshot.currentEpochIndices,
        snapshot.currentSuccessfulEpochBlocks,
        successfulCycles,
        snapshot.activeNetuids,
        args.epochCycles,
      );
      assertCoverageState(evaluation, block);
      const completedSubnets = evaluation.progress.filter(
        ({ completedCycles }) => completedCycles >= BigInt(args.epochCycles),
      ).length;

      if (
        evaluation.complete ||
        completedSubnets !== lastCompletedSubnets ||
        block >= lastReportBlock + 100
      ) {
        lastCompletedSubnets = completedSubnets;
        lastReportBlock = block;
        report.coverage = serializeCoverage(evaluation);
        writeJson(args.reportFile, report);
        console.log(
          `Epoch coverage: block=${block} completed=${completedSubnets}/${baseline.length} ` +
            `elapsed_blocks=${block - baselineBlock}/${budget.blockBudget}`,
        );
      }

      const minimumBlockCoverageComplete = block >= minimumCompletionBlock;
      if (evaluation.complete && minimumBlockCoverageComplete) {
        const completed = await api.at(header.hash);
        const completionTimestampMs = BigInt((await completed.query.timestamp.now()).toString());
        const elapsedMs = completionTimestampMs - baselineTimestampMs;
        const expectedElapsedMs = BigInt(
          (block - baselineBlock) * EXPECTED_CHAIN_BLOCK_TIME_MS,
        );
        if (elapsedMs !== expectedElapsedMs) {
          throw new Error(
            `accelerated clone changed chain-time semantics: elapsed_ms=${elapsedMs} ` +
              `expected_ms=${expectedElapsedMs} blocks=${block - baselineBlock}`,
          );
        }
        const completionIssuance = await readIssuanceMirror(api, header.hash);
        assertIssuanceMirror(completionIssuance, "epoch soak completion");
        report.status = "passed";
        report.finishedAt = new Date().toISOString();
        report.completionBlock = block;
        report.coverage = serializeCoverage(evaluation);
        report.chainTiming = {
          baselineTimestampMs: baselineTimestampMs.toString(),
          completionTimestampMs: completionTimestampMs.toString(),
          elapsedMs: elapsedMs.toString(),
          expectedElapsedMs: expectedElapsedMs.toString(),
          millisecondsPerBlock: EXPECTED_CHAIN_BLOCK_TIME_MS,
        };
        report.invariants = {
          baseline: serializeIssuanceMirror(baselineIssuance),
          completion: serializeIssuanceMirror(completionIssuance),
        };
        writeJson(args.reportFile, report);
        appendStepSummary(report);
        const completionScope =
          args.releaseGate === "none"
            ? "pre-upgrade baseline"
            : `${block - args.upgradeBlock} post-upgrade blocks observed`;
        console.log(
          `Epoch soak complete at block ${block}: every baseline subnet advanced ` +
            `${args.epochCycles} successful epoch(s); ${completionScope}.`,
        );
        return;
      }
      if (!evaluation.complete && block > blockDeadline) {
        throw new Error(
          `epoch coverage exceeded block budget at ${block}; baseline=${baselineBlock} ` +
            `budget=${budget.blockBudget} completed=${completedSubnets}/${baseline.length}`,
        );
      }
      await delay(POLL_INTERVAL_MS);
    }
  } catch (error) {
    const failure = error instanceof Error ? error.message : String(error);
    report.status = "failed";
    report.finishedAt = new Date().toISOString();
    report.failure = failure;
    writeJson(args.reportFile, report);
    appendStepSummary(report);
    console.error(`EPOCH SOAK FAILURE: ${failure}`);
    throw error;
  } finally {
    await api?.disconnect().catch(() => undefined);
    await logger.flush();
  }
}

async function readEpochBaseline(api: ApiPromise, hash: ChainHash): Promise<EpochBaseline[]> {
  const at = await api.at(hash);
  const query = at.query.subtensorModule;
  const entries = await query.networksAdded.entries();
  const netuids = entries
    .filter(([, isAdded]) => isAdded.toString() === "true")
    .map(([key]) => Number(key.args[0].toString()))
    .filter((netuid) => netuid !== ROOT_NETUID)
    .sort((left, right) => left - right);
  if (netuids.length === 0) {
    throw new Error("post-migration state has no active non-root subnets");
  }

  const [tempos, epochIndices, lastSuccessfulEpochBlocks] = await Promise.all([
    query.tempo.multi(netuids),
    query.subnetEpochIndex.multi(netuids),
    query.lastMechansimStepBlock.multi(netuids),
  ]);
  return netuids.map((netuid, index) => ({
    netuid,
    tempo: Number(tempos[index].toString()),
    epochIndex: BigInt(epochIndices[index].toString()),
    lastSuccessfulEpochBlock: BigInt(lastSuccessfulEpochBlocks[index].toString()),
  }));
}

async function readCoverageAt(
  api: ApiPromise,
  hash: ChainHash,
  baseline: readonly EpochBaseline[],
): Promise<{
  activeNetuids: Set<number>;
  currentEpochIndices: Map<number, bigint>;
  currentSuccessfulEpochBlocks: Map<number, bigint>;
}> {
  const at = await api.at(hash);
  const query = at.query.subtensorModule;
  const netuids = baseline.map(({ netuid }) => netuid);
  const [added, epochIndices, lastSuccessfulEpochBlocks] = await Promise.all([
    query.networksAdded.multi(netuids),
    query.subnetEpochIndex.multi(netuids),
    query.lastMechansimStepBlock.multi(netuids),
  ]);
  const activeNetuids = new Set<number>();
  const currentEpochIndices = new Map<number, bigint>();
  const currentSuccessfulEpochBlocks = new Map<number, bigint>();
  netuids.forEach((netuid, index) => {
    if (added[index].toString() === "true") {
      activeNetuids.add(netuid);
    }
    currentEpochIndices.set(netuid, BigInt(epochIndices[index].toString()));
    currentSuccessfulEpochBlocks.set(
      netuid,
      BigInt(lastSuccessfulEpochBlocks[index].toString()),
    );
  });
  return { activeNetuids, currentEpochIndices, currentSuccessfulEpochBlocks };
}

function assertCoverageState(evaluation: EpochCoverageEvaluation, block: number) {
  if (evaluation.removedNetuids.length > 0) {
    throw new Error(
      `baseline subnet(s) removed before coverage at block ${block}: ${evaluation.removedNetuids.join(",")}`,
    );
  }
  if (evaluation.missingNetuids.length > 0) {
    throw new Error(
      `baseline subnet epoch state missing at block ${block}: ${evaluation.missingNetuids.join(",")}`,
    );
  }
  if (evaluation.regressedNetuids.length > 0) {
    throw new Error(
      `subnet epoch index regressed at block ${block}: ${evaluation.regressedNetuids.join(",")}`,
    );
  }
  if (evaluation.skippedNetuids.length > 0) {
    throw new Error(
      `subnet epoch slot(s) advanced without successful execution at block ${block}: ` +
        evaluation.skippedNetuids.join(","),
    );
  }
}

function serializeCoverage(evaluation: EpochCoverageEvaluation) {
  return evaluation.progress.map((subnet) => ({
    netuid: subnet.netuid,
    tempo: subnet.tempo,
    baselineEpochIndex: subnet.epochIndex.toString(),
    currentEpochIndex: subnet.currentEpochIndex.toString(),
    baselineSuccessfulEpochBlock: subnet.lastSuccessfulEpochBlock.toString(),
    currentSuccessfulEpochBlock: subnet.currentSuccessfulEpochBlock.toString(),
    attemptedCycles: subnet.attemptedCycles.toString(),
    completedCycles: subnet.completedCycles.toString(),
  }));
}

async function bestHeader(api: ApiPromise) {
  return api.rpc.chain.getHeader();
}

function parseArguments(values: string[]): SoakArguments {
  const { values: options } = parseArgs({
    args: values,
    allowPositionals: false,
    strict: true,
    options: {
      "epoch-cycles": { type: "string" },
      "release-gate": { type: "string" },
      "upgrade-block": { type: "string" },
      "minimum-post-upgrade-blocks": { type: "string" },
      "deadline-epoch-ms": { type: "string" },
      report: { type: "string" },
      "log-name": { type: "string" },
    },
  });
  const required = (name: keyof typeof options): string => {
    const value = options[name];
    if (value === undefined || value.length === 0) {
      throw new Error(`missing --${name}`);
    }
    return value;
  };
  const integer = (name: keyof typeof options, minimum: number): number => {
    const raw = required(name);
    const value = Number(raw);
    if (!Number.isSafeInteger(value) || value < minimum) {
      throw new Error(`invalid --${name}: ${raw}`);
    }
    return value;
  };

  const epochCycles = integer("epoch-cycles", 1);
  if (epochCycles > 3) {
    throw new Error(`--epoch-cycles must be 1, 2, or 3; got ${epochCycles}`);
  }
  const releaseGate = required("release-gate");
  if (releaseGate !== "none" && releaseGate !== "beta-basket-v2") {
    throw new Error(`invalid --release-gate: ${releaseGate}`);
  }
  const logName = required("log-name");
  if (path.basename(logName) !== logName) {
    throw new Error(`--log-name must be a filename: ${logName}`);
  }
  return {
    epochCycles,
    releaseGate,
    upgradeBlock: integer("upgrade-block", 0),
    minimumPostUpgradeBlocks: integer("minimum-post-upgrade-blocks", 0),
    deadlineEpochMs: integer("deadline-epoch-ms", 1),
    reportFile: path.resolve(required("report")),
    logName,
  };
}

function ensureWallDeadline(deadlineEpochMs: number, label = "soak wall") {
  if (Date.now() >= deadlineEpochMs) {
    throw new Error(`${label} deadline ${new Date(deadlineEpochMs).toISOString()} was reached`);
  }
}

function writeJson(filename: string, report: EpochSoakReport) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, `${JSON.stringify(report, null, 2)}\n`);
}

function appendStepSummary(report: EpochSoakReport) {
  const filename = process.env.GITHUB_STEP_SUMMARY;
  if (!filename || report.status === "running") {
    return;
  }
  fs.appendFileSync(
    filename,
    `${[
      report.releaseGate === "none"
        ? "### Pre-upgrade epoch baseline"
        : "### Post-upgrade epoch soak",
      `- Status: **${report.status}**`,
      `- Requested cycles: ${report.requestedCycles}`,
      `- Release gate: ${report.releaseGate}`,
      `- Migration mode: ${report.betaBasketV2Readiness?.mode ?? "not-applicable"}`,
      `- Migration cursor completion block: ${report.betaBasketV2Readiness?.cursorCompletionBlock ?? "not-observed"}`,
      `- Fully writable block: ${report.betaBasketV2Readiness?.readinessBlock ?? "not-applicable"}`,
      `- Deferred release observed: ${report.betaBasketV2Readiness?.sawDeferredEntries ?? false}`,
      `- Baseline / completion block: ${report.baselineBlock ?? "n/a"} / ${report.completionBlock ?? "n/a"}`,
      `- Upgrade block / minimum post-upgrade blocks: ${report.upgradeBlock} / ${report.minimumPostUpgradeBlocks}`,
      `- Active non-root subnets: ${report.budget?.activeSubnets ?? "n/a"}`,
      `- Maximum tempo: ${report.budget?.maxTempo ?? "n/a"}`,
      `- Block budget: ${report.budget?.blockBudget ?? "n/a"}`,
      `- Nominal coverage / available window: ${
        report.coverageWindow
          ? `${formatDuration(report.coverageWindow.nominalWallMs)} / ${formatDuration(
              report.coverageWindow.availableWallMs,
            )}`
          : "n/a"
      }`,
      `- Verified chain block time: ${report.chainTiming?.millisecondsPerBlock ?? "n/a"}ms`,
      report.failure ? `- Failure: ${report.failure}` : "",
    ]
      .filter(Boolean)
      .join("\n")}\n\n`,
  );
}

function formatDuration(milliseconds: number): string {
  return `${(milliseconds / 60_000).toFixed(1)}m`;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
