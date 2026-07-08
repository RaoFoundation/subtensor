import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const RUN_ID = process.env.LOCK_DUST_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const FUND_SOURCE_URI = process.env.LOCK_DUST_FUND_SOURCE_URI ?? "//Alice";
const TEST_COLDKEY_URI = process.env.LOCK_DUST_COLDKEY_URI ?? `//LockDustCleanup//${RUN_ID}//coldkey`;
const TEST_COLDKEY1_URI = process.env.LOCK_DUST_COLDKEY1_URI ?? `//LockDustCleanup//${RUN_ID}//coldkey1`;
const TEST_COLDKEY2_URI = process.env.LOCK_DUST_COLDKEY2_URI ?? `//LockDustCleanup//${RUN_ID}//coldkey2`;
const SOURCE_FUND_AMOUNT = BigInt(process.env.LOCK_DUST_SOURCE_FUND_AMOUNT ?? "100000000000");
const STAKE_AMOUNT = BigInt(process.env.LOCK_DUST_STAKE_AMOUNT ?? "10000000000");
const DUST_LOCK_AMOUNT = BigInt(process.env.LOCK_DUST_LOCK_AMOUNT ?? "101");
const FAST_DECAY_RATE = BigInt(process.env.LOCK_DUST_FAST_DECAY_RATE ?? "1");
const MULTI_SOURCE_FUND_AMOUNT = BigInt(process.env.LOCK_DUST_MULTI_SOURCE_FUND_AMOUNT ?? "2000000000000");
const MULTI_STAKE_AMOUNT = BigInt(process.env.LOCK_DUST_MULTI_STAKE_AMOUNT ?? "1000000000000");
const ONE_ALPHA = 1_000_000_000n;
const LOCK_STATE_ZERO_THRESHOLD = 100n;
const DUST_AGGREGATE_LOCK_AMOUNT = 100n;
const DUST_SCENARIO_DECAY_RATE = BigInt(process.env.LOCK_DUST_SCENARIO_DECAY_RATE ?? "216000");
const DUST_SCENARIO_SETUP_DECAY_RATE = BigInt(
  process.env.LOCK_DUST_SCENARIO_SETUP_DECAY_RATE ?? "18446744073709551615"
);
const FAST_DUST_WAIT_BLOCKS = blocksToDecayBelowDust(DUST_LOCK_AMOUNT, FAST_DECAY_RATE);
const DUST_SCENARIO_WAIT_BLOCKS = blocksToDecayBelowDust(DUST_AGGREGATE_LOCK_AMOUNT, DUST_SCENARIO_DECAY_RATE);
const UNSTAKE_ONE_ALPHA = ONE_ALPHA;
const MIN_PRICE = 0n;

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const testColdkey = keyring.addFromUri(TEST_COLDKEY_URI);
const testColdkey1 = keyring.addFromUri(TEST_COLDKEY1_URI);
const testColdkey2 = keyring.addFromUri(TEST_COLDKEY2_URI);
const logger = createTempLogger("test-lock-dust-cleanup.log");
logger.captureConsole();

