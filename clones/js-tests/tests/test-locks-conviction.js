import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const STAKE_AMOUNT = BigInt(process.env.LOCK_TEST_STAKE_AMOUNT ?? "10000000000");
const SOURCE_FUND_AMOUNT = BigInt(process.env.LOCK_TEST_SOURCE_FUND_AMOUNT ?? "100000000000");
const MIN_PRICE = 0n;
const RUN_ID = process.env.LOCK_TEST_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const FUND_SOURCE_URI = process.env.LOCK_TEST_FUND_SOURCE_URI ?? "//Alice";
const TRANSFER_SOURCE_URI = process.env.TRANSFER_SOURCE_URI ?? `//LockConvictionTest//${RUN_ID}//source`;
const TRANSFER_DEST_URI = process.env.TRANSFER_DEST_URI ?? `//LockConvictionTest//${RUN_ID}//destination`;
const OWNER_HOTKEY_SOURCE_URI =
  process.env.OWNER_HOTKEY_SOURCE_URI ?? `//LockConvictionTest//${RUN_ID}//owner-hotkey-source`;
const OWNER_STAKE_SOURCE_URI =
  process.env.OWNER_STAKE_SOURCE_URI ?? `//LockConvictionTest//${RUN_ID}//owner-stake-source`;
const OWNER_TRANSFER_DEST_URI =
  process.env.OWNER_TRANSFER_DEST_URI ?? `//LockConvictionTest//${RUN_ID}//owner-transfer-destination`;
const BLOCKS_BETWEEN_LOCK_ACTIONS = Number(process.env.LOCK_TEST_WAIT_BLOCKS ?? 3);

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const source = keyring.addFromUri(TRANSFER_SOURCE_URI);
const destination = keyring.addFromUri(TRANSFER_DEST_URI);
const ownerHotkeySource = keyring.addFromUri(OWNER_HOTKEY_SOURCE_URI);
const ownerStakeSource = keyring.addFromUri(OWNER_STAKE_SOURCE_URI);
const ownerTransferDestination = keyring.addFromUri(OWNER_TRANSFER_DEST_URI);
const logger = createTempLogger("test-locks-conviction.log");
logger.captureConsole();

