import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const logger = createTempLogger("testnet-maturity-rate-read.log");
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
      api.query.subtensorModule?.maturityRate,
      "SubtensorModule.MaturityRate storage is unavailable on this endpoint"
    );

    const maturityRate = await api.query.subtensorModule.maturityRate();
    const maturityRateValue = maturityRate.toBigInt();

    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("block:", header.number.toString());
    console.log("SubtensorModule.MaturityRate:", maturityRateValue.toString());

    assert.ok(maturityRateValue > 0n, `expected positive MaturityRate, got ${maturityRateValue}`);
    console.log("testnet maturity rate read: ok");
  } finally {
    await api?.disconnect();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});
