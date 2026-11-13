import assert from "node:assert/strict";

import { createTempLogger } from "../lib/file-log.js";
import { submitRuntimeUpgrade } from "../lib/runtime-upgrade.js";

const logger = createTempLogger("unit-runtime-upgrade.log");
const WASM = Buffer.from([0, 97, 115, 109, 1]);
const CODE = `0x${WASM.toString("hex")}`;
const BLOCK = `0x${"12".repeat(32)}`;
const signer = {};
const versionError = { isModule: true, asModule: { index: 0, error: 2 } };

function event(section, method, ...data) {
  return { event: { section, method, data } };
}

function fixture(events, installedCode: string | null = CODE, finalized = true) {
  const reads = [];
  let unsubscribed = 0;
  const api = {
    registry: {
      findMetaError: () => ({
        section: "system",
        name: "SpecVersionNeedsToIncrease",
        docs: ["Version must increase."],
      }),
    },
    tx: {
      system: { setCode: (code) => ({ code }) },
      sudo: {
        sudo: (call) => {
          assert.equal(call.code, CODE);
          return {
            signAndSend: async (actualSigner, callback) => {
              assert.equal(actualSigner, signer);
              callback({
                status: {
                  isInBlock: !finalized,
                  isFinalized: finalized,
                  asFinalized: { toString: () => BLOCK },
                },
                events,
              });
              return () => { unsubscribed += 1; };
            },
          };
        },
      },
    },
    rpc: {
      state: {
        getStorage: async (...args) => {
          reads.push(args);
          return {
            isNone: installedCode === null,
            unwrap: () => ({ toHex: () => installedCode }),
          };
        },
      },
    },
  };
  return { api, reads, unsubscribed: () => unsubscribed };
}

async function main() {
  await logger.start();
  // Same-spec setCode emits an inner error even though sudo's outer call succeeds.
  for (const finalized of [false, true]) {
    const state = fixture([
      event("sudo", "Sudid", { isErr: true, asErr: versionError }),
      event("system", "ExtrinsicSuccess"),
    ], CODE, finalized);
    await assert.rejects(
      submitRuntimeUpgrade(state.api, signer, WASM),
      /system.SpecVersionNeedsToIncrease/
    );
    assert.deepEqual(state.reads, []);
    assert.equal(state.unsubscribed(), 1);
  }
  await logger.info("same-version sudo error rejected at inclusion and finalization: ok");

  const successEvents = [
    event("sudo", "Sudid", { isErr: false, isOk: true }),
    event("system", "ExtrinsicSuccess"),
  ];
  const success = fixture(successEvents);
  assert.equal(await submitRuntimeUpgrade(success.api, signer, WASM), BLOCK);
  assert.deepEqual(success.reads, [["0x3a636f6465", BLOCK]]);
  assert.equal(success.unsubscribed(), 1);
  await logger.info("successful sudo checks exact installed code at finalized block: ok");

  for (const code of [null, "0x0061736d02"]) {
    const state = fixture(successEvents, code);
    await assert.rejects(
      submitRuntimeUpgrade(state.api, signer, WASM),
      /installed :code does not match/
    );
  }
  await logger.info("missing or different installed code rejected: ok");

  const missing = fixture([event("system", "ExtrinsicSuccess")]);
  await assert.rejects(
    submitRuntimeUpgrade(missing.api, signer, WASM),
    /without a successful Sudo.Sudid/
  );
  assert.deepEqual(missing.reads, []);
  const failed = fixture([event("system", "ExtrinsicFailed", versionError)]);
  await assert.rejects(
    submitRuntimeUpgrade(failed.api, signer, WASM),
    /system.SpecVersionNeedsToIncrease/
  );
  assert.deepEqual(failed.reads, []);
  await logger.info("missing sudo result and outer dispatch failure rejected: ok");
}

main().then(() => logger.flush()).catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
