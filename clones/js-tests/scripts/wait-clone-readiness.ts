import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import { connectApi } from "../lib/api.js";
import {
  waitForMigrationReadiness,
  type MigrationReadinessObservation,
} from "../lib/clone-readiness.js";
import { createTempLogger } from "../lib/file-log.js";

interface ReadinessArguments {
  label: string;
  timeoutMs: number;
  reportFile: string;
}

interface ReadinessReport {
  schemaVersion: 1;
  status: "passed" | "failed";
  startedAt: string;
  finishedAt: string;
  timeoutMs: number;
  observation?: MigrationReadinessObservation;
  failure?: string;
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const logger = createTempLogger(`clone-readiness-${args.label}.log`);
  await logger.start();
  logger.captureConsole();
  const startedAt = new Date();
  let api;
  let report: ReadinessReport;

  try {
    api = await connectApi(process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944", {
      log: (message) => console.log(message),
    });
    const observation = await waitForMigrationReadiness(api, {
      deadlineEpochMs: startedAt.getTime() + args.timeoutMs,
      log: (message) => console.log(message),
    });
    report = {
      schemaVersion: 1,
      status: "passed",
      startedAt: startedAt.toISOString(),
      finishedAt: new Date().toISOString(),
      timeoutMs: args.timeoutMs,
      observation,
    };
    console.log(
      `Clone readiness passed: mode=${observation.mode} start=${observation.startBlock} ` +
        `cursor_completion=${observation.cursorCompletionBlock ?? "not-observed"} ` +
        `ready=${observation.readinessBlock}`,
    );
  } catch (error) {
    const failure = error instanceof Error ? error.message : String(error);
    report = {
      schemaVersion: 1,
      status: "failed",
      startedAt: startedAt.toISOString(),
      finishedAt: new Date().toISOString(),
      timeoutMs: args.timeoutMs,
      failure,
    };
    console.error(`CLONE READINESS FAILURE: ${failure}`);
    process.exitCode = 1;
  } finally {
    await api?.disconnect().catch(() => undefined);
  }

  writeJson(args.reportFile, report);
  appendStepSummary(report);
  await logger.flush();
}

function parseArguments(values: string[]): ReadinessArguments {
  const { values: options } = parseArgs({
    args: values,
    allowPositionals: false,
    strict: true,
    options: {
      label: { type: "string" },
      "timeout-ms": { type: "string" },
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
  const label = required("label");
  if (!/^[a-z0-9-]+$/.test(label)) {
    throw new Error(`invalid --label: ${label}`);
  }
  const timeoutMs = Number(required("timeout-ms"));
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) {
    throw new Error(`invalid --timeout-ms: ${options["timeout-ms"]}`);
  }
  return {
    label,
    timeoutMs,
    reportFile: path.resolve(required("report")),
  };
}

function writeJson(filename: string, report: ReadinessReport) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, `${JSON.stringify(report, null, 2)}\n`);
}

function appendStepSummary(report: ReadinessReport) {
  const filename = process.env.GITHUB_STEP_SUMMARY;
  if (!filename) {
    return;
  }
  const observation = report.observation;
  fs.appendFileSync(
    filename,
    `${[
      "### Clone post-upgrade readiness",
      `- Status: **${report.status}**`,
      `- Migration mode: ${observation?.mode ?? "unresolved"}`,
      `- Migration cursor completion block: ${observation?.cursorCompletionBlock ?? "not-observed"}`,
      `- Fully writable block: ${observation?.readinessBlock ?? "unresolved"}`,
      `- Deferred release observed: ${observation?.sawDeferredEntries ?? false}`,
      report.failure ? `- Failure: ${report.failure}` : "",
    ]
      .filter(Boolean)
      .join("\n")}\n\n`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
