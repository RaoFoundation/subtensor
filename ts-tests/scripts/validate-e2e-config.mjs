import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

import { loadShardManifest, materializeShardEnvironments, validateManifestCoverage } from "./e2e-shard-plan.mjs";
import { decodeRuntimeWasm } from "./extract-runtime-wasm.mjs";

const tsTestsDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const baseConfigPath = process.env.E2E_CONFIG_PATH ?? join(tsTestsDir, "moonwall.config.json");
const baseConfig = JSON.parse(readFileSync(baseConfigPath, "utf8"));
const suiteOwnership = JSON.parse(readFileSync(join(tsTestsDir, "e2e-suite-ownership.json"), "utf8"));
const shardManifest = loadShardManifest(join(tsTestsDir, "e2e-shards.json"));
validateManifestCoverage(shardManifest, tsTestsDir);

const ownershipFix = [
    "Fix: edit ts-tests/e2e-suite-ownership.json.",
    'Use owner="pull_request" plus a selector for PR E2E coverage, or owner="scheduled" plus selector=null.',
    "Then run: node ts-tests/scripts/validate-e2e-config.mjs && node ts-tests/scripts/test-e2e-shard-plan.mjs && .github/scripts/test-classify-typescript-e2e-changes.sh",
];
const ownershipError = (summary, details = []) => {
    throw new Error([`TypeScript E2E suite ownership error: ${summary}`, ...details, ...ownershipFix].join("\n"));
};

