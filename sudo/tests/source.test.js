"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
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
