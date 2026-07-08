import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const SIGNER_URI = process.env.TESTNET_LOCK_SMOKE_URI ?? "//TestnetLockConvictionSmoke//funded";
const STAKE_AMOUNT = BigInt(process.env.TESTNET_LOCK_SMOKE_STAKE_AMOUNT ?? "1000000000");
const MIN_FREE_BALANCE = BigInt(process.env.TESTNET_LOCK_SMOKE_MIN_FREE_BALANCE ?? "2000000000");
const MIN_PRICE = BigInt(process.env.TESTNET_LOCK_SMOKE_MIN_PRICE ?? "0");
const NETUID = optionalNumber(process.env.TESTNET_LOCK_SMOKE_NETUID);
const HOTKEY = process.env.TESTNET_LOCK_SMOKE_HOTKEY;
const BLOCKS_AFTER_DECAYING = Number(process.env.TESTNET_LOCK_SMOKE_WAIT_BLOCKS ?? "2");

const keyring = new Keyring({ type: "sr25519" });
const signer = keyring.addFromUri(SIGNER_URI);
const logger = createTempLogger("testnet-lock-conviction-smoke.log");
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
    console.log("signer coldkey:", signer.address);

    assertLockMetadataAvailable();

    const free = (await api.query.system.account(signer.address)).data.free.toBigInt();
    console.log("signer free balance:", free.toString());
    assert.ok(
      free >= MIN_FREE_BALANCE,
      `fund ${signer.address} before running smoke test; free=${free}, required>=${MIN_FREE_BALANCE}`
    );

    const rates = await readRates();
    console.log("lock rates:", `unlock=${rates.unlockRate}`, `maturity=${rates.maturityRate}`);

    const { netuid, hotkey } = await resolveTarget();
    console.log("target netuid:", netuid);
    console.log("target hotkey:", hotkey);

    const lockBefore = await readLock(signer.address, netuid, hotkey);
    const lockedBefore = lockBefore?.lockedMass ?? 0n;
    const wasPerpetual = await hasPerpetualFlag(signer.address, netuid);
    if (lockBefore) {
      console.log("pre-existing lock:", formatLock(lockBefore), `perpetual=${wasPerpetual}`);
    }

    const alphaAdded = await addStake(hotkey, netuid);
    assert.ok(alphaAdded > 1n, "addStake returned too little alpha to lock");
    console.log("alpha added:", alphaAdded.toString());

    const lockAmount = alphaAdded / 2n;
    assert.ok(lockAmount > 0n, "computed lock amount rounded to zero");
    await lockStake(hotkey, netuid, lockAmount, "initial lockStake");

    const initialLock = await requireLock(signer.address, netuid, hotkey, "initial signer lock");
    assert.ok(
      initialLock.lockedMass > lockedBefore,
      `locked mass did not increase: before=${lockedBefore}, amount=${lockAmount}, after=${initialLock.lockedMass}`
    );
    if (wasPerpetual) {
      await requireAggregateLock("hotkeyLock", netuid, hotkey, "perpetual aggregate lock");
    } else {
      await requireAggregateLock("decayingHotkeyLock", netuid, hotkey, "default decaying aggregate lock");
    }
    console.log("initial lock:", formatLock(initialLock));

    const pairStake = await readPairStake(hotkey, signer.address, netuid);
    const availableToUnstake = pairStake > initialLock.lockedMass ? pairStake - initialLock.lockedMass : 0n;
    const lockedUnstakeAttempt = availableToUnstake + initialLock.lockedMass / 2n;
    assert.ok(
      lockedUnstakeAttempt <= pairStake,
      `locked unstake attempt exceeded pair stake: attempt=${lockedUnstakeAttempt}, pairStake=${pairStake}`
    );
    console.log(
      "pair stake:",
      pairStake.toString(),
      "available to unstake:",
      availableToUnstake.toString(),
      "locked unstake attempt:",
      lockedUnstakeAttempt.toString()
    );

    const rejectedLockedUnstake = await maybeExpectDispatchError(
      api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, lockedUnstakeAttempt, MIN_PRICE, false),
      "remove locked stake",
      "StakeUnavailable"
    );
    console.log(
      rejectedLockedUnstake
        ? "over-unstake while locked rejected: ok"
        : "over-unstake while locked was accepted on this test account; continuing smoke"
    );

    await submitAndWait(
      api.tx.subtensorModule.setPerpetualLock(netuid, false),
      "setPerpetualLock false"
    );
    await assertNoPerpetualFlag(signer.address, netuid);
    const decayingAggregate = await requireAggregateLock(
      "decayingHotkeyLock",
      netuid,
      hotkey,
      "decaying aggregate lock"
    );
    console.log("decaying aggregate lock:", formatLock(decayingAggregate));

    await waitForFinalizedBlocks(BLOCKS_AFTER_DECAYING);

    const decayingLock = await requireLock(signer.address, netuid, hotkey, "decaying signer lock");
    assert.ok(decayingLock.lastUpdate > 0n, "decaying lock did not record last_update");
    console.log("decaying signer lock:", formatLock(decayingLock));
    console.log("testnet lock/conviction smoke: ok");
  } finally {
    await api?.disconnect();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});