if (suiteOwnership.version !== 1) {
    ownershipError(`expected registry version 1, found ${JSON.stringify(suiteOwnership.version)}`);
}
if (
    suiteOwnership.suites === null ||
    typeof suiteOwnership.suites !== "object" ||
    Array.isArray(suiteOwnership.suites)
) {
    ownershipError('the registry must contain a "suites" object');
}
const suiteDirectories = readdirSync(join(tsTestsDir, "suites"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map(({ name }) => name)
    .sort();
const registeredSuites = Object.keys(suiteOwnership.suites).sort();
const unregisteredSuites = suiteDirectories.filter((suite) => !registeredSuites.includes(suite));
const staleSuites = registeredSuites.filter((suite) => !suiteDirectories.includes(suite));
if (unregisteredSuites.length > 0 || staleSuites.length > 0) {
    ownershipError("suite directories and registry entries do not match", [
        `Unregistered ts-tests/suites directories: ${unregisteredSuites.join(", ") || "none"}`,
        `Registry entries with no directory: ${staleSuites.join(", ") || "none"}`,
    ]);
}

const registeredEnvironments = [];
const pullRequestSelectors = new Set();
const canonicalEnvironments = new Map(baseConfig.environments.map((environment) => [environment.name, environment]));
for (const [suite, registration] of Object.entries(suiteOwnership.suites)) {
    const fields = Object.keys(registration).sort();
    if (!isDeepStrictEqual(fields, ["environments", "owner", "selector"])) {
        ownershipError(`suite "${suite}" must contain exactly owner, selector, and environments`, [
            `Found fields: ${fields.join(", ") || "none"}`,
        ]);
    }
    if (registration.owner !== "pull_request" && registration.owner !== "scheduled") {
        ownershipError(`suite "${suite}" has invalid owner ${JSON.stringify(registration.owner)}`);
    }
    if (!Array.isArray(registration.environments) || registration.environments.length === 0) {
        ownershipError(`suite "${suite}" must list at least one canonical Moonwall environment`);
    }
    for (const environment of registration.environments) {
        if (typeof environment !== "string" || environment.length === 0) {
            ownershipError(`suite "${suite}" contains an invalid Moonwall environment name`);
        }
        if (registeredEnvironments.includes(environment)) {
            ownershipError(`Moonwall environment "${environment}" is owned by more than one suite`);
        }
        const environmentConfig = canonicalEnvironments.get(environment);
        if (!environmentConfig) {
            ownershipError(`suite "${suite}" references missing Moonwall environment "${environment}"`);
        }
        const expectedTestDirectory = `suites/${suite}`;
        if (!isDeepStrictEqual(environmentConfig.testFileDir, [expectedTestDirectory])) {
            ownershipError(`environment "${environment}" does not exclusively execute suite "${suite}"`, [
                `Expected testFileDir: [${expectedTestDirectory}]`,
                `Found testFileDir: ${JSON.stringify(environmentConfig.testFileDir)}`,
                "Fix: edit ts-tests/moonwall.config.json so the environment points exactly at its owned suite directory.",
            ]);
        }
        for (const filter of ["include", "exclude", "skipTests"]) {
            if (Object.hasOwn(environmentConfig, filter)) {
                ownershipError(`canonical environment "${environment}" may not define ${filter}`, [
                    `A canonical environment must discover every test beneath ${expectedTestDirectory}; ${filter} can silently omit owned coverage.`,
                    `Fix: remove ${filter} from ${environment} in ts-tests/moonwall.config.json. Shard include lists belong only in ts-tests/e2e-shards.json.`,
                ]);
            }
        }
        registeredEnvironments.push(environment);
    }
    if (registration.owner === "pull_request") {
        if (typeof registration.selector !== "string" || !/^[a-z][a-z0-9_]*$/.test(registration.selector)) {
            ownershipError(`PR-owned suite "${suite}" has invalid selector ${JSON.stringify(registration.selector)}`);
        }
        if (pullRequestSelectors.has(registration.selector)) {
            ownershipError(`PR selector "${registration.selector}" is assigned to more than one suite`);
        }
        pullRequestSelectors.add(registration.selector);
    } else if (registration.selector !== null) {
        ownershipError(`scheduled suite "${suite}" must use selector=null`);
    }
}
const configuredEnvironments = baseConfig.environments.map(({ name }) => name).sort();
registeredEnvironments.sort();
const unownedEnvironments = configuredEnvironments.filter((name) => !registeredEnvironments.includes(name));
const staleEnvironments = registeredEnvironments.filter((name) => !configuredEnvironments.includes(name));
if (unownedEnvironments.length > 0 || staleEnvironments.length > 0) {
    ownershipError("canonical Moonwall environments and registry ownership do not match", [
        `Moonwall environments with no owner: ${unownedEnvironments.join(", ") || "none"}`,
        `Registered environments missing from moonwall.config.json: ${staleEnvironments.join(", ") || "none"}`,
    ]);
}

const shardNames = new Set(Object.values(shardManifest.suites).flatMap(({ shards }) => shards.map(({ name }) => name)));
for (const { name } of baseConfig.environments) {
    if (shardNames.has(name)) throw new Error(`${name} is generated from e2e-shards.json and must not be checked in`);
}
const config = materializeShardEnvironments(baseConfig, shardManifest);
const singleNodeSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_single_node.json"), "utf8"));
const shieldSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_extended.json"), "utf8"));
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

const evmShards = shardManifest.suites.evm.shards;
const evmFiles = evmShards.flatMap(({ files }) => files).sort();
const evmShardNames = evmShards.map(({ name }) => name);
const stakingShards = shardManifest.suites.staking.shards;
const stakingFiles = stakingShards.flatMap(({ files }) => files).sort();
const stakingShardNames = stakingShards.map(({ name }) => name);
const shieldShards = shardManifest.suites.shield.shards;
const shieldFiles = shieldShards.flatMap(({ files }) => files).sort();
const shieldShardNames = shieldShards.map(({ name }) => name);
const expectedShieldBinaries = new Map(shieldShardNames.map((name) => [name, "release"]));
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
const shieldConcurrency = new Map(shieldShards.map(({ name, maxConcurrency }) => [name, maxConcurrency]));

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

const stakingShardIncludes = stakingShardNames.map((name) => {
    const includes = environments.get(name)?.include ?? [];
    if (includes.length === 0) {
        throw new Error(`${name} must include at least one staking test`);
    }
    return includes;
});
const stakingIncludes = stakingShardIncludes.flat();
const duplicateStakingFiles = stakingIncludes.filter((file, index) => stakingIncludes.indexOf(file) !== index);
if (duplicateStakingFiles.length > 0) {
    throw new Error(`Staking tests assigned to multiple shards: ${[...new Set(duplicateStakingFiles)].join(", ")}`);
}
const sortedStakingIncludes = [...stakingIncludes].sort();
if (!isDeepStrictEqual(sortedStakingIncludes, stakingFiles)) {
    const missing = stakingFiles.filter((file) => !sortedStakingIncludes.includes(file));
    const unknown = sortedStakingIncludes.filter((file) => !stakingFiles.includes(file));
    throw new Error(`Staking shard coverage mismatch; missing=[${missing.join(", ")}] unknown=[${unknown.join(", ")}]`);
}

const canonicalStakingEnvironment = environments.get("zombienet_staking");
if (!canonicalStakingEnvironment) {
    throw new Error("Missing Moonwall environment: zombienet_staking");
}
const sharedStakingSettings = ({ name: _name, include: _include, ...settings }) => settings;
const canonicalStakingSettings = sharedStakingSettings(canonicalStakingEnvironment);
for (const name of stakingShardNames) {
    if (!isDeepStrictEqual(sharedStakingSettings(environments.get(name)), canonicalStakingSettings)) {
        throw new Error(`${name} settings must match zombienet_staking except for name and include`);
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
    ...stakingShardNames,
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
    `Validated ${shieldFiles.length} Shield files across ${shieldShardNames.length} production-runtime shards, ${evmFiles.length} EVM files across ${evmShardNames.length} shards, ${stakingFiles.length} staking files across ${stakingShardNames.length} shards, ${shieldShardNames.length + 1} multi-node Shield environments, and eight single-node state environments.`
);
