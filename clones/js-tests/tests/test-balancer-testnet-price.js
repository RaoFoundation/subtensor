import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const EXPECTED_RUNTIME = Number(process.env.EXPECTED_RUNTIME ?? 420);
const IS_LOCAL_CLONE = /^ws:\/\/(127\.0\.0\.1|localhost):9944\b/.test(WS_ENDPOINT);
const FUNDED_URI = process.env.TESTNET_BALANCER_FUNDED_URI ?? (IS_LOCAL_CLONE ? "//Alice" : "//TestnetFunded");
const HOTKEY_URI = process.env.TESTNET_BALANCER_HOTKEY_URI ?? `${FUNDED_URI}//balancer-price-hotkey`;
const STAKE_AMOUNT = BigInt(process.env.TESTNET_BALANCER_STAKE_AMOUNT ?? "1000000000");
const MAX_PRICE = 18_446_744_073_709_551_615n;
const MIN_PRICE = 0n;
const HALF_PERQUINTILL = 500_000_000_000_000_000n;
const PERQUINTILL = 1_000_000_000_000_000_000n;
const RAO_PER_TAO = 1_000_000_000n;
const PRICE_TOLERANCE_PPM = 1_000n;
const FEE_TOLERANCE_PPM = 10_000n;

const keyring = new Keyring({ type: "sr25519" });
const funded = keyring.addFromUri(FUNDED_URI);
const fallbackHotkey = keyring.addFromUri(HOTKEY_URI);
const logger = createTempLogger("test-balancer-testnet-price.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const header = await api.rpc.chain.getHeader();
    const specVersion = runtimeVersion.specVersion.toNumber();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), specVersion.toString());
    console.log("block:", header.number.toString());
    console.log("funded coldkey:", funded.address);
    console.log("fallback hotkey:", fallbackHotkey.address);

    assert.equal(specVersion, EXPECTED_RUNTIME, `expected runtime ${EXPECTED_RUNTIME}, got ${specVersion}`);
    assertMetadataAvailable();

    const migrationSummary = await assertBalancerMigrationState();
    const target = await findStakeTarget(migrationSummary.nonHalfNetuids);
    console.log("selected price target:", `netuid=${target.netuid}`, `hotkey=${target.hotkey}`);

    await assertPriceRpcMatchesStorage(target.netuid, "before stake");
    const beforeStakePrice = await currentAlphaPriceRpc(target.netuid);
    const addSim = await simSwapTaoForAlpha(target.netuid, STAKE_AMOUNT);
    assert.ok(addSim.alphaAmount > 0n, `simSwapTaoForAlpha returned zero alpha for ${STAKE_AMOUNT}`);
    assert.ok(addSim.taoFee > 0n, "simSwapTaoForAlpha returned zero TAO fee");
    console.log("sim stake:", formatSim(addSim));

    const add = await addStake(target.hotkey, target.netuid);
    assert.ok(add.alphaAdded > 0n, "StakeAdded emitted zero alpha");
    assertFeeClose(add.feePaid, addSim.taoFee, "StakeAdded TAO fee");
    const afterStakePrice = await currentAlphaPriceRpc(target.netuid);
    assert.ok(
      afterStakePrice > beforeStakePrice,
      `staking should raise price: ${beforeStakePrice}->${afterStakePrice}`
    );
    await assertPriceRpcMatchesStorage(target.netuid, "after stake");

    const unstakeAmount = add.alphaAdded / 2n;
    assert.ok(unstakeAmount > 0n, "added alpha too small to test unstake");
    const removeSim = await simSwapAlphaForTao(target.netuid, unstakeAmount);
    assert.ok(removeSim.taoAmount > 0n, `simSwapAlphaForTao returned zero TAO for ${unstakeAmount}`);
    assert.ok(removeSim.alphaFee > 0n, "simSwapAlphaForTao returned zero alpha fee");
    console.log("sim unstake:", formatSim(removeSim));

    const remove = await removeStake(target.hotkey, target.netuid, unstakeAmount);
    assert.ok(remove.alphaRemoved > 0n, "StakeRemoved emitted zero alpha");
    assertFeeClose(remove.feePaid, removeSim.alphaFee, "StakeRemoved alpha fee");
    const afterUnstakePrice = await currentAlphaPriceRpc(target.netuid);
    assert.ok(
      afterUnstakePrice < afterStakePrice,
      `unstaking should lower price: ${afterStakePrice}->${afterUnstakePrice}`
    );
    await assertPriceRpcMatchesStorage(target.netuid, "after unstake");

    console.log(
      "testnet balancer price scenario: ok",
      `initialized=${migrationSummary.initializedCount}`,
      `nonHalf=${migrationSummary.nonHalfCount}`,
      `netuid=${target.netuid}`,
      `price=${beforeStakePrice}->${afterStakePrice}->${afterUnstakePrice}`,
      `stakeFee=${add.feePaid}`,
      `unstakeFee=${remove.feePaid}`
    );
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
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.addStakeLimit", api.tx.subtensorModule?.addStakeLimit],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `${missing.map(([name]) => name).join(", ")} unavailable`);
}