let api;
let originalUnlockRate;
let originalMaturityRate;

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
    console.log("test coldkey:", testColdkey.address);
    console.log("test coldkey1:", testColdkey1.address);
    console.log("test coldkey2:", testColdkey2.address);

    assertMetadataAvailable();

    originalUnlockRate = (await api.query.subtensorModule.unlockRate()).toBigInt();
    originalMaturityRate = (await api.query.subtensorModule.maturityRate()).toBigInt();
    console.log("original rates:", `unlock=${originalUnlockRate}`, `maturity=${originalMaturityRate}`);

    const { netuid, hotkey } = await findTestHotkey();
    const aggregateStorage = await aggregateStorageForLock(testColdkey.address, netuid, hotkey);
    assert.equal(aggregateStorage, "decayingHotkeyLock", "dust cleanup test must use DecayingHotkeyLock");
    console.log("test subnet:", netuid);
    console.log("test hotkey:", hotkey);
    console.log("test aggregate storage:", aggregateStorage);

    await fundAccount(testColdkey.address, SOURCE_FUND_AMOUNT, "test coldkey");
    await setLockRates(FAST_DECAY_RATE, FAST_DECAY_RATE, "set fast lock dust decay rates");

    const alphaAdded = await addStake(testColdkey, hotkey, netuid);
    assert.ok(alphaAdded > DUST_LOCK_AMOUNT, `addStake returned too little alpha: ${alphaAdded}`);
    console.log("alpha added:", alphaAdded.toString());

    await lockStake(testColdkey, hotkey, netuid, DUST_LOCK_AMOUNT);
    const lockAfterAdd = await requireLock(testColdkey.address, netuid, hotkey, "dust lock after add");
    const aggregateAfterAdd = await requireAggregateLock(
      aggregateStorage,
      netuid,
      hotkey,
      "aggregate dust lock after add"
    );
    assert.equal(lockAfterAdd.lockedMass, DUST_LOCK_AMOUNT, "lockStake should persist the small active lock");
    assert.equal(
      aggregateAfterAdd.lockedMass,
      DUST_LOCK_AMOUNT,
      `${aggregateStorage} should include the small active lock`
    );
    await assertLockingColdkeysContains(netuid, hotkey, testColdkey.address, "after lockStake");
    console.log("lock after add:", formatLock(lockAfterAdd));
    console.log("aggregate after add:", formatLock(aggregateAfterAdd));
    console.log("LockingColdkeys contains test coldkey after lockStake: ok");

    console.log("fast dust wait blocks:", FAST_DUST_WAIT_BLOCKS.toString());
    await waitForFinalizedBlocks(FAST_DUST_WAIT_BLOCKS);
    const unstakeAmount = alphaAdded / 4n;
    assert.ok(unstakeAmount > 0n, "not enough alpha to trigger unstake cleanup");
    const unstakeResult = await submitAndWait(
      testColdkey,
      api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, unstakeAmount, MIN_PRICE, false),
      "removeStakeLimit to trigger lock roll-forward cleanup"
    );
    const alphaRemoved = assertStakeRemovedEvent(unstakeResult.events, hotkey, netuid);
    console.log("alpha unstaked:", alphaRemoved.toString());

    await assertNoLock(testColdkey.address, netuid, hotkey, "dust lock after unstake cleanup");
    const aggregateAfterCleanup = await aggregateLockedMassOrZero(aggregateStorage, netuid, hotkey);
    assert.equal(
      aggregateAfterAdd.lockedMass - aggregateAfterCleanup,
      DUST_LOCK_AMOUNT,
      "DecayingHotkeyLock should be reduced by the collected dust amount"
    );
    await assertNoAggregateLock(aggregateStorage, netuid, hotkey, "aggregate dust lock after unstake cleanup");
    await assertLockingColdkeysDoesNotContain(netuid, hotkey, testColdkey.address, "after dust cleanup");
    console.log("DecayingHotkeyLock dust reduction:", (aggregateAfterAdd.lockedMass - aggregateAfterCleanup).toString());
    console.log("Lock dust cleanup removed Lock, aggregate lock, and LockingColdkeys entry: ok");
    await runTwoColdkeyDustAggregateScenario();
  } finally {
    if (api && originalUnlockRate !== undefined && originalMaturityRate !== undefined) {
      await setLockRates(originalUnlockRate, originalMaturityRate, "restore original lock rates");
    }
    await api?.disconnect();
    await logger.flush();
  }
}

