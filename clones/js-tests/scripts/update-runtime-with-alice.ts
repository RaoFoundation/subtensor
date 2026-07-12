import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
// clones/js-tests/scripts -> monorepo root
const REPO_ROOT = path.resolve(__dirname, "..", "..", "..");
const DEFAULT_WASM_PATH = path.join(
  REPO_ROOT,
  "target",
  "release",
  "wbuild",
  "node-subtensor-runtime",
  "node_subtensor_runtime.compact.compressed.wasm"
);
const WASM_PATH = process.env.RUNTIME_WASM_PATH ?? DEFAULT_WASM_PATH;

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri("//Alice");
const logger = createTempLogger("runtime-update-alice.log");
logger.captureConsole();

function formatDispatchError(api, error) {
  if (!error.isModule) {
    return error.toString();
  }

  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}

async function submitAndWait(api, signer, tx, label) {
  return new Promise<any>((resolve, reject) => {
    let unsubscribe;
    let settled = false;

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      unsubscribe?.();
      fn(value);
    };

    tx.signAndSend(signer, ({ status, events, dispatchError }) => {
      if (dispatchError) {
        finish(reject, new Error(`${label} failed: ${formatDispatchError(api, dispatchError)}`));
        return;
      }

      if (status.isInBlock || status.isFinalized) {
        for (const { event } of events) {
          if (event.section === "system" && event.method === "ExtrinsicFailed") {
            const [error] = event.data;
            finish(reject, new Error(`${label} failed: ${formatDispatchError(api, error)}`));
            return;
          }
        }
      }

      if (status.isFinalized) {
        finish(resolve, status.asFinalized.toString());
      }
    })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

async function connect() {
  return connectApi(WS_ENDPOINT, { log: console.log });
}

async function main() {
  await logger.start();
  assert.ok(fs.existsSync(WASM_PATH), `runtime wasm not found: ${WASM_PATH}`);

  let api = await connect();

  try {
    assert.ok(api.tx.sudo?.sudo, "Sudo.sudo is not available in runtime metadata");
    assert.ok(api.tx.system?.setCode, "System.setCode is not available in runtime metadata");

    const sudoKey = await api.query.sudo.key();
    assert.equal(
      sudoKey.toString(),
      alice.address,
      `Alice is not the sudo key; sudo key is ${sudoKey.toString()}`
    );

    const before = await api.rpc.state.getRuntimeVersion();
    const wasm = fs.readFileSync(WASM_PATH);

    console.log("endpoint:", WS_ENDPOINT);
    console.log("sudo:", alice.address);
    console.log("runtime before:", before.specName.toString(), before.specVersion.toString());
    console.log("wasm:", WASM_PATH);
    console.log("wasm bytes:", wasm.length);

    const blockHash = await submitAndWait(
      api,
      alice,
      api.tx.sudo.sudo(api.tx.system.setCode(`0x${wasm.toString("hex")}`)),
      "runtime upgrade"
    );

    console.log("runtime upgrade finalized in block:", blockHash);

    await api.disconnect();
    api = await connect();

    const after = await api.rpc.state.getRuntimeVersion();
    console.log("runtime after:", after.specName.toString(), after.specVersion.toString());
  } finally {
    await api.disconnect();
  }
}

main().then(() => logger.flush()).catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
