import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";
import { clearLastRateLimitedBlocks } from "../lib/rate-limit-storage.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://dev.chain.opentensor.ai:443";
const RUN_ID = process.env.DEVNET_BALANCER_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const EXISTING_NETUID = process.env.DEVNET_BALANCER_NETUID ? Number(process.env.DEVNET_BALANCER_NETUID) : null;
const IS_LOCAL_CLONE = /^ws:\/\/(127\.0\.0\.1|localhost):9944\b/.test(WS_ENDPOINT);
const FUND_SOURCE_URI = process.env.DEVNET_BALANCER_FUND_SOURCE_URI ?? (IS_LOCAL_CLONE ? "//Alice" : "//TestnetFunded");
const OWNER_URI = process.env.DEVNET_BALANCER_OWNER_URI ?? `//DevnetBalancer//${RUN_ID}//owner`;
const OWNER_HOTKEY_URI =
  process.env.DEVNET_BALANCER_OWNER_HOTKEY_URI ?? `//DevnetBalancer//${RUN_ID}//owner-hotkey`;
const STAKER_URI = process.env.DEVNET_BALANCER_STAKER_URI ?? `//DevnetBalancer//${RUN_ID}//staker`;
const STAKE_AMOUNT = BigInt(process.env.DEVNET_BALANCER_STAKE_AMOUNT ?? "1000000000");
const STAKER_FUND_AMOUNT = BigInt(process.env.DEVNET_BALANCER_STAKER_FUND_AMOUNT ?? "5000000000");
const OWNER_EXTRA_FUND_AMOUNT = BigInt(process.env.DEVNET_BALANCER_OWNER_EXTRA_FUND_AMOUNT ?? "100000000000");
const MAX_PRICE = 18_446_744_073_709_551_615n;
const MIN_PRICE = 0n;
const RAO_PER_TAO = 1_000_000_000n;
const PERQUINTILL = 1_000_000_000_000_000_000n;
const PRICE_TOLERANCE_PPM = 1_000n;
const QUOTE_WEIGHT_TOLERANCE_PPM = 1n;

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const owner = keyring.addFromUri(OWNER_URI);
const ownerHotkey = keyring.addFromUri(OWNER_HOTKEY_URI);
const staker = keyring.addFromUri(STAKER_URI);
const logger = createTempLogger("test-balancer-devnet-live.log");
logger.captureConsole();

