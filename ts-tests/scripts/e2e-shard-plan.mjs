#!/usr/bin/env node

import { readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SUITES = {
    evm: {
        baseEnvironment: "zombienet_evm",
        directory: "suites/zombienet_evm",
        binary: "fast",
        lane: "state",
    },
    staking: {
        baseEnvironment: "zombienet_staking",
        directory: "suites/zombienet_staking",
        binary: "fast",
        lane: "state",
    },
    shield: {
        baseEnvironment: "zombienet_shield",
        directory: "suites/zombienet_shield",
        binary: "release",
        lane: "shield",
    },
};

const BOOLEAN_NAMES = ["evm", "staking", "coldkey_swap", "dev", "subnets", "shield"];

function exactKeys(value, expected, label) {
    const actual = Object.keys(value).sort();
    const wanted = [...expected].sort();
    if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
        throw new Error(`${label} keys must be exactly [${wanted.join(", ")}]`);
    }
}

function objectValue(value, label) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value;
}

export function validateShardManifest(payload) {
    const manifest = objectValue(payload, "shard manifest");
    exactKeys(manifest, ["version", "suites"], "shard manifest");
    if (manifest.version !== 1) throw new Error("shard manifest version must be 1");

    const suites = objectValue(manifest.suites, "shard manifest suites");
    exactKeys(suites, Object.keys(SUITES), "shard manifest suites");

    for (const [suiteName, contract] of Object.entries(SUITES)) {
        const suite = objectValue(suites[suiteName], `${suiteName} suite`);
        exactKeys(suite, ["shards"], `${suiteName} suite`);
        if (!Array.isArray(suite.shards) || suite.shards.length === 0 || suite.shards.length > 26) {
            throw new Error(`${suiteName} must contain between 1 and 26 shards`);
        }

        const seenNames = new Set();
        const seenFiles = new Set();
        for (const [index, shardValue] of suite.shards.entries()) {
            const shard = objectValue(shardValue, `${suiteName} shard ${index}`);
            const keys = suiteName === "shield" ? ["name", "files", "maxConcurrency"] : ["name", "files"];
            exactKeys(shard, keys, `${suiteName} shard ${index}`);

            const expectedName = `${contract.baseEnvironment}_${String.fromCharCode(97 + index)}`;
            if (shard.name !== expectedName || seenNames.has(shard.name)) {
                throw new Error(`${suiteName} shard ${index} must be named ${expectedName}`);
            }
            seenNames.add(shard.name);

            if (!Array.isArray(shard.files) || shard.files.length === 0) {
                throw new Error(`${shard.name} must include at least one test file`);
            }
            for (const file of shard.files) {
                if (
                    typeof file !== "string" ||
                    !file.startsWith(`${contract.directory}/`) ||
                    !/^[A-Za-z0-9._/-]+\.test\.ts$/.test(file) ||
                    file.split("/").some((segment) => segment === "." || segment === ".." || segment === "") ||
                    seenFiles.has(file)
                ) {
                    throw new Error(`${shard.name} contains an unsafe or duplicate test file: ${String(file)}`);
                }
                seenFiles.add(file);
            }

            if (
                suiteName === "shield" &&
                (!Number.isInteger(shard.maxConcurrency) || shard.maxConcurrency < 1 || shard.maxConcurrency > 8)
            ) {
                throw new Error(`${shard.name} maxConcurrency must be an integer from 1 through 8`);
            }
        }
    }
    return manifest;
}

export function loadShardManifest(path) {
    return validateShardManifest(JSON.parse(readFileSync(path, "utf8")));
}

function testFiles(directory, relativeDirectory) {
    return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
        const relativePath = `${relativeDirectory}/${entry.name}`;
        if (entry.isDirectory()) return testFiles(join(directory, entry.name), relativePath);
        return entry.isFile() && entry.name.endsWith(".test.ts") ? [relativePath] : [];
    });
}

export function validateManifestCoverage(manifest, tsTestsDir) {
    for (const [suiteName, contract] of Object.entries(SUITES)) {
        const expected = testFiles(join(tsTestsDir, contract.directory), contract.directory).sort();
        const assigned = manifest.suites[suiteName].shards.flatMap(({ files }) => files).sort();
        if (JSON.stringify(assigned) !== JSON.stringify(expected)) {
            const missing = expected.filter((file) => !assigned.includes(file));
            const unknown = assigned.filter((file) => !expected.includes(file));
            throw new Error(
                [
                    `${suiteName} shard coverage does not match the test directory.`,
                    `Unassigned test files: ${missing.join(", ") || "none"}`,
                    `Stale or invalid manifest entries: ${unknown.join(", ") || "none"}`,
                    `Fix: edit ts-tests/e2e-shards.json so every suites/zombienet_${suiteName}/**/*.test.ts file appears exactly once under suites.${suiteName}.shards[].files.`,
                    "Verify: node ts-tests/scripts/test-e2e-shard-plan.mjs && node ts-tests/scripts/validate-e2e-config.mjs",
                ].join("\n")
            );
        }
    }
}

