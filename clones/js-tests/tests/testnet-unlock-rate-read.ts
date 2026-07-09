import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const logger = createTempLogger("testnet-unlock-rate-read.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const header = await api.rpc.chain.getHeader();

    assert.ok(
      api.query.subtensorModule?.unlockRate,
      "SubtensorModule.UnlockRate storage is unavailable on this endpoint"
    );

    // Meaningless comment for commit plumbing.
    const unlockRate = await api.query.subtensorModule.unlockRate();
    const unlockRateValue = unlockRate.toBigInt();

    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("block:", header.number.toString());
    console.log("SubtensorModule.UnlockRate:", unlockRateValue.toString());

    assert.ok(unlockRateValue > 0n, `expected positive UnlockRate, got ${unlockRateValue}`);
    console.log("testnet unlock rate read: ok");
  } finally {
    await api?.disconnect();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});