let api;
let ownerToRestore = null;
let ownerRestoreNetuid = null;

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

  assertLockMetadataAvailable();

  const rates = await readRates();
  console.log("lock rates:", `unlock=${rates.unlockRate}`, `maturity=${rates.maturityRate}`);

  const { netuid, ownerHotkey, hotkeys } = await findTestSubnet();
  const [originHotkey, destinationHotkey] = hotkeys;
  console.log("run id:", RUN_ID);
  console.log("source coldkey:", source.address);
  console.log("destination coldkey:", destination.address);
  console.log("owner-hotkey source coldkey:", ownerHotkeySource.address);
  console.log("test subnet owner coldkey:", ownerStakeSource.address);
  console.log("owner transfer destination coldkey:", ownerTransferDestination.address);
  console.log("test subnet:", netuid);
  console.log("subnet owner hotkey:", ownerHotkey);
  console.log("origin hotkey:", originHotkey);
  console.log("move-lock destination hotkey:", destinationHotkey);

  const accountsToFund = [
    [source, "source"],
    [destination, "destination"],
    [ownerHotkeySource, "owner-hotkey source"],
    [ownerStakeSource, "owner stake source"],
    [ownerTransferDestination, "owner transfer destination"],
  ];

  const originalSubnetOwner = (await api.query.subtensorModule.subnetOwner(netuid)).toString();
  ownerToRestore = originalSubnetOwner;
  ownerRestoreNetuid = netuid;
  const initialOwnerCutAutoLock = await api.query.subtensorModule.ownerCutAutoLockEnabled(netuid);
  if (initialOwnerCutAutoLock.isTrue) {
    console.log("owner cut auto-lock default: true");
  } else {
    console.log("owner cut auto-lock already false before setup; continuing with opt-out assertion");
  }
  await prepareTestState(accountsToFund, netuid, ownerStakeSource.address, false);
  const disabledOwnerCutAutoLock = await api.query.subtensorModule.ownerCutAutoLockEnabled(netuid);
  assert.equal(disabledOwnerCutAutoLock.isFalse, true, "owner cut auto-lock should be disabled after opt-out");
  console.log("owner cut auto-lock default and opt-out: ok");

  const ownerHotkeyAlphaAdded = await addStake(ownerHotkeySource, ownerHotkey, netuid);
  const ownerHotkeyLockAmount = ownerHotkeyAlphaAdded / 2n;
  await lockStake(
    ownerHotkeySource,
    ownerHotkey,
    netuid,
    ownerHotkeyLockAmount,
    "passive owner-hotkey lockStake"
  );
  const ownerHotkeyLock = await requireLock(
    ownerHotkeySource.address,
    netuid,
    ownerHotkey,
    "passive owner-hotkey lock"
  );
  assertConvictionAtLeast(
    ownerHotkeyLock,
    ownerHotkeyLockAmount,
    "non-owner coldkey lock to subnet owner hotkey should receive immediate owner conviction"
  );
  const ownerAggregate = await requireAggregateLock("decayingOwnerLock", netuid, ownerHotkey, "default decaying owner aggregate lock");
  assertConvictionAtLeast(
    ownerAggregate,
    ownerHotkeyLockAmount,
    "DecayingOwnerLock should include immediate owner conviction"
  );
  await assertNoAggregateLock("hotkeyLock", netuid, ownerHotkey, "owner-hotkey lock should not use general HotkeyLock");
  await assertNoAggregateLock("decayingHotkeyLock", netuid, ownerHotkey, "owner-hotkey lock should not use general DecayingHotkeyLock");
  console.log("default decaying owner aggregate lock:", formatLock(ownerAggregate));
  console.log("passive owner-hotkey immediate conviction:", formatLock(ownerHotkeyLock));

  await submitAndWait(
    api,
    ownerHotkeySource,
    api.tx.subtensorModule.setPerpetualLock(netuid, false),
    "setPerpetualLock false for owner-hotkey lock"
  );
  await assertNoDecayingFlag(ownerHotkeySource.address, netuid);
  const decayingOwnerAggregate = await requireAggregateLock(
    "decayingOwnerLock",
    netuid,
    ownerHotkey,
    "decaying owner aggregate lock"
  );
  assertConvictionAtLeast(
    decayingOwnerAggregate,
    ownerHotkeyLockAmount,
    "DecayingOwnerLock should retain immediate owner conviction"
  );
  await assertNoAggregateLock("decayingHotkeyLock", netuid, ownerHotkey, "owner-hotkey lock should not use general DecayingHotkeyLock");
  console.log("decaying owner aggregate lock:", formatLock(decayingOwnerAggregate));

  await submitAndWait(
    api,
    ownerHotkeySource,
    api.tx.subtensorModule.moveLock(destinationHotkey, netuid),
    "move owner-hotkey lock to non-owner hotkey"
  );
  await assertNoLock(ownerHotkeySource.address, netuid, ownerHotkey, "owner-hotkey moved lock should leave owner hotkey");
  const movedFromOwner = await requireLock(
    ownerHotkeySource.address,
    netuid,
    destinationHotkey,
    "owner-hotkey lock moved to non-owner hotkey"
  );
  assertConvictionZero(movedFromOwner, "moveLock from owner hotkey to non-owner hotkey should reset conviction");
  await waitForFinalizedBlocks(BLOCKS_BETWEEN_LOCK_ACTIONS);
  const movedFromOwnerTopUp = ownerHotkeyLockAmount / 10n;
  await lockStake(
    ownerHotkeySource,
    destinationHotkey,
    netuid,
    movedFromOwnerTopUp,
    "top-up moved owner-hotkey lock"
  );
  const movedFromOwnerAfterTopUp = await requireLock(
    ownerHotkeySource.address,
    netuid,
    destinationHotkey,
    "moved owner-hotkey lock after top-up"
  );
  assertConvictionNonZero(
    movedFromOwnerAfterTopUp,
    "moved owner-hotkey lock should start accumulating non-owner conviction"
  );
  assertConvictionLessThanWhole(
    movedFromOwnerAfterTopUp,
    movedFromOwnerAfterTopUp.lockedMass,
    "moved owner-hotkey lock should accumulate gradually after leaving owner hotkey"
  );
  console.log("moved owner-hotkey lock after gradual accumulation:", formatLock(movedFromOwnerAfterTopUp));

  const ownerAlphaAdded = await addStake(ownerStakeSource, ownerHotkey, netuid);
  const ownerLockAmount = ownerAlphaAdded / 2n;
  await lockStake(
    ownerStakeSource,
    ownerHotkey,
    netuid,
    ownerLockAmount,
    "subnet-owner coldkey lockStake to owner hotkey"
  );
  const ownerColdkeyLock = await requireLock(
    ownerStakeSource.address,
    netuid,
    ownerHotkey,
    "subnet-owner coldkey owner-hotkey lock"
  );
  assertConvictionAtLeast(
    ownerColdkeyLock,
    ownerColdkeyLock.lockedMass,
    "subnet-owner coldkey lock to subnet owner hotkey should receive immediate owner conviction"
  );
  console.log("subnet-owner coldkey immediate owner conviction:", formatLock(ownerColdkeyLock));

  await submitAndWait(
    api,
    ownerTransferDestination,
    api.tx.subtensorModule.setPerpetualLock(netuid, true),
    "setPerpetualLock true for owner transfer destination"
  );
  await requireDecayingFlag(ownerTransferDestination.address, netuid);

  const ownerTransferBefore = await requireLock(
    ownerStakeSource.address,
    netuid,
    ownerHotkey,
    "owner coldkey pre-transfer lock"
  );
  const ownerUnlockedAlpha = ownerAlphaAdded - ownerTransferBefore.lockedMass;
  const ownerSameSubnetTransferAmount = ownerUnlockedAlpha + ownerTransferBefore.lockedMass / 2n;
  await submitAndWait(
    api,
    ownerStakeSource,
    api.tx.subtensorModule.transferStake(
      ownerTransferDestination.address,
      ownerHotkey,
      netuid,
      netuid,
      ownerSameSubnetTransferAmount
    ),
    "owner-coldkey same-subnet transferStake"
  );
  const ownerSourceAfterTransfer = await requireLock(
    ownerStakeSource.address,
    netuid,
    ownerHotkey,
    "owner coldkey post-transfer lock"
  );
  const ownerDestinationAfterTransfer = await requireLock(
    ownerTransferDestination.address,
    netuid,
    ownerHotkey,
    "owner-coldkey transferred lock"
  );
  assertProportionalConvictionSplit(
    ownerSourceAfterTransfer,
    ownerDestinationAfterTransfer,
    "owner-coldkey transferStake should split conviction proportionally"
  );
  assertConvictionAtLeast(
    ownerDestinationAfterTransfer,
    ownerDestinationAfterTransfer.lockedMass,
    "owner-coldkey transferred lock to owner hotkey should keep immediate owner conviction"
  );
  console.log("owner source lock after same-subnet transfer:", formatLock(ownerSourceAfterTransfer));
  console.log("owner destination lock after same-subnet transfer:", formatLock(ownerDestinationAfterTransfer));

  const alphaAdded = await addStake(source, originHotkey, netuid);
  assert.ok(alphaAdded > 4n, "addStake returned too little alpha to exercise lock transfer");
  console.log("alpha added:", alphaAdded.toString());

  const initialLockAmount = alphaAdded / 2n;
  await lockStake(source, originHotkey, netuid, initialLockAmount, "initial lockStake");
  let sourceLock = await requireLock(source.address, netuid, originHotkey, "source initial lock");
  assert.equal(sourceLock.lockedMass, initialLockAmount, "initial Lock.lockedMass did not match locked amount");
  await requireAggregateLock("decayingHotkeyLock", netuid, originHotkey, "default decaying general aggregate lock");
  await assertNoLock(destination.address, netuid, originHotkey, "destination should start without this lock");
  console.log("initial lock:", formatLock(sourceLock));

  await expectDispatchError(
    api.tx.subtensorModule.lockStake(destinationHotkey, netuid, 1n),
    source,
    "lockStake to second hotkey",
    "LockHotkeyMismatch"
  );
  console.log("wrong-hotkey lock rejected: ok");

  await expectDispatchError(
    api.tx.subtensorModule.removeStakeLimit(originHotkey, netuid, alphaAdded, MIN_PRICE, false),
    source,
    "remove locked stake",
    "StakeUnavailable"
  );
  console.log("over-unstake while locked rejected: ok");

  await submitAndWait(
    api,
    source,
    api.tx.subtensorModule.setPerpetualLock(netuid, false),
    "setPerpetualLock false"
  );
  await assertNoDecayingFlag(source.address, netuid);
  await requireAggregateLock("decayingHotkeyLock", netuid, originHotkey, "decaying general aggregate lock");
  console.log("setPerpetualLock(false): ok");

  await submitAndWait(
    api,
    destination,
    api.tx.subtensorModule.setPerpetualLock(netuid, true),
    "setPerpetualLock true for transfer destination"
  );
  await requireDecayingFlag(destination.address, netuid);

  await waitForFinalizedBlocks(BLOCKS_BETWEEN_LOCK_ACTIONS);
  const topUpAmount = initialLockAmount / 4n;
  assert.ok(topUpAmount > 0n, "top-up amount rounded to zero");
  await lockStake(source, originHotkey, netuid, topUpAmount, "top-up lockStake");
  sourceLock = await requireLock(source.address, netuid, originHotkey, "source top-up lock");
  assert.ok(
    sourceLock.lockedMass > initialLockAmount,
    `top-up did not increase locked mass: ${sourceLock.lockedMass} <= ${initialLockAmount}`
  );
  assert.ok(sourceLock.lastUpdate > 0n, "top-up lock did not record a last_update block");
  console.log("top-up lock:", formatLock(sourceLock));

  const transferBefore = await requireLock(source.address, netuid, originHotkey, "source pre-transfer lock");
  const unlockedAlpha = alphaAdded - transferBefore.lockedMass;
  const sameSubnetTransferAmount = unlockedAlpha + transferBefore.lockedMass / 2n;
  assert.ok(
    sameSubnetTransferAmount > unlockedAlpha,
    "same-subnet transfer amount must exceed unlocked alpha to move part of the lock"
  );

  await submitAndWait(
    api,
    source,
    api.tx.subtensorModule.transferStake(
      destination.address,
      originHotkey,
      netuid,
      netuid,
      sameSubnetTransferAmount
    ),
    "same-subnet transferStake"
  );

  const sourceAfterTransfer = await requireLock(source.address, netuid, originHotkey, "source post-transfer lock");
  const destinationAfterTransfer = await requireLock(
    destination.address,
    netuid,
    originHotkey,
    "destination transferred lock"
  );
  assert.ok(
    sourceAfterTransfer.lockedMass < transferBefore.lockedMass,
    "same-subnet transfer did not reduce source locked mass"
  );
  assert.ok(destinationAfterTransfer.lockedMass > 0n, "same-subnet transfer did not move locked mass");
  assertConvictionNonZero(destinationAfterTransfer, "destination transferred lock should have conviction before moveLock");
  assertProportionalConvictionSplit(
    sourceAfterTransfer,
    destinationAfterTransfer,
    "non-owner transferStake should split conviction proportionally"
  );
  await assertAggregateMatchesLock(
    "decayingHotkeyLock",
    sourceAfterTransfer,
    netuid,
    originHotkey,
    "decaying source aggregate after transfer"
  );
  await assertAggregateMatchesLock(
    "hotkeyLock",
    destinationAfterTransfer,
    netuid,
    originHotkey,
    "perpetual destination aggregate after transfer"
  );
  console.log("source lock after same-subnet transfer:", formatLock(sourceAfterTransfer));
  console.log("destination lock after same-subnet transfer:", formatLock(destinationAfterTransfer));

  await fundDestinationFees();

  await submitAndWait(
    api,
    destination,
    api.tx.subtensorModule.moveLock(destinationHotkey, netuid),
    "moveLock"
  );
  await assertNoLock(destination.address, netuid, originHotkey, "moved lock should leave origin hotkey");
  const movedLock = await requireLock(destination.address, netuid, destinationHotkey, "moved destination lock");
  assert.ok(movedLock.lockedMass > 0n, "moveLock created an empty lock");
  assertConvictionZero(movedLock, "moveLock to a hotkey owned by a different coldkey should reset conviction");
  console.log("moved destination lock:", formatLock(movedLock));

  await submitAndWait(
    api,
    destination,
    api.tx.subtensorModule.setPerpetualLock(netuid, true),
    "setPerpetualLock true"
  );
  await requireDecayingFlag(destination.address, netuid);
  await requireAggregateLock("hotkeyLock", netuid, destinationHotkey, "moved perpetual aggregate lock");
  console.log("setPerpetualLock(true): ok");

  console.log("locks and conviction live-clone test: ok");
  console.log("not tested on this clone: owner reassignment is disabled in current coinbase code.");
  } finally {
    if (ownerToRestore !== null && ownerRestoreNetuid !== null) {
      await setSubnetOwnerForTest(ownerRestoreNetuid, ownerToRestore);
      console.log("subnet owner restored:", ownerToRestore);
      ownerToRestore = null;
      ownerRestoreNetuid = null;
    }
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

async function setSubnetOwnerForTest(netuid, ownerAddress) {
  const key = api.query.subtensorModule.subnetOwner.key(netuid);
  const value = api.createType("AccountId", ownerAddress).toHex();
  await submitAndWait(
    api,
    fundSource,
    api.tx.sudo.sudo(api.tx.system.setStorage([[key, value]])),
    `sudo set SubnetOwner(${netuid})`
  );
  const stored = (await api.query.subtensorModule.subnetOwner(netuid)).toString();
  assert.equal(stored, ownerAddress, `SubnetOwner(${netuid}) was not updated`);
  console.log(`SubnetOwner(${netuid}) set for test:`, ownerAddress);
}

async function prepareTestState(accountsWithLabels, netuid, ownerAddress, autoLockEnabled) {
  const accountsToFund = accountsWithLabels.filter(([account]) => account.address !== fundSource.address);
  const calls = accountsToFund.map(([account]) =>
    api.tx.balances.forceSetBalance(account.address, SOURCE_FUND_AMOUNT)
  );
  const ownerKey = api.query.subtensorModule.subnetOwner.key(netuid);
  const ownerValue = api.createType("AccountId", ownerAddress).toHex();
  const autoLockKey = api.query.subtensorModule.ownerCutAutoLockEnabled.key(netuid);
  const autoLockValue = autoLockEnabled ? "0x01" : "0x00";
  calls.push(api.tx.system.setStorage([
    [ownerKey, ownerValue],
    [autoLockKey, autoLockValue],
  ]));
  const batched = api.tx.utility?.batchAll ? api.tx.utility.batchAll(calls) : api.tx.utility.batch(calls);
  await submitAndWait(
    api,
    fundSource,
    api.tx.sudo.sudo(batched),
    "sudo prepare lock conviction test state"
  );

  for (const [account, label] of accountsToFund) {
    const free = (await api.query.system.account(account.address)).data.free.toBigInt();
    assert.ok(free >= SOURCE_FUND_AMOUNT, `${label} funding failed: free=${free}`);
    console.log(`${label} funded:`, free.toString());
  }
  const storedOwner = (await api.query.subtensorModule.subnetOwner(netuid)).toString();
  assert.equal(storedOwner, ownerAddress, `SubnetOwner(${netuid}) was not updated`);
  console.log(`SubnetOwner(${netuid}) set for test:`, ownerAddress);
  console.log(`OwnerCutAutoLockEnabled(${netuid}) set for test:`, autoLockEnabled);
}

function assertLockMetadataAvailable() {
  const missing = [
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Utility.batch", api.tx.utility?.batch],
    ["Balances.forceSetBalance", api.tx.balances?.forceSetBalance],
    ["SubtensorModule.lockStake", api.tx.subtensorModule?.lockStake],
    ["SubtensorModule.moveLock", api.tx.subtensorModule?.moveLock],
    ["SubtensorModule.setPerpetualLock", api.tx.subtensorModule?.setPerpetualLock],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.transferStake", api.tx.subtensorModule?.transferStake],
    ["SubtensorModule.Lock", api.query.subtensorModule?.lock],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["SubtensorModule.OwnerLock", api.query.subtensorModule?.ownerLock],
    ["SubtensorModule.DecayingOwnerLock", api.query.subtensorModule?.decayingOwnerLock],
    ["SubtensorModule.DecayingLock", api.query.subtensorModule?.decayingLock],
    ["SubtensorModule.SubnetOwnerHotkey", api.query.subtensorModule?.subnetOwnerHotkey],
    ["SubtensorModule.Owner", api.query.subtensorModule?.owner],
    ["SubtensorModule.SubnetOwner", api.query.subtensorModule?.subnetOwner],
    ["SubtensorModule.OwnerCutAutoLockEnabled", api.query.subtensorModule?.ownerCutAutoLockEnabled],
    ["SubtensorModule.UnlockRate", api.query.subtensorModule?.unlockRate],
    ["SubtensorModule.MaturityRate", api.query.subtensorModule?.maturityRate],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["Swap.PalSwapInitialized or Swap.SwapV3Initialized", initializedSubnetStorage()],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to a Conviction v2 runtime`
  );
}

async function readRates() {
  const [unlockRate, maturityRate] = await Promise.all([
    api.query.subtensorModule.unlockRate(),
    api.query.subtensorModule.maturityRate(),
  ]);
  return {
    unlockRate: unlockRate.toBigInt(),
    maturityRate: maturityRate.toBigInt(),
  };
}

async function findTestSubnet() {
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
      .filter((hotkey, index, all) => hotkey && all.indexOf(hotkey) === index);

    const movableHotkeys = hotkeys.filter((hotkey) => hotkey !== ownerHotkey);
    const hotkeyOwners = await Promise.all(
      movableHotkeys.map(async (hotkey) => ({
        hotkey,
        owner: (await api.query.subtensorModule.owner(hotkey)).toString(),
      }))
    );

    for (const origin of hotkeyOwners) {
      const destination = hotkeyOwners.find(({ hotkey, owner }) => hotkey !== origin.hotkey && owner !== origin.owner);
      if (destination) {
        return { netuid, ownerHotkey, hotkeys: [origin.hotkey, destination.hotkey] };
      }
    }
  }

  throw new Error("no initialized transfer-enabled subnet with an owner hotkey and two differently owned hotkeys found");
}

function initializedSubnetStorage() {
  return api.query.swap?.palSwapInitialized ?? api.query.swap?.swapV3Initialized;
}

async function addStake(signer, hotkey, netuid) {
  console.log(`submitting addStake: signer=${signer.address} hotkey=${hotkey} netuid=${netuid}`);
  const result = await submitAndWait(
    api,
    signer,
    api.tx.subtensorModule.addStake(hotkey, netuid, STAKE_AMOUNT),
    `addStake on netuid ${netuid}`
  );
  return assertStakeAddedEvent(result.events, hotkey, netuid);
}

async function lockStake(signer, hotkey, netuid, amount, label) {
  const result = await submitAndWait(
    api,
    signer,
    api.tx.subtensorModule.lockStake(hotkey, netuid, amount),
    label
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

async function assertAggregateMatchesLock(storageName, lock, netuid, hotkey, label) {
  const aggregate = await requireAggregateLock(storageName, netuid, hotkey, label);
  assert.ok(
    aggregate.lockedMass >= lock.lockedMass,
    `${label}: aggregate locked mass ${aggregate.lockedMass} is less than lock ${lock.lockedMass}`
  );
  assert.ok(
    aggregate.convictionBits >= lock.convictionBits,
    `${label}: aggregate conviction ${aggregate.convictionBits} is less than lock ${lock.convictionBits}`
  );
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

async function requireDecayingFlag(coldkey, netuid) {
  const maybeFlag = await api.query.subtensorModule.decayingLock(coldkey, netuid);
  assert.ok(maybeFlag.isSome, `expected DecayingLock(${coldkey}, ${netuid}) to exist`);
  assert.equal(maybeFlag.unwrap().toString(), "false", "DecayingLock flag should store false sentinel");
}

async function assertNoDecayingFlag(coldkey, netuid) {
  const maybeFlag = await api.query.subtensorModule.decayingLock(coldkey, netuid);
  assert.ok(maybeFlag.isNone, `expected DecayingLock(${coldkey}, ${netuid}) to be absent`);
}

async function fundDestinationFees() {
  const minimumFree = 100_000_000n;
  const destinationFree = (await api.query.system.account(destination.address)).data.free.toBigInt();
  if (destinationFree >= minimumFree) {
    return;
  }

  await setFreeBalance(destination.address, SOURCE_FUND_AMOUNT, "destination fee balance");
}

async function fundAccounts(accountsWithLabels) {
  const accountsToFund = accountsWithLabels.filter(([account]) => account.address !== fundSource.address);
  if (accountsToFund.length === 0) {
    return;
  }

  const calls = accountsToFund.map(([account]) => api.tx.balances.forceSetBalance(account.address, SOURCE_FUND_AMOUNT));
  const batched = api.tx.utility?.batchAll ? api.tx.utility.batchAll(calls) : api.tx.utility.batch(calls);
  await submitAndWait(api, fundSource, api.tx.sudo.sudo(batched), "sudo batch fund test accounts");

  for (const [account, label] of accountsToFund) {
    const free = (await api.query.system.account(account.address)).data.free.toBigInt();
    assert.ok(free >= SOURCE_FUND_AMOUNT, `${label} funding failed: free=${free}`);
    console.log(`${label} funded:`, free.toString());
  }
}

async function setFreeBalance(address, amount, label) {
  await submitAndWait(
    api,
    fundSource,
    api.tx.sudo.sudo(api.tx.balances.forceSetBalance(address, amount)),
    `sudo fund ${label}`
  );
  console.log(`${label} funded:`, amount.toString());
}

async function fundAccount(account, label) {
  if (account.address === fundSource.address) {
    return;
  }

  const sourceFree = (await api.query.system.account(account.address)).data.free.toBigInt();
  if (sourceFree >= SOURCE_FUND_AMOUNT / 2n) {
    return;
  }

  const funderFree = (await api.query.system.account(fundSource.address)).data.free.toBigInt();
  assert.ok(
    funderFree > SOURCE_FUND_AMOUNT,
    `fund source ${fundSource.address} has ${funderFree}, cannot transfer ${SOURCE_FUND_AMOUNT}`
  );

  await submitAndWait(
    api,
    fundSource,
    balancesTransfer(account.address, SOURCE_FUND_AMOUNT),
    `${label} balance transfer ${fundSource.address} -> ${account.address}`
  );
  console.log(`${label} funded:`, SOURCE_FUND_AMOUNT.toString());
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
  if (typeof value === "bigint") {
    return value;
  }
  if (typeof value === "number") {
    return BigInt(value);
  }
  if (typeof value === "string") {
    return BigInt(value.replaceAll(",", ""));
  }
  throw new Error(`could not decode conviction bits from ${value}`);
}

function assertConvictionAtLeast(lock, wholeConviction, label) {
  const wholeBits = wholeConviction << 64n;
  assert.ok(
    lock.convictionBits >= wholeBits,
    `${label}: conviction ${lock.conviction} is less than ${wholeConviction}`
  );
}

function assertConvictionLessThanWhole(lock, wholeConviction, label) {
  const wholeBits = wholeConviction << 64n;
  assert.ok(
    lock.convictionBits < wholeBits,
    `${label}: conviction ${lock.conviction} is not less than ${wholeConviction}`
  );
}

function assertConvictionNonZero(lock, label) {
  assert.ok(lock.convictionBits > 0n, `${label}: conviction was ${lock.conviction}`);
}

function assertConvictionZero(lock, label) {
  assert.equal(lock.convictionBits, 0n, `${label}: conviction was ${lock.conviction}`);
}

function assertProportionalConvictionSplit(sourceLock, destinationLock, label) {
  const totalLocked = sourceLock.lockedMass + destinationLock.lockedMass;
  const totalConviction = sourceLock.convictionBits + destinationLock.convictionBits;
  assert.ok(totalLocked > 0n, `${label}: total locked mass is zero`);
  assert.ok(totalConviction > 0n, `${label}: total conviction is zero`);

  const expectedDestinationConviction = totalConviction * destinationLock.lockedMass / totalLocked;
  assertWithinTolerance(
    destinationLock.convictionBits,
    expectedDestinationConviction,
    convictionTolerance(totalConviction),
    `${label}: destination conviction is not proportional to moved locked mass`
  );
}

function convictionTolerance(value) {
  return value / 1000000n + (1n << 64n);
}

function assertWithinTolerance(actual, expected, tolerance, label) {
  const diff = actual > expected ? actual - expected : expected - actual;
  assert.ok(
    diff <= tolerance,
    `${label}: actual=${actual} expected=${expected} tolerance=${tolerance}`
  );
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

async function expectDispatchError(tx, signer, label, expectedName) {
  await assert.rejects(
    () => submitAndWait(api, signer, tx, label),
    (error) => {
      assert.match(error.message, new RegExp(`\\b${expectedName}\\b`));
      return true;
    }
  );
}

async function submitAndWait(api, signer, tx, label) {
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
  for (let i = 0; i < count; i++) {
    await waitForFinalizedBlock();
  }
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

function formatLock(lock) {
  return `locked_mass=${lock.lockedMass} conviction=${lock.conviction} last_update=${lock.lastUpdate}`;
}
