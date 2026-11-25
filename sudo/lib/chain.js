"use strict";

const process = require("node:process");
const { ApiPromise, WsProvider } = require("@polkadot/api");
const { apiName, normalize, storagePlan } = require("./diff");

async function connect(endpoint, timeoutMs = 60_000) {
  let timer;
  const provider = new WsProvider(endpoint, false);
  try {
    return await Promise.race([
      ApiPromise.create({
        provider,
        noInitWarn: true,
        throwOnConnect: true,
      }),
      // Start only after the API attaches listeners. This is a one-shot read.
      provider.connect().then(() => new Promise(() => {})),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`Timed out connecting to ${endpoint}`)), timeoutMs);
      }),
    ]);
  } catch (error) {
    await provider.disconnect().catch(() => {});
    throw error;
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
      values[entry.key] = normalize((await getter.at(blockHash)).toPrimitive());
    } catch (error) {
      errors[entry.key] = error.message;
    }
  }
  return { values, errors };
}

async function finalizedContext(api) {
  const blockHash = (await api.rpc.chain.getFinalizedHead()).toHex();
  const [header, version] = await Promise.all([
    api.rpc.chain.getHeader(blockHash),
    api.rpc.state.getRuntimeVersion(blockHash),
  ]);
  const specVersion = version.specVersion.toNumber();
  if (specVersion !== api.runtimeVersion.specVersion.toNumber()) {
    throw new Error("Live call metadata and finalized block have different runtime versions; retry after finalization");
  }
  return {
    blockHash,
    blockNumber: header.number.toNumber(),
    genesisHash: api.genesisHash.toHex(),
    specVersion,
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

// A WebSocket implementation can leave its RPC timeout interval alive if a
// failed handshake never emits close. These are one-shot CLIs: after awaited
// disconnect and flushing output, a failed invocation must actually exit.
async function runCli(main) {
  try {
    await main();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
  if (process.exitCode) {
    await Promise.all([process.stdout, process.stderr].map((stream) =>
      new Promise((resolve) => stream.write("", resolve)),
    ));
    process.exit(process.exitCode);
  }
}

module.exports = { connect, finalizedContext, readAt, withChain, runCli };