function assertLockMetadataAvailable() {
  const missing = [
    ["SubtensorModule.lockStake", api.tx.subtensorModule?.lockStake],
    ["SubtensorModule.setPerpetualLock", api.tx.subtensorModule?.setPerpetualLock],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.Lock", api.query.subtensorModule?.lock],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["SubtensorModule.DecayingLock", api.query.subtensorModule?.decayingLock],
    ["SubtensorModule.UnlockRate", api.query.subtensorModule?.unlockRate],
    ["SubtensorModule.MaturityRate", api.query.subtensorModule?.maturityRate],
    ["SubtensorModule.AlphaV2", api.query.subtensorModule?.alphaV2],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.TotalHotkeySharesV2", api.query.subtensorModule?.totalHotkeySharesV2],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["Swap.PalSwapInitialized or Swap.SwapV3Initialized", initializedSubnetStorage()],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after lock/conviction is deployed to testnet`
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

async function resolveTarget() {
  if (NETUID !== undefined && HOTKEY) {
    return { netuid: NETUID, hotkey: HOTKEY };
  }

  const initializedEntries = await initializedSubnetStorage().entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);

  for (const netuid of initializedNetuids) {
    if (NETUID !== undefined && netuid !== NETUID) continue;

    const transferEnabled = await api.query.subtensorModule.transferToggle(netuid);
    if (!transferEnabled.isTrue) continue;

    const hotkey = HOTKEY ?? (await firstSubnetHotkey(netuid));
    if (hotkey) {
      return { netuid, hotkey };
    }
  }

  throw new Error("no initialized transfer-enabled subnet with at least one hotkey found");
}

async function firstSubnetHotkey(netuid) {
  const entries = await api.query.subtensorModule.keys.entries(netuid);
  const hotkeyEntry = entries.find(([, hotkey]) => hotkey.toString());
  return hotkeyEntry?.[1].toString();
}

function initializedSubnetStorage() {
  return api.query.swap?.palSwapInitialized ?? api.query.swap?.swapV3Initialized;
}

async function addStake(hotkey, netuid) {
  const result = await submitAndWait(
    api.tx.subtensorModule.addStake(hotkey, netuid, STAKE_AMOUNT),
    `addStake on netuid ${netuid}`
  );
  return assertStakeAddedEvent(result.events, hotkey, netuid);
}

async function lockStake(hotkey, netuid, amount, label) {
  const result = await submitAndWait(api.tx.subtensorModule.lockStake(hotkey, netuid, amount), label);
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
  const lock = await readLock(coldkey, netuid, hotkey);
  assert.ok(lock, `${label}: expected Lock(${coldkey}, ${netuid}, ${hotkey}) to exist`);
  return lock;
}

async function readLock(coldkey, netuid, hotkey) {
  const maybeLock = await api.query.subtensorModule.lock(coldkey, netuid, hotkey);
  return maybeLock.isSome ? decodeLockState(maybeLock.unwrap()) : undefined;
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

async function assertNoPerpetualFlag(coldkey, netuid) {
  const maybeFlag = await api.query.subtensorModule.decayingLock(coldkey, netuid);
  assert.ok(maybeFlag.isNone, `expected DecayingLock(${coldkey}, ${netuid}) to be absent`);
}

async function hasPerpetualFlag(coldkey, netuid) {
  const maybeFlag = await api.query.subtensorModule.decayingLock(coldkey, netuid);
  return maybeFlag.isSome;
}

async function readPairStake(hotkey, coldkey, netuid) {
  const [share, totalHotkeyStake, totalHotkeyShares] = await Promise.all([
    readAlphaShare(hotkey, coldkey, netuid),
    api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid),
    readTotalHotkeyShares(hotkey, netuid),
  ]);

  return fixedMulDivToBigInt(share, totalHotkeyStake.toBigInt(), totalHotkeyShares);
}

async function readAlphaShare(hotkey, coldkey, netuid) {
  if (api.query.subtensorModule?.alpha) {
    const legacyShare = await readOptionalStorage(
      api.query.subtensorModule.alpha,
      hotkey,
      coldkey,
      netuid
    );
    if (legacyShare) {
      return decodeFixedRational(legacyShare);
    }
  }

  const shareV2 = await api.query.subtensorModule.alphaV2(hotkey, coldkey, netuid);
  return decodeFixedRational(shareV2);
}

async function readTotalHotkeyShares(hotkey, netuid) {
  if (api.query.subtensorModule?.totalHotkeyShares) {
    const legacyShares = await readOptionalStorage(
      api.query.subtensorModule.totalHotkeyShares,
      hotkey,
      netuid
    );
    if (legacyShares) {
      return decodeFixedRational(legacyShares);
    }
  }

  const sharesV2 = await api.query.subtensorModule.totalHotkeySharesV2(hotkey, netuid);
  return decodeFixedRational(sharesV2);
}

async function readOptionalStorage(query, ...args) {
  const storageKey = query.key(...args);
  const storage = await api.rpc.state.getStorage(storageKey);
  if (!storage || storage.isNone || storage.isEmpty || storage.unwrap?.().isEmpty) {
    return undefined;
  }
  return query(...args);
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

function decodeFixedRational(value) {
  const human = value.toHuman?.();
  if (human?.mantissa !== undefined || value.mantissa !== undefined || value.get?.("mantissa")) {
    const mantissa = parseBigIntish(human?.mantissa ?? structField(value, "mantissa").toString());
    const exponent = Number(
      String(human?.exponent ?? structField(value, "exponent").toString()).replaceAll(",", "")
    );
    return rationalFromDecimalExponent(mantissa, exponent);
  }

  const bits = value.toBigInt ? value.toBigInt() : parseBigIntish(value.toString());
  return { numerator: bits, denominator: 1n << 64n };
}

function rationalFromDecimalExponent(mantissa, exponent) {
  if (exponent >= 0) {
    return { numerator: mantissa * 10n ** BigInt(exponent), denominator: 1n };
  }

  return { numerator: mantissa, denominator: 10n ** BigInt(-exponent) };
}

function fixedMulDivToBigInt(multiplier, value, divisor) {
  assert.ok(divisor.numerator > 0n, "total hotkey shares must be positive");
  return (
    multiplier.numerator *
    value *
    divisor.denominator /
    (multiplier.denominator * divisor.numerator)
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

async function expectDispatchError(tx, label, expectedName) {
  await assert.rejects(
    () => submitAndWait(tx, label),
    (error) => {
      assert.match(error.message, new RegExp(`\\b${expectedName}\\b`));
      return true;
    }
  );
}

async function maybeExpectDispatchError(tx, label, expectedName) {
  try {
    await submitAndWait(tx, label);
    return false;
  } catch (error) {
    assert.match(error.message, new RegExp(`\\b${expectedName}\\b`));
    return true;
  }
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
  assert.ok(Number.isInteger(parsed), `expected integer, got ${value}`);
  return parsed;
}
