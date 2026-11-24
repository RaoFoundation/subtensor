#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const process = require("node:process");
const { stringify: stringifyToml } = require("smol-toml");
const { params, groups } = require("./lib/params");
const { withChain, finalizedContext, readAt } = require("./lib/chain");

function inlineValue(value) {
  if (typeof value === "bigint") return value.toString();
  if (value === null) return "{ none = true }";
  if (typeof value === "object") {
    return `{ ${Object.entries(value).map(([key, item]) => `${key} = ${inlineValue(item)}`).join(", ")} }`;
  }
  return stringifyToml({ value }).trim().replace(/^value = /, "");
}

function render(values, context, errors = {}) {
  const lines = [
    "# Root-controlled chain-wide parameters only; balances are raw rao.",
    "# Fixed-point values are raw integers, not decimal fractions.",
    "# A change is a proposal until reviewed, signed, and executed on chain.",
    "# Do not include subnet-owner-settable or netuid-keyed values here.",
    "",
    "[metadata]",
    `network = ${JSON.stringify(context.network)}`,
    `block_number = ${context.blockNumber}`,
    `block_hash = ${JSON.stringify(context.blockHash)}`,
    `genesis_hash = ${JSON.stringify(context.genesisHash)}`,
    `spec_version = ${context.specVersion}`,
    "",
  ];
  for (const param of params) {
    if (!(param.name in values)) continue;
    lines.push(
      `# Storage: ${param.pallet}.${param.storage}`,
      `[${param.name}]`,
      `setter = ${JSON.stringify(`${param.callPallet}.${param.call}`)}`,
      `value = ${inlineValue(values[param.name])}`,
      "",
    );
  }
  for (const [name, group] of Object.entries(groups)) {
    if (Object.keys(group.storage).some((field) => !(`${name}.${field}` in values))) continue;
    lines.push(`# Atomic root call: all four values are written together.`, `[${name}]`);
    lines.push(`setter = ${JSON.stringify(`${group.callPallet}.${group.call}`)}`);
    for (const [field, [pallet, item]] of Object.entries(group.storage)) {
      lines.push(`# Storage: ${pallet}.${item}`);
      lines.push(stringifyToml({ [field]: values[`${name}.${field}`] }).trimEnd());
    }
    lines.push("");
  }
  if (Object.keys(errors).length) {
    lines.push("# UNREAD LIVE VALUES (not exported):");
    for (const [key, error] of Object.entries(errors)) lines.push(`# ${key}: ${error.replaceAll("\n", " ")}`);
  }
  return lines.join("\n");
}

function usage() {
  console.error("Usage: node export.js <devnet|testnet|mainnet|all> [--output-dir DIR] [--ws URL]");
}

async function main() {
  const [network = "all", ...args] = process.argv.slice(2);
  let outputDir = __dirname;
  let endpoint = null;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--output-dir") outputDir = args[++i];
    else if (args[i] === "--ws") endpoint = args[++i];
    else throw new Error(`Unknown argument: ${args[i]}`);
  }
  if (!network || !outputDir) { usage(); process.exitCode = 2; return; }
  const networks = JSON.parse(fs.readFileSync(path.join(__dirname, "networks.json"), "utf8"));
  if (network !== "all" && !networks[network]) { usage(); process.exitCode = 2; return; }
  let failed = false;
  for (const [name, config] of Object.entries(networks)) {
    if (network !== "all" && network !== name) continue;
    try {
      const result = await withChain(endpoint || config.endpoint, async (api) => {
        const context = await finalizedContext(api);
        return { context, ...(await readAt(api, context.blockHash)) };
      });
      if (Object.keys(result.errors).length) failed = true;
      fs.mkdirSync(outputDir, { recursive: true });
      fs.writeFileSync(path.join(outputDir, `${name}.toml`), render(result.values, { network: name, ...result.context }, result.errors));
      console.log(`${name}: exported ${Object.keys(result.values).length} values at finalized block ${result.context.blockNumber}`);
      for (const [key, error] of Object.entries(result.errors)) console.error(`${name}: UNREAD ${key}: ${error}`);
    } catch (error) {
      failed = true;
      console.error(`${name}: export failed: ${error.message}`);
    }
  }
  if (failed) process.exitCode = 1;
}

if (require.main === module) main().catch((error) => { console.error(error); process.exitCode = 1; });
module.exports = { render };
