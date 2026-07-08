import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const MAX_BLOCKS_TO_WAIT = Number(process.env.NET_TAO_FLOW_WAIT_BLOCKS ?? 120);

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri(process.env.NET_TAO_FLOW_SUDO_URI ?? "//Alice");
const logger = createTempLogger("test-net-tao-flow-emission-allocation.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connect();

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const startHeader = await api.rpc.chain.getHeader();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("start block:", startHeader.number.toString());

    assertMetadataAvailable();
    runNetTaoFlowFormulaChecks();
    await assertLiveFlowAuditStorage();

    const originalNetTaoFlowEnabled = await api.query.subtensorModule.netTaoFlowEnabled();
    console.log("initial NetTaoFlowEnabled:", originalNetTaoFlowEnabled.toString());

    try {
      const disabledAt = await setNetTaoFlowEnabled(false);
      assert.equal(
        (await api.query.subtensorModule.netTaoFlowEnabled()).isTrue,
        false,
        "NetTaoFlowEnabled did not switch off"
      );

      const disabledRows = await waitForProtocolEmaAtOrAfter(disabledAt.number);
      const disabledSample = maxProtocolBlockRow(disabledRows);
      assert.equal(
        (await api.query.subtensorModule.netTaoFlowEnabled()).isTrue,
        false,
        "NetTaoFlowEnabled changed while checking disabled path"
      );
      assert.equal(computeNormFactor(disabledRows, false), 0, "disabled norm factor must be zero");
      console.log(
        "disabled protocol EMA warmed:",
        `disableBlock=${disabledAt.number}`,
        `netuid=${disabledSample.netuid}`,
        `protocolBlock=${disabledSample.protocolBlock}`,
        `protocol=${disabledSample.protocol}`
      );
      console.log("disabled formula path: pure gross flow confirmed by norm_factor=0");

      await setNetTaoFlowEnabled(true);
      assert.equal(
        (await api.query.subtensorModule.netTaoFlowEnabled()).isTrue,
        true,
        "NetTaoFlowEnabled did not switch on"
      );

      const enabledRows = await readFlowRows();
      const normFactor = computeNormFactor(enabledRows, true);
      assert.ok(normFactor >= 0 && normFactor <= 1, `enabled norm factor ${normFactor} is outside [0, 1]`);
      console.log("enabled flow rows checked:", enabledRows.length);
      console.log("enabled norm factor:", normFactor.toFixed(12));
      console.log("net tao flow emission allocation live-clone test: ok");
    } finally {
      await setNetTaoFlowEnabled(originalNetTaoFlowEnabled.isTrue);
      console.log("restored NetTaoFlowEnabled:", originalNetTaoFlowEnabled.toString());
    }
  } finally {
    await api.disconnect();
  }
}

async function connect() {
  return connectApi(WS_ENDPOINT, { log: console.log });
}

