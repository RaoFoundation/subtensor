// js-tests/tests/clone-smoke-test.js
import { ApiPromise, WsProvider } from "@polkadot/api";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = "ws://127.0.0.1:9944";
const logger = createTempLogger("clone-smoke-test.log");

async function main() {
  await logger.start();
  logger.info(`Connecting to ${WS_ENDPOINT} ...`);
  const provider = new WsProvider(WS_ENDPOINT);
  const api = await ApiPromise.create({ provider });
  await api.isReady;
  logger.info("Connected.");

  try {
    const chain = await api.rpc.system.chain();
    const header = await api.rpc.chain.getHeader();

    await logger.info("chain:", chain.toString());
    await logger.info("block:", header.number.toString());
  } finally {
    await api.disconnect();
  }
}

main().catch((error) => {
  void logger.error(error);
  process.exit(1);
});
