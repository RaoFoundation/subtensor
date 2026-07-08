import assert from "node:assert/strict";
import fs from "node:fs";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

loadDotenv();

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const RUN_ID = process.env.TESTNET_SWAP_HOTKEY_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const SIGNER_URI = process.env.TESTNET_SWAP_HOTKEY_SIGNER_URI ?? "//TestnetLockConvictionSmoke//funded";
const OLD_HOTKEY_URI = process.env.TESTNET_SWAP_HOTKEY_OLD_URI ?? `//SwapHotkeyV2Lock//${RUN_ID}//old`;
const NEW_HOTKEY_URI = process.env.TESTNET_SWAP_HOTKEY_NEW_URI ?? `//SwapHotkeyV2Lock//${RUN_ID}//new`;
const STAKE_AMOUNT = BigInt(process.env.TESTNET_SWAP_HOTKEY_STAKE_AMOUNT ?? "1000000000");
const MIN_FREE_BALANCE = BigInt(process.env.TESTNET_SWAP_HOTKEY_MIN_FREE_BALANCE ?? "3000000000");
const NETUID = optionalNumber(process.env.TESTNET_SWAP_HOTKEY_NETUID);
const WAIT_BLOCKS = Number(process.env.TESTNET_SWAP_HOTKEY_WAIT_BLOCKS ?? "3");
const MIN_PRICE = 0n;