let api;

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
    console.log("run id:", RUN_ID);
    console.log("fund source:", fundSource.address);
    console.log("owner:", owner.address);
    console.log("owner hotkey:", ownerHotkey.address);
    console.log("staker:", staker.address);

    assertMetadataAvailable();
    if (EXISTING_NETUID === null) {
      await assertLiveDevnetFundingAvailable();
      await fundTestAccounts();
    } else {
      console.log("using existing devnet subnet:", EXISTING_NETUID);
    }

    const netuid = EXISTING_NETUID ?? (await registerSubnet());
    if (EXISTING_NETUID === null) {
      console.log("registered devnet subnet:", netuid);
    }

    const initial = await assertBalancerPrice(netuid, "after registerNetwork");
    assert.ok(initial.tao > 0n, `fresh subnet ${netuid} has zero SubnetTAO reserve`);
    assert.ok(initial.alpha > 0n, `fresh subnet ${netuid} has zero SubnetAlphaIn reserve`);
    await ensureSubtokenEnabled(netuid);

    const addSim = await simSwapTaoForAlpha(netuid, STAKE_AMOUNT);
    console.log("sim swap tao for alpha before addStakeLimit:", formatSimSwap(addSim));
    assert.ok(readSimAmount(addSim, ["amount_paid_out", "amountPaidOut"]) > 0n, "simSwapTaoForAlpha returned no alpha");

    const addedAlpha = await addStake(netuid);
    assert.ok(addedAlpha > 0n, "addStakeLimit emitted zero alpha");
    const afterAdd = await assertBalancerPrice(netuid, "after addStakeLimit");
    assert.ok(afterAdd.tao > initial.tao, `addStakeLimit did not increase TAO reserve: ${initial.tao}->${afterAdd.tao}`);
    assert.ok(
      afterAdd.alpha < initial.alpha,
      `addStakeLimit did not decrease alpha reserve: ${initial.alpha}->${afterAdd.alpha}`
    );
    assert.ok(afterAdd.price >= initial.price, `buying alpha should not lower price: ${initial.price}->${afterAdd.price}`);
    assertWithinRelativeTolerance(
      afterAdd.quoteWeight,
      initial.quoteWeight,
      QUOTE_WEIGHT_TOLERANCE_PPM,
      `staking swap changed balancer quote weight: ${initial.quoteWeight}->${afterAdd.quoteWeight}`
    );

    const removeAmount = addedAlpha / 2n;
    assert.ok(removeAmount > 0n, "added alpha was too small to test removeStakeLimit");
    const removeSim = await simSwapAlphaForTao(netuid, removeAmount);
    console.log("sim swap alpha for tao before removeStakeLimit:", formatSimSwap(removeSim));
    assert.ok(readSimAmount(removeSim, ["amount_paid_out", "amountPaidOut"]) > 0n, "simSwapAlphaForTao returned no TAO");

    const removed = await removeStake(netuid, removeAmount);
    assert.ok(removed.alphaRemoved > 0n, "removeStakeLimit emitted zero alpha removed");
    const afterRemove = await assertBalancerPrice(netuid, "after removeStakeLimit");
    assert.ok(
      afterRemove.tao < afterAdd.tao,
      `removeStakeLimit did not decrease TAO reserve: ${afterAdd.tao}->${afterRemove.tao}`
    );
    assert.ok(
      afterRemove.alpha > afterAdd.alpha,
      `removeStakeLimit did not increase alpha reserve: ${afterAdd.alpha}->${afterRemove.alpha}`
    );
    assert.ok(
      afterRemove.price <= afterAdd.price,
      `selling alpha should not raise price: ${afterAdd.price}->${afterRemove.price}`
    );
    assertWithinRelativeTolerance(
      afterRemove.quoteWeight,
      afterAdd.quoteWeight,
      QUOTE_WEIGHT_TOLERANCE_PPM,
      `unstaking swap changed balancer quote weight: ${afterAdd.quoteWeight}->${afterRemove.quoteWeight}`
    );

    console.log(
      "devnet balancer live scenario: ok",
      `netuid=${netuid}`,
      `alphaAdded=${addedAlpha}`,
      `alphaRemoved=${removed.alphaRemoved}`,
      `removeFee=${removed.feePaid}`
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
    ["Balances.transfer", balancesTransfer],
    ["Swap.SwapBalancer", api.query.swap?.swapBalancer],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
    ["SubtensorModule.registerNetwork", api.tx.subtensorModule?.registerNetwork],
    ["SubtensorModule.addStakeLimit", api.tx.subtensorModule?.addStakeLimit],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.startCall", api.tx.subtensorModule?.startCall],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.NetworkLastLockCost", api.query.subtensorModule?.networkLastLockCost],
    ["SubtensorModule.SubtokenEnabled", api.query.subtensorModule?.subtokenEnabled],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.SubnetLimit", EXISTING_NETUID === null && IS_LOCAL_CLONE ? api.query.subtensorModule?.subnetLimit : true],
    ["SubtensorModule.NetworkRateLimit", EXISTING_NETUID === null && IS_LOCAL_CLONE ? api.query.subtensorModule?.networkRateLimit : true],
    ["SubtensorModule.NetworkRegistrationStartBlock", EXISTING_NETUID === null && IS_LOCAL_CLONE ? api.query.subtensorModule?.networkRegistrationStartBlock : true],
    ["Sudo.sudo", EXISTING_NETUID === null && IS_LOCAL_CLONE ? api.tx.sudo?.sudo : true],
    ["System.setStorage", EXISTING_NETUID === null && IS_LOCAL_CLONE ? api.tx.system?.setStorage : true],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `${missing.map(([name]) => name).join(", ")} unavailable on devnet`);
}

