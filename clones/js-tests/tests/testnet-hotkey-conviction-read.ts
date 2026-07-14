import assert from "node:assert/strict";
import fs from "node:fs";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

loadDotenv();

const HOTKEY = process.env.HOTKEY ?? "5Eeq15e55UTfFcsw6kUNRhzRwTsAHciXk5sPajARWXxtjzyA";
const NETUID = process.env.NETUID === undefined ? 39 : Number(process.env.NETUID);
const NETWORK = process.env.NETWORK ?? "mainnet";
const WS_ENDPOINT = process.env.WS_ENDPOINT ?? defaultEndpoint();
const FRACTION_BITS = 64n;
const U64F64_SCALE = 1n << FRACTION_BITS;
const RAO_PER_TAO = 1_000_000_000n;
const logger = createTempLogger("testnet-hotkey-conviction-read.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: (message) => console.log(redactEndpoint(message)) });

  try {
    assertLockMetadataAvailable();

    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();

    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("network:", NETWORK);
    console.log("endpoint:", redactEndpoint(WS_ENDPOINT));
    console.log("hotkey:", HOTKEY);
    console.log("netuid:", NETUID ?? "all");

    const first = await readHotkeyConvictionAtCurrentBlock();
    await waitForNextBlock(first.blockNumber);
    const second = await readHotkeyConvictionAtCurrentBlock();

    printSample("first", first);
    printSample("second", second);

    assert.ok(second.blockNumber > first.blockNumber, "expected second read to be at a later block");
    if (NETUID !== null) {
      assert.ok(
        second.totalBits > first.totalBits,
        `expected conviction to increase on netuid ${NETUID}: ${first.totalBits} -> ${second.totalBits}`
      );
    }
    console.log("testnet hotkey conviction read: ok");
  } finally {
    await api?.disconnect();
  }
}

async function readHotkeyConvictionAtCurrentBlock() {
  const header = await api.rpc.chain.getHeader();
  const blockHash = header.hash;
  const blockNumber = header.number.toBigInt();
  const [unlockRate, maturityRate, lockRows] = await Promise.all([
    api.query.subtensorModule.unlockRate.at(blockHash),
    api.query.subtensorModule.maturityRate.at(blockHash),
    readHotkeyLockRows(blockHash),
  ]);

  const rolledRows = lockRows.map((row) => ({
    ...row,
    rolled: rollForwardLock(row.lock, blockNumber, unlockRate.toBigInt(), maturityRate.toBigInt(), row.perpetual, row.owner),
  }));
  const totalBits = rolledRows.reduce((sum, row) => sum + row.rolled.convictionBits, 0n);

  return {
    blockNumber,
    blockHash: blockHash.toString(),
    unlockRate: unlockRate.toBigInt(),
    maturityRate: maturityRate.toBigInt(),
    totalBits,
    rows: rolledRows,
  };
}

async function readHotkeyLockRows(blockHash) {
  if (NETUID !== null) {
    assert.ok(Number.isInteger(NETUID) && NETUID >= 0, `invalid NETUID: ${process.env.NETUID}`);
    return readHotkeyLockRowsForNetuid(blockHash, NETUID);
  }

  const ownerHotkeys = await readOwnerHotkeys(blockHash);
  const hotkeyLocks = await readHotkeyLocksAt(blockHash, "hotkeyLock");
  const decayingHotkeyLocks = await readHotkeyLocksAt(blockHash, "decayingHotkeyLock");
  const ownerLocks = await readOwnerLocksForHotkeyAt(blockHash, "ownerLock", ownerHotkeys);
  const decayingOwnerLocks = await readOwnerLocksForHotkeyAt(blockHash, "decayingOwnerLock", ownerHotkeys);

  return [
    ...hotkeyLocks.map((row) => ({ ...row, storage: "HotkeyLock", perpetual: true, owner: false })),
    ...decayingHotkeyLocks.map((row) => ({
      ...row,
      storage: "DecayingHotkeyLock",
      perpetual: false,
      owner: false,
    })),
    ...ownerLocks.map((row) => ({ ...row, storage: "OwnerLock", perpetual: true, owner: true })),
    ...decayingOwnerLocks.map((row) => ({
      ...row,
      storage: "DecayingOwnerLock",
      perpetual: false,
      owner: true,
    })),
  ];
}