const keyring = new Keyring({ type: "sr25519" });
const signer = keyring.addFromUri(SIGNER_URI);
const oldHotkey = keyring.addFromUri(OLD_HOTKEY_URI);
const newHotkey = keyring.addFromUri(NEW_HOTKEY_URI);
const logger = createTempLogger("testnet-swap-hotkey-v2-lock-conviction.log");
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
    console.log("signer coldkey:", signer.address);
    console.log("old hotkey:", oldHotkey.address);
    console.log("new hotkey:", newHotkey.address);

    assertMetadataAvailable();

    const free = (await api.query.system.account(signer.address)).data.free.toBigInt();
    console.log("signer free balance:", free.toString());
    assert.ok(
      free >= MIN_FREE_BALANCE,
      `fund ${signer.address} before running test; free=${free}, required>=${MIN_FREE_BALANCE}`
    );

    const netuid = await resolveTargetNetuid();
    console.log("target netuid:", netuid);
    await assertHotkeyNotRegistered(oldHotkey.address, netuid, "old hotkey before registration");
    await assertHotkeyNotRegistered(newHotkey.address, netuid, "new hotkey before swap");

    await submitAndWait(api.tx.subtensorModule.burnedRegister(netuid, oldHotkey.address), "burnedRegister old hotkey");
    await assertHotkeyRegistered(oldHotkey.address, netuid, "old hotkey after registration");
    await assertOwner(oldHotkey.address, signer.address, "old hotkey owner after registration");
    console.log("registered old hotkey on netuid:", netuid);

    const alphaAdded = await addStake(oldHotkey.address, netuid, STAKE_AMOUNT);
    assert.ok(alphaAdded > 2n, `addStake returned too little alpha to lock: ${alphaAdded}`);
    console.log("alpha added:", alphaAdded.toString());

    const firstLockAmount = alphaAdded / 2n;
    await lockStake(oldHotkey.address, netuid, firstLockAmount, "initial lockStake");
    await waitForFinalizedBlocks(WAIT_BLOCKS);
    await lockStake(oldHotkey.address, netuid, 1n, "top-up lockStake to persist positive conviction");

    const oldLockBeforeSwap = await requireLock(signer.address, netuid, oldHotkey.address, "old hotkey lock before swap");
    assert.ok(oldLockBeforeSwap.lockedMass > 0n, "old lock decayed to zero before swap");
    assert.ok(
      oldLockBeforeSwap.convictionBits > 0n,
      `expected stored conviction to be positive before swap, got ${oldLockBeforeSwap.conviction}`
    );
    const aggregateStorage = await aggregateStorageForHotkey(netuid, oldHotkey.address);
    const oldAggregateBeforeSwap = await requireAggregateLock(
      aggregateStorage,
      netuid,
      oldHotkey.address,
      "old hotkey aggregate before swap"
    );
    assert.equal(oldAggregateBeforeSwap.lockedMass, oldLockBeforeSwap.lockedMass, "aggregate locked mass before swap");
    assert.equal(oldAggregateBeforeSwap.convictionBits, oldLockBeforeSwap.convictionBits, "aggregate conviction before swap");
    console.log("aggregate storage:", aggregateStorage);
    console.log("old lock before swap:", formatLock(oldLockBeforeSwap));
    console.log("old aggregate before swap:", formatLock(oldAggregateBeforeSwap));

    const swapResult = await submitAndWait(
      api.tx.subtensorModule.swapHotkeyV2(oldHotkey.address, newHotkey.address, netuid, false),
      "swapHotkeyV2 on single netuid"
    );
    assertHotkeySwappedOnSubnetEvent(swapResult.events, netuid);

    await assertNoLock(signer.address, netuid, oldHotkey.address, "old hotkey lock after swap");
    await assertNoAggregateLock(aggregateStorage, netuid, oldHotkey.address, "old hotkey aggregate after swap");
    const newLockAfterSwap = await requireLock(signer.address, netuid, newHotkey.address, "new hotkey lock after swap");
    const newAggregateAfterSwap = await requireAggregateLock(
      aggregateStorage,
      netuid,
      newHotkey.address,
      "new hotkey aggregate after swap"
    );
    console.log("new lock after swap:", formatLock(newLockAfterSwap));
    console.log("new aggregate after swap:", formatLock(newAggregateAfterSwap));

    assert.ok(newLockAfterSwap.lockedMass > 0n, "swap should move positive Lock locked mass");
    assert.ok(newLockAfterSwap.convictionBits > 0n, "swap should move positive Lock conviction without resetting it");
    assert.equal(
      newAggregateAfterSwap.lockedMass,
      newLockAfterSwap.lockedMass,
      "new aggregate locked mass should match moved Lock"
    );
    assert.equal(
      newAggregateAfterSwap.convictionBits,
      newLockAfterSwap.convictionBits,
      "new aggregate conviction should match moved Lock"
    );
    await assertHotkeyRegistered(newHotkey.address, netuid, "new hotkey after swap");
    await assertOwner(newHotkey.address, signer.address, "new hotkey owner after swap");

    console.log("testnet swapHotkeyV2 lock and conviction move: ok");
  } finally {
    await api?.disconnect();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["SubtensorModule.burnedRegister", api.tx.subtensorModule?.burnedRegister],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.lockStake", api.tx.subtensorModule?.lockStake],
    ["SubtensorModule.swapHotkeyV2", api.tx.subtensorModule?.swapHotkeyV2],
    ["SubtensorModule.Lock", api.query.subtensorModule?.lock],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["SubtensorModule.AlphaV2", api.query.subtensorModule?.alphaV2],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.Owner", api.query.subtensorModule?.owner],
    ["SubtensorModule.NetworkRegistrationAllowed", api.query.subtensorModule?.networkRegistrationAllowed],
    ["SubtensorModule.Burn", api.query.subtensorModule?.burn],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["Swap.PalSwapInitialized or Swap.SwapV3Initialized", initializedSubnetStorage()],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after swapHotkeyV2 lock fix is deployed to testnet`
  );
}

async function resolveTargetNetuid() {
  if (NETUID !== undefined) {
    await assertUsableNetuid(NETUID);
    return NETUID;
  }

  const initializedEntries = await initializedSubnetStorage().entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .filter((netuid) => netuid !== 0)
    .sort((a, b) => a - b);

  for (const netuid of initializedNetuids.slice(0, 64)) {
    if (await isUsableNetuid(netuid)) {
      return netuid;
    }
  }

  throw new Error("no initialized, registration-enabled, transfer-enabled non-root subnet found");
}

async function assertUsableNetuid(netuid) {
  assert.ok(await isUsableNetuid(netuid), `netuid ${netuid} is not initialized, registration-enabled, and transfer-enabled`);
}

async function isUsableNetuid(netuid) {
  const [initialized, registrationAllowed, transferEnabled, signerAlreadyLocked] = await Promise.all([
    initializedSubnetStorage()(netuid),
    api.query.subtensorModule.networkRegistrationAllowed(netuid),
    api.query.subtensorModule.transferToggle(netuid),
    signerHasLockOnNetuid(netuid),
  ]);
  return initialized.isTrue && registrationAllowed.isTrue && transferEnabled.isTrue && !signerAlreadyLocked;
}

async function signerHasLockOnNetuid(netuid) {
  const entries = await api.query.subtensorModule.lock.entries(signer.address);
  return entries.some(([key]) => key.args[1].toNumber() === netuid);
}

function initializedSubnetStorage() {
  return api.query.swap?.palSwapInitialized ?? api.query.swap?.swapV3Initialized;
}

async function assertHotkeyRegistered(hotkey, netuid, label) {
  const entries = await api.query.subtensorModule.keys.entries(netuid);
  assert.ok(
    entries.some(([, value]) => value.toString() === hotkey),
    `${label}: ${hotkey} is not registered on netuid ${netuid}`
  );
}

async function assertHotkeyNotRegistered(hotkey, netuid, label) {
  const entries = await api.query.subtensorModule.keys.entries(netuid);
  assert.ok(
    entries.every(([, value]) => value.toString() !== hotkey),
    `${label}: ${hotkey} is already registered on netuid ${netuid}`
  );
}

async function assertOwner(hotkey, expectedColdkey, label) {
  const owner = await api.query.subtensorModule.owner(hotkey);
  assert.equal(owner.toString(), expectedColdkey, `${label}: owner mismatch`);
}

async function addStake(hotkey, netuid, amount) {
  const result = await submitAndWait(api.tx.subtensorModule.addStake(hotkey, netuid, amount), "addStake");
  return assertStakeAddedEvent(result.events, hotkey, netuid);
}

async function lockStake(hotkey, netuid, amount, label) {
  const result = await submitAndWait(api.tx.subtensorModule.lockStake(hotkey, netuid, amount), label);
  assertEvent(result.events, "StakeLocked", ({ event }) => {
    const [, eventHotkey, eventNetuid, eventAmount] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && eventAmount.toBigInt() === amount;
  });
}

async function requireLock(coldkey, netuid, hotkey, label) {
  const lock = await readLock(coldkey, netuid, hotkey);
  assert.ok(lock, `${label}: expected Lock(${coldkey}, ${netuid}, ${hotkey}) to exist`);
  return lock;
}

async function readLock(coldkey, netuid, hotkey) {
  const maybeLock = await api.query.subtensorModule.lock(coldkey, netuid, hotkey);
  return maybeLock.isSome ? decodeLockState(maybeLock.unwrap()) : undefined;
}

async function assertNoLock(coldkey, netuid, hotkey, label) {
  const maybeLock = await api.query.subtensorModule.lock(coldkey, netuid, hotkey);
  assert.ok(maybeLock.isNone, `${label}: unexpected Lock(${coldkey}, ${netuid}, ${hotkey}) exists`);
}

async function aggregateStorageForHotkey(netuid, hotkey) {
  const hotkeyLock = await api.query.subtensorModule.hotkeyLock(netuid, hotkey);
  const decayingHotkeyLock = await api.query.subtensorModule.decayingHotkeyLock(netuid, hotkey);
  if (hotkeyLock.isSome) return "hotkeyLock";
  if (decayingHotkeyLock.isSome) return "decayingHotkeyLock";
  throw new Error(`expected aggregate lock for hotkey ${hotkey} on netuid ${netuid}`);
}

async function requireAggregateLock(storageName, netuid, hotkey, label) {
  const maybeLock = await api.query.subtensorModule[storageName](netuid, hotkey);
  assert.ok(maybeLock.isSome, `${label}: expected ${storageName}(${netuid}, ${hotkey}) to exist`);
  return decodeLockState(maybeLock.unwrap());
}

async function assertNoAggregateLock(storageName, netuid, hotkey, label) {
  const maybeLock = await api.query.subtensorModule[storageName](netuid, hotkey);
  assert.ok(maybeLock.isNone, `${label}: unexpected ${storageName}(${netuid}, ${hotkey}) exists`);
}

function decodeLockState(lockState) {
  const lockedMass = structField(lockState, "lockedMass", "locked_mass").toBigInt();
  const convictionValue = structField(lockState, "conviction");
  const conviction = convictionValue.toString();
  const convictionBits = decodeConvictionBits(convictionValue);
  const lastUpdate = structField(lockState, "lastUpdate", "last_update").toBigInt();
  return { lockedMass, conviction, convictionBits, lastUpdate };
}

function decodeConvictionBits(value) {
  if (value.toBigInt) {
    return value.toBigInt();
  }

  const json = value.toJSON?.();
  if (json?.bits !== undefined) {
    return parseBigIntish(json.bits);
  }

  const human = value.toHuman?.();
  if (human?.bits !== undefined) {
    return parseBigIntish(human.bits);
  }

  const parsed = JSON.parse(value.toString());
  return parseBigIntish(parsed.bits);
}

function parseBigIntish(value) {
  if (typeof value === "bigint") return value;
  if (typeof value === "number") return BigInt(value);
  if (typeof value === "string") return BigInt(value.replaceAll(",", ""));
  throw new Error(`could not decode bigint from ${value}`);
}

function structField(value, ...names) {
  for (const name of names) {
    if (value[name]) return value[name];
    const field = value.get?.(name);
    if (field) return field;
  }
  throw new Error(`could not decode field ${names.join("/")} from ${value.toString()}`);
}

function assertStakeAddedEvent(events, hotkey, netuid) {
  const event = assertEvent(events, "StakeAdded", ({ event }) => {
    const [, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && alphaStaked.toBigInt() > 0n;
  });
  return event.event.data[3].toBigInt();
}

function assertHotkeySwappedOnSubnetEvent(events, netuid) {
  assertEvent(events, "HotkeySwappedOnSubnet", ({ event }) => {
    const [eventColdkey, eventOldHotkey, eventNewHotkey, eventNetuid] = event.data;
    return (
      eventColdkey.toString() === signer.address &&
      eventOldHotkey.toString() === oldHotkey.address &&
      eventNewHotkey.toString() === newHotkey.address &&
      eventNetuid.toNumber() === netuid
    );
  });
}

function assertEvent(events, method, predicate) {
  const event = events.find((record) => {
    return (
      record.event.section === "subtensorModule" &&
      record.event.method === method &&
      (!predicate || predicate(record))
    );
  });
  assert.ok(event, `${method} event not found`);
  return event;
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

      if (status.isInBlock) {
        finish(resolve, { blockHash: status.asInBlock.toString(), events });
      } else if (status.isFinalized) {
        finish(resolve, { blockHash: status.asFinalized.toString(), events });
      }
    })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

async function waitForFinalizedBlocks(count) {
  let last = await getFinalizedBlockNumber();
  for (let i = 0; i < count; i++) {
    last = await waitForNextFinalizedBlock(last);
    console.log("finalized block:", last.toString());
  }
}

async function waitForNextFinalizedBlock(previous) {
  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline) {
    const current = await getFinalizedBlockNumber();
    if (current > previous) {
      return current;
    }
    await sleep(6_000);
  }
  throw new Error(`timed out waiting for finalized block after ${previous}`);
}

async function getFinalizedBlockNumber() {
  const hash = await api.rpc.chain.getFinalizedHead();
  const header = await api.rpc.chain.getHeader(hash);
  return header.number.toBigInt();
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function formatDispatchError(error) {
  if (!error.isModule) {
    return error.toString();
  }

  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}

function formatLock(lock) {
  return `locked_mass=${lock.lockedMass} conviction=${lock.conviction} conviction_bits=${lock.convictionBits} last_update=${lock.lastUpdate}`;
}

function optionalNumber(value) {
  if (value === undefined || value === "") {
    return undefined;
  }
  const parsed = Number(value);
  assert.ok(Number.isInteger(parsed) && parsed >= 0, `invalid netuid: ${value}`);
  return parsed;
}

function loadDotenv() {
  try {
    const text = fs.readFileSync(new URL("../../.env", import.meta.url), "utf8");
    for (const line of text.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const match = trimmed.match(/^([A-Za-z_][A-Za-z0-9_]*)=(.*)$/);
      if (!match || process.env[match[1]] !== undefined) continue;
      process.env[match[1]] = match[2].replace(/^['"]|['"]$/g, "");
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}
