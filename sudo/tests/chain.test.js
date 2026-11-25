"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const { readAt } = require("../lib/chain");

test("live large integers use decimal primitive values and runtime acronyms resolve", async () => {
  const getter = Object.assign(() => {}, {
    at: async (block) => {
      assert.equal(block, "finalized");
      return { toPrimitive: () => "18446744073709551615", toJSON: () => "0xffffffffffffffff" };
    },
  });
  const { values, errors } = await readAt({
    query: { subtensorModule: { ckBurn: getter }, evmChainId: { chainId: getter } },
  }, "finalized");
  assert.equal(values.ck_burn, "18446744073709551615");
  assert.equal(values.evm_chain_id, "18446744073709551615");
  assert.equal(errors.ck_burn, undefined);
  assert.equal(errors.evm_chain_id, undefined);
});

for (const mode of ["reject", "timeout"]) {
  test(`connection ${mode} disconnects provider and exits CLI`, () => {
    const result = spawnSync(process.execPath, ["-e", `
      const { ApiPromise, WsProvider } = require("@polkadot/api");
      WsProvider.prototype.connect = async () => {};
      WsProvider.prototype.disconnect = async () => { console.log("DISCONNECTED"); };
      ApiPromise.create = () => ${mode === "reject" ? 'Promise.reject(new Error("rejected"))' : 'new Promise(() => {})'};
      const { connect, runCli } = require("./lib/chain");
      setInterval(() => {}, 10_000); // model a stale timeout from the socket
      runCli(() => connect("ws://localhost:1", 30));
    `], { cwd: require("node:path").resolve(__dirname, ".."), encoding: "utf8", timeout: 5000 });
    assert.equal(result.status, 1, result.error?.message);
    assert.match(result.stdout, /DISCONNECTED/);
    assert.match(result.stderr, mode === "reject" ? /rejected/ : /Timed out/);
  });
}
