import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
    buildE2EPlan,
    loadShardManifest,
    materializeShardEnvironments,
    validateManifestCoverage,
    validateShardManifest,
} from "./e2e-shard-plan.mjs";

const tsTestsDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = loadShardManifest(join(tsTestsDir, "e2e-shards.json"));
const suiteOwnership = JSON.parse(readFileSync(join(tsTestsDir, "e2e-suite-ownership.json"), "utf8"));
const all = Object.fromEntries(
    ["evm", "staking", "coldkey_swap", "dev", "subnets", "shield"].map((name) => [name, true])
);
const registeredSelectors = Object.values(suiteOwnership.suites)
    .filter(({ owner }) => owner === "pull_request")
    .map(({ selector }) => selector)
    .sort();
const plannerSelectors = Object.keys(all).sort();
if (JSON.stringify(registeredSelectors) !== JSON.stringify(plannerSelectors)) {
    throw new Error(
        [
            "PR suite ownership and planner selectors do not match.",
            `Registered selectors: ${registeredSelectors.join(", ") || "none"}`,
            `Planner selectors: ${plannerSelectors.join(", ") || "none"}`,
            "Fix: update BOOLEAN_NAMES and buildE2EPlan in ts-tests/scripts/e2e-shard-plan.mjs, then update .github/scripts/classify-typescript-e2e-changes.sh for the same selector.",
            "Verify: node ts-tests/scripts/test-e2e-shard-plan.mjs && .github/scripts/test-classify-typescript-e2e-changes.sh",
        ].join("\n")
    );
}
const plan = buildE2EPlan(manifest, all);
assert.equal(plan.state_count, 7);
assert.equal(plan.shield_count, 5);
assert.deepEqual(
    plan.build_matrix.include.map(({ variant }) => variant),
    ["release", "fast"]
);

for (const [suite, registration] of Object.entries(suiteOwnership.suites)) {
    if (registration.owner !== "pull_request") continue;
    const selected = Object.fromEntries(Object.keys(all).map((name) => [name, name === registration.selector]));
    const suitePlan = buildE2EPlan(manifest, selected);
    if (suitePlan.build_count !== 1 || suitePlan.state_count + suitePlan.shield_count === 0) {
        throw new Error(
            [
                `PR-owned suite "${suite}" with selector "${registration.selector}" is not fully routed.`,
                `Planner result: builds=${suitePlan.build_count}, state jobs=${suitePlan.state_count}, Shield jobs=${suitePlan.shield_count}.`,
                "Fix: add the selector's binary and execution lane to buildE2EPlan in ts-tests/scripts/e2e-shard-plan.mjs.",
                "Verify: node ts-tests/scripts/test-e2e-shard-plan.mjs",
            ].join("\n")
        );
    }
}

const evmOnly = buildE2EPlan(manifest, {
    ...all,
    staking: false,
    coldkey_swap: false,
    dev: false,
    subnets: false,
    shield: false,
});
assert.equal(evmOnly.state_count, 3);
assert.equal(evmOnly.build_count, 1);
assert.equal(evmOnly.build_matrix.include[0].variant, "fast");

const future = structuredClone(manifest);
future.suites.evm.shards.push({
    name: "zombienet_evm_d",
    files: ["suites/zombienet_evm/future.test.ts"],
});
const futurePlan = buildE2EPlan(validateShardManifest(future), all);
assert.equal(futurePlan.state_count, 8, "a proposed shard must enter the matrix without changing trusted code");

const unsafe = structuredClone(manifest);
unsafe.suites.evm.shards[0].files = ["suites/zombienet_evm/../zombienet_shield/00.00-basic.test.ts"];
assert.throws(() => validateShardManifest(unsafe), /unsafe or duplicate test file/);

const coverageRoot = mkdtempSync(join(tmpdir(), "e2e-shard-coverage-"));
try {
    for (const file of Object.values(manifest.suites).flatMap(({ shards }) => shards.flatMap(({ files }) => files))) {
        const path = join(coverageRoot, file);
        mkdirSync(dirname(path), { recursive: true });
        writeFileSync(path, "");
    }
    validateManifestCoverage(manifest, coverageRoot);
    const nested = join(coverageRoot, "suites/zombienet_evm/nested/future.test.ts");
    mkdirSync(dirname(nested), { recursive: true });
    writeFileSync(nested, "");
    assert.throws(
        () => validateManifestCoverage(manifest, coverageRoot),
        /Unassigned test files: suites\/zombienet_evm\/nested\/future\.test\.ts[\s\S]*Fix: edit ts-tests\/e2e-shards\.json/,
        "nested test files must not bypass shard coverage"
    );
} finally {
    rmSync(coverageRoot, { recursive: true, force: true });
}

const baseConfig = {
    environments: [
        { name: "zombienet_evm", vitestArgs: { bail: 1 } },
        { name: "zombienet_staking", vitestArgs: { bail: 1 } },
        { name: "zombienet_shield", vitestArgs: { bail: 1 }, envVars: ["SHIELD_RUNTIME=release"] },
    ],
};
const materialized = materializeShardEnvironments(baseConfig, manifest);
assert.equal(materialized.environments.length, 13);
assert.deepEqual(materialized.environments.find(({ name }) => name === "zombienet_evm_a").include, [
    "suites/zombienet_evm/01-contract-deploy-call.test.ts",
]);
assert.equal(materialized.environments.find(({ name }) => name === "zombienet_shield_a").vitestArgs.maxConcurrency, 6);

console.log("TypeScript E2E shard plan tests passed");