async function readHotkeyLockRowsForNetuid(blockHash, netuid) {
  const ownerHotkey = (await api.query.subtensorModule.subnetOwnerHotkey.at(blockHash, netuid)).toString();
  const rows = [
    await readMaybeLockAt(blockHash, "hotkeyLock", [netuid, HOTKEY], {
      netuid,
      hotkey: HOTKEY,
      storage: "HotkeyLock",
      perpetual: true,
      owner: false,
    }),
    await readMaybeLockAt(blockHash, "decayingHotkeyLock", [netuid, HOTKEY], {
      netuid,
      hotkey: HOTKEY,
      storage: "DecayingHotkeyLock",
      perpetual: false,
      owner: false,
    }),
  ];

  if (ownerHotkey === HOTKEY) {
    rows.push(
      await readMaybeLockAt(blockHash, "ownerLock", [netuid], {
        netuid,
        hotkey: HOTKEY,
        storage: "OwnerLock",
        perpetual: true,
        owner: true,
      }),
      await readMaybeLockAt(blockHash, "decayingOwnerLock", [netuid], {
        netuid,
        hotkey: HOTKEY,
        storage: "DecayingOwnerLock",
        perpetual: false,
        owner: true,
      })
    );
  }

  return rows.filter(Boolean);
}

async function readMaybeLockAt(blockHash, storageName, args, row) {
  const maybeLock = await api.query.subtensorModule[storageName].at(blockHash, ...args);
  if (maybeLock.isNone) {
    return null;
  }
  return {
    ...row,
    lock: decodeLockState(maybeLock.unwrap()),
  };
}

async function waitForNextBlock(previousBlockNumber) {
  for (;;) {
    await sleep(1000);
    const header = await api.rpc.chain.getHeader();
    if (header.number.toBigInt() > previousBlockNumber) {
      return;
    }
  }
}

async function readOwnerHotkeys(blockHash) {
  const entries = await api.query.subtensorModule.subnetOwnerHotkey.entriesAt(blockHash);
  const ownerHotkeys = new Map();
  for (const [key, value] of entries) {
    ownerHotkeys.set(key.args[0].toNumber(), value.toString());
  }
  return ownerHotkeys;
}

async function readHotkeyLocksAt(blockHash, storageName) {
  const entries = await api.query.subtensorModule[storageName].entriesAt(blockHash);
  return entries
    .map(([key, value]) => {
      const [netuid, hotkey] = key.args;
      return {
        netuid: netuid.toNumber(),
        hotkey: hotkey.toString(),
        lock: decodeLockState(value),
      };
    })
    .filter((row) => row.hotkey === HOTKEY);
}

async function readOwnerLocksForHotkeyAt(blockHash, storageName, ownerHotkeys) {
  const entries = await api.query.subtensorModule[storageName].entriesAt(blockHash);
  return entries
    .map(([key, value]) => {
      const netuid = key.args[0].toNumber();
      return {
        netuid,
        hotkey: ownerHotkeys.get(netuid),
        lock: decodeLockState(value),
      };
    })
    .filter((row) => row.hotkey === HOTKEY);
}

function rollForwardLock(lock, now, unlockRate, maturityRate, perpetualLock, ownerLock) {
  if (now <= lock.lastUpdate) {
    return lock;
  }

  const dt = now - lock.lastUpdate;
  const unlockDecay = expDecay(dt, unlockRate);
  const maturityDecay = expDecay(dt, maturityRate);
  const lockedMass = Number(lock.lockedMass);
  const conviction = Number(lock.convictionBits) / Number(U64F64_SCALE);
  const newLockedMass = perpetualLock ? lockedMass : lockedMass * unlockDecay;

  let newConviction;
  if (perpetualLock) {
    newConviction = conviction * maturityDecay + lockedMass * (1 - maturityDecay);
  } else if (unlockRate === maturityRate) {
    newConviction = conviction * maturityDecay + lockedMass * (Number(dt) / Number(maturityRate)) * maturityDecay;
  } else if (unlockRate === 0n || maturityRate === 0n) {
    newConviction = conviction * maturityDecay;
  } else {
    const gamma = (Number(unlockRate) * (unlockDecay - maturityDecay)) / Number(unlockRate - maturityRate);
    newConviction = conviction * maturityDecay + lockedMass * Math.max(gamma, 0);
  }

  if (ownerLock) {
    newConviction = newLockedMass;
  }

  return {
    lockedMass: BigInt(Math.trunc(newLockedMass)),
    convictionBits: decimalToU64F64Bits(newConviction),
    lastUpdate: now,
  };
}

