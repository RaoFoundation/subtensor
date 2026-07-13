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

const CLONE_REGRESSIONS = [
  // This test asserts the global issuance mirrors before and after each
  // scenario, so run it before forceSetBalance-based tests mutate the clone.
  "test-total-issuance-trackers.ts",
  "test-balancer-operation.ts",
  "test-balancer-edge-emission-issuance.ts",
  "test-locks-conviction.ts",
  "test-lock-dust-cleanup.ts",
  "test-proxy-filter-security-regressions.ts",
  "test-hotkey-swap-and-proxy-stake.ts",
  "test-net-tao-flow-emission-allocation.ts",
  "test-alpha-deprecated-stake-histogram.ts",
];

// CI shards the suite across parallel clones; CLONE_REGRESSION_TESTS selects
// a subset (whitespace/comma separated). Unknown names fail loudly so a typo
// in a shard definition can't silently skip a regression test.
const requested = (process.env.CLONE_REGRESSION_TESTS ?? "").split(/[\s,]+/).filter(Boolean);
const unknown = requested.filter((name) => !CLONE_REGRESSIONS.includes(name));
if (unknown.length > 0) {
  console.error(`Unknown regression test(s): ${unknown.join(", ")}`);
  process.exit(1);
}
const selected =
  requested.length > 0
    ? CLONE_REGRESSIONS.filter((name) => requested.includes(name))
    : CLONE_REGRESSIONS;

for (const name of selected) {
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