async function runTwoColdkeyDustAggregateScenario() {
  const { netuid, stakeHotkey, lockHotkey } = await findTestHotkeyPair();
  console.log("two-coldkey dust subnet:", netuid);
  console.log("two-coldkey stake hotkey:", stakeHotkey);
  console.log("two-coldkey lock hotkey:", lockHotkey);

  await setLockRates(
    DUST_SCENARIO_SETUP_DECAY_RATE,
    DUST_SCENARIO_SETUP_DECAY_RATE,
    "set two-coldkey setup no-decay rates"
  );
  await fundAccount(testColdkey1.address, MULTI_SOURCE_FUND_AMOUNT, "test coldkey1");
  await fundAccount(testColdkey2.address, MULTI_SOURCE_FUND_AMOUNT, "test coldkey2");

  const coldkey1Alpha = await addStake(testColdkey1, stakeHotkey, netuid, MULTI_STAKE_AMOUNT, "coldkey1 stake to hotkey1");
  const coldkey2Alpha = await addStake(testColdkey2, stakeHotkey, netuid, MULTI_STAKE_AMOUNT, "coldkey2 stake to hotkey1");
  assert.ok(coldkey1Alpha > ONE_ALPHA + UNSTAKE_ONE_ALPHA, `coldkey1 alpha too low: ${coldkey1Alpha}`);
  assert.ok(coldkey2Alpha > UNSTAKE_ONE_ALPHA, `coldkey2 alpha too low: ${coldkey2Alpha}`);
  console.log("coldkey1 alpha added:", coldkey1Alpha.toString());
  console.log("coldkey2 alpha added:", coldkey2Alpha.toString());

  await Promise.all([
    lockStake(testColdkey1, lockHotkey, netuid, ONE_ALPHA),
    lockStake(testColdkey2, lockHotkey, netuid, DUST_AGGREGATE_LOCK_AMOUNT),
  ]);

  const aggregateAfterLocks = await requireAggregateLock(
    "decayingHotkeyLock",
    netuid,
    lockHotkey,
    "two-coldkey aggregate after locks"
  );
  assert.equal(
    aggregateAfterLocks.lockedMass,
    ONE_ALPHA + DUST_AGGREGATE_LOCK_AMOUNT,
    "DecayingHotkeyLock aggregate after both locks"
  );
  await assertLockingColdkeysContains(netuid, lockHotkey, testColdkey1.address, "after coldkey1 lockStake");
  await assertLockingColdkeysContains(netuid, lockHotkey, testColdkey2.address, "after coldkey2 lockStake");
  console.log("two-coldkey aggregate after locks:", formatLock(aggregateAfterLocks));

  await setLockRates(DUST_SCENARIO_DECAY_RATE, DUST_SCENARIO_DECAY_RATE, "set two-coldkey dust scenario rates");
  console.log("two-coldkey dust wait blocks:", DUST_SCENARIO_WAIT_BLOCKS.toString());
  await waitForFinalizedBlocks(DUST_SCENARIO_WAIT_BLOCKS);

  await removeStake(testColdkey1, stakeHotkey, netuid, UNSTAKE_ONE_ALPHA, "coldkey1 removes 1 alpha stake");
  const coldkey1LockAfterUnstake = await requireLock(
    testColdkey1.address,
    netuid,
    lockHotkey,
    "coldkey1 lock after unstake roll-forward"
  );
  const aggregateAfterColdkey1 = await requireAggregateLock(
    "decayingHotkeyLock",
    netuid,
    lockHotkey,
    "aggregate after coldkey1 unstake"
  );
  assert.ok(
    coldkey1LockAfterUnstake.lockedMass >= 999_000_000n && coldkey1LockAfterUnstake.lockedMass < ONE_ALPHA,
    `expected coldkey1 lock to decay to 999??????, got ${coldkey1LockAfterUnstake.lockedMass}`
  );
  assert.equal(
    aggregateAfterColdkey1.lockedMass,
    coldkey1LockAfterUnstake.lockedMass + DUST_AGGREGATE_LOCK_AMOUNT,
    "DecayingHotkeyLock aggregate after coldkey1 roll-forward should be decayed 1 alpha plus 100 dust"
  );
  console.log("coldkey1 lock after unstake:", formatLock(coldkey1LockAfterUnstake));
  console.log("aggregate after coldkey1 unstake:", formatLock(aggregateAfterColdkey1));

  await removeStake(testColdkey2, stakeHotkey, netuid, UNSTAKE_ONE_ALPHA, "coldkey2 removes 1 alpha stake");
  await assertNoLock(testColdkey2.address, netuid, lockHotkey, "coldkey2 dust lock after unstake cleanup");
  await assertLockingColdkeysDoesNotContain(netuid, lockHotkey, testColdkey2.address, "after coldkey2 dust cleanup");
  const coldkey1LockAfterColdkey2 = await requireLock(
    testColdkey1.address,
    netuid,
    lockHotkey,
    "coldkey1 lock after coldkey2 dust cleanup"
  );
  const aggregateAfterColdkey2 = await requireAggregateLock(
    "decayingHotkeyLock",
    netuid,
    lockHotkey,
    "aggregate after coldkey2 dust cleanup"
  );
  assert.ok(
    aggregateAfterColdkey2.lockedMass >= 999_000_000n && aggregateAfterColdkey2.lockedMass < ONE_ALPHA,
    `expected aggregate to remain 999?????? after dust cleanup, got ${aggregateAfterColdkey2.lockedMass}`
  );
  assert.equal(
    aggregateAfterColdkey2.lockedMass,
    coldkey1LockAfterColdkey2.lockedMass,
    "DecayingHotkeyLock aggregate after coldkey2 roll-forward should match coldkey1 without 100 dust"
  );
  assert.equal(
    aggregateAfterColdkey1.lockedMass - aggregateAfterColdkey2.lockedMass,
    DUST_AGGREGATE_LOCK_AMOUNT,
    "coldkey2 cleanup should remove exactly 100 aggregate dust"
  );
  console.log("coldkey1 lock after coldkey2 cleanup:", formatLock(coldkey1LockAfterColdkey2));
  console.log("aggregate after coldkey2 cleanup:", formatLock(aggregateAfterColdkey2));
  console.log("two-coldkey DecayingHotkeyLock dust cleanup removed 100 aggregate dust: ok");
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Balances.forceSetBalance", api.tx.balances?.forceSetBalance],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.lockStake", api.tx.subtensorModule?.lockStake],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.Lock", api.query.subtensorModule?.lock],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["SubtensorModule.OwnerLock", api.query.subtensorModule?.ownerLock],
    ["SubtensorModule.DecayingOwnerLock", api.query.subtensorModule?.decayingOwnerLock],
    ["SubtensorModule.DecayingLock", api.query.subtensorModule?.decayingLock],
    ["SubtensorModule.LockingColdkeys", api.query.subtensorModule?.lockingColdkeys],
    ["SubtensorModule.UnlockRate", api.query.subtensorModule?.unlockRate],
    ["SubtensorModule.MaturityRate", api.query.subtensorModule?.maturityRate],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.SubnetOwnerHotkey", api.query.subtensorModule?.subnetOwnerHotkey],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["Swap.PalSwapInitialized or Swap.SwapV3Initialized", initializedSubnetStorage()],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to the dust-lock runtime`
  );
}

