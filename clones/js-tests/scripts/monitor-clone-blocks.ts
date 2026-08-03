import fs from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import { parseArgs } from "node:util";
import { connectApi } from "../lib/api.js";
import {
  DEFAULT_BLOCK_FAILURE_MS,
  DEFAULT_BLOCK_WARNING_MS,
  COLLECT_HEAD_TIMEOUT_MS,
  DEFAULT_HEAD_TIMEOUT_MS,
  DEFAULT_HEAD_VIOLATION_MS,
  DEFAULT_HEAD_WARNING_MS,
  DEFAULT_MIN_BLOCK_SAMPLES,
  DEFAULT_SAMPLE_DRAIN_TIMEOUT_MS,
  BestHeadLiveness,
  NodeLogTail,
  bestHeadFailureReasons,
  blockLatencyFailureReasons,
  parsePreparedBlockChunk,
  postUpgradeBlockSamples,
  remainingRequiredBlockSamples,
  sampleDrainFailureReason,
  summarizeBlockSamples,
  type BlockConstructionSample,
} from "../lib/clone-performance.js";
import { createTempLogger } from "../lib/file-log.js";

type MonitorPolicy = "fail-fast" | "collect" | "baseline";

const MONITOR_POLICIES: Record<
  MonitorPolicy,
  {
    failImmediatelyOnLatency: boolean;
    enforceLatency: boolean;
    enforceStalls: boolean;
    headTimeoutMs: number;
  }
> = {
  "fail-fast": {
    failImmediatelyOnLatency: true,
    enforceLatency: true,
    enforceStalls: true,
    headTimeoutMs: DEFAULT_HEAD_TIMEOUT_MS,
  },
  collect: {
    failImmediatelyOnLatency: false,
    enforceLatency: true,
    enforceStalls: true,
    headTimeoutMs: COLLECT_HEAD_TIMEOUT_MS,
  },
  baseline: {
    failImmediatelyOnLatency: false,
    enforceLatency: false,
    enforceStalls: false,
    headTimeoutMs: COLLECT_HEAD_TIMEOUT_MS,
  },
};

interface RuntimeUpgradeActivation {
  upgradeBlock: number;
  finalizedAtEpochMs: number;
}

interface MonitorArguments {
  policy: MonitorPolicy;
  nodeLog: string;
  startOffset: number;
  reportFile: string;
  logName: string;
  activationReport?: string;
  readyFile?: string;
  baselineReport?: string;
  diagnosticFile?: string;
}

interface MonitorReport {
  schemaVersion: 2;
  status: "passed" | "failed";
  policy: MonitorPolicy;
  startedAt: string;
  finishedAt: string;
  nodeLog: string;
  startOffset: number;
  activation: RuntimeUpgradeActivation | null;
  thresholds: {
    blockWarningMs: number;
    blockFailureMs: number;
    headWarningMs: number;
    headViolationMs: number;
    headTimeoutMs: number;
    minimumSamples: number;
    sampleDrainTimeoutMs: number;
  };
  bestHead: {
    lastBlock: number | null;
    stalls: ReturnType<BestHeadLiveness["getStalls"]>;
  };
  latency: ReturnType<typeof summarizeBlockSamples>;
  baselineComparison: LatencyComparison | null;
  diagnosticFile: string | null;
  failureReasons: string[];
}

interface LatencyComparison {
  baselineSamples: number;
  currentSamples: number;
  meanMs: MetricComparison;
  p50Ms: MetricComparison;
  p95Ms: MetricComparison;
  p99Ms: MetricComparison;
  maximumMs: MetricComparison;
}

interface MetricComparison {
  baseline: number | null;
  current: number | null;
  deltaMs: number | null;
  ratio: number | null;
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger(args.logName);
  await logger.start();
  logger.captureConsole();

  const startedAt = new Date();
  const samples: BlockConstructionSample[] = [];
  const pendingSamples: BlockConstructionSample[] = [];
  const pendingHeads: Array<{ block: number; observedAtMs: number }> = [];
  const failureReasons: string[] = [];
  const tail = new NodeLogTail(args.nodeLog, args.startOffset);
  const policy = MONITOR_POLICIES[args.policy];
  const headTimeoutMs = policy.headTimeoutMs;
  const liveness = new BestHeadLiveness(
    DEFAULT_HEAD_WARNING_MS,
    DEFAULT_HEAD_VIOLATION_MS,
    headTimeoutMs,
  );
  let shutdownRequested = false;
  let shutdownRequestedAtMs: number | undefined;
  let shutdownNoticePrinted = false;
  let disconnecting = false;
  let fatalError: Error | undefined;
  let activation: RuntimeUpgradeActivation | undefined;
  let activated = args.activationReport === undefined;
  let api;
  let unsubscribe: (() => void) | undefined;

