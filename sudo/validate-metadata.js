#!/usr/bin/env node
"use strict";

const networks = require("./networks.json");
const { compare, encodeProposal, normalize, storagePlan } = require("./lib/diff");
const { withChain, finalizedContext, readAt, runCli } = require("./lib/chain");

function alternatives(value, key) {
  if (typeof value === "boolean") return [!value];
  if (key === "tao_usd_price_oracle") {
    return [value === null ? "5E7RCRrPVS8TckCDjr92B5ciGziwz2kfvxe4URy3L7AgirGJ" : null];
  }
  if (key === "basket_trade_price_reference") {
    return [value === "SpotOnly" ? "StrictestEmaAndSpot" : "SpotOnly"];
  }
  const step = (n) => BigInt(n) > 0n ? (BigInt(n) - 1n).toString() : "1";
  if (value && typeof value === "object" && "bits" in value) {
    return [{ bits: key === "tao_flow_cutoff" ? (value.bits === "-1" ? "0" : "-1") : step(value.bits) }];
  }
  if (typeof value === "string" && /^\d+$/.test(value)) return [step(value)];
  throw new Error(`No alternate metadata test value for ${key}`);
}

function validateMetadata(live, api) {
  for (const { key } of storagePlan()) {
    const current = normalize(live[key]);
    // Also exercise both oracle representations and false independent of live configuration.
    const samples = alternatives(current, key);
    if (key === "tao_usd_price_oracle") samples.push(current);
    if (typeof current === "boolean") samples.push(current);
    for (const value of samples) {
      const before = { ...live, [key]: alternatives(value, key)[0] };
      const diff = compare({ ...live, [key]: value }, before, before);
      try {
        if (encodeProposal(diff, api)?.setters.length !== 1) throw new Error("Expected one setter");
      } catch (error) {
        throw new Error(`${key}: ${error.message}`, { cause: error });
      }
    }
  }
}

async function main() {
  const network = process.argv[2];
  if (!Object.hasOwn(networks, network)) throw new Error("Expected a configured network");
  await withChain(networks[network].endpoint, async (api) => {
    const { blockHash, blockNumber, specVersion } = await finalizedContext(api);
    const live = await readAt(api, blockHash);
    if (Object.keys(live.errors).length) throw new Error(JSON.stringify(live.errors));
    validateMetadata(live.values, api);
    console.log(`${network}: validated all ${storagePlan().length} parameters at block ${blockNumber} (spec ${specVersion})`);
  });
}
if (require.main === module) runCli(main);
module.exports = { validateMetadata };