async function findTestHotkey() {
  const initializedEntries = await initializedSubnetStorage().entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);

  for (const netuid of initializedNetuids) {
    const transferEnabled = await api.query.subtensorModule.transferToggle(netuid);
    if (!transferEnabled.isTrue) continue;

    const ownerHotkey = (await api.query.subtensorModule.subnetOwnerHotkey(netuid)).toString();
    const hotkeys = (await api.query.subtensorModule.keys.entries(netuid))
      .map(([, hotkey]) => hotkey.toString())
      .filter((hotkey, index, all) => hotkey && hotkey !== ownerHotkey && all.indexOf(hotkey) === index);
    for (const hotkey of hotkeys) {
      if (await hasNoDecayingHotkeyLock(netuid, hotkey)) {
        return { netuid, hotkey };
      }
    }
  }

  throw new Error("no initialized transfer-enabled subnet with a non-owner hotkey and no decaying hotkey lock found");
}

async function findTestHotkeyPair() {
  const initializedEntries = await initializedSubnetStorage().entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);

  for (const netuid of initializedNetuids) {
    const transferEnabled = await api.query.subtensorModule.transferToggle(netuid);
    if (!transferEnabled.isTrue) continue;

    const ownerHotkey = (await api.query.subtensorModule.subnetOwnerHotkey(netuid)).toString();
    const hotkeys = (await api.query.subtensorModule.keys.entries(netuid))
      .map(([, hotkey]) => hotkey.toString())
      .filter((hotkey, index, all) => hotkey && hotkey !== ownerHotkey && all.indexOf(hotkey) === index);

    for (const lockHotkey of hotkeys) {
      if (!(await hasNoDecayingHotkeyLock(netuid, lockHotkey))) continue;

      const stakeHotkey = hotkeys.find((hotkey) => hotkey !== lockHotkey);
      if (stakeHotkey) {
        return { netuid, stakeHotkey, lockHotkey };
      }
    }
  }

  throw new Error(
    "no initialized transfer-enabled subnet with two non-owner hotkeys and an unused decaying lock target found"
  );
}

function initializedSubnetStorage() {
  return api.query.swap?.palSwapInitialized ?? api.query.swap?.swapV3Initialized;
}

async function fundAccount(address, amount, label) {
  await submitAndWait(
    fundSource,
    api.tx.sudo.sudo(api.tx.balances.forceSetBalance(address, amount)),
    `sudo fund ${label}`
  );
  const free = (await api.query.system.account(address)).data.free.toBigInt();
  assert.ok(free >= amount, `${label} funding failed: free=${free}`);
  console.log(`${label} funded:`, free.toString());
}