  const requestStop = () => {
    shutdownRequested = true;
    shutdownRequestedAtMs ??= Date.now();
  };
  process.once("SIGINT", requestStop);
  process.once("SIGTERM", requestStop);

  const acceptSamples = (newSamples: BlockConstructionSample[]) => {
    for (const sample of newSamples) {
      samples.push(sample);
      if (sample.durationMs >= DEFAULT_BLOCK_FAILURE_MS) {
        const message =
          `block ${sample.block} construction took ${sample.durationMs}ms ` +
          `(limit ${DEFAULT_BLOCK_FAILURE_MS}ms)`;
        console.error(`LATENCY VIOLATION: ${message}`);
        if (policy.failImmediatelyOnLatency && fatalError === undefined) {
          fatalError = new Error(message);
        }
      } else if (sample.durationMs >= DEFAULT_BLOCK_WARNING_MS) {
        console.log(
          `LATENCY WARNING: block ${sample.block} construction took ${sample.durationMs}ms`,
        );
      }
    }
  };

  const observeHead = (block: number, observedAtMs: number) => {
    const recovered = liveness.observe(block, observedAtMs);
    if (recovered !== undefined) {
      console.log(
        `BEST HEAD RECOVERED: block ${block} after ${recovered.durationMs}ms`,
      );
    }
  };

  const activate = (value: RuntimeUpgradeActivation) => {
    activation = value;
    activated = true;
    const boundary = pendingHeads.find(({ block }) => block === value.upgradeBlock);
    observeHead(value.upgradeBlock, boundary?.observedAtMs ?? value.finalizedAtEpochMs);
    for (const observation of pendingHeads) {
      if (observation.block > value.upgradeBlock) {
        observeHead(observation.block, observation.observedAtMs);
      }
    }
    acceptSamples(postUpgradeBlockSamples(pendingSamples, value.upgradeBlock));
    pendingHeads.length = 0;
    pendingSamples.length = 0;
    console.log(`Block monitor activated after runtime upgrade block ${value.upgradeBlock}.`);
  };

  try {
    console.log(
      `Starting clone block monitor policy=${args.policy} node_log=${args.nodeLog} ` +
        `offset=${args.startOffset}`,
    );
    api = await connectApi(process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944", {
      log: (message) => console.log(message),
    });

    const initialHeader = await api.rpc.chain.getHeader();
    if (activated) {
      observeHead(initialHeader.number.toNumber(), Date.now());
    } else {
      pendingHeads.push({ block: initialHeader.number.toNumber(), observedAtMs: Date.now() });
    }
    console.log(`Initial best block: ${initialHeader.number.toNumber()}`);

    const markRpcFailure = (value: unknown) => {
      if (disconnecting || fatalError !== undefined) {
        return;
      }
      fatalError = value instanceof Error ? value : new Error(String(value));
    };
    api.on("disconnected", () => markRpcFailure(new Error("clone RPC disconnected")));
    api.on("error", markRpcFailure);

    unsubscribe = await api.rpc.chain.subscribeNewHeads((header) => {
      try {
        const observation = { block: header.number.toNumber(), observedAtMs: Date.now() };
        if (activated) {
          observeHead(observation.block, observation.observedAtMs);
        } else {
          pendingHeads.push(observation);
        }
      } catch (error) {
        markRpcFailure(error);
      }
    });
    if (args.readyFile) {
      writeJson(args.readyFile, {
        schemaVersion: 1,
        readyAt: new Date().toISOString(),
        initialBestBlock: initialHeader.number.toNumber(),
      });
    }