async function assertBalancerMigrationState() {
  const addedEntries = await api.query.subtensorModule.networksAdded.entries();
  const addedNetuids = addedEntries
    .filter(([, added]) => added.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);
  assert.ok(addedNetuids.length > 2, `expected more than two added subnets, got ${addedNetuids.length}`);

  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);
  assert.ok(initializedNetuids.length > 1, `expected multiple initialized balancer subnets, got ${initializedNetuids.length}`);

  const balancers = [];
  for (const netuid of initializedNetuids) {
    const [tao, alpha, balancer] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
      api.query.swap.swapBalancer(netuid),
    ]);
    const weight = extractQuotePerquintill(balancer);
    if (weight === null) continue;
    balancers.push({ netuid, tao: tao.toBigInt(), alpha: alpha.toBigInt(), weight });
  }

  assert.ok(balancers.length > 1, "no initialized balancer weights could be decoded");
  const nonHalf = balancers.filter(({ weight }) => weight !== HALF_PERQUINTILL);
  assert.ok(nonHalf.length > 0, "expected at least one initialized subnet balancer weight not equal to 0.5");

  console.log("networks added:", addedNetuids.join(", "));
  console.log("initialized balancers:", initializedNetuids.join(", "));
  console.log(
    "non-0.5 balancer weights:",
    nonHalf.slice(0, 20).map(({ netuid, weight }) => `${netuid}:${formatPerquintill(weight)}`).join(", ")
  );

  return {
    initializedCount: initializedNetuids.length,
    nonHalfCount: nonHalf.length,
    nonHalfNetuids: nonHalf.map(({ netuid }) => netuid),
  };
}

async function findStakeTarget(preferredNetuids) {
  const candidates = [...preferredNetuids];
  for (const netuid of candidates) {
    const [tao, alpha] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
    ]);
    if (tao.toBigInt() === 0n || alpha.toBigInt() === 0n) continue;

    const keys = await api.query.subtensorModule.keys.entries(netuid);
    if (keys.length > 0) {
      return { netuid, hotkey: keys[0][1].toString() };
    }
  }

  return { netuid: candidates[0], hotkey: fallbackHotkey.address };
}

async function assertPriceRpcMatchesStorage(netuid, label) {
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
  assert.ok(snapshot.weight !== null, `SwapBalancer(${netuid}) could not be decoded`);
  assert.ok(snapshot.tao > 0n, `SubnetTAO(${netuid}) is zero`);
  assert.ok(snapshot.alpha > 0n, `SubnetAlphaIn(${netuid}) is zero`);

  const expected = weightedBalancerPrice(snapshot);
  const actual = await currentAlphaPriceRpc(netuid);
  assertWithinRelativeTolerance(actual, expected, PRICE_TOLERANCE_PPM, `${label} price`);
  console.log(
    label,
    `netuid=${netuid}`,
    `tao=${snapshot.tao}`,
    `alpha=${snapshot.alpha}`,
    `quote=${formatPerquintill(snapshot.weight)}`,
    `expected=${expected}`,
    `rpc=${actual}`
  );
  return actual;
}

