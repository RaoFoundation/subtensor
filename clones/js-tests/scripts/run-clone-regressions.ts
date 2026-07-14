/**
 * Run regression tests that target the local mainnet clone
 * (default ws://127.0.0.1:9944). Invoked by CI after sudo-upgrade.
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const testsDir = path.join(__dirname, "..", "tests");

// Per-test ceiling; override with CLONE_REGRESSION_TIMEOUT_MS. Healthy tests
// finish in 2-10 min, so 15 min is a hang, not a slow pass — failing fast
// beats burning half an hour of runner time per stuck test.
const TEST_TIMEOUT_MS = Number(process.env.CLONE_REGRESSION_TIMEOUT_MS ?? 15 * 60 * 1000);

const CLONE_REGRESSION_PHASES = ["pristine", "remaining"] as const;
type CloneRegressionPhase = (typeof CLONE_REGRESSION_PHASES)[number];

const CLONE_REGRESSIONS = [
  // This test asserts the global issuance mirrors before and after each
  // scenario, so it owns the pristine phase before forceSetBalance-based tests
  // mutate the clone.
  { name: "test-total-issuance-trackers.ts", phase: "pristine" },
  { name: "test-balancer-operation.ts", phase: "remaining" },
  { name: "test-balancer-edge-emission-issuance.ts", phase: "remaining" },
  { name: "test-locks-conviction.ts", phase: "remaining" },
  { name: "test-lock-dust-cleanup.ts", phase: "remaining" },
  { name: "test-proxy-filter-security-regressions.ts", phase: "remaining" },
  { name: "test-hotkey-swap-and-proxy-stake.ts", phase: "remaining" },
  { name: "test-net-tao-flow-emission-allocation.ts", phase: "remaining" },
  { name: "test-alpha-deprecated-stake-histogram.ts", phase: "remaining" },
] as const satisfies ReadonlyArray<{ name: string; phase: CloneRegressionPhase }>;

const regressionNames = new Set<string>(CLONE_REGRESSIONS.map(({ name }) => name));
const regressionPhases = new Set<string>(CLONE_REGRESSION_PHASES);

// CI selects a named phase, keeping suite membership canonical here. Explicit
// filename selection remains available for targeted local/debug runs.
const requestedPhase = (process.env.CLONE_REGRESSION_PHASE ?? "").trim();
const requested = (process.env.CLONE_REGRESSION_TESTS ?? "").split(/[\s,]+/).filter(Boolean);
const cliArgs = process.argv.slice(2);
const unknownArgs = cliArgs.filter((arg) => arg !== "--list");
if (unknownArgs.length > 0) {
  console.error(`Unknown argument(s): ${unknownArgs.join(", ")}`);
  process.exit(1);
}
if (requestedPhase && requested.length > 0) {
  console.error("Set either CLONE_REGRESSION_PHASE or CLONE_REGRESSION_TESTS, not both");
  process.exit(1);
}
if (requestedPhase && !regressionPhases.has(requestedPhase)) {
  console.error(`Unknown regression phase: ${requestedPhase}`);
  process.exit(1);
}
const unknown = requested.filter((name) => !regressionNames.has(name));
if (unknown.length > 0) {
  console.error(`Unknown regression test(s): ${unknown.join(", ")}`);
  process.exit(1);
}
const selected =
  requested.length > 0
    ? CLONE_REGRESSIONS.filter(({ name }) => requested.includes(name))
    : requestedPhase
      ? CLONE_REGRESSIONS.filter(({ phase }) => phase === requestedPhase)
      : CLONE_REGRESSIONS;

if (cliArgs.includes("--list")) {
  console.log(selected.map(({ name }) => name).join("\n"));
  process.exit(0);
}

for (const { name } of selected) {
  const script = path.join(testsDir, name);
  console.log(`\n=== clone regression: ${name} (timeout ${TEST_TIMEOUT_MS}ms) ===`);
  const result = spawnSync(process.execPath, ["--import", "tsx", script], {
    stdio: "inherit",
    env: process.env,
    timeout: TEST_TIMEOUT_MS,
  });
  if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ETIMEDOUT") {
    console.error(`TIMEOUT: ${name} exceeded ${TEST_TIMEOUT_MS}ms`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`FAILED: ${name}`);
    process.exit(1);
  }
}
