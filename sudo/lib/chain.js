"use strict";

const { ApiPromise, WsProvider } = require("@polkadot/api");
const { apiName, normalize, storagePlan } = require("./diff");

async function connect(endpoint, timeoutMs = 60_000) {
  let timer;
  try {
    return await Promise.race([
      ApiPromise.create({
        provider: new WsProvider(endpoint, 1_000),
        noInitWarn: true,
        throwOnConnect: true,
      }),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`Timed out connecting to ${endpoint}`)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function readAt(api, blockHash) {
  const values = {};
  const errors = {};
  for (const entry of storagePlan()) {
    const pallet = api.query[apiName(entry.pallet)];
    const getter = pallet?.[apiName(entry.item)];
    if (typeof getter !== "function") {
      errors[entry.key] = `Missing live storage ${entry.pallet}.${entry.item}`;
      continue;
    }
    try {
      values[entry.key] = normalize((await getter.at(blockHash)).toJSON());
    } catch (error) {
      errors[entry.key] = error.message;
    }
  }
  return { values, errors };
}

async function finalizedContext(api) {
  const blockHash = (await api.rpc.chain.getFinalizedHead()).toHex();
  const header = await api.rpc.chain.getHeader(blockHash);
  return {
    blockHash,
    blockNumber: header.number.toNumber(),
    genesisHash: api.genesisHash.toHex(),
    specVersion: api.consts.system.version.specVersion.toNumber(),
  };
}

async function withChain(endpoint, callback) {
  const api = await connect(endpoint);
  try {
    return await callback(api);
  } finally {
    await api.disconnect();
  }
}

module.exports = { connect, finalizedContext, readAt, withChain };