async function assertLiveDevnetFundingAvailable() {
  const lockCost = await networkLockCost();
  const requiredOwner = lockCost + OWNER_EXTRA_FUND_AMOUNT;
  const requiredFunder = requiredOwner + STAKER_FUND_AMOUNT;
  const funderFree = await freeBalance(fundSource.address);
  console.log("network lock cost:", lockCost.toString());
  console.log("fund source free:", funderFree.toString());
  assert.ok(
    funderFree >= requiredFunder,
    [
      `fund source ${fundSource.address} has ${funderFree} rao, required at least ${requiredFunder} rao`,
      `set DEVNET_BALANCER_FUND_SOURCE_URI to a funded devnet signer or fund this account before running`,
    ].join("; ")
  );
}

async function fundTestAccounts() {
  await submitAndWait(
    fundSource,
    balancesTransfer(owner.address, (await networkLockCost()) + OWNER_EXTRA_FUND_AMOUNT),
    "fund subnet owner"
  );
  await submitAndWait(fundSource, balancesTransfer(staker.address, STAKER_FUND_AMOUNT), "fund staker");

  const ownerFree = await freeBalance(owner.address);
  const stakerFree = await freeBalance(staker.address);
  console.log("owner funded:", ownerFree.toString());
  console.log("staker funded:", stakerFree.toString());
}

async function registerSubnet() {
  if (IS_LOCAL_CLONE) {
    await prepareLocalCloneSubnetRegistration();
    const cleared = await clearLastRateLimitedBlocks(
      api,
      fundSource,
      submitAndWait,
      "clear inherited local-clone LastRateLimitedBlock before registerNetwork"
    );
    console.log("cleared local-clone rate-limit storage:", JSON.stringify(cleared));
  }

  const result = await submitAndWait(
    owner,
    api.tx.subtensorModule.registerNetwork(ownerHotkey.address),
    "registerNetwork"
  );
  const event = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkAdded"
  );
  assert.ok(event, "registerNetwork did not emit NetworkAdded");
  const netuid = event.event.data[0].toNumber();
  assert.equal((await api.query.subtensorModule.networksAdded(netuid)).isTrue, true, `netuid ${netuid} was not added`);
  return netuid;
}