async function currentAlphaPriceRpc(netuid) {
  if (api.rpc.swap?.currentAlphaPrice) {
    return (await api.rpc.swap.currentAlphaPrice(netuid)).toBigInt();
  }

  const value = await api._rpcCore.provider.send("swap_currentAlphaPrice", [netuid, null]);
  return BigInt(value.toString());
}

async function simSwapTaoForAlpha(netuid, amount) {
  const value = await api._rpcCore.provider.send("swap_simSwapTaoForAlpha", [netuid, Number(amount), null]);
  return normalizeSimSwap(value, "taoForAlpha");
}

async function simSwapAlphaForTao(netuid, amount) {
  const value = await api._rpcCore.provider.send("swap_simSwapAlphaForTao", [netuid, Number(amount), null]);
  return normalizeSimSwap(value, "alphaForTao");
}

async function addStake(hotkey, netuid) {
  const before = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const result = await submitAndWait(
    funded,
    api.tx.subtensorModule.addStakeLimit(hotkey, netuid, STAKE_AMOUNT, MAX_PRICE, false),
    `addStakeLimit netuid ${netuid}`
  );
  const after = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  assert.ok(after > before, `TotalHotkeyAlpha did not increase: ${before}->${after}`);
  const event = findStakeEvent(result.events, "StakeAdded", hotkey, netuid);
  const alphaAdded = event.event.data[3].toBigInt();
  const feePaid = event.event.data[5]?.toBigInt() ?? 0n;
  console.log("addStakeLimit:", `alpha=${before}->${after}`, `alphaAdded=${alphaAdded}`, `fee=${feePaid}`);
  return { alphaAdded, feePaid };
}

async function removeStake(hotkey, netuid, amount) {
  const before = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const result = await submitAndWait(
    funded,
    api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, amount, MIN_PRICE, false),
    `removeStakeLimit netuid ${netuid}`
  );
  const after = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  assert.ok(after < before, `TotalHotkeyAlpha did not decrease: ${before}->${after}`);
  const event = findStakeEvent(result.events, "StakeRemoved", hotkey, netuid);
  const alphaRemoved = event.event.data[3].toBigInt();
  const feePaid = event.event.data[5]?.toBigInt() ?? 0n;
  console.log("removeStakeLimit:", `alpha=${before}->${after}`, `alphaRemoved=${alphaRemoved}`, `fee=${feePaid}`);
  return { alphaRemoved, feePaid };
}