function generatedShardName(name) {
    return Object.values(SUITES).some(({ baseEnvironment }) => new RegExp(`^${baseEnvironment}_[a-z]+$`).test(name));
}

export function stripGeneratedShardEnvironments(config) {
    const copy = structuredClone(config);
    if (!Array.isArray(copy.environments)) throw new Error("Moonwall config has no environments array");
    copy.environments = copy.environments.filter(({ name }) => !generatedShardName(name));
    return copy;
}

export function materializeShardEnvironments(config, manifest) {
    const baseConfig = stripGeneratedShardEnvironments(config);
    const shardsByBase = new Map(
        Object.entries(SUITES).map(([suiteName, contract]) => [contract.baseEnvironment, { suiteName, contract }])
    );
    const environments = [];

    for (const environment of baseConfig.environments) {
        environments.push(environment);
        const selected = shardsByBase.get(environment.name);
        if (!selected) continue;
        for (const shard of manifest.suites[selected.suiteName].shards) {
            const generated = structuredClone(environment);
            generated.name = shard.name;
            generated.include = [...shard.files];
            if (selected.suiteName === "shield") {
                generated.vitestArgs = {
                    ...generated.vitestArgs,
                    sequence: { concurrent: true },
                    maxConcurrency: shard.maxConcurrency,
                };
            }
            environments.push(generated);
        }
    }
    baseConfig.environments = environments;
    return baseConfig;
}

function booleanValue(value, name) {
    if (value === "true") return true;
    if (value === "false") return false;
    throw new Error(`${name} selection must be true or false`);
}

export function buildE2EPlan(manifest, selected) {
    const stateEntries = [];
    for (const suiteName of ["evm", "staking"]) {
        if (!selected[suiteName]) continue;
        const binary = SUITES[suiteName].binary;
        stateEntries.push(...manifest.suites[suiteName].shards.map(({ name }) => ({ test: name, binary })));
    }
    if (selected.coldkey_swap && selected.subnets) {
        stateEntries.push({
            test: "zombienet_coldkey_swap",
            additional_test: "zombienet_subnets",
            binary: "fast",
        });
    } else if (selected.coldkey_swap) {
        stateEntries.push({ test: "zombienet_coldkey_swap", binary: "fast" });
    } else if (selected.subnets) {
        stateEntries.push({ test: "zombienet_subnets", binary: "fast" });
    }
    if (selected.dev) stateEntries.push({ test: "dev", binary: "release" });

    const shieldEntries = selected.shield
        ? manifest.suites.shield.shards.map(({ name }) => ({ test: name, binary: SUITES.shield.binary }))
        : [];
    const needsRelease = selected.dev || selected.shield;
    const needsFast = stateEntries.some(({ binary }) => binary === "fast");
    const buildEntries = [];
    if (needsRelease) buildEntries.push({ variant: "release", flags: "" });
    if (needsFast) buildEntries.push({ variant: "fast", flags: "--features fast-runtime" });

    return {
        state_count: stateEntries.length,
        state_matrix: { include: stateEntries },
        shield_count: shieldEntries.length,
        shield_matrix: { include: shieldEntries },
        build_count: buildEntries.length,
        build_matrix: { include: buildEntries },
    };
}

function appendPlan(path, plan) {
    const lines = [];
    for (const [name, value] of Object.entries(plan)) {
        lines.push(`${name}=${typeof value === "number" ? value : JSON.stringify(value)}`);
    }
    writeFileSync(path, `${lines.join("\n")}\n`, { flag: "a" });
}

function writeConfig(path, config) {
    const temporary = `${path}.generated`;
    writeFileSync(temporary, `${JSON.stringify(config, null, 4)}\n`, { mode: 0o600 });
    renameSync(temporary, path);
}

function usage() {
    throw new Error(
        "usage: e2e-shard-plan.mjs plan MANIFEST OUTPUT EVM STAKING COLDKEY_SWAP DEV SUBNETS SHIELD | materialize CONFIG MANIFEST | strip CONFIG"
    );
}

function main(argv) {
    const [command, ...args] = argv;
    if (command === "plan" && args.length === 8) {
        const [manifestPath, outputPath, ...selections] = args;
        const selected = Object.fromEntries(
            BOOLEAN_NAMES.map((name, index) => [name, booleanValue(selections[index], name)])
        );
        appendPlan(outputPath, buildE2EPlan(loadShardManifest(manifestPath), selected));
        return;
    }
    if (command === "materialize" && args.length === 2) {
        const [configPath, manifestPath] = args;
        const config = JSON.parse(readFileSync(configPath, "utf8"));
        writeConfig(configPath, materializeShardEnvironments(config, loadShardManifest(manifestPath)));
        return;
    }
    if (command === "strip" && args.length === 1) {
        const [configPath] = args;
        writeConfig(configPath, stripGeneratedShardEnvironments(JSON.parse(readFileSync(configPath, "utf8"))));
        return;
    }
    usage();
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
    try {
        main(process.argv.slice(2));
    } catch (error) {
        console.error(`E2E shard plan error: ${error.message}`);
        process.exitCode = 1;
    }
}