function expDecay(dt, tau) {
  if (dt === 0n) return 1;
  if (tau === 0n) return 0;
  return Math.exp(Math.max(-40, -Number(dt) / Number(tau)));
}

function decimalToU64F64Bits(value) {
  if (!Number.isFinite(value) || value <= 0) {
    return 0n;
  }
  return BigInt(Math.trunc(value * Number(U64F64_SCALE)));
}

function decodeLockState(lockState) {
  const lockedMass = parseBigIntish(structField(lockState, "lockedMass", "locked_mass"));
  const convictionValue = structField(lockState, "conviction");
  const convictionBits = decodeConvictionBits(convictionValue);
  const lastUpdate = parseBigIntish(structField(lockState, "lastUpdate", "last_update"));
  return { lockedMass, convictionBits, lastUpdate };
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

function formatU64F64(bits, fractionalDigits = 9) {
  const whole = bits / U64F64_SCALE;
  const fraction = bits % U64F64_SCALE;
  const scaledFraction = (fraction * 10n ** BigInt(fractionalDigits)) / U64F64_SCALE;
  return `${whole}.${scaledFraction.toString().padStart(fractionalDigits, "0")}`;
}

function formatTaoFromU64F64(bits, fractionalDigits = 9) {
  const scaled = (bits * 10n ** BigInt(fractionalDigits)) / U64F64_SCALE / RAO_PER_TAO;
  const whole = scaled / 10n ** BigInt(fractionalDigits);
  const fraction = scaled % 10n ** BigInt(fractionalDigits);
  return `${whole}.${fraction.toString().padStart(fractionalDigits, "0")}`;
}

function printSample(label, sample) {
  console.log(`${label} block:`, sample.blockNumber.toString());
  console.log(`${label} block hash:`, sample.blockHash);
  console.log(`${label} UnlockRate:`, sample.unlockRate.toString());
  console.log(`${label} MaturityRate:`, sample.maturityRate.toString());
  console.log(`${label} lock rows:`, sample.rows.length.toString());
  console.log(`${label} conviction:`, formatU64F64(sample.totalBits));
  console.log(`${label} conviction_tao_units:`, formatTaoFromU64F64(sample.totalBits));

  for (const row of sample.rows) {
    console.log(
      `${label} row:`,
      `storage=${row.storage}`,
      `netuid=${row.netuid}`,
      `locked_mass=${row.rolled.lockedMass}`,
      `conviction=${formatU64F64(row.rolled.convictionBits)}`,
      `conviction_tao_units=${formatTaoFromU64F64(row.rolled.convictionBits)}`,
      `last_update=${row.rolled.lastUpdate}`
    );
  }
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
  if (value?.toBigInt) {
    return value.toBigInt();
  }
  if (value?.toString && value.toString !== Object.prototype.toString) {
    return BigInt(value.toString().replaceAll(",", ""));
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

function assertLockMetadataAvailable() {
  const query = api.query.subtensorModule;
  const missing = [
    ["HotkeyLock", query.hotkeyLock],
    ["DecayingHotkeyLock", query.decayingHotkeyLock],
    ["OwnerLock", query.ownerLock],
    ["DecayingOwnerLock", query.decayingOwnerLock],
    ["SubnetOwnerHotkey", query.subnetOwnerHotkey],
    ["UnlockRate", query.unlockRate],
    ["MaturityRate", query.maturityRate],
  ].filter(([, storage]) => !storage);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after lock/conviction is deployed to testnet`
  );
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
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

function defaultEndpoint() {
  if (NETWORK === "mainnet") {
    assert.ok(
      process.env.ONFINALITY_API_KEY,
      "ONFINALITY_API_KEY is required for mainnet unless WS_ENDPOINT is set"
    );
    return `wss://bittensor-finney.api.onfinality.io/ws?apikey=${process.env.ONFINALITY_API_KEY}`;
  }

  return "wss://test.finney.opentensor.ai:443";
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});