function findStakeEvent(events, method, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== method) return false;
    return event.data[1].toString() === hotkey && event.data[4].toNumber() === netuid;
  });
  assert.ok(event, `${method} event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return event;
}

async function submitAndWait(signer, tx, label) {
  return new Promise((resolve, reject) => {
    let unsubscribe;
    let settled = false;

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      try {
        unsubscribe?.();
      } catch {
        // Ignore unsubscribe races.
      }
      fn(value);
    };

    tx.signAndSend(signer, ({ status, dispatchError, events }) => {
      if (dispatchError) {
        finish(reject, new Error(`${label} dispatch failed: ${formatDispatchError(dispatchError)}`));
        return;
      }

      if (status.isInBlock || status.isFinalized) {
        const failed = events.find(({ event }) => event.section === "system" && event.method === "ExtrinsicFailed");
        if (failed) {
          finish(reject, new Error(`${label} extrinsic failed: ${formatDispatchError(failed.event.data[0])}`));
          return;
        }

        finish(resolve, { status, events });
      }
    })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

function normalizeSimSwap(value, direction) {
  const decoded = Array.isArray(value) ? decodeSimSwapBytes(value) : normalizeRpcObject(value);
  if (decoded.tao_amount !== undefined || decoded.taoAmount !== undefined) {
    return {
      taoAmount: readBigInt(decoded, ["tao_amount", "taoAmount"]),
      alphaAmount: readBigInt(decoded, ["alpha_amount", "alphaAmount"]),
      taoFee: readBigInt(decoded, ["tao_fee", "taoFee"]),
      alphaFee: readBigInt(decoded, ["alpha_fee", "alphaFee"]),
      taoSlippage: readBigInt(decoded, ["tao_slippage", "taoSlippage"]),
      alphaSlippage: readBigInt(decoded, ["alpha_slippage", "alphaSlippage"]),
    };
  }

  const amountPaidIn = readBigInt(decoded, ["amount_paid_in", "amountPaidIn"]);
  const amountPaidOut = readBigInt(decoded, ["amount_paid_out", "amountPaidOut"]);
  const feePaid = readBigInt(decoded, ["fee_paid", "feePaid"]);
  return direction === "taoForAlpha"
    ? { taoAmount: amountPaidIn, alphaAmount: amountPaidOut, taoFee: feePaid, alphaFee: 0n, taoSlippage: 0n, alphaSlippage: 0n }
    : { taoAmount: amountPaidOut, alphaAmount: amountPaidIn, taoFee: 0n, alphaFee: feePaid, taoSlippage: 0n, alphaSlippage: 0n };
}

function normalizeRpcObject(value) {
  if (value && typeof value.toJSON === "function") {
    return value.toJSON();
  }
  if (value && typeof value === "object") {
    return value;
  }
  return { value: value?.toString?.() ?? String(value) };
}

function decodeSimSwapBytes(bytes) {
  assert.ok(bytes.length === 48 || bytes.length >= 32, `sim swap result was ${bytes.length} bytes`);
  if (bytes.length >= 48) {
    return {
      tao_amount: readLittleEndianU64(bytes, 0),
      alpha_amount: readLittleEndianU64(bytes, 8),
      tao_fee: readLittleEndianU64(bytes, 16),
      alpha_fee: readLittleEndianU64(bytes, 24),
      tao_slippage: readLittleEndianU64(bytes, 32),
      alpha_slippage: readLittleEndianU64(bytes, 40),
    };
  }

  return {
    amountPaidIn: readLittleEndianU64(bytes, 0),
    amountPaidOut: readLittleEndianU64(bytes, 8),
    feePaid: readLittleEndianU64(bytes, 16),
    feeToBlockAuthor: readLittleEndianU64(bytes, 24),
  };
}

function readBigInt(object, names) {
  for (const name of names) {
    if (object[name] !== undefined) {
      return BigInt(object[name].toString());
    }
  }
  return 0n;
}

function readLittleEndianU64(bytes, offset) {
  let value = 0n;
  for (let index = 7; index >= 0; index--) {
    value = (value << 8n) + BigInt(bytes[offset + index]);
  }
  return value;
}

function weightedBalancerPrice({ tao, alpha, weight }) {
  const baseWeight = PERQUINTILL - weight;
  return (baseWeight * tao * RAO_PER_TAO) / (weight * alpha);
}

function assertWithinRelativeTolerance(actual, expected, tolerancePpm, label) {
  const diff = actual > expected ? actual - expected : expected - actual;
  const allowed = (expected * BigInt(tolerancePpm)) / 1_000_000n + 1n;
  assert.ok(diff <= allowed, `${label}: actual=${actual}, expected=${expected}, diff=${diff}, allowed=${allowed}`);
}

function assertFeeClose(actual, expected, label) {
  assert.ok(actual > 0n, `${label} was not positive`);
  const diff = actual > expected ? actual - expected : expected - actual;
  const allowed = (expected * BigInt(FEE_TOLERANCE_PPM)) / 1_000_000n + 1n;
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
  const integer = value / PERQUINTILL;
  const fractional = (value % PERQUINTILL).toString().padStart(18, "0");
  return `${integer}.${fractional.slice(0, 6)}`;
}

function formatSim(sim) {
  return JSON.stringify(sim, (_key, value) => (typeof value === "bigint" ? value.toString() : value));
}

function formatDispatchError(dispatchError) {
  if (dispatchError.isModule) {
    const decoded = api.registry.findMetaError(dispatchError.asModule);
    return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
  }
  return dispatchError.toString();
}
