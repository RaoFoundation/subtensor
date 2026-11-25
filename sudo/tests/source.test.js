"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { params, groups } = require("../lib/params");
const { loadParams, storagePlan } = require("../lib/diff");

test("committed network files have every value and the correct network", () => {
  for (const network of ["devnet", "testnet", "mainnet"]) {
    const loaded = loadParams(fs.readFileSync(path.join(__dirname, "..", `${network}.toml`), "utf8"));
    assert.equal(loaded.metadata.network, network);
    assert.equal(Object.keys(loaded.values).length, 51);
    assert.equal(new Set(storagePlan().map((entry) => entry.key)).size, 51);
  }
});

test("every setter exists as an ensure_root function in the checked-in pallet", () => {
  const root = path.join(__dirname, "..", "..");
  for (const entry of storagePlan()) {
    const file = entry.callPallet === "AdminUtils" ?
      path.join(root, "pallets", "admin-utils", "src", "lib.rs") :
      path.join(root, "pallets", "subtensor", "src", "macros", "dispatches.rs");
    const source = fs.readFileSync(file, "utf8");
    const declaration = new RegExp(`pub fn ${entry.call}\\([^)]*\\)[^{]*\\{([\\s\\S]*?)\\n        \\}`);
    const match = source.match(declaration);
    assert.ok(match, `${entry.callPallet}.${entry.call} not found`);
    assert.match(match[1], /ensure_root\(origin\)\?/, `${entry.call} is not ensure_root`);
    if (entry.callPallet === "AdminUtils") {
      assert.doesNotMatch(match[0], /netuid\s*:/, `${entry.call} is netuid-keyed`);
    }
  }
});

test("CLI rejects a baseline file for the wrong network", () => {
  const { argumentsOf } = require("../diff");
  assert.deepEqual(argumentsOf(["--file", "desired.toml", "--base-file", "base.toml"]), {
    file: "desired.toml", baseFile: "base.toml", outputDir: null, ws: null, sourceCommit: null, baseCommit: null,
  });
  assert.throws(() => argumentsOf(["--unknown"]), /Invalid argument/);
});

test("all group fields have the matching call argument", () => {
  for (const [name, group] of Object.entries(groups)) {
    assert.equal(group.callArguments.length, Object.keys(group.storage).length, name);
  }
});

test("only successive runs for the same PR share a cancellation group", () => {
  const workflow = fs.readFileSync(path.join(__dirname, "..", "..", ".github", "workflows", "sudo-params.yml"), "utf8");
  assert.match(workflow, /group:.*format\('pr-\{0\}', github.event.pull_request.number\).*format\('run-\{0\}', github.run_id\)/);
  assert.match(workflow, /cancel-in-progress: \$\{\{ github.event_name == 'pull_request' \}\}/);
});

test("CI reserves signable proposals and notifications for main pushes", () => {
  const workflow = fs.readFileSync(path.join(__dirname, "..", "..", ".github", "workflows", "sudo-params.yml"), "utf8");
  assert.match(workflow, /BASE_REF:.*github.event_name == 'push' && github.event.before/);
  assert.match(workflow, /MANUAL_COMPARISON: .*github.event_name == 'workflow_dispatch'/);
  assert.match(workflow, /if \[ "\$MANUAL_COMPARISON" = "true" \]; then\s+args\+=\(--comparison-only\)/);
  assert.match(workflow, /TRUSTED_MAIN_PUSH: .*github.event_name == 'push' && github.ref == 'refs\/heads\/main'/);
  assert.match(workflow, /Notify Discord about a real proposal\s+if: github.event_name == 'push' && github.ref == 'refs\/heads\/main'/);
});

test("publishing requires the exact before and after from a main push", (t) => {
  const { verifyMainPush } = require("../diff");
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-push-event-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const eventPath = path.join(directory, "event.json");
  const ref = "refs/heads/main";
  const before = "a".repeat(40);
  const after = "b".repeat(40);
  fs.writeFileSync(eventPath, JSON.stringify({ ref, before, after }));
  const env = { GITHUB_EVENT_NAME: "push", GITHUB_REF: ref, GITHUB_EVENT_PATH: eventPath };
  const options = { publishMainPush: true, baseCommit: before, sourceCommit: after };
  assert.doesNotThrow(() => verifyMainPush(options, env));
  assert.throws(() => verifyMainPush({ ...options, baseCommit: "c".repeat(40) }, env), /match the main push event/);
  assert.throws(() => verifyMainPush({ ...options, sourceCommit: "c".repeat(40) }, env), /match the main push event/);
  assert.throws(() => verifyMainPush(options, { ...env, GITHUB_EVENT_NAME: "workflow_dispatch" }), /trusted push to main/);
  assert.throws(() => verifyMainPush({ ...options, comparisonOnly: true }, env), /trusted push to main/);
  assert.throws(() => verifyMainPush(options, {}), /trusted push to main/);
});
