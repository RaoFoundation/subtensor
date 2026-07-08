import assert from "node:assert/strict";
import fs from "node:fs";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

loadDotenv();

const NETWORK = "mainnet";
const WS_ENDPOINT = process.env.WS_ENDPOINT ?? defaultEndpoint();
const logger = createTempLogger("mainnet-lock-conviction-read.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: (message) => console.log(redactEndpoint(message)) });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const finalizedHash = await api.rpc.chain.getFinalizedHead();
    const header = await api.rpc.chain.getHeader(finalizedHash);
    const blockNumber = header.number.toBigInt();

    assertLockMetadataAvailable();

    const [unlockRate, maturityRate] = await Promise.all([
      api.query.subtensorModule.unlockRate.at(finalizedHash),
      api.query.subtensorModule.maturityRate.at(finalizedHash),
    ]);
    const unlockRateValue = unlockRate.toBigInt();
    const maturityRateValue = maturityRate.toBigInt();

    console.log("network:", NETWORK);
    console.log("endpoint:", redactEndpoint(WS_ENDPOINT));
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("block:", blockNumber.toString());
    console.log("block hash:", finalizedHash.toString());
    console.log("SubtensorModule.UnlockRate:", unlockRateValue.toString());
    console.log("SubtensorModule.MaturityRate:", maturityRateValue.toString());

    assert.ok(unlockRateValue > 0n, `expected positive UnlockRate, got ${unlockRateValue}`);
    assert.ok(maturityRateValue > 0n, `expected positive MaturityRate, got ${maturityRateValue}`);

    const ownerHotkeys = await readOwnerHotkeys(finalizedHash);
    const individualLocks = await readIndividualLocks(finalizedHash);
    const aggregateLocks = await readAggregateLocks(finalizedHash);
    const perpetualFlags = await readPerpetualFlags(finalizedHash);

    console.log("SubnetOwnerHotkey entries:", ownerHotkeys.size);
    console.log("Lock entries:", individualLocks.length);
    console.log("HotkeyLock entries:", aggregateLocks.hotkeyLock.size);
    console.log("DecayingHotkeyLock entries:", aggregateLocks.decayingHotkeyLock.size);
    console.log("OwnerLock entries:", aggregateLocks.ownerLock.size);
    console.log("DecayingOwnerLock entries:", aggregateLocks.decayingOwnerLock.size);
    console.log("DecayingLock perpetual flag entries:", perpetualFlags.size);

    const expectedAggregates = {
      hotkeyLock: new Map(),
      decayingHotkeyLock: new Map(),
      ownerLock: new Map(),
      decayingOwnerLock: new Map(),
    };
    const seenPerpetualFlagKeys = new Set();
    const failures = [];

    for (const lock of individualLocks) {
      const lastUpdateOk = lock.lastUpdate <= blockNumber;
      const activeOk = lock.lockedMass > 0n || lock.convictionBits > 0n;
      const ownerHotkey = ownerHotkeys.get(lock.netuid);
      const isOwnerLock = ownerHotkey === lock.hotkey;
      const flagKey = coldkeyNetuidKey(lock.coldkey, lock.netuid);
      const isPerpetual = perpetualFlags.has(flagKey);
      if (isPerpetual) {
        seenPerpetualFlagKeys.add(flagKey);
      }

      const aggregateName = aggregateStorageName(isOwnerLock, isPerpetual);
      const aggregateKey = isOwnerLock ? String(lock.netuid) : netuidHotkeyKey(lock.netuid, lock.hotkey);
      addLock(expectedAggregates[aggregateName], aggregateKey, lock);

      if (!activeOk || !lastUpdateOk) {
        failures.push(
          `${formatLockId(lock)} active=${activeOk} lastUpdateOk=${lastUpdateOk} ${formatLock(lock)}`
        );
      }

      const ownerConvictionBits = lock.lockedMass << 64n;
      if (isOwnerLock && lock.convictionBits !== ownerConvictionBits) {
        failures.push(
          `${formatLockId(lock)} owner lock conviction_bits ${lock.convictionBits} != lockedMass<<64 ${ownerConvictionBits}`
        );
      }
    }

    for (const flagKey of perpetualFlags.keys()) {
      if (!seenPerpetualFlagKeys.has(flagKey)) {
        failures.push(`DecayingLock perpetual flag has no matching Lock entries: ${flagKey}`);
      }
    }

    for (const [storageName, expectedMap] of Object.entries(expectedAggregates)) {
      compareAggregateMap(storageName, expectedMap, aggregateLocks[storageName], failures);
    }

    for (const lock of individualLocks.slice(0, 25)) {
      console.log("sample lock:", formatLockId(lock), formatLock(lock));
    }

    if (failures.length > 0) {
      for (const failure of failures) {
        console.log("failure:", failure);
      }
    }

    assert.equal(failures.length, 0, `lock/conviction sanity failures=${failures.length}`);

    if (individualLocks.length === 0) {
      console.log("mainnet lock/conviction read: ok (no live lock entries found)");
    } else {
      console.log(`mainnet lock/conviction read: ok (${individualLocks.length} lock entries checked)`);
    }
  } finally {
    await api?.disconnect();
  }
}

