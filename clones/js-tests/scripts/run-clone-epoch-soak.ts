import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import type { ApiPromise } from "@polkadot/api";
import { xxhashAsHex } from "@polkadot/util-crypto";
import { connectApi } from "../lib/api.js";
import {
  computeEpochCoverageBudget,
  evaluateEpochCoverage,
  evaluateMigrationGate,
  type EpochBaseline,
  type EpochCoverageBudget,
  type EpochCoverageEvaluation,
} from "../lib/clone-performance.js";
import { createTempLogger } from "../lib/file-log.js";

const ROOT_NETUID = 0;
const MIGRATION_NAME = "migrate_seed_beta_basket_v2";
const POLL_INTERVAL_MS = 1_000;
const COVERAGE_POLL_BLOCKS = 10;
const EXPECTED_CHAIN_BLOCK_TIME_MS = 12_000;

interface SoakArguments {
  epochCycles: number;
  deadlineEpochMs: number;
  reportFile: string;
}

interface MigrationObservation {
  sawCursor: boolean;
  startBlock: number;
  completionBlock: number;
}

interface EpochSoakReport {
  schemaVersion: 1;
  status: "running" | "passed" | "failed";
  startedAt: string;
  finishedAt?: string;
  requestedCycles: number;
  deadlineEpochMs: number;
  migration?: MigrationObservation;
  baselineBlock?: number;
  completionBlock?: number;
  budget?: EpochCoverageBudget;
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

type ChainHash = Awaited<ReturnType<typeof finalizedHeader>>["hash"];

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger("clone-epoch-soak.log");
  await logger.start();
  logger.captureConsole();

  const report: EpochSoakReport = {
    schemaVersion: 1,
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
    report.migration = await waitForMigration(api, args.deadlineEpochMs);
    console.log(
      `Migration completed at block ${report.migration.completionBlock}; ` +
        `saw_cursor=${report.migration.sawCursor}`,
    );

    const baselineHeader = await finalizedHeader(api);
    const baselineBlock = baselineHeader.number.toNumber();
    const baseline = await readEpochBaseline(api, baselineHeader.hash);
    const atBaseline = await api.at(baselineHeader.hash);
    const maxEpochsPerBlock = Number(
      (await atBaseline.query.subtensorModule.maxEpochsPerBlock()).toString(),
    );
    const baselineTimestampMs = BigInt((await atBaseline.query.timestamp.now()).toString());
    const budget = computeEpochCoverageBudget(baseline, args.epochCycles, maxEpochsPerBlock);
    const remainingWallMs = args.deadlineEpochMs - Date.now();
    if (budget.nominalWallMs > remainingWallMs) {
      throw new Error(
        `epoch coverage cannot fit: nominal ${formatDuration(budget.nominalWallMs)} exceeds ` +
          `${formatDuration(Math.max(0, remainingWallMs))} remaining`,
      );
    }

    report.baselineBlock = baselineBlock;
    report.budget = budget;
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
        `nominal=${formatDuration(budget.nominalWallMs)}`,
    );

    const blockDeadline = baselineBlock + budget.blockBudget;
    let lastBlock = baselineBlock;
    let lastCoverageBlock = baselineBlock - COVERAGE_POLL_BLOCKS;
    let lastCompletedSubnets = -1;
    let lastReportBlock = baselineBlock - 100;

    for (;;) {
      ensureWallDeadline(args.deadlineEpochMs);
      const header = await finalizedHeader(api);
      const block = header.number.toNumber();
      if (block === lastBlock) {
        await delay(POLL_INTERVAL_MS);
        continue;
      }
      if (block < lastBlock) {
        throw new Error(`finalized block regressed from ${lastBlock} to ${block}`);
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

async function waitForMigration(
  api: ApiPromise,
  deadlineEpochMs: number,
): Promise<MigrationObservation> {
  const start = await finalizedHeader(api);
  const startBlock = start.number.toNumber();
  const cursorKey = storagePrefix("SeedBetaBasketV2Migration");
  let sawCursor = false;
  let lastBlock = -1;
  let lastReportedBlock = startBlock - 50;

  for (;;) {
    ensureWallDeadline(deadlineEpochMs);
    const header = await finalizedHeader(api);
    const block = header.number.toNumber();
    if (block === lastBlock) {
      await delay(POLL_INTERVAL_MS);
      continue;
    }
    lastBlock = block;

    const [cursorExists, migrationRan] = await Promise.all([
      storageExistsAt(api, cursorKey, header.hash),
      hasMigrationRunAt(api, header.hash),
    ]);
    const gate = evaluateMigrationGate(cursorExists, migrationRan, sawCursor);
    sawCursor = gate.sawCursor;
    if (gate.kind === "complete") {
      return { sawCursor, startBlock, completionBlock: block };
    }
    if (gate.kind === "invalid") {
      throw new Error(`${gate.reason} at block ${block}`);
    }
    if (block >= lastReportedBlock + 50) {
      lastReportedBlock = block;
      console.log(
        `Waiting for migration: block=${block} cursor=${cursorExists} completed=${migrationRan}`,
      );
    }
    await delay(POLL_INTERVAL_MS);
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

async function finalizedHeader(api: ApiPromise) {
  const hash = await api.rpc.chain.getFinalizedHead();
  return api.rpc.chain.getHeader(hash);
}

async function hasMigrationRunAt(api: ApiPromise, hash: ChainHash): Promise<boolean> {
  const at = await api.at(hash);
  const value = await at.query.subtensorModule.hasMigrationRun([...Buffer.from(MIGRATION_NAME)]);
  return value.toString() === "true";
}

async function storageExistsAt(api: ApiPromise, key: string, hash: ChainHash): Promise<boolean> {
  const value = await api.rpc.state.getStorage(key, hash);
  if (value === null || typeof value !== "object" || !("isSome" in value)) {
    throw new Error(`unexpected storage response for ${key}`);
  }
  return value.isSome === true;
}

function storagePrefix(item: string): string {
  return `${xxhashAsHex("SubtensorModule", 128)}${xxhashAsHex(item, 128).slice(2)}`;
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

function ensureWallDeadline(deadlineEpochMs: number) {
  if (Date.now() >= deadlineEpochMs) {
    throw new Error(`soak wall deadline ${new Date(deadlineEpochMs).toISOString()} was reached`);
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
      `- Migration completion block: ${report.migration?.completionBlock ?? "unresolved"}`,
      `- Baseline / completion block: ${report.baselineBlock ?? "n/a"} / ${report.completionBlock ?? "n/a"}`,
      `- Active non-root subnets: ${report.budget?.activeSubnets ?? "n/a"}`,
      `- Maximum tempo: ${report.budget?.maxTempo ?? "n/a"}`,
      `- Block budget: ${report.budget?.blockBudget ?? "n/a"}`,
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