async function setLockRates(unlockRate, maturityRate, label) {
  const calls = [
    [api.query.subtensorModule.unlockRate.key(), u8aToHex(api.createType("u64", unlockRate).toU8a())],
    [api.query.subtensorModule.maturityRate.key(), u8aToHex(api.createType("u64", maturityRate).toU8a())],
  ];
  await submitAndWait(fundSource, api.tx.sudo.sudo(api.tx.system.setStorage(calls)), label);
  const storedUnlockRate = (await api.query.subtensorModule.unlockRate()).toBigInt();
  const storedMaturityRate = (await api.query.subtensorModule.maturityRate()).toBigInt();
  assert.equal(storedUnlockRate, unlockRate, "UnlockRate was not updated");
  assert.equal(storedMaturityRate, maturityRate, "MaturityRate was not updated");
  console.log("lock rates:", `unlock=${storedUnlockRate}`, `maturity=${storedMaturityRate}`);
}

async function addStake(signer, hotkey, netuid, amount = STAKE_AMOUNT, label = "addStake for dust cleanup test") {
  const result = await submitAndWait(
    signer,
    api.tx.subtensorModule.addStake(hotkey, netuid, amount),
    label
  );
  return assertStakeAddedEvent(result.events, hotkey, netuid);
}

async function removeStake(signer, hotkey, netuid, amount, label) {
  const result = await submitAndWait(
    signer,
    api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, amount, MIN_PRICE, false),
    label
  );
  const alphaRemoved = assertStakeRemovedEvent(result.events, hotkey, netuid);
  console.log(`${label} alpha unstaked:`, alphaRemoved.toString());
  return alphaRemoved;
}

async function lockStake(signer, hotkey, netuid, amount) {
  const result = await submitAndWait(
    signer,
    api.tx.subtensorModule.lockStake(hotkey, netuid, amount),
    "lockStake dust amount"
  );
  assertEvent(result.events, "StakeLocked", ({ event }) => {
    const [, eventHotkey, eventNetuid, eventAmount] = event.data;
    return (
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      eventAmount.toBigInt() === amount
    );
  });
}

async function requireLock(coldkey, netuid, hotkey, label) {
  const maybeLock = await api.query.subtensorModule.lock(coldkey, netuid, hotkey);
  assert.ok(maybeLock.isSome, `${label}: expected Lock(${coldkey}, ${netuid}, ${hotkey}) to exist`);
  return decodeLockState(maybeLock.unwrap());
}

async function assertNoLock(coldkey, netuid, hotkey, label) {
  const maybeLock = await api.query.subtensorModule.lock(coldkey, netuid, hotkey);
  assert.ok(maybeLock.isNone, `${label}: unexpected Lock(${coldkey}, ${netuid}, ${hotkey}) exists`);
}

async function requireAggregateLock(storageName, netuid, hotkey, label) {
  const maybeLock = await queryAggregateLock(storageName, netuid, hotkey);
  assert.ok(maybeLock.isSome, `${label}: expected ${aggregateLabel(storageName, netuid, hotkey)} to exist`);
  return decodeLockState(maybeLock.unwrap());
}

async function assertNoAggregateLock(storageName, netuid, hotkey, label) {
  const maybeLock = await queryAggregateLock(storageName, netuid, hotkey);
  assert.ok(maybeLock.isNone, `${label}: unexpected ${aggregateLabel(storageName, netuid, hotkey)} exists`);
}

async function aggregateLockedMassOrZero(storageName, netuid, hotkey) {
  const maybeLock = await queryAggregateLock(storageName, netuid, hotkey);
  return maybeLock.isSome ? decodeLockState(maybeLock.unwrap()).lockedMass : 0n;
}

async function assertLockingColdkeysContains(netuid, hotkey, coldkey, label) {
  const coldkeys = await lockingColdkeys(netuid, hotkey);
  assert.ok(
    coldkeys.includes(coldkey),
    `${label}: expected LockingColdkeys(${netuid}, ${hotkey}) to contain ${coldkey}; got ${coldkeys.join(",")}`
  );
}

async function assertLockingColdkeysDoesNotContain(netuid, hotkey, coldkey, label) {
  const coldkeys = await lockingColdkeys(netuid, hotkey);
  assert.ok(
    !coldkeys.includes(coldkey),
    `${label}: expected LockingColdkeys(${netuid}, ${hotkey}) to omit ${coldkey}; got ${coldkeys.join(",")}`
  );
}

async function lockingColdkeys(netuid, hotkey) {
  try {
    const coldkeys = await api.query.subtensorModule.lockingColdkeys(netuid, hotkey);
    return coldkeys.map((coldkey) => coldkey.toString());
  } catch (error) {
    if (!String(error.message ?? error).includes("requiring 3 arguments")) {
      throw error;
    }
  }

  const entries = await api.query.subtensorModule.lockingColdkeys.entries(netuid, hotkey);
  return entries.map(([key]) => key.args[2].toString());
}