main().then(() => logger.flush()).catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["SubtensorModule.NetTaoFlowEnabled", api.query.subtensorModule?.netTaoFlowEnabled],
    ["SubtensorModule.SubnetExcessTao", api.query.subtensorModule?.subnetExcessTao],
    ["SubtensorModule.SubnetRootSellTao", api.query.subtensorModule?.subnetRootSellTao],
    ["SubtensorModule.SubnetProtocolFlow", api.query.subtensorModule?.subnetProtocolFlow],
    ["SubtensorModule.SubnetEmaTaoFlow", api.query.subtensorModule?.subnetEmaTaoFlow],
    ["SubtensorModule.SubnetEmaProtocolFlow", api.query.subtensorModule?.subnetEmaProtocolFlow],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to the PR runtime`
  );
}

function runNetTaoFlowFormulaChecks() {
  const accounting = [
    {
      netuid: 1,
      userBuys: 1_000,
      userSells: 100,
      emissionInjected: 300,
      chainBuys: 80,
      rootSells: 50,
    },
    {
      netuid: 2,
      userBuys: 700,
      userSells: 250,
      emissionInjected: 500,
      chainBuys: 120,
      rootSells: 20,
    },
    {
      netuid: 3,
      userBuys: 250,
      userSells: 400,
      emissionInjected: 50,
      chainBuys: 0,
      rootSells: 200,
    },
  ].map(toFlowRow);

  assert.deepEqual(
    accounting.map(({ user }) => user),
    [900, 450, -150],
    "user flow should track buys-sells only"
  );
  assert.deepEqual(
    accounting.map(({ protocol }) => protocol),
    [330, 600, -150],
    "protocol flow should track emission + chain buys - root sells only"
  );
  assert.deepEqual(
    applyRawNetFlow(accounting).map(({ net }) => net),
    [570, -150, 0],
    "PR #2634 raw net flow should subtract protocol cost and keep root-sell benefits"
  );

  const shares = normalizePositiveShares(applyRawNetFlow(accounting));
  assert.equal(shares.get(1), 1, "only the positive raw net-flow subnet should receive emission share");
  assert.equal(shares.get(2), 0, "protocol cost above user demand should filter the subnet");
  assert.equal(shares.get(3), 0, "non-positive user demand should not create positive raw net flow here");

  const steady = [
    { netuid: 1, user: 100, protocol: 40 },
    { netuid: 2, user: 200, protocol: 60 },
  ];
  const steadyFactor = computeNormFactor(steady, true);
  assert.equal(steadyFactor, 1, "steady-state factor should be one when user positive EMA covers protocol cost");
  assert.deepEqual(
    applyNetFlow(steady, true).map(({ net }) => net),
    [60, 140],
    "steady-state net flow should be user minus protocol when norm_factor is one"
  );

  const concentrated = Array.from({ length: 70 }, (_, index) => ({
    netuid: index + 1,
    user: 100,
    protocol: 40 + 4 * index,
  }));
  const concentratedFactor = computeNormFactor(concentrated, true);
  const unnormalizedEligible = concentrated.filter(({ user, protocol }) => user > protocol).length;
  const normalizedEligible = applyNetFlow(concentrated, true).filter(({ net }) => net > 0).length;
  const floor = concentrated.length / 2;

  assert.ok(concentratedFactor > 0 && concentratedFactor < 1, "concentration should drop norm_factor below one");
  assert.ok(
    normalizedEligible > unnormalizedEligible,
    "normalization should keep more subnets eligible than raw user-protocol subtraction"
  );
  assert.ok(normalizedEligible >= floor, "normalization should resist concentration collapse");

  const disabled = applyNetFlow(concentrated, false);
  assert.equal(computeNormFactor(concentrated, false), 0, "disabled norm factor should be zero");
  assert.deepEqual(
    disabled.map(({ net }) => net),
    concentrated.map(({ user }) => user),
    "disabled path should use gross user flow"
  );

  console.log(
    "formula checks: separated accounting, raw net shares, steady-state, disabled gross flow, bounded factor, concentration resistance ok"
  );
  console.log(
    "concentration sample:",
    `factor=${concentratedFactor.toFixed(12)}`,
    `unnormalizedEligible=${unnormalizedEligible}`,
    `normalizedEligible=${normalizedEligible}`
  );
}

function toFlowRow(row) {
  return {
    netuid: row.netuid,
    user: row.userBuys - row.userSells,
    protocol: row.emissionInjected + row.chainBuys - row.rootSells,
  };
}

function applyRawNetFlow(rows) {
  return rows.map(({ netuid, user, protocol }) => ({ netuid, net: user - protocol }));
}

function normalizePositiveShares(rows) {
  const positive = rows.map(({ net }) => Math.max(net, 0));
  const sum = positive.reduce((total, value) => total + value, 0);
  return new Map(rows.map(({ netuid }, index) => [netuid, sum > 0 ? positive[index] / sum : 0]));
}

function computeNormFactor(rows, enabled) {
  if (!enabled) {
    return 0;
  }

  const userPositive = rows.reduce((sum, { user }) => sum + Math.max(user, 0), 0);
  const protocolPositive = rows.reduce((sum, { protocol }) => sum + Math.max(protocol, 0), 0);
  return protocolPositive > 0 ? Math.min(1, userPositive / protocolPositive) : 0;
}

async function assertLiveFlowAuditStorage() {
  const [userEntries, protocolEntries, excessEntries, rootSellEntries, protocolFlowEntries] = await Promise.all([
    api.query.subtensorModule.subnetEmaTaoFlow.entries(),
    api.query.subtensorModule.subnetEmaProtocolFlow.entries(),
    api.query.subtensorModule.subnetExcessTao.entries(),
    api.query.subtensorModule.subnetRootSellTao.entries(),
    api.query.subtensorModule.subnetProtocolFlow.entries(),
  ]);

  assert.ok(userEntries.length > 0, "SubnetEmaTaoFlow should have live user EMA entries");
  assert.ok(protocolEntries.length > 0, "SubnetEmaProtocolFlow should have live protocol EMA entries");

  const protocolRows = await readFlowRows();
  assert.ok(
    protocolRows.some(({ user }) => user !== 0),
    "live user EMA rows should contain non-zero flow values"
  );
  assert.ok(
    protocolRows.some(({ protocol }) => protocol !== 0),
    "live protocol EMA rows should contain non-zero protocol cost values"
  );

  const sample = protocolRows.find(({ user, protocol }) => user !== 0 && protocol !== 0) ?? protocolRows[0];
  const [sampleExcess, sampleRootSell, sampleProtocolFlow] = await Promise.all([
    api.query.subtensorModule.subnetExcessTao(sample.netuid),
    api.query.subtensorModule.subnetRootSellTao(sample.netuid),
    api.query.subtensorModule.subnetProtocolFlow(sample.netuid),
  ]);

  console.log(
    "live audit storage:",
    `userEma=${userEntries.length}`,
    `protocolEma=${protocolEntries.length}`,
    `excess=${excessEntries.length}`,
    `rootSell=${rootSellEntries.length}`,
    `protocolAccumulator=${protocolFlowEntries.length}`,
    `sampleNetuid=${sample.netuid}`,
    `sampleExcess=${sampleExcess.toString()}`,
    `sampleRootSell=${sampleRootSell.toString()}`,
    `sampleProtocolFlow=${sampleProtocolFlow.toString()}`
  );
}

function applyNetFlow(rows, enabled) {
  const normFactor = computeNormFactor(rows, enabled);
  return rows.map(({ netuid, user, protocol }) => {
    const scaledProtocol = enabled && protocol > 0 ? normFactor * protocol : enabled ? protocol : 0;
    return { netuid, net: user - scaledProtocol };
  });
}

async function setNetTaoFlowEnabled(enabled) {
  const key = api.query.subtensorModule.netTaoFlowEnabled.key();
  const value = enabled ? "0x01" : "0x00";
  const label = `set NetTaoFlowEnabled=${enabled}`;
  const blockHash = await submitAndWait(api.tx.sudo.sudo(api.tx.system.setStorage([[key, value]])), label);
  const header = await api.rpc.chain.getHeader(blockHash);
  const number = header.number.toNumber();
  console.log(`${label}: ok at block ${number}`);
  return { blockHash, number };
}

async function waitForProtocolEmaAtOrAfter(blockNumber) {
  for (let blocks = 1; blocks <= MAX_BLOCKS_TO_WAIT; blocks++) {
    const current = await readFlowRows();
    if (current.some(({ protocolBlock }) => protocolBlock >= blockNumber)) {
      return current;
    }

    await waitForFinalizedBlock();
    if (blocks % 20 === 0) {
      console.log(`waited ${blocks}/${MAX_BLOCKS_TO_WAIT} finalized blocks for protocol EMA at or after ${blockNumber}`);
    }
  }

  throw new Error(`SubnetEmaProtocolFlow did not reach block ${blockNumber} within ${MAX_BLOCKS_TO_WAIT} finalized blocks`);
}

async function readFlowRows() {
  const [userEntries, protocolEntries] = await Promise.all([
    api.query.subtensorModule.subnetEmaTaoFlow.entries(),
    api.query.subtensorModule.subnetEmaProtocolFlow.entries(),
  ]);
  const rows = new Map();

  for (const [key, value] of userEntries) {
    const netuid = key.args[0].toNumber();
    const decoded = decodeOptionalEma(value);
    if (decoded) {
      rows.set(netuid, { netuid, userBlock: decoded.block, user: decoded.flow, protocolBlock: 0, protocol: 0 });
    }
  }

  for (const [key, value] of protocolEntries) {
    const netuid = key.args[0].toNumber();
    const decoded = decodeOptionalEma(value);
    if (!decoded) {
      continue;
    }

    const row = rows.get(netuid) ?? { netuid, userBlock: 0, user: 0, protocolBlock: 0, protocol: 0 };
    row.protocolBlock = decoded.block;
    row.protocol = decoded.flow;
    rows.set(netuid, row);
  }

  return [...rows.values()].sort((a, b) => a.netuid - b.netuid);
}

function maxProtocolBlockRow(rows) {
  const sorted = [...rows].sort((a, b) => b.protocolBlock - a.protocolBlock);
  assert.ok(sorted[0]?.protocolBlock > 0, "no SubnetEmaProtocolFlow entries were initialized");
  return sorted[0];
}

function decodeOptionalEma(value) {
  if (value.isNone) {
    return null;
  }

  const tuple = value.unwrap ? value.unwrap() : value;
  return {
    block: tuple[0].toNumber(),
    flow: fixedI64F64ToNumber(tuple[1]),
  };
}

function fixedI64F64ToNumber(value) {
  const raw = codecToBigInt(value);
  const scale = 2n ** 64n;
  const whole = raw / scale;
  const fraction = raw % scale;
  return Number(whole) + Number(fraction) / Number(scale);
}

function codecToBigInt(value) {
  if (typeof value.toBigInt === "function") {
    return value.toBigInt();
  }

  const json = value.toJSON?.();
  if (json && typeof json === "object" && typeof json.bits === "string") {
    return signedI128FromHex(json.bits);
  }

  const stringValue = value.toString().replaceAll(",", "");
  if (stringValue.includes(".")) {
    return BigInt(Math.trunc(Number(stringValue) * 2 ** 64));
  }
  return BigInt(stringValue);
}

function signedI128FromHex(hex) {
  const raw = BigInt(hex);
  const signBit = 1n << 127n;
  const modulus = 1n << 128n;
  return raw & signBit ? raw - modulus : raw;
}

async function submitAndWait(tx, label) {
  return new Promise((resolve, reject) => {
    let unsubscribe;
    let settled = false;

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      unsubscribe?.();
      fn(value);
    };

    tx.signAndSend(alice, ({ status, events, dispatchError }) => {
      if (dispatchError) {
        finish(reject, new Error(`${label} failed: ${formatDispatchError(dispatchError)}`));
        return;
      }

      if (status.isInBlock || status.isFinalized) {
        for (const { event } of events) {
          if (event.section === "system" && event.method === "ExtrinsicFailed") {
            const [error] = event.data;
            finish(reject, new Error(`${label} failed: ${formatDispatchError(error)}`));
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

function waitForFinalizedBlock() {
  return new Promise((resolve, reject) => {
    let unsubscribe;
    let settled = false;

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      unsubscribe?.();
      fn(value);
    };

    api.rpc.chain
      .subscribeFinalizedHeads((header) => finish(resolve, header))
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

function formatDispatchError(error) {
  if (!error.isModule) {
    return error.toString();
  }

  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}