    while (fatalError === undefined) {
      const newSamples = tail.read();
      if (activated) {
        acceptSamples(newSamples);
      } else {
        pendingSamples.push(...newSamples);
        const discovered = readActivationReport(args.activationReport);
        if (discovered !== undefined) {
          activate(discovered);
        }
      }
      if (fatalError !== undefined) {
        break;
      }

      if (!activated) {
        if (shutdownRequested) {
          fatalError = new Error("block monitor stopped before runtime upgrade activation");
          break;
        }
        await delay(50);
        continue;
      }

      const missingSamples = remainingRequiredBlockSamples(samples.length);
      if (shutdownRequested && missingSamples === 0) {
        break;
      }
      if (shutdownRequested && !shutdownNoticePrinted) {
        shutdownNoticePrinted = true;
        console.log(
          `Graceful shutdown requested; waiting for ${missingSamples} more proposer sample(s).`,
        );
      }
      if (shutdownRequestedAtMs !== undefined) {
        const drainFailure = sampleDrainFailureReason(
          samples.length,
          Date.now() - shutdownRequestedAtMs,
        );
        if (drainFailure !== undefined) {
          fatalError = new Error(drainFailure);
          break;
        }
      }

      const tick = liveness.tick(Date.now());
      if (tick.kind === "warning") {
        console.log(
          `BEST HEAD WARNING: no new best head for ${tick.stall.durationMs}ms ` +
            `after block ${tick.stall.afterBlock}`,
        );
      } else if (tick.kind === "abort") {
        fatalError = new Error(
          `no new best head for ${tick.stall.durationMs}ms after block ${tick.stall.afterBlock}`,
        );
        break;
      } else if (tick.kind === "violation") {
        console.error(
          `BEST HEAD VIOLATION: no new best head for ${tick.stall.durationMs}ms ` +
            `after block ${tick.stall.afterBlock}; continuing until recovery or ` +
            `${headTimeoutMs}ms hard timeout`,
        );
      }

      await delay(200);
    }

