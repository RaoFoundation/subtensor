#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const process = require("node:process");
const { loadParams, compare, encodeProposal } = require("./lib/diff");
const { withChain, finalizedContext, readAt } = require("./lib/chain");
const { writeProposal } = require("./lib/proposal");
const { createKeyMulti, encodeAddress, sortAddresses } = require("@polkadot/util-crypto");

function argumentsOf(argv) {
  const options = { file: null, baseFile: null, outputDir: null, ws: null };
  for (let i = 0; i < argv.length; i++) {
    const key = { "--file": "file", "--base-file": "baseFile", "--output-dir": "outputDir", "--ws": "ws" }[argv[i]];
    if (!key || i + 1 >= argv.length) throw new Error(`Invalid argument: ${argv[i]}`);
    options[key] = argv[++i];
  }
  return options;
}

async function main() {
  const [network, ...argv] = process.argv.slice(2);
  const options = argumentsOf(argv);
  if (!network) throw new Error("Usage: node diff.js <network> [--file FILE --base-file FILE --output-dir DIR --ws URL]");
  const root = __dirname;
  const file = path.resolve(options.file || path.join(root, `${network}.toml`));
  const baseFile = options.baseFile ? path.resolve(options.baseFile) : null;
  const config = JSON.parse(fs.readFileSync(path.join(root, "networks.json"), "utf8"))[network];
  if (!config) throw new Error(`Unknown network: ${network}`);
  const desired = loadParams(fs.readFileSync(file, "utf8"));
  if (desired.metadata.network && desired.metadata.network !== network) {
    throw new Error(`File network is ${desired.metadata.network}, expected ${network}`);
  }
  const baselineFile = baseFile ? loadParams(fs.readFileSync(baseFile, "utf8")) : null;
  if (baselineFile?.metadata.network && baselineFile.metadata.network !== network) {
    throw new Error(`Base file network is ${baselineFile.metadata.network}, expected ${network}`);
  }
  const baseline = baselineFile?.values || null;
  if (baseline && Object.keys(baseline).join() !== Object.keys(desired.values).join()) {
    throw new Error("Parameter schemas differ between base and desired files");
  }
  const result = await withChain(options.ws || config.endpoint, async (api) => {
    const context = await finalizedContext(api);
    const live = await readAt(api, context.blockHash);
    if (Object.keys(live.errors).length) throw new Error(JSON.stringify(live.errors, null, 2));
    const diff = compare(desired.values, live.values, baseline);
    const encoded = encodeProposal(diff, api);
    let sudoKey = null;
    if (network === "mainnet") {
      sudoKey = (await api.query.sudo.key.at(context.blockHash)).toJSON();
      if (encoded) {
        const signers = JSON.parse(fs.readFileSync(path.join(root, "..", ".github", "scripts", "deploy", "sudo-signatories.json"), "utf8"));
        const derived = encodeAddress(createKeyMulti(sortAddresses(signers.signatories), signers.threshold), 42);
        if (derived !== sudoKey) throw new Error("Triumvirate configuration does not derive the live sudo key");
      }
    }
    return { diff, encoded, context: { ...context, sudoKey } };
  });
  const outputDir = path.resolve(options.outputDir || root);
  writeProposal(result.diff, result.encoded, network, result.context, outputDir, { file, baseFile });
  console.log(`${network}: ${result.diff.changed.length} proposed value change(s), ${result.diff.drift.length} unrelated drift item(s), ${result.diff.unchanged.length} already correct.`);
  for (const entry of result.diff.changed) console.log(`CHANGE ${entry.key}: ${JSON.stringify(entry.chain)} -> ${JSON.stringify(entry.desired)}`);
  for (const entry of result.diff.drift) console.log(`DRIFT ${entry.key}: file ${JSON.stringify(entry.desired)}, live ${JSON.stringify(entry.chain)} (not proposed)`);
  if (result.encoded) console.log(`Validated encoded call ${result.encoded.callHash}; proposal written to ${outputDir}`);
  else console.log("No proposed call data.");
}

if (require.main === module) main().catch((error) => { console.error(error.message); process.exitCode = 1; });
module.exports = { argumentsOf };
