import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const tsTestsDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(tsTestsDir, "moonwall.config.json"), "utf8"));
const singleNodeSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_single_node.json"), "utf8"));
const shieldSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_extended.json"), "utf8"));
const e2eWorkflow = readFileSync(join(tsTestsDir, "..", ".github/workflows/typescript-e2e.yml"), "utf8");
const environments = new Map(config.environments.map((environment) => [environment.name, environment]));

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
const shieldBasicVitestArgs = {
    ...shieldDefaultVitestArgs,
    sequence: { concurrent: true },
    maxConcurrency: 6,
};
const productionTimingVitestArgs = {
    ...shieldDefaultVitestArgs,
    sequence: { concurrent: true },
    maxConcurrency: 4,
};

for (const [index, includes] of shieldShardIncludes.entries()) {
    const name = shieldShardNames[index];
    const binary = expectedShieldBinaries.get(name);
    const containsProductionTiming = includes.includes(productionTimingFile);
    if (containsProductionTiming && (binary !== "release" || includes.length !== 1)) {
        throw new Error(`${productionTimingFile} must be the only file in one release-runtime shard`);
    }
    const environment = environments.get(name);
    const expectedVitestArgs = containsProductionTiming
        ? productionTimingVitestArgs
        : name === "zombienet_shield_a"
          ? shieldBasicVitestArgs
          : shieldDefaultVitestArgs;
    if (!isDeepStrictEqual(environment?.vitestArgs, expectedVitestArgs)) {
        throw new Error(
            `${name} must ${containsProductionTiming || name === "zombienet_shield_a" ? "run its state-isolated cases concurrently" : "not enable test concurrency"}`
        );
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

const singleNodeConfig = "./configs/zombie_single_node.json";
const singleNodes = singleNodeSpec.relaychain?.nodes ?? [];
if (
    singleNodes.length !== 1 ||
    singleNodes[0]?.validator !== true ||
    !singleNodeSpec.relaychain?.default_args?.includes("--sealing=100")
) {
    throw new Error("Single-node state spec must contain one validator using --sealing=100");
}

for (const name of ["zombienet_staking", "zombienet_coldkey_swap", "zombienet_evm", "zombienet_subnets"]) {
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
    `Validated ${shieldFiles.length} Shield files across ${shieldShardNames.length} production-runtime shards, ${shieldShardNames.length + 1} multi-node Shield environments, and four single-node state suites.`
);
