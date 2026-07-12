import assert from "node:assert/strict";
import fs from "node:fs";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

loadDotenv();

const NETWORK = "mainnet";
const HOTKEY = parseHotkey();
const WS_ENDPOINT = process.env.WS_ENDPOINT ?? defaultEndpoint();
const MIN_FINALIZED_BLOCK_DELTA = Number(process.env.CONVICTION_WAIT_FINALIZED_BLOCKS ?? 3);
const logger = createTempLogger("mainnet-hotkey-conviction-increases.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: (message) => console.log(redactEndpoint(message)) });

  try {
    assertMetadataAvailable();

    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const first = await finalizedPoint();
    const netuids = await liveNetuidsAt(first.hash);
    const firstRows = await readHotkeyConvictions(netuids);
    const firstPositiveRows = firstRows.filter((row) => row.convictionBits > 0n);

    console.log("network:", NETWORK);
    console.log("endpoint:", redactEndpoint(WS_ENDPOINT));
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("hotkey:", HOTKEY);
    console.log("first finalized block:", first.number.toString());
    console.log("first finalized hash:", first.hash.toString());
    console.log("live netuids checked:", netuids.join(", "));

    for (const row of firstPositiveRows) {
      console.log("current conviction:", formatConvictionRow(row));
    }
    if (firstPositiveRows.length === 0) {
      console.log("current conviction: zero on every live netuid checked");
    }

    assert.ok(
      firstPositiveRows.length > 0,
      `expected ${HOTKEY} to have positive conviction on at least one live mainnet netuid`
    );

    const later = await waitForFinalizedBlock(first.number + BigInt(MIN_FINALIZED_BLOCK_DELTA));
    const laterRows = await readHotkeyConvictions(firstPositiveRows.map((row) => row.netuid));
    const laterByNetuid = new Map(laterRows.map((row) => [row.netuid, row]));
    const increases = [];

    console.log("later finalized block:", later.number.toString());
    console.log("later finalized hash:", later.hash.toString());

    for (const firstRow of firstPositiveRows) {
      const laterRow = laterByNetuid.get(firstRow.netuid);
      assert.ok(laterRow, `missing later conviction row for netuid ${firstRow.netuid}`);
      const deltaBits = laterRow.convictionBits - firstRow.convictionBits;
      const increased = deltaBits > 0n;
      if (increased) {
        increases.push({ netuid: firstRow.netuid, deltaBits });
      }
      console.log(
        "conviction comparison:",
        `netuid=${firstRow.netuid}`,
        `first_bits=${firstRow.convictionBits}`,
        `later_bits=${laterRow.convictionBits}`,
        `delta_bits=${deltaBits}`,
        `first=${firstRow.conviction}`,
        `later=${laterRow.conviction}`
      );
    }

    assert.ok(
      increases.length > 0,
      `expected ${HOTKEY} conviction to increase across ${MIN_FINALIZED_BLOCK_DELTA} finalized blocks`
    );

    console.log(
      "mainnet hotkey conviction increase: ok",
      `increased_netuids=${increases.map((increase) => increase.netuid).join(",")}`
    );
  } finally {
    await api?.disconnect();
  }
}

function assertMetadataAvailable() {
  const missing = [
    ["StakeInfoRuntimeApi.getHotkeyConviction", api.call.stakeInfoRuntimeApi?.getHotkeyConviction],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `${missing.map(([name]) => name).join(", ")} unavailable on mainnet`);
}

async function finalizedPoint() {
  const hash = await api.rpc.chain.getFinalizedHead();
  const header = await api.rpc.chain.getHeader(hash);
  return {
    hash,
    number: header.number.toBigInt(),
  };
}

async function waitForFinalizedBlock(targetBlock) {
  const timeoutAt = Date.now() + Number(process.env.CONVICTION_WAIT_TIMEOUT_MS ?? 180_000);

  while (Date.now() < timeoutAt) {
    const point = await finalizedPoint();
    if (point.number >= targetBlock) {
      return point;
    }
    await sleep(6_000);
  }

  throw new Error(`timed out waiting for finalized block ${targetBlock}`);
}

async function liveNetuidsAt(blockHash) {
  const entries = await api.query.subtensorModule.networksAdded.entriesAt(blockHash);
  return entries
    .filter(([, value]) => value.isTrue)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);
}

async function readHotkeyConvictions(netuids) {
  const rows = [];
  for (const netuid of netuids) {
    const conviction = await api.call.stakeInfoRuntimeApi.getHotkeyConviction(HOTKEY, netuid);
    rows.push({
      netuid,
      conviction,
      convictionBits: decodeFixedBits(conviction),
    });
  }
  return rows;
}

function decodeFixedBits(value) {
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
  if (json !== undefined) {
    return parseBigIntish(json);
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
  if (value?.toString) {
    return BigInt(value.toString().replaceAll(",", ""));
  }
  throw new Error(`could not decode bigint value from ${value}`);
}

function formatConvictionRow(row) {
  return `netuid=${row.netuid} conviction=${row.conviction} conviction_bits=${row.convictionBits}`;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function defaultEndpoint() {
  assert.ok(
    process.env.ONFINALITY_API_KEY,
    "ONFINALITY_API_KEY is required for mainnet unless WS_ENDPOINT is set"
  );
  return `wss://bittensor-finney.api.onfinality.io/ws?apikey=${process.env.ONFINALITY_API_KEY}`;
}

function parseHotkey() {
  const hotkeyArgIndex = process.argv.findIndex((arg) => arg === "--hotkey");
  const hotkey =
    process.argv.find((arg) => arg.startsWith("--hotkey="))?.slice("--hotkey=".length) ??
    (hotkeyArgIndex >= 0 ? process.argv[hotkeyArgIndex + 1] : undefined) ??
    process.env.HOTKEY ??
    "5Fc3ZZQAYB3SPXKcFnd1WJeyQvArSZZeB6LU1rb7zvQ6XvDh";

  assert.ok(hotkey, "HOTKEY or --hotkey is required");
  return hotkey;
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
