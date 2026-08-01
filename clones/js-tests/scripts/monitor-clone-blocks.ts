import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import { connectApi } from "../lib/api.js";
import {
  DEFAULT_BLOCK_FAILURE_MS,
  DEFAULT_BLOCK_WARNING_MS,
  DEFAULT_HEAD_TIMEOUT_MS,
  DEFAULT_HEAD_WARNING_MS,
  DEFAULT_MIN_BLOCK_SAMPLES,
  BestHeadLiveness,
  NodeLogTail,
  blockLatencyFailureReasons,
  remainingRequiredBlockSamples,
  summarizeBlockSamples,
  type BlockConstructionSample,
} from "../lib/clone-performance.js";
import { createTempLogger } from "../lib/file-log.js";

type MonitorPolicy = "fail-fast" | "collect";

interface MonitorArguments {
  policy: MonitorPolicy;
  nodeLog: string;
  startOffset: number;
  reportFile: string;
  logName: string;
}

interface MonitorReport {
  schemaVersion: 1;
  status: "passed" | "failed";
  policy: MonitorPolicy;
  startedAt: string;
  finishedAt: string;
  nodeLog: string;
  startOffset: number;
  thresholds: {
    blockWarningMs: number;
    blockFailureMs: number;
    headWarningMs: number;
    headTimeoutMs: number;
    minimumSamples: number;
  };
  bestHead: {
    lastBlock: number | null;
    stalls: ReturnType<BestHeadLiveness["getStalls"]>;
  };
  latency: ReturnType<typeof summarizeBlockSamples>;
  failureReasons: string[];
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger(args.logName);
  await logger.start();
  logger.captureConsole();

  const startedAt = new Date();
  const samples: BlockConstructionSample[] = [];
  const failureReasons: string[] = [];
  const tail = new NodeLogTail(args.nodeLog, args.startOffset);
  const liveness = new BestHeadLiveness();
  let shutdownRequested = false;
  let shutdownNoticePrinted = false;
  let disconnecting = false;
  let fatalError: Error | undefined;
  let api;
  let unsubscribe: (() => void) | undefined;

  const requestStop = () => {
    shutdownRequested = true;
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
        if (args.policy === "fail-fast" && fatalError === undefined) {
          fatalError = new Error(message);
        }
      } else if (sample.durationMs >= DEFAULT_BLOCK_WARNING_MS) {
        console.log(
          `LATENCY WARNING: block ${sample.block} construction took ${sample.durationMs}ms`,
        );
      }
    }
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
    liveness.observe(initialHeader.number.toNumber(), Date.now());
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
        const recovered = liveness.observe(header.number.toNumber(), Date.now());
        if (recovered !== undefined) {
          console.log(
            `BEST HEAD RECOVERED: block ${header.number.toNumber()} after ${recovered.durationMs}ms`,
          );
        }
      } catch (error) {
        markRpcFailure(error);
      }
    });

    while (fatalError === undefined) {
      acceptSamples(tail.read());
      if (fatalError !== undefined) {
        break;
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
      }

      await delay(200);
    }

    acceptSamples(tail.read(true));
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
  failureReasons.push(
    ...blockLatencyFailureReasons(latency, DEFAULT_MIN_BLOCK_SAMPLES),
  );

  const report: MonitorReport = {
    schemaVersion: 1,
    status: failureReasons.length === 0 ? "passed" : "failed",
    policy: args.policy,
    startedAt: startedAt.toISOString(),
    finishedAt: new Date().toISOString(),
    nodeLog: args.nodeLog,
    startOffset: args.startOffset,
    thresholds: {
      blockWarningMs: DEFAULT_BLOCK_WARNING_MS,
      blockFailureMs: DEFAULT_BLOCK_FAILURE_MS,
      headWarningMs: DEFAULT_HEAD_WARNING_MS,
      headTimeoutMs: DEFAULT_HEAD_TIMEOUT_MS,
      minimumSamples: DEFAULT_MIN_BLOCK_SAMPLES,
    },
    bestHead: {
      lastBlock: liveness.getLastBlock() ?? null,
      stalls: liveness.getStalls(),
    },
    latency,
    failureReasons,
  };

  writeJson(args.reportFile, report);
  appendStepSummary(report);
  console.log(
    `Clone block monitor ${report.status}: samples=${latency.sampleCount} ` +
      `max_ms=${latency.maximumMs ?? "none"} warnings=${latency.warnings.length} ` +
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
  if (policy !== "fail-fast" && policy !== "collect") {
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
  };
}

function writeJson(filename: string, value: unknown) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, `${JSON.stringify(value, bigintJson, 2)}\n`);
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
  fs.appendFileSync(
    filename,
    [
      "### Clone block performance",
      `- Status: **${report.status}**`,
      `- Samples: ${report.latency.sampleCount}`,
      `- Maximum: ${report.latency.maximumMs ?? "n/a"}ms`,
      `- p50 / p95 / p99: ${report.latency.p50Ms ?? "n/a"} / ${report.latency.p95Ms ?? "n/a"} / ${report.latency.p99Ms ?? "n/a"} ms`,
      `- Warnings / violations: ${report.latency.warnings.length} / ${report.latency.violations.length}`,
      `- Best-head stalls: ${report.bestHead.stalls.length}`,
      `- Slowest blocks: ${slowest || "none"}`,
      "",
    ].join("\n"),
  );
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