    const finalSamples = tail.read(true);
    if (activated && activation !== undefined) {
      acceptSamples(postUpgradeBlockSamples(finalSamples, activation.upgradeBlock));
    } else if (activated) {
      acceptSamples(finalSamples);
    }
  } catch (error) {
    fatalError = error instanceof Error ? error : new Error(String(error));
  } finally {
    disconnecting = true;
    try {
      unsubscribe?.();
    } catch {
      // The report remains authoritative even if an already-dead RPC cannot unsubscribe.
    }
    await api?.disconnect().catch(() => undefined);
    process.removeListener("SIGINT", requestStop);
    process.removeListener("SIGTERM", requestStop);
  }

  if (fatalError !== undefined) {
    failureReasons.push(fatalError.message);
  }

  const latency = summarizeBlockSamples(samples);
  if (policy.enforceLatency) {
    failureReasons.push(
      ...blockLatencyFailureReasons(latency, DEFAULT_MIN_BLOCK_SAMPLES),
    );
  } else if (latency.sampleCount < DEFAULT_MIN_BLOCK_SAMPLES) {
    failureReasons.push(
      ...blockLatencyFailureReasons(latency, DEFAULT_MIN_BLOCK_SAMPLES).filter((reason) =>
        reason.startsWith("observed "),
      ),
    );
  }
  const stalls = liveness.getStalls();
  if (policy.enforceStalls) {
    failureReasons.push(...bestHeadFailureReasons(stalls));
  }
  let baselineComparison: LatencyComparison | null = null;
  try {
    baselineComparison = readBaselineComparison(args.baselineReport, latency);
  } catch (error) {
    failureReasons.push(error instanceof Error ? error.message : String(error));
  }
  if (args.diagnosticFile) {
    try {
      await writeDiagnosticContext(
        args.nodeLog,
        args.diagnosticFile,
        latency.slowestBlocks.slice(0, 10),
      );
    } catch (error) {
      console.error(
        `Unable to write slow-block diagnostic context: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }

  const report: MonitorReport = {
    schemaVersion: 2,
    status: failureReasons.length === 0 ? "passed" : "failed",
    policy: args.policy,
    startedAt: startedAt.toISOString(),
    finishedAt: new Date().toISOString(),
    nodeLog: args.nodeLog,
    startOffset: args.startOffset,
    activation: activation ?? null,
    thresholds: {
      blockWarningMs: DEFAULT_BLOCK_WARNING_MS,
      blockFailureMs: DEFAULT_BLOCK_FAILURE_MS,
      headWarningMs: DEFAULT_HEAD_WARNING_MS,
      headViolationMs: DEFAULT_HEAD_VIOLATION_MS,
      headTimeoutMs,
      minimumSamples: DEFAULT_MIN_BLOCK_SAMPLES,
      sampleDrainTimeoutMs: DEFAULT_SAMPLE_DRAIN_TIMEOUT_MS,
    },
    bestHead: {
      lastBlock: liveness.getLastBlock() ?? null,
      stalls,
    },
    latency,
    baselineComparison,
    diagnosticFile: args.diagnosticFile ?? null,
    failureReasons,
  };

  writeJson(args.reportFile, report);
  appendStepSummary(report);
  console.log(
    `Clone block monitor ${report.status}: samples=${latency.sampleCount} ` +
      `mean_ms=${latency.meanMs ?? "none"} max_ms=${latency.maximumMs ?? "none"} ` +
      `warnings=${latency.warnings.length} ` +
      `violations=${latency.violations.length} stalls=${report.bestHead.stalls.length}`,
  );
  if (failureReasons.length > 0) {
    for (const reason of failureReasons) {
      console.error(`BLOCK MONITOR FAILURE: ${reason}`);
    }
    process.exitCode = 1;
  }
  await logger.flush();
}

function parseArguments(values: string[]): MonitorArguments {
  const { values: options } = parseArgs({
    args: values,
    allowPositionals: false,
    strict: true,
    options: {
      policy: { type: "string" },
      "node-log": { type: "string" },
      "start-offset": { type: "string" },
      report: { type: "string" },
      "log-name": { type: "string" },
      "activation-report": { type: "string" },
      "ready-file": { type: "string" },
      "baseline-report": { type: "string" },
      "diagnostic-file": { type: "string" },
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

  const policy = required("policy");
  if (policy !== "fail-fast" && policy !== "collect" && policy !== "baseline") {
    throw new Error(`invalid --policy: ${policy}`);
  }
  const logName = required("log-name");
  if (path.basename(logName) !== logName) {
    throw new Error(`--log-name must be a filename: ${logName}`);
  }

  return {
    policy,
    nodeLog: path.resolve(required("node-log")),
    startOffset: integer("start-offset", 0),
    reportFile: path.resolve(required("report")),
    logName,
    activationReport: options["activation-report"]
      ? path.resolve(options["activation-report"])
      : undefined,
    readyFile: options["ready-file"] ? path.resolve(options["ready-file"]) : undefined,
    baselineReport: options["baseline-report"]
      ? path.resolve(options["baseline-report"])
      : undefined,
    diagnosticFile: options["diagnostic-file"]
      ? path.resolve(options["diagnostic-file"])
      : undefined,
  };
}

function writeJson(filename: string, value: unknown) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, `${JSON.stringify(value, bigintJson, 2)}\n`);
}

function readActivationReport(filename?: string): RuntimeUpgradeActivation | undefined {
  if (filename === undefined || !fs.existsSync(filename)) {
    return undefined;
  }
  const value: unknown = JSON.parse(fs.readFileSync(filename, "utf8"));
  if (!isRecord(value)) {
    throw new Error(`invalid runtime upgrade report: ${filename}`);
  }
  const upgradeBlock = value.upgradeBlock;
  const finalizedAtEpochMs = value.finalizedAtEpochMs;
  if (!isNonnegativeSafeInteger(upgradeBlock)) {
    throw new Error(`invalid runtime upgrade block in ${filename}`);
  }
  if (!isPositiveSafeInteger(finalizedAtEpochMs)) {
    throw new Error(`invalid runtime upgrade finalization time in ${filename}`);
  }
  return {
    upgradeBlock,
    finalizedAtEpochMs,
  };
}

function readBaselineComparison(
  filename: string | undefined,
  current: ReturnType<typeof summarizeBlockSamples>,
): LatencyComparison | null {
  if (filename === undefined) {
    return null;
  }
  const report: unknown = JSON.parse(fs.readFileSync(filename, "utf8"));
  if (!isRecord(report) || !isRecord(report.latency)) {
    throw new Error(`invalid baseline block monitor report: ${filename}`);
  }
  const latency = report.latency;
  const sampleCount = latency.sampleCount;
  if (!isNonnegativeSafeInteger(sampleCount)) {
    throw new Error(`invalid baseline sample count in ${filename}`);
  }
  const meanMs = nullableMetric(latency.meanMs, "meanMs", filename);
  const p50Ms = nullableMetric(latency.p50Ms, "p50Ms", filename);
  const p95Ms = nullableMetric(latency.p95Ms, "p95Ms", filename);
  const p99Ms = nullableMetric(latency.p99Ms, "p99Ms", filename);
  const maximumMs = nullableMetric(latency.maximumMs, "maximumMs", filename);
  return {
    baselineSamples: sampleCount,
    currentSamples: current.sampleCount,
    meanMs: compareMetric(meanMs, current.meanMs),
    p50Ms: compareMetric(p50Ms, current.p50Ms),
    p95Ms: compareMetric(p95Ms, current.p95Ms),
    p99Ms: compareMetric(p99Ms, current.p99Ms),
    maximumMs: compareMetric(maximumMs, current.maximumMs),
  };
}

function nullableMetric(value: unknown, name: string, filename: string): number | null {
  if (value === null) {
    return null;
  }
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new Error(`invalid baseline ${name} in ${filename}`);
  }
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isPositiveSafeInteger(value: unknown): value is number {
  return isNonnegativeSafeInteger(value) && value > 0;
}

function compareMetric(baseline: number | null, current: number | null): MetricComparison {
  return {
    baseline,
    current,
    deltaMs:
      baseline === null || current === null
        ? null
        : Math.round((current - baseline) * 10) / 10,
    ratio:
      baseline === null || baseline === 0 || current === null
        ? null
        : Math.round((current / baseline) * 1_000) / 1_000,
  };
}

async function writeDiagnosticContext(
  nodeLog: string,
  filename: string,
  slowestBlocks: readonly BlockConstructionSample[],
) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  if (slowestBlocks.length === 0) {
    fs.writeFileSync(filename, "No proposer samples were observed.\n");
    return;
  }

  const targets = new Set(slowestBlocks.map(({ block }) => block));
  const contexts = new Map<number, string[]>();
  const previousLines: string[] = [];
  const active = new Map<number, number>();
  const lines = createInterface({
    input: fs.createReadStream(nodeLog, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });

  for await (const line of lines) {
    for (const [block, remaining] of active) {
      contexts.get(block)?.push(line);
      if (remaining === 1) {
        active.delete(block);
      } else {
        active.set(block, remaining - 1);
      }
    }

    const parsed = parsePreparedBlockChunk("", `${line}\n`).samples[0];
    if (parsed !== undefined && targets.has(parsed.block) && !contexts.has(parsed.block)) {
      contexts.set(parsed.block, [...previousLines, line]);
      active.set(parsed.block, 5);
    }
    previousLines.push(line);
    if (previousLines.length > 5) {
      previousLines.shift();
    }
  }

  const sections = slowestBlocks.map(({ block, durationMs }) => {
    const context = contexts.get(block) ?? ["(proposer line was not found in the retained node log)"];
    return [`===== block ${block} (${durationMs}ms) =====`, ...context].join("\n");
  });
  fs.writeFileSync(filename, `${sections.join("\n\n")}\n`);
}

function appendStepSummary(report: MonitorReport) {
  const filename = process.env.GITHUB_STEP_SUMMARY;
  if (!filename) {
    return;
  }
  const slowest = report.latency.slowestBlocks
    .slice(0, 10)
    .map(({ block, durationMs }) => `#${block}: ${durationMs}ms`)
    .join(", ");
  const comparison = report.baselineComparison;
  const comparisonLines = comparison
    ? [
        `- Baseline / candidate samples: ${comparison.baselineSamples} / ${comparison.currentSamples}`,
        `- Mean delta / ratio: ${formatComparison(comparison.meanMs)}`,
        `- p50 delta / ratio: ${formatComparison(comparison.p50Ms)}`,
        `- p95 delta / ratio: ${formatComparison(comparison.p95Ms)}`,
        `- p99 delta / ratio: ${formatComparison(comparison.p99Ms)}`,
        `- Maximum delta / ratio: ${formatComparison(comparison.maximumMs)}`,
      ]
    : [];
  fs.appendFileSync(
    filename,
    [
      "### Clone block performance",
      `- Status: **${report.status}**`,
      `- Samples: ${report.latency.sampleCount}`,
      `- Mean: ${report.latency.meanMs ?? "n/a"}ms`,
      `- Maximum: ${report.latency.maximumMs ?? "n/a"}ms`,
      `- p50 / p95 / p99: ${report.latency.p50Ms ?? "n/a"} / ${report.latency.p95Ms ?? "n/a"} / ${report.latency.p99Ms ?? "n/a"} ms`,
      `- Warnings / violations: ${report.latency.warnings.length} / ${report.latency.violations.length}`,
      `- Best-head stalls: ${report.bestHead.stalls.length}`,
      `- Slowest blocks: ${slowest || "none"}`,
      ...comparisonLines,
      report.diagnosticFile
        ? `- Slow-block diagnostic context: ${path.basename(report.diagnosticFile)}`
        : "",
      "",
    ]
      .filter((line) => line !== "")
      .concat("")
      .join("\n"),
  );
}

function formatComparison(value: MetricComparison): string {
  const delta =
    value.deltaMs === null
      ? "n/a"
      : `${value.deltaMs >= 0 ? "+" : ""}${value.deltaMs}ms`;
  const ratio = value.ratio === null ? "n/a" : `${value.ratio}x`;
  return `${delta} / ${ratio}`;
}

function bigintJson(_key: string, value: unknown) {
  return typeof value === "bigint" ? value.toString() : value;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
