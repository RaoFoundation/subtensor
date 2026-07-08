import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://dev.chain.opentensor.ai:443";
const RAO_PER_TAO = 1_000_000_000n;
const PERQUINTILL = 1_000_000_000_000_000_000n;
const PRICE_TOLERANCE_PPM = 1_000n;

const logger = createTempLogger("test-balancer-devnet-status.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const header = await api.rpc.chain.getHeader();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("block:", header.number.toString());

    assertMetadataAvailable();

    const sudoKey = api.query.sudo?.key ? (await api.query.sudo.key()).toString() : null;
    console.log("sudo key:", sudoKey ?? "unavailable");

    const initializedEntries = await api.query.swap.palSwapInitialized.entries();
    const initializedNetuids = initializedEntries
      .filter(([, initialized]) => initialized.isTrue)
      .map(([key]) => key.args[0].toNumber());
    const balancerEntries = await api.query.swap.swapBalancer.entries();
    const networkEntries = await api.query.subtensorModule.networksAdded.entries();
    const addedNetuids = networkEntries
      .filter(([, added]) => added.isTrue)
      .map(([key]) => key.args[0].toNumber())
      .sort((a, b) => a - b);

    console.log("networks added:", addedNetuids.join(", "));
    console.log("PalSwapInitialized entries:", initializedEntries.length);
    console.log("PalSwapInitialized true:", initializedNetuids.length ? initializedNetuids.join(", ") : "none");
    console.log(
      "SwapBalancer entries:",
      balancerEntries.length
        ? balancerEntries.map(([key, value]) => `${key.args[0].toString()}:${formatPerquintill(extractQuotePerquintill(value))}`).join(", ")
        : "none"
    );

    if (initializedNetuids.length > 0) {
      await assertInitializedBalancerPrices(initializedNetuids);
    } else {
      await assertUninitializedReservePrices(addedNetuids);
      console.log("devnet has no initialized PalSwap pools; existing balancer operation tests cannot verify live balancer behavior on this chain state");
    }
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
  const missing = [
    ["Swap.SwapBalancer", api.query.swap?.swapBalancer],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `${missing.map(([name]) => name).join(", ")} unavailable`);
}

async function assertInitializedBalancerPrices(netuids) {
  const checked = [];
  for (const netuid of netuids) {
    const [tao, alpha, balancer] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
      api.query.swap.swapBalancer(netuid),
    ]);
    const snapshot = {
      netuid,
      tao: tao.toBigInt(),
      alpha: alpha.toBigInt(),
      weight: extractQuotePerquintill(balancer),
    };
    if (snapshot.tao === 0n || snapshot.alpha === 0n || snapshot.weight === null) continue;

    const expected = weightedBalancerPrice(snapshot);
    const actual = await currentAlphaPriceRpc(netuid);
    assertWithinRelativeTolerance(actual, expected, PRICE_TOLERANCE_PPM, `netuid ${netuid} initialized balancer price`);
    checked.push(`${netuid}:expected=${expected},rpc=${actual},quote=${formatPerquintill(snapshot.weight)}`);
  }

  assert.ok(checked.length > 0, "initialized PalSwap pools exist, but none had non-zero reserves and decodable balancer weights");
  console.log("initialized balancer price checks:", checked.join("; "));
}

async function assertUninitializedReservePrices(netuids) {
  const checked = [];
  for (const netuid of netuids) {
    if (netuid === 0) continue;
    const [tao, alpha] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
    ]);
    const taoReserve = tao.toBigInt();
    const alphaReserve = alpha.toBigInt();
    if (taoReserve === 0n || alphaReserve === 0n) continue;

    const expected = (taoReserve * RAO_PER_TAO) / alphaReserve;
    const actual = await currentAlphaPriceRpc(netuid);
    assertWithinRelativeTolerance(actual, expected, PRICE_TOLERANCE_PPM, `netuid ${netuid} uninitialized reserve price`);
    checked.push(`${netuid}:tao=${taoReserve},alpha=${alphaReserve},expected=${expected},rpc=${actual}`);
  }

  assert.ok(checked.length > 0, "no non-root non-zero reserve subnet was available for uninitialized price checks");
  console.log("uninitialized reserve price checks:", checked.join("; "));
}

async function currentAlphaPriceRpc(netuid) {
  if (api.rpc.swap?.currentAlphaPrice) {
    return (await api.rpc.swap.currentAlphaPrice(netuid)).toBigInt();
  }

  const value = await api._rpcCore.provider.send("swap_currentAlphaPrice", [netuid, null]);
  return BigInt(value.toString());
}

function weightedBalancerPrice({ tao, alpha, weight }) {
  assert.ok(weight !== null, "missing balancer quote weight");
  assert.ok(weight > 0n && weight < PERQUINTILL, `invalid balancer quote weight ${weight}`);
  assert.ok(alpha > 0n, "cannot calculate price with zero alpha reserve");

  const baseWeight = PERQUINTILL - weight;
  return (baseWeight * tao * RAO_PER_TAO) / (weight * alpha);
}

function assertWithinRelativeTolerance(actual, expected, tolerancePpm, label) {
  const diff = actual > expected ? actual - expected : expected - actual;
  const allowed = (expected * BigInt(tolerancePpm)) / 1_000_000n + 1n;
  assert.ok(diff <= allowed, `${label}: actual=${actual}, expected=${expected}, diff=${diff}, allowed=${allowed}`);
}

function extractQuotePerquintill(value) {
  if (value.quote?.toBigInt) {
    return value.quote.toBigInt();
  }

  const json = value.toJSON();
  if (json && typeof json === "object") {
    const quote = json.quote ?? json.Quote;
    if (typeof quote === "number" || typeof quote === "string") {
      return BigInt(quote);
    }
  }

  const human = value.toHuman();
  if (human && typeof human === "object") {
    const quote = human.quote ?? human.Quote;
    if (typeof quote === "string") {
      return quote.endsWith("%")
        ? BigInt(Math.round(Number(quote.slice(0, -1)) * 10_000_000_000_000_000))
        : BigInt(quote.replaceAll(",", ""));
    }
  }

  return null;
}

function formatPerquintill(value) {
  if (value === null) return "unknown";
  const integer = value / PERQUINTILL;
  const fractional = (value % PERQUINTILL).toString().padStart(18, "0");
  return `${integer}.${fractional.slice(0, 6)}`;
}
