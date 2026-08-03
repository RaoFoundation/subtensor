import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import type { ApiPromise } from "@polkadot/api";
import { connectApi } from "../lib/api.js";
import {
  waitForMigrationReadiness,
  type MigrationReadinessObservation,
} from "../lib/clone-readiness.js";
import {
  computeEpochCoverageBudget,
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

interface SoakArguments {
  epochCycles: number;
  deadlineEpochMs: number;
  reportFile: string;
}

interface EpochSoakReport {
  schemaVersion: 2;
  status: "running" | "passed" | "failed";
  startedAt: string;
  finishedAt?: string;
  requestedCycles: number;
  deadlineEpochMs: number;
  migration?: MigrationReadinessObservation;
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
  baseline?: Array<{ netuid: number; tempo: number; epochIndex: string }>;
  coverage?: Array<{
    netuid: number;
    tempo: number;
    baselineEpochIndex: string;
    currentEpochIndex: string;
    completedCycles: string;
  }>;
  failure?: string;
}

type ChainHash = Awaited<ReturnType<typeof bestHeader>>["hash"];

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger("clone-epoch-soak.log");
  await logger.start();
  logger.captureConsole();

  const report: EpochSoakReport = {
    schemaVersion: 2,
    status: "running",
    startedAt: new Date().toISOString(),
    requestedCycles: args.epochCycles,
    deadlineEpochMs: args.deadlineEpochMs,
  };
  let api: ApiPromise | undefined;

  try {
    api = await connectApi(process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944", {
      log: (message) => console.log(message),
    });
    report.migration = await waitForMigrationReadiness(api, {
      deadlineEpochMs: args.deadlineEpochMs,
      log: (message) => console.log(message),
    });
    console.log(
      `Migration gate resolved at block ${report.migration.readinessBlock}; ` +
        `mode=${report.migration.mode} saw_cursor=${report.migration.sawCursor} ` +
        `saw_deferred=${report.migration.sawDeferredEntries}`,
    );

    const baselineHeader = await bestHeader(api);
    const baselineBlock = baselineHeader.number.toNumber();
    const baseline = await readEpochBaseline(api, baselineHeader.hash);
    const atBaseline = await api.at(baselineHeader.hash);
    const maxEpochsPerBlock = Number(
      (await atBaseline.query.subtensorModule.maxEpochsPerBlock()).toString(),
    );
    const baselineTimestampMs = BigInt((await atBaseline.query.timestamp.now()).toString());
    const budget = computeEpochCoverageBudget(baseline, args.epochCycles, maxEpochsPerBlock);
    const coverageStartedAtEpochMs = Date.now();
    const remainingWallMs = Math.max(0, args.deadlineEpochMs - coverageStartedAtEpochMs);
    if (budget.nominalWallMs > remainingWallMs) {
      throw new Error(
        `epoch coverage cannot fit: nominal ${formatDuration(budget.nominalWallMs)} exceeds ` +
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
      nominalWallMs: budget.nominalWallMs,
    };
    report.baseline = baseline.map(({ netuid, tempo, epochIndex }) => ({
      netuid,
      tempo,
      epochIndex: epochIndex.toString(),
    }));
    writeJson(args.reportFile, report);

    console.log(
      `Epoch baseline: block=${baselineBlock} active_non_root=${baseline.length} ` +
        `max_tempo=${budget.maxTempo} max_epochs_per_block=${maxEpochsPerBlock} ` +
        `cycles=${args.epochCycles} block_budget=${budget.blockBudget} ` +
        `nominal=${formatDuration(budget.nominalWallMs)} ` +
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
      lastCoverageBlock = block;

      const evaluation = await readCoverageAt(api, header.hash, baseline, args.epochCycles);
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
        report.completionBlock = evaluation.complete ? block : undefined;
        report.coverage = serializeCoverage(evaluation);
        writeJson(args.reportFile, report);
        console.log(
          `Epoch coverage: block=${block} completed=${completedSubnets}/${baseline.length} ` +
            `elapsed_blocks=${block - baselineBlock}/${budget.blockBudget}`,
        );
      }

      if (evaluation.complete) {
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
        writeJson(args.reportFile, report);
        appendStepSummary(report);
        console.log(
          `Epoch soak complete at block ${block}: every baseline subnet advanced ` +
            `${args.epochCycles} epoch(s).`,
        );
        return;
      }
      if (block > blockDeadline) {
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

  const [tempos, epochIndices] = await Promise.all([
    query.tempo.multi(netuids),
    query.subnetEpochIndex.multi(netuids),
  ]);
  return netuids.map((netuid, index) => ({
    netuid,
    tempo: Number(tempos[index].toString()),
    epochIndex: BigInt(epochIndices[index].toString()),
  }));
}

async function readCoverageAt(
  api: ApiPromise,
  hash: ChainHash,
  baseline: readonly EpochBaseline[],
  requiredCycles: number,
): Promise<EpochCoverageEvaluation> {
  const at = await api.at(hash);
  const query = at.query.subtensorModule;
  const netuids = baseline.map(({ netuid }) => netuid);
  const [added, epochIndices] = await Promise.all([
    query.networksAdded.multi(netuids),
    query.subnetEpochIndex.multi(netuids),
  ]);
  const activeNetuids = new Set<number>();
  const currentEpochIndices = new Map<number, bigint>();
  netuids.forEach((netuid, index) => {
    if (added[index].toString() === "true") {
      activeNetuids.add(netuid);
    }
    currentEpochIndices.set(netuid, BigInt(epochIndices[index].toString()));
  });
  return evaluateEpochCoverage(baseline, currentEpochIndices, activeNetuids, requiredCycles);
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
}

function serializeCoverage(evaluation: EpochCoverageEvaluation) {
  return evaluation.progress.map((subnet) => ({
    netuid: subnet.netuid,
    tempo: subnet.tempo,
    baselineEpochIndex: subnet.epochIndex.toString(),
    currentEpochIndex: subnet.currentEpochIndex.toString(),
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
      "deadline-epoch-ms": { type: "string" },
      report: { type: "string" },
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
  return {
    epochCycles,
    deadlineEpochMs: integer("deadline-epoch-ms", 1),
    reportFile: path.resolve(required("report")),
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
      "### Post-upgrade epoch soak",
      `- Status: **${report.status}**`,
      `- Requested cycles: ${report.requestedCycles}`,
      `- Migration mode: ${report.migration?.mode ?? "unresolved"}`,
      `- Migration cursor completion block: ${report.migration?.cursorCompletionBlock ?? "not-observed"}`,
      `- Fully writable block: ${report.migration?.readinessBlock ?? "unresolved"}`,
      `- Deferred release observed: ${report.migration?.sawDeferredEntries ?? false}`,
      `- Baseline / completion block: ${report.baselineBlock ?? "n/a"} / ${report.completionBlock ?? "n/a"}`,
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