async function aggregateStorageForLock(coldkey, netuid, hotkey) {
  const ownerHotkey = (await api.query.subtensorModule.subnetOwnerHotkey(netuid)).toString();
  const isOwnerLock = hotkey === ownerHotkey;
  const maybeDecayingLock = await api.query.subtensorModule.decayingLock(coldkey, netuid);
  const isPerpetual = maybeDecayingLock.isSome && maybeDecayingLock.unwrap().isFalse;

  if (isOwnerLock) {
    return isPerpetual ? "ownerLock" : "decayingOwnerLock";
  }
  return isPerpetual ? "hotkeyLock" : "decayingHotkeyLock";
}

async function hasNoDecayingHotkeyLock(netuid, hotkey) {
  return (await api.query.subtensorModule.decayingHotkeyLock(netuid, hotkey)).isNone;
}

function queryAggregateLock(storageName, netuid, hotkey) {
  if (storageName === "ownerLock" || storageName === "decayingOwnerLock") {
    return api.query.subtensorModule[storageName](netuid);
  }
  return api.query.subtensorModule[storageName](netuid, hotkey);
}

function aggregateLabel(storageName, netuid, hotkey) {
  if (storageName === "ownerLock" || storageName === "decayingOwnerLock") {
    return `${storageName}(${netuid})`;
  }
  return `${storageName}(${netuid}, ${hotkey})`;
}

function decodeLockState(lockState) {
  const lockedMass = structField(lockState, "lockedMass", "locked_mass").toBigInt();
  const conviction = structField(lockState, "conviction").toString();
  const lastUpdate = structField(lockState, "lastUpdate", "last_update").toBigInt();
  return { lockedMass, conviction, lastUpdate };
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
    return (
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      alphaStaked.toBigInt() > 0n
    );
  });
  return event.event.data[3].toBigInt();
}

function assertStakeRemovedEvent(events, hotkey, netuid) {
  const event = assertEvent(events, "StakeRemoved", ({ event }) => {
    const [, eventHotkey, , alphaUnstaked, eventNetuid] = event.data;
    return (
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      alphaUnstaked.toBigInt() > 0n
    );
  });
  return event.event.data[3].toBigInt();
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

async function submitAndWait(signer, tx, label) {
  return new Promise((resolve, reject) => {
    console.log(`submitting tx: ${label}`);
    let unsubscribe;
    let settled = false;
    const timeout = setTimeout(
      () => finish(reject, new Error(`${label} timed out waiting for finalization`)),
      Number(process.env.SUBMIT_TIMEOUT_MS ?? 180_000)
    );

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
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

async function waitForFinalizedBlocks(count) {
  let previous = await getFinalizedBlockNumber();
  for (let i = 0; i < count; i++) {
    previous = await waitForNextFinalizedBlock(previous);
    console.log("finalized wait block:", previous.toString());
  }
}

async function waitForNextFinalizedBlock(previous) {
  const timeoutAt = Date.now() + Number(process.env.FINALIZED_BLOCK_TIMEOUT_MS ?? 180_000);
  while (Date.now() < timeoutAt) {
    const current = await getFinalizedBlockNumber();
    if (current > previous) {
      return current;
    }
    await sleep(1_000);
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

function blocksToDecayBelowDust(amount, tau) {
  if (amount < LOCK_STATE_ZERO_THRESHOLD) {
    return 0;
  }

  if (tau === 0n) {
    return 1;
  }

  const estimate = Math.max(
    1,
    Math.floor(Number(tau) * Math.log(Number(amount) / Number(LOCK_STATE_ZERO_THRESHOLD)))
  );
  let blocks = estimate;
  while (decayedLockedMass(amount, blocks, tau) >= LOCK_STATE_ZERO_THRESHOLD) {
    blocks += 1;
  }
  while (blocks > 0 && decayedLockedMass(amount, blocks - 1, tau) < LOCK_STATE_ZERO_THRESHOLD) {
    blocks -= 1;
  }
  return blocks;
}

function decayedLockedMass(amount, blocks, tau) {
  if (blocks === 0) {
    return amount;
  }
  if (tau === 0n) {
    return 0n;
  }

  const decay = Math.exp(Math.max(-40, -blocks / Number(tau)));
  return BigInt(Math.trunc(Number(amount) * decay));
}

function formatLock(lock) {
  return `locked_mass=${lock.lockedMass} conviction=${lock.conviction} last_update=${lock.lastUpdate}`;
}