function assertLockMetadataAvailable() {
  const missing = [
    ["SubtensorModule.lockStake", api.tx.subtensorModule?.lockStake],
    ["SubtensorModule.setPerpetualLock", api.tx.subtensorModule?.setPerpetualLock],
    ["SubtensorModule.moveLock", api.tx.subtensorModule?.moveLock],
    ["SubtensorModule.Lock", api.query.subtensorModule?.lock],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["SubtensorModule.OwnerLock", api.query.subtensorModule?.ownerLock],
    ["SubtensorModule.DecayingOwnerLock", api.query.subtensorModule?.decayingOwnerLock],
    ["SubtensorModule.DecayingLock", api.query.subtensorModule?.decayingLock],
    ["SubtensorModule.UnlockRate", api.query.subtensorModule?.unlockRate],
    ["SubtensorModule.MaturityRate", api.query.subtensorModule?.maturityRate],
    ["SubtensorModule.SubnetOwnerHotkey", api.query.subtensorModule?.subnetOwnerHotkey],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable on mainnet`
  );
}

async function readOwnerHotkeys(blockHash) {
  const entries = await api.query.subtensorModule.subnetOwnerHotkey.entriesAt(blockHash);
  const ownerHotkeys = new Map();
  for (const [key, value] of entries) {
    ownerHotkeys.set(key.args[0].toNumber(), value.toString());
  }
  return ownerHotkeys;
}

async function readIndividualLocks(blockHash) {
  const entries = await api.query.subtensorModule.lock.entriesAt(blockHash);
  return entries.map(([key, value]) => {
    const [coldkey, netuid, hotkey] = key.args;
    return {
      coldkey: coldkey.toString(),
      netuid: netuid.toNumber(),
      hotkey: hotkey.toString(),
      ...decodeLockState(value),
    };
  });
}

async function readAggregateLocks(blockHash) {
  return {
    hotkeyLock: await readNetuidHotkeyAggregates("hotkeyLock", blockHash),
    decayingHotkeyLock: await readNetuidHotkeyAggregates("decayingHotkeyLock", blockHash),
    ownerLock: await readNetuidAggregates("ownerLock", blockHash),
    decayingOwnerLock: await readNetuidAggregates("decayingOwnerLock", blockHash),
  };
}

async function readNetuidHotkeyAggregates(storageName, blockHash) {
  const entries = await api.query.subtensorModule[storageName].entriesAt(blockHash);
  const aggregates = new Map();
  for (const [key, value] of entries) {
    const [netuid, hotkey] = key.args;
    aggregates.set(netuidHotkeyKey(netuid.toNumber(), hotkey.toString()), decodeLockState(value));
  }
  return aggregates;
}

async function readNetuidAggregates(storageName, blockHash) {
  const entries = await api.query.subtensorModule[storageName].entriesAt(blockHash);
  const aggregates = new Map();
  for (const [key, value] of entries) {
    aggregates.set(String(key.args[0].toNumber()), decodeLockState(value));
  }
  return aggregates;
}

async function readPerpetualFlags(blockHash) {
  const entries = await api.query.subtensorModule.decayingLock.entriesAt(blockHash);
  const flags = new Map();
  for (const [key, value] of entries) {
    const [coldkey, netuid] = key.args;
    const flag = value.toJSON();
    assert.equal(flag, false, `expected DecayingLock(${coldkey}, ${netuid}) to store false`);
    flags.set(coldkeyNetuidKey(coldkey.toString(), netuid.toNumber()), flag);
  }
  return flags;
}

function decodeLockState(lockState) {
  const lockedMass = parseBigIntish(structField(lockState, "lockedMass", "locked_mass"));
  const convictionValue = structField(lockState, "conviction");
  const convictionBits = decodeConvictionBits(convictionValue);
  const convictionDecimal = formatConvictionValue(convictionValue);
  const lastUpdate = parseBigIntish(structField(lockState, "lastUpdate", "last_update"));
  return { lockedMass, convictionBits, convictionDecimal, lastUpdate };
}

function decodeConvictionBits(value) {
  if (value?.bits !== undefined) {
    return parseBigIntish(value.bits);
  }

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

function formatConvictionValue(value) {
  if (value?.toString && value.toString !== Object.prototype.toString) {
    return value.toString();
  }
  if (value?.bits !== undefined) {
    return JSON.stringify({ bits: value.bits.toString?.() ?? value.bits });
  }
  return String(value);
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
  throw new Error(`could not decode bigint value from ${value}`);
}

function structField(value, ...names) {
  for (const name of names) {
    if (value[name]) return value[name];
    const field = value.get?.(name);
    if (field) return field;
  }

  const json = value.toJSON?.();
  if (json) {
    for (const name of names) {
      if (json[name] !== undefined) return json[name];
    }
  }

  throw new Error(`could not decode field ${names.join("/")} from ${value.toString()}`);
}

function aggregateStorageName(isOwnerLock, isPerpetual) {
  if (isOwnerLock) {
    return isPerpetual ? "ownerLock" : "decayingOwnerLock";
  }
  return isPerpetual ? "hotkeyLock" : "decayingHotkeyLock";
}

function addLock(map, key, lock) {
  const existing = map.get(key) ?? {
    lockedMass: 0n,
    convictionBits: 0n,
  };
  map.set(key, {
    lockedMass: existing.lockedMass + lock.lockedMass,
    convictionBits: existing.convictionBits + lock.convictionBits,
  });
}

function compareAggregateMap(storageName, expectedMap, actualMap, failures) {
  for (const [key, expected] of expectedMap) {
    const actual = actualMap.get(key);
    if (!actual) {
      failures.push(`${storageName}(${key}) missing; expected ${formatAggregate(expected)}`);
      continue;
    }

    if (
      actual.lockedMass !== expected.lockedMass ||
      actual.convictionBits !== expected.convictionBits
    ) {
      failures.push(
        `${storageName}(${key}) expected ${formatAggregate(expected)} actual ${formatLock(actual)}`
      );
    }
  }

  for (const [key, actual] of actualMap) {
    if (!expectedMap.has(key)) {
      failures.push(`${storageName}(${key}) has no matching individual locks: ${formatLock(actual)}`);
    }
  }
}

function formatAggregate(lock) {
  return `locked_mass=${lock.lockedMass} conviction_bits=${lock.convictionBits}`;
}

function formatLock(lock) {
  return `locked_mass=${lock.lockedMass} conviction=${lock.convictionDecimal} conviction_bits=${lock.convictionBits} last_update=${lock.lastUpdate}`;
}

function formatLockId(lock) {
  return `Lock(${lock.coldkey}, ${lock.netuid}, ${lock.hotkey})`;
}

function coldkeyNetuidKey(coldkey, netuid) {
  return `${coldkey}|${netuid}`;
}

function netuidHotkeyKey(netuid, hotkey) {
  return `${netuid}|${hotkey}`;
}

function defaultEndpoint() {
  assert.ok(
    process.env.ONFINALITY_API_KEY,
    "ONFINALITY_API_KEY is required for mainnet unless WS_ENDPOINT is set"
  );
  return `wss://bittensor-finney.api.onfinality.io/ws?apikey=${process.env.ONFINALITY_API_KEY}`;
}

function loadDotenv() {
  for (const path of [".env", "../.env"]) {
    if (!fs.existsSync(path)) {
      continue;
    }

    for (const line of fs.readFileSync(path, "utf8").split("\n")) {
      const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$/);
      if (!match || process.env[match[1]] !== undefined) {
        continue;
      }

      process.env[match[1]] = match[2].replace(/^['"]|['"]$/g, "");
    }
  }
}

function redactEndpoint(endpoint) {
  return endpoint.replace(/(apikey=)[^&]+/i, "$1<redacted>");
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});
