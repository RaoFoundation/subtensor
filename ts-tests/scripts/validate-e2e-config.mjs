import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

import { decodeRuntimeWasm } from "./extract-runtime-wasm.mjs";

const tsTestsDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(tsTestsDir, "moonwall.config.json"), "utf8"));
const singleNodeSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_single_node.json"), "utf8"));
const shieldSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_extended.json"), "utf8"));
const e2eWorkflow = readFileSync(join(tsTestsDir, "..", ".github/workflows/typescript-e2e.yml"), "utf8");
const environments = new Map(config.environments.map((environment) => [environment.name, environment]));

assert.deepEqual(
    [...decodeRuntimeWasm({ genesis: { raw: { top: { "0x3a636f6465": "0x0061736d" } } } })],
    [0, 97, 115, 109]
);
assert.throws(() => decodeRuntimeWasm({}), /missing a valid/);
assert.throws(
    () => decodeRuntimeWasm({ genesis: { raw: { top: { "0x3a636f6465": "0xnot-hex" } } } }),
    /missing a valid/
);

const evmFiles = readdirSync(join(tsTestsDir, "suites/zombienet_evm"))
    .filter((file) => file.endsWith(".test.ts"))
    .map((file) => `suites/zombienet_evm/${file}`)
    .sort();
const evmShardNames = ["zombienet_evm_a", "zombienet_evm_b"];
const shieldFiles = readdirSync(join(tsTestsDir, "suites/zombienet_shield"))
    .filter((file) => file.endsWith(".test.ts"))
    .map((file) => `suites/zombienet_shield/${file}`)
    .sort();
const shieldShardNames = ["zombienet_shield_a", "zombienet_shield_b", "zombienet_shield_c", "zombienet_shield_d"];
const expectedShieldBinaries = new Map([
    ["zombienet_shield_a", "release"],
    ["zombienet_shield_b", "release"],
    ["zombienet_shield_c", "release"],
    ["zombienet_shield_d", "release"],
]);
const shieldJob = e2eWorkflow.match(/\n {2}run-shield-tests:\n(?<body>[\s\S]*?)\n {2}shield-result:\n/)?.groups?.body;
if (!shieldJob) {
    throw new Error("Could not find the Shield matrix in typescript-e2e.yml");
}
const workflowShieldEntries = [
    ...shieldJob.matchAll(/^\s+- test: (zombienet_shield_[a-z]+)\s*\n\s+binary: (fast|release)\s*$/gm),
].map((match) => ({ name: match[1], binary: match[2] }));
const workflowShieldShardNames = workflowShieldEntries.map(({ name }) => name);
if (!isDeepStrictEqual(workflowShieldShardNames, shieldShardNames)) {
    throw new Error(
        `Shield workflow/config mismatch; workflow=[${workflowShieldShardNames.join(", ")}] config=[${shieldShardNames.join(", ")}]`
    );
}
for (const { name, binary } of workflowShieldEntries) {
    if (binary !== expectedShieldBinaries.get(name)) {
        throw new Error(`${name} must use the ${expectedShieldBinaries.get(name)} binary; found ${binary}`);
    }
}
const shieldShardIncludes = shieldShardNames.map((name) => {
    const environment = environments.get(name);
    if (!environment) {
        throw new Error(`Missing Moonwall environment: ${name}`);
    }
    const includes = environment.include ?? [];
    if (includes.length === 0) {
        throw new Error(`${name} must include at least one Shield test`);
    }
    return includes;
});
// File counts intentionally differ: the shards are balanced by measured runtime.
const shieldIncludes = shieldShardIncludes.flat();
const productionTimingFile = "suites/zombienet_shield/03-timing.test.ts";
const shieldDefaultVitestArgs = { bail: 1 };
const shieldConcurrency = new Map([
    ["zombienet_shield_a", 6],
    ["zombienet_shield_b", 4],
    ["zombienet_shield_c", 6],
    ["zombienet_shield_d", 3],
]);

for (const [index, includes] of shieldShardIncludes.entries()) {
    const name = shieldShardNames[index];
    const binary = expectedShieldBinaries.get(name);
    const containsProductionTiming = includes.includes(productionTimingFile);
    if (containsProductionTiming && (binary !== "release" || includes.length !== 1)) {
        throw new Error(`${productionTimingFile} must be the only file in one release-runtime shard`);
    }
    const environment = environments.get(name);
    const maxConcurrency = shieldConcurrency.get(name);
    const expectedVitestArgs = {
        ...shieldDefaultVitestArgs,
        sequence: { concurrent: true },
        maxConcurrency,
    };
    if (!isDeepStrictEqual(environment?.vitestArgs, expectedVitestArgs)) {
        throw new Error(`${name} must run its state-isolated cases with maxConcurrency=${maxConcurrency}`);
    }
}

const duplicateShieldFiles = shieldIncludes.filter((file, index) => shieldIncludes.indexOf(file) !== index);
if (duplicateShieldFiles.length > 0) {
    throw new Error(`Shield tests assigned to multiple shards: ${[...new Set(duplicateShieldFiles)].join(", ")}`);
}

const sortedIncludes = [...shieldIncludes].sort();
if (JSON.stringify(sortedIncludes) !== JSON.stringify(shieldFiles)) {
    const missing = shieldFiles.filter((file) => !sortedIncludes.includes(file));
    const unknown = sortedIncludes.filter((file) => !shieldFiles.includes(file));
    throw new Error(`Shield shard coverage mismatch; missing=[${missing.join(", ")}] unknown=[${unknown.join(", ")}]`);
}

