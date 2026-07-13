import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const tsTestsDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(tsTestsDir, "moonwall.config.json"), "utf8"));
const singleNodeSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_single_node.json"), "utf8"));
const shieldSpec = JSON.parse(readFileSync(join(tsTestsDir, "configs/zombie_extended.json"), "utf8"));
const environments = new Map(config.environments.map((environment) => [environment.name, environment]));

const shieldFiles = readdirSync(join(tsTestsDir, "suites/zombienet_shield"))
    .filter((file) => file.endsWith(".test.ts"))
    .map((file) => `suites/zombienet_shield/${file}`)
    .sort();
const shieldShardNames = ["zombienet_shield_a", "zombienet_shield_b"];
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
const shardSizes = shieldShardIncludes.map((includes) => includes.length);
if (Math.max(...shardSizes) - Math.min(...shardSizes) > 1) {
    throw new Error(`Shield shards must stay balanced; found sizes ${shardSizes.join(" and ")}`);
}
const shieldIncludes = shieldShardIncludes.flat();

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

for (const name of ["zombienet_shield", "zombienet_shield_a", "zombienet_shield_b"]) {
    const environment = environments.get(name);
    const configPath = environment?.foundation?.zombieSpec?.configPath;
    const connectionNames = new Set((environment?.connections ?? []).map((connection) => connection.name));
    if (configPath !== shieldConfig) {
        throw new Error(`${name} must use ${shieldConfig}; found ${configPath ?? "no config"}`);
    }
    if (!connectionNames.has("Node") || !connectionNames.has("NodeFull")) {
        throw new Error(`${name} must expose authority and full-node connections`);
    }
}

console.log(
    `Validated ${shieldFiles.length} Shield files, three multi-node Shield environments, and four single-node state suites.`
);
