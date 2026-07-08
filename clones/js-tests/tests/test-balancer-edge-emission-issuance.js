import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const MAX_BLOCKS_TO_WAIT = Number(process.env.MAX_EDGE_EMISSION_WAIT_BLOCKS ?? 120);
const HIGH_QUOTE_WEIGHT = 990_000_000_000_000_000n;
const LOW_QUOTE_WEIGHT = 10_000_000_000_000_000n;
const HIGH_EDGE_FLOOR = 980_000_000_000_000_000n;
const LOW_EDGE_CEILING = 20_000_000_000_000_000n;
const EDGE_TAO_RESERVE = 1_000n;
const EDGE_ALPHA_RESERVE = 1_000_000_000_000n;

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri(process.env.SUDO_URI ?? "//Alice");
const logger = createTempLogger("test-balancer-edge-emission-issuance.log");
logger.captureConsole();

let api;
let lastFinalizedBlockNumber = null;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const startHeader = await api.rpc.chain.getHeader();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("start block:", startHeader.number.toString());

    if (!assertMetadataAvailable()) {
      return;
    }
    await repairIssuanceMirrorIfNeeded("pre-test setup");
    await assertIssuanceMatch("initial");

    const netuid = await findEmissionSubnet();
    console.log("selected netuid:", netuid);

    const original = await captureOriginals(netuid);

    try {
      await sudoSetStorage(
        [[api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", 1)]],
        `sudo set tempo 1 for netuid ${netuid}`
      );

      const highEdge = await runEdgeWeightScenario(netuid, HIGH_QUOTE_WEIGHT, "quote=0.99 high edge");
      assert.ok(
        highEdge.after.taoReservoir > highEdge.before.taoReservoir,
        `high-edge scenario did not leave non-zero BalancerTaoReservoir: before=${highEdge.before.taoReservoir}, after=${highEdge.after.taoReservoir}`
      );

      await runEdgeWeightScenario(netuid, LOW_QUOTE_WEIGHT, "quote=0.01 low edge");
    } finally {
      await restoreOriginals(netuid, original);
    }

    await assertIssuanceMatch("final after restore");
    console.log("balancer edge emission issuance scenarios: ok");
  } finally {
    await api?.disconnect();
    await logger.flush();
  }
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  if (!api.query.swap?.balancerTaoReservoir && !api.query.swap?.balancerAlphaReservoir) {
    // Runtime 420 removed the balancer reservoir maps this historical edge test targeted.
    // The current swap pallet exposes ScrapReservoirAlpha only, so there is no TAO reservoir
    // issuance edge path left for this test to force and observe.
    console.log(
      "balancer edge emission issuance scenarios: skipped obsolete reservoir storage",
      "missing=Swap.BalancerTaoReservoir,Swap.BalancerAlphaReservoir",
      `available_swap_storage=${Object.keys(api.query.swap ?? {}).sort().join(",")}`
    );
    return false;
  }

  const missing = [
    ["Balances.TotalIssuance", api.query.balances?.totalIssuance],
    ["SubtensorModule.TotalIssuance", api.query.subtensorModule?.totalIssuance],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
    ["SubtensorModule.SubnetExcessTao", api.query.subtensorModule?.subnetExcessTao],
    ["SubtensorModule.SubnetTaoInEmission", api.query.subtensorModule?.subnetTaoInEmission],
    ["SubtensorModule.SubnetEmissionEnabled", api.query.subtensorModule?.subnetEmissionEnabled],
    ["SubtensorModule.Tempo", api.query.subtensorModule?.tempo],
    ["Swap.SwapBalancer", api.query.swap?.swapBalancer],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
    ["Swap.BalancerTaoReservoir", api.query.swap?.balancerTaoReservoir],
    ["Swap.BalancerAlphaReservoir", api.query.swap?.balancerAlphaReservoir],
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to the current runtime`
  );
  return true;
}

async function findEmissionSubnet() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  for (const [key, initialized] of initializedEntries) {
    if (!initialized.isTrue) continue;

    const netuid = key.args[0].toNumber();
    if (netuid === 0) continue;
    if ((await api.query.subtensorModule.networksAdded(netuid)).isFalse) continue;
    if ((await api.query.subtensorModule.subnetEmissionEnabled(netuid)).isFalse) continue;

    const [tao, alpha] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
    ]);
    if (tao.toBigInt() > 0n && alpha.toBigInt() > 0n) {
      return netuid;
    }
  }

  throw new Error("no initialized emission-enabled subnet with non-zero TAO and alpha reserves found");
}

async function runEdgeWeightScenario(netuid, quoteWeight, label) {
  const { blockHash } = await sudoSetStorage(
    [
      [api.query.swap.swapBalancer.key(netuid), balancerValueHex(quoteWeight)],
      [api.query.subtensorModule.subnetTAO.key(netuid), storageValueHex("u64", EDGE_TAO_RESERVE)],
      [api.query.subtensorModule.subnetAlphaIn.key(netuid), storageValueHex("u64", EDGE_ALPHA_RESERVE)],
      [api.query.swap.balancerTaoReservoir.key(netuid), storageValueHex("u64", 0n)],
      [api.query.swap.balancerAlphaReservoir.key(netuid), storageValueHex("u64", 0n)],
    ],
    `sudo force balancer ${label} on netuid ${netuid}`
  );

  const forcedWeight = extractQuotePerquintill(await api.query.swap.swapBalancer.at(blockHash, netuid));
  assert.ok(
    quoteWeight === HIGH_QUOTE_WEIGHT ? forcedWeight >= HIGH_EDGE_FLOOR : forcedWeight <= LOW_EDGE_CEILING,
    `${label}: forced balancer weight ${forcedWeight} is no longer edge-cased`
  );
  await assertIssuanceMatch(`${label} after force storage`);

  const setHeader = await api.rpc.chain.getHeader(blockHash);
  lastFinalizedBlockNumber = setHeader.number.toNumber();
  const before = await emissionSnapshot(netuid, blockHash);
  console.log(
    `${label} before:`,
    `block=${before.block}`,
    `tao=${before.tao}`,
    `taoReservoir=${before.taoReservoir}`,
    `alphaReservoir=${before.alphaReservoir}`,
    `sum=${before.taoPlusReservoir}`
  );

  for (let blocks = 1; blocks <= MAX_BLOCKS_TO_WAIT; blocks++) {
    const header = await waitForFinalizedBlock();
    await assertIssuanceMatch(`${label} block ${header.number.toString()}`);

    const after = await emissionSnapshot(netuid, header.hash);
    const sumDelta = after.taoPlusReservoir - before.taoPlusReservoir;

    if (sumDelta > 0n) {
      console.log(
        `${label} after:`,
        `block=${after.block}`,
        `tao=${after.tao}`,
        `taoReservoir=${after.taoReservoir}`,
        `alphaReservoir=${after.alphaReservoir}`,
        `taoInEmission=${after.taoInEmission}`,
        `subnetExcessTao=${after.subnetExcessTao}`,
        `sum=${after.taoPlusReservoir}`,
        `sumDelta=${sumDelta}`
      );
      const reservoirDelta = after.taoReservoir - before.taoReservoir;
      assert.equal(
        sumDelta,
        after.taoInEmission + after.subnetExcessTao + reservoirDelta,
        `${label}: SubnetTAO + BalancerTaoReservoir did not increase only by the observed TAO injection path`
      );
      return { before, after };
    }

    if (blocks % 30 === 0) {
      console.log(`${label}: waited ${blocks}/${MAX_BLOCKS_TO_WAIT} finalized blocks for TAO injection`);
    }
  }

  throw new Error(`${label}: no SubnetTAO + BalancerTaoReservoir increase within ${MAX_BLOCKS_TO_WAIT} finalized blocks`);
}

async function emissionSnapshot(netuid, blockHash = null) {
  const at = (query, ...args) => blockHash ? query.at(blockHash, ...args) : query(...args);
  const [header, tao, taoReservoir, alphaReservoir, taoInEmission, subnetExcessTao] = await Promise.all([
    blockHash ? api.rpc.chain.getHeader(blockHash) : api.rpc.chain.getHeader(),
    at(api.query.subtensorModule.subnetTAO, netuid),
    at(api.query.swap.balancerTaoReservoir, netuid),
    at(api.query.swap.balancerAlphaReservoir, netuid),
    at(api.query.subtensorModule.subnetTaoInEmission, netuid),
    at(api.query.subtensorModule.subnetExcessTao, netuid),
  ]);
  const taoBig = tao.toBigInt();
  const taoReservoirBig = taoReservoir.toBigInt();

  return {
    block: header.number.toString(),
    tao: taoBig,
    taoReservoir: taoReservoirBig,
    alphaReservoir: alphaReservoir.toBigInt(),
    taoInEmission: taoInEmission.toBigInt(),
    subnetExcessTao: subnetExcessTao.toBigInt(),
    taoPlusReservoir: taoBig + taoReservoirBig,
  };
}

async function captureOriginals(netuid) {
  const [tempo, balancer, subnetTao, subnetAlphaIn, taoReservoir, alphaReservoir] = await Promise.all([
    api.query.subtensorModule.tempo(netuid),
    api.query.swap.swapBalancer(netuid),
    api.query.subtensorModule.subnetTAO(netuid),
    api.query.subtensorModule.subnetAlphaIn(netuid),
    api.query.swap.balancerTaoReservoir(netuid),
    api.query.swap.balancerAlphaReservoir(netuid),
  ]);

  return {
    tempo: tempo.toBigInt(),
    balancerHex: balancer.toHex(),
    subnetTao: subnetTao.toBigInt(),
    subnetAlphaIn: subnetAlphaIn.toBigInt(),
    taoReservoir: taoReservoir.toBigInt(),
    alphaReservoir: alphaReservoir.toBigInt(),
  };
}

async function restoreOriginals(netuid, original) {
  await sudoSetStorage(
    [
      [api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", original.tempo)],
      [api.query.swap.swapBalancer.key(netuid), original.balancerHex],
      [api.query.subtensorModule.subnetTAO.key(netuid), storageValueHex("u64", original.subnetTao)],
      [api.query.subtensorModule.subnetAlphaIn.key(netuid), storageValueHex("u64", original.subnetAlphaIn)],
      [api.query.swap.balancerTaoReservoir.key(netuid), storageValueHex("u64", original.taoReservoir)],
      [api.query.swap.balancerAlphaReservoir.key(netuid), storageValueHex("u64", original.alphaReservoir)],
    ],
    `sudo restore edge-emission storage on netuid ${netuid}`
  );
  console.log(`restored original balancer storage for netuid ${netuid}`);
}

async function repairIssuanceMirrorIfNeeded(label) {
  const balances = (await api.query.balances.totalIssuance()).toBigInt();
  const subtensor = (await api.query.subtensorModule.totalIssuance()).toBigInt();
  const diff = balances - subtensor;
  if (diff === 0n) {
    console.log(`${label}: issuance matched`, balances.toString());
    return;
  }

  const target = balances + diff;
  assert.ok(target > 0n, `cannot repair issuance mirror: computed target ${target}`);
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(api.tx.system.setStorage([
      [api.query.subtensorModule.totalIssuance.key(), storageValueHex("u64", target)],
    ])),
    "sudo repair Subtensor TotalIssuance mirror"
  );
  await assertIssuanceMatch(`${label} repaired`);
}

async function assertIssuanceMatch(label) {
  const [balancesIssuance, subtensorIssuance] = await Promise.all([
    api.query.balances.totalIssuance(),
    api.query.subtensorModule.totalIssuance(),
  ]);
  const balances = balancesIssuance.toBigInt();
  const subtensor = subtensorIssuance.toBigInt();
  assert.equal(
    subtensor,
    balances,
    `${label}: SubtensorModule.TotalIssuance ${subtensor} does not match Balances.TotalIssuance ${balances}`
  );
  console.log(`${label}: issuance match`, balances.toString());
}

async function sudoSetStorage(entries, label) {
  return submitAndWait(alice, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), label);
}

async function submitAndWait(signer, tx, label) {
  return new Promise((resolve, reject) => {
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
        finish(resolve, { blockHash: status.asFinalized.toString(), events });
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
      .subscribeFinalizedHeads((header) => {
        const blockNumber = header.number.toNumber();
        if (lastFinalizedBlockNumber !== null && blockNumber <= lastFinalizedBlockNumber) {
          return;
        }
        lastFinalizedBlockNumber = blockNumber;
        finish(resolve, header);
      })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

function balancerValueHex(quoteWeight) {
  const quote = api.createType("Perquintill", quoteWeight.toString());
  return u8aToHex(api.createType("PalletSubtensorSwapBalancer", { quote }).toU8a());
}

function storageValueHex(type, value) {
  return u8aToHex(api.createType(type, value).toU8a());
}

function extractQuotePerquintill(value) {
  if (value.quote?.toBigInt) {
    return value.quote.toBigInt();
  }

  const json = value.toJSON();

  if (json && typeof json === "object") {
    const quote = json.quote ?? json.Quote;
    if (typeof quote === "number") {
      return BigInt(quote);
    }
    if (typeof quote === "string" && !quote.startsWith("0x")) {
      return BigInt(quote);
    }
  }

  const human = value.toHuman();
  if (human && typeof human === "object") {
    const quote = human.quote ?? human.Quote;
    if (typeof quote === "string") {
      const normalized = quote.endsWith("%")
        ? BigInt(Math.round(Number(quote.slice(0, -1)) * 10_000_000_000_000_000))
        : BigInt(quote.replaceAll(",", ""));
      return normalized;
    }
  }

  return null;
}

function formatDispatchError(error) {
  if (!error.isModule) {
    return error.toString();
  }
  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}