const evmShardIncludes = evmShardNames.map((name) => {
    const includes = environments.get(name)?.include ?? [];
    if (includes.length === 0) {
        throw new Error(`${name} must include at least one EVM test`);
    }
    return includes;
});
const evmIncludes = evmShardIncludes.flat();
const duplicateEvmFiles = evmIncludes.filter((file, index) => evmIncludes.indexOf(file) !== index);
if (duplicateEvmFiles.length > 0) {
    throw new Error(`EVM tests assigned to multiple shards: ${[...new Set(duplicateEvmFiles)].join(", ")}`);
}
const sortedEvmIncludes = [...evmIncludes].sort();
if (!isDeepStrictEqual(sortedEvmIncludes, evmFiles)) {
    const missing = evmFiles.filter((file) => !sortedEvmIncludes.includes(file));
    const unknown = sortedEvmIncludes.filter((file) => !evmFiles.includes(file));
    throw new Error(`EVM shard coverage mismatch; missing=[${missing.join(", ")}] unknown=[${unknown.join(", ")}]`);
}

const canonicalEvmEnvironment = environments.get("zombienet_evm");
if (!canonicalEvmEnvironment) {
    throw new Error("Missing Moonwall environment: zombienet_evm");
}
const sharedEvmSettings = ({ name: _name, include: _include, ...settings }) => settings;
const canonicalEvmSettings = sharedEvmSettings(canonicalEvmEnvironment);
for (const name of evmShardNames) {
    if (!isDeepStrictEqual(sharedEvmSettings(environments.get(name)), canonicalEvmSettings)) {
        throw new Error(`${name} settings must match zombienet_evm except for name and include`);
    }
}

const singleNodeConfig = "./configs/zombie_single_node.json";
const singleNodes = singleNodeSpec.relaychain?.nodes ?? [];
if (
    singleNodes.length !== 1 ||
    singleNodes[0]?.validator !== true ||
    !singleNodeSpec.relaychain?.default_args?.includes("--sealing=100")
) {
    throw new Error("Single-node state spec must contain one validator using --sealing=100");
}

const descriptorScripts = ["build-spec.sh", "generate-types-from-chain-spec.sh"];
for (const environment of config.environments.filter(({ foundation }) => foundation?.type === "zombie")) {
    if (!isDeepStrictEqual(environment.runScripts?.slice(0, 2), descriptorScripts)) {
        throw new Error(`${environment.name} must build its chain spec before generating exact-runtime descriptors`);
    }
}
if (!isDeepStrictEqual(environments.get("dev")?.runScripts, ["generate-types.sh"])) {
    throw new Error("dev must retain live-node descriptor generation");
}

for (const name of [
    "zombienet_staking",
    "zombienet_coldkey_swap",
    "zombienet_evm",
    ...evmShardNames,
    "zombienet_subnets",
]) {
    const configPath = environments.get(name)?.foundation?.zombieSpec?.configPath;
    if (configPath !== singleNodeConfig) {
        throw new Error(`${name} must use ${singleNodeConfig}; found ${configPath ?? "no config"}`);
    }
}

const shieldConfig = "./configs/zombie_extended.json";
const shieldNodes = shieldSpec.relaychain?.nodes ?? [];
const shieldValidators = shieldNodes.filter((node) => node.validator === true);
if (shieldNodes.length !== 6 || shieldValidators.length !== 3) {
    throw new Error(
        `Shield spec must retain six nodes and three validators; found ${shieldNodes.length} nodes and ${shieldValidators.length} validators`
    );
}

const canonicalShieldEnvironment = environments.get("zombienet_shield");
if (!canonicalShieldEnvironment) {
    throw new Error("Missing Moonwall environment: zombienet_shield");
}
const sharedShieldSettings = ({
    name: _name,
    include: _include,
    envVars: _envVars,
    vitestArgs: _vitestArgs,
    ...settings
}) => settings;
const canonicalShieldSettings = sharedShieldSettings(canonicalShieldEnvironment);
if (!isDeepStrictEqual(canonicalShieldEnvironment.vitestArgs, shieldDefaultVitestArgs)) {
    throw new Error("zombienet_shield must retain the default fail-fast Vitest configuration");
}

for (const name of ["zombienet_shield", ...shieldShardNames]) {
    const environment = environments.get(name);
    const expectedRuntime = name === "zombienet_shield" ? "release" : expectedShieldBinaries.get(name);
    const configPath = environment?.foundation?.zombieSpec?.configPath;
    const connectionNames = new Set((environment?.connections ?? []).map((connection) => connection.name));
    if (configPath !== shieldConfig) {
        throw new Error(`${name} must use ${shieldConfig}; found ${configPath ?? "no config"}`);
    }
    if (!connectionNames.has("Node") || !connectionNames.has("NodeFull")) {
        throw new Error(`${name} must expose authority and full-node connections`);
    }
    if (!isDeepStrictEqual(environment?.envVars, [`SHIELD_RUNTIME=${expectedRuntime}`])) {
        throw new Error(`${name} must declare SHIELD_RUNTIME=${expectedRuntime}`);
    }
    if (name !== "zombienet_shield") {
        if (!isDeepStrictEqual(sharedShieldSettings(environment), canonicalShieldSettings)) {
            throw new Error(`${name} settings must match zombienet_shield except for name and include`);
        }
    }
}

console.log(
    `Validated ${shieldFiles.length} Shield files across ${shieldShardNames.length} production-runtime shards, ${evmFiles.length} EVM files across ${evmShardNames.length} shards, ${shieldShardNames.length + 1} multi-node Shield environments, and six single-node state environments.`
);
