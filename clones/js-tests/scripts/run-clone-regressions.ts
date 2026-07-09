/**
 * Run regression tests that target the local mainnet clone
 * (default ws://127.0.0.1:9944). Invoked by CI after sudo-upgrade.
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const testsDir = path.join(__dirname, "..", "tests");

// Per-test ceiling; override with CLONE_REGRESSION_TIMEOUT_MS (default 30 min).
const TEST_TIMEOUT_MS = Number(process.env.CLONE_REGRESSION_TIMEOUT_MS ?? 30 * 60 * 1000);

const CLONE_REGRESSIONS = [
  "test-balancer-operation.ts",
  "test-balancer-edge-emission-issuance.ts",
  "test-locks-conviction.ts",
  "test-lock-dust-cleanup.ts",
  "test-proxy-filter-security-regressions.ts",
  "test-hotkey-swap-and-proxy-stake.ts",
  "test-net-tao-flow-emission-allocation.ts",
  "test-total-issuance-trackers.ts",
  "test-alpha-deprecated-stake-histogram.ts",
];

let failed = false;

for (const name of CLONE_REGRESSIONS) {
  const script = path.join(testsDir, name);
  console.log(`\n=== clone regression: ${name} (timeout ${TEST_TIMEOUT_MS}ms) ===`);
  const result = spawnSync(process.execPath, ["--import", "tsx", script], {
    stdio: "inherit",
    env: process.env,
    timeout: TEST_TIMEOUT_MS,
  });
  if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ETIMEDOUT") {
    failed = true;
    console.error(`TIMEOUT: ${name} exceeded ${TEST_TIMEOUT_MS}ms`);
    continue;
  }
  if (result.status !== 0) {
    failed = true;
    console.error(`FAILED: ${name}`);
  }
}

process.exit(failed ? 1 : 0);