async function prepareLocalCloneSubnetRegistration() {
  const activeCount = await activeNonRootSubnetCount();
  const subnetLimit = (await api.query.subtensorModule.subnetLimit()).toNumber();
  const targetLimit = Math.max(subnetLimit, activeCount + 1);
  await submitAndWait(
    fundSource,
    api.tx.sudo.sudo(api.tx.system.setStorage([
      [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", targetLimit)],
      [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", 0n)],
      [api.query.subtensorModule.networkRegistrationStartBlock.key(), storageValueHex("u64", 0n)],
    ])),
    "sudo prepare local-clone subnet registration"
  );
  console.log("local-clone registration settings:", `subnet_limit=${targetLimit}`, "network_rate_limit=0", "start_block=0");
}

async function activeNonRootSubnetCount() {
  const entries = await api.query.subtensorModule.networksAdded.entries();
  return entries.filter(([key, value]) => value.isTrue && key.args[0].toNumber() !== 0).length;
}

async function ensureSubtokenEnabled(netuid) {
  const before = await api.query.subtensorModule.subtokenEnabled(netuid);
  if (before.isTrue) {
    console.log("subtoken already enabled:", netuid);
    return;
  }

  await submitAndWait(owner, api.tx.subtensorModule.startCall(netuid), "startCall");
  const after = await api.query.subtensorModule.subtokenEnabled(netuid);
  assert.equal(after.isTrue, true, `startCall did not enable subtoken for netuid ${netuid}`);
  console.log("subtoken enabled with startCall:", netuid);
}

async function addStake(netuid) {
  const before = (await api.query.subtensorModule.totalHotkeyAlpha(ownerHotkey.address, netuid)).toBigInt();
  const result = await submitAndWait(
    staker,
    api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, STAKE_AMOUNT, MAX_PRICE, false),
    "addStakeLimit"
  );
  const after = (await api.query.subtensorModule.totalHotkeyAlpha(ownerHotkey.address, netuid)).toBigInt();
  assert.ok(after > before, `TotalHotkeyAlpha did not increase: ${before}->${after}`);
  const alphaAdded = stakeAddedFromEvents(result.events, ownerHotkey.address, netuid);
  console.log("addStakeLimit:", `totalHotkeyAlpha=${before}->${after}`, `alphaAdded=${alphaAdded}`);
  return alphaAdded;
}

async function removeStake(netuid, amount) {
  const before = (await api.query.subtensorModule.totalHotkeyAlpha(ownerHotkey.address, netuid)).toBigInt();
  const result = await submitAndWait(
    staker,
    api.tx.subtensorModule.removeStakeLimit(ownerHotkey.address, netuid, amount, MIN_PRICE, false),
    "removeStakeLimit"
  );
  const after = (await api.query.subtensorModule.totalHotkeyAlpha(ownerHotkey.address, netuid)).toBigInt();
  assert.ok(after < before, `TotalHotkeyAlpha did not decrease: ${before}->${after}`);
  const removed = stakeRemovedFromEvents(result.events, ownerHotkey.address, netuid);
  console.log(
    "removeStakeLimit:",
    `totalHotkeyAlpha=${before}->${after}`,
    `alphaRemoved=${removed.alphaRemoved}`,
    `feePaid=${removed.feePaid}`
  );
  return removed;
}

async function assertBalancerPrice(netuid, label) {
  const [tao, alpha, balancer, initialized] = await Promise.all([
    api.query.subtensorModule.subnetTAO(netuid),
    api.query.subtensorModule.subnetAlphaIn(netuid),
    api.query.swap.swapBalancer(netuid),
    api.query.swap.palSwapInitialized(netuid),
  ]);
  const snapshot = {
    netuid,
    tao: tao.toBigInt(),
    alpha: alpha.toBigInt(),
    quoteWeight: extractQuotePerquintill(balancer),
    initialized: initialized.isTrue,
  };
  assert.ok(snapshot.quoteWeight !== null, `SwapBalancer(${netuid}) could not be decoded`);
  assert.ok(snapshot.quoteWeight > 0n && snapshot.quoteWeight < PERQUINTILL, `invalid quote weight ${snapshot.quoteWeight}`);
  assert.ok(snapshot.alpha > 0n, `cannot price subnet ${netuid} with zero alpha reserve`);

  const expected = weightedBalancerPrice(snapshot);
  const actual = await currentAlphaPriceRpc(netuid);
  assertWithinRelativeTolerance(actual, expected, PRICE_TOLERANCE_PPM, `${label} weighted balancer price`);
  console.log(
    label,
    `netuid=${netuid}`,
    `initialized=${snapshot.initialized}`,
    `tao=${snapshot.tao}`,
    `alpha=${snapshot.alpha}`,
    `quoteWeight=${formatPerquintill(snapshot.quoteWeight)}`,
    `expectedPrice=${expected}`,
    `rpcPrice=${actual}`
  );
  return { ...snapshot, price: actual };
}

async function currentAlphaPriceRpc(netuid) {
  const value = await api._rpcCore.provider.send("swap_currentAlphaPrice", [netuid, null]);
  return BigInt(value.toString());
}

async function simSwapTaoForAlpha(netuid, amount) {
  return api._rpcCore.provider.send("swap_simSwapTaoForAlpha", [netuid, Number(amount), null]);
}

async function simSwapAlphaForTao(netuid, amount) {
  return api._rpcCore.provider.send("swap_simSwapAlphaForTao", [netuid, Number(amount), null]);
}

function weightedBalancerPrice({ tao, alpha, quoteWeight }) {
  const baseWeight = PERQUINTILL - quoteWeight;
  return (baseWeight * tao * RAO_PER_TAO) / (quoteWeight * alpha);
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

function stakeAddedFromEvents(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeAdded") return false;
    const [, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && alphaStaked.toBigInt() > 0n;
  });
  assert.ok(event, `StakeAdded event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return event.event.data[3].toBigInt();
}

function stakeRemovedFromEvents(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeRemoved") return false;
    const [, eventHotkey, , alphaUnstaked, eventNetuid] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && alphaUnstaked.toBigInt() > 0n;
  });
  assert.ok(event, `StakeRemoved event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return {
    alphaRemoved: event.event.data[3].toBigInt(),
    feePaid: event.event.data[5]?.toBigInt() ?? 0n,
  };
}

function readSimAmount(value, names) {
  const object = normalizeRpcObject(value);
  for (const name of names) {
    if (object[name] !== undefined) return BigInt(object[name].toString());
  }
  return 0n;
}

function formatSimSwap(value) {
  return JSON.stringify(normalizeRpcObject(value), (_key, item) =>
    typeof item === "bigint" ? item.toString() : item
  );
}

function normalizeRpcObject(value) {
  if (Array.isArray(value)) {
    return decodeSimSwapBytes(value);
  }
  if (value && typeof value.toJSON === "function") {
    return value.toJSON();
  }
  if (value && typeof value === "object") {
    return value;
  }
  return { value: value?.toString?.() ?? String(value) };
}

function decodeSimSwapBytes(bytes) {
  assert.ok(bytes.length >= 32, `sim swap result was ${bytes.length} bytes, expected at least 32`);
  return {
    amountPaidIn: readLittleEndianU64(bytes, 0),
    amountPaidOut: readLittleEndianU64(bytes, 8),
    feePaid: readLittleEndianU64(bytes, 16),
    feeToBlockAuthor: readLittleEndianU64(bytes, 24),
  };
}

function readLittleEndianU64(bytes, offset) {
  let value = 0n;
  for (let index = 7; index >= 0; index--) {
    value = (value << 8n) + BigInt(bytes[offset + index]);
  }
  return value;
}

async function networkLockCost() {
  try {
    const value = await api._rpcCore.provider.send("subnetInfo_getLockCost", []);
    return BigInt(value.toString());
  } catch {
    // Older/local runtimes may not expose the registration lock-cost RPC.
  }

  return (await api.query.subtensorModule.networkLastLockCost()).toBigInt();
}

async function freeBalance(address) {
  return (await api.query.system.account(address)).data.free.toBigInt();
}

function balancesTransfer(dest, amount) {
  if (api.tx.balances.transferKeepAlive) {
    return api.tx.balances.transferKeepAlive(dest, amount);
  }
  if (api.tx.balances.transferAllowDeath) {
    return api.tx.balances.transferAllowDeath(dest, amount);
  }
  return api.tx.balances.transfer(dest, amount);
}

function storageValueHex(type, value) {
  return u8aToHex(api.createType(type, value).toU8a());
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
        // ignore unsubscribe races
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

function formatDispatchError(dispatchError) {
  if (dispatchError.isModule) {
    const decoded = api.registry.findMetaError(dispatchError.asModule);
    return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
  }
  return dispatchError.toString();
}

function formatPerquintill(value) {
  const integer = value / PERQUINTILL;
  const fractional = (value % PERQUINTILL).toString().padStart(18, "0");
  return `${integer}.${fractional.slice(0, 6)}`;
}
