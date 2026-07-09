import assert from "node:assert/strict";
import fs from "node:fs";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

loadDotenv();

const NETWORK = parseNetwork();
const WS_ENDPOINT = process.env.WS_ENDPOINT ?? defaultEndpoint(NETWORK);
const logger = createTempLogger(`subnet-account-balances-${NETWORK}.log`);
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const finalizedHash = await api.rpc.chain.getFinalizedHead();
    const header = await api.rpc.chain.getHeader(finalizedHash);

    assert.ok(
      api.query.subtensorModule?.subnetTAO,
      "SubtensorModule.SubnetTAO storage is unavailable on this endpoint"
    );

    const docsSubnetAccounts = readDocsSubnetAccounts();
    const entries = await api.query.subtensorModule.subnetTAO.entriesAt(finalizedHash);

    assert.ok(entries.length > 0, "expected at least one SubnetTAO entry");

    console.log("network:", NETWORK);
    console.log("endpoint:", redactEndpoint(WS_ENDPOINT));
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("block:", header.number.toString());
    console.log("block hash:", finalizedHash.toString());
    console.log("SubnetTAO entries:", entries.length);

    const failures = [];

    for (const [key, value] of entries) {
      const netuid = key.args[0].toNumber();
      const subnetTao = value.toBigInt();
      const subnetAccount = await getSubnetAccount(netuid, docsSubnetAccounts, finalizedHash);
      const account = await api.query.system.account.at(finalizedHash, subnetAccount);
      const free = account.data.free.toBigInt();
      const reserved = account.data.reserved.toBigInt();
      const ok = free >= subnetTao;

      console.log(
        `netuid=${netuid}`,
        `account=${subnetAccount}`,
        `free=${free}`,
        `reserved=${reserved}`,
        `subnetTAO=${subnetTao}`,
        `ok=${ok}`
      );

      if (!ok) {
        failures.push({ netuid, subnetAccount, free, subnetTao, deficit: subnetTao - free });
      }
    }

    if (failures.length > 0) {
      for (const failure of failures) {
        console.log(
          "failure:",
          `netuid=${failure.netuid}`,
          `account=${failure.subnetAccount}`,
          `free=${failure.free}`,
          `subnetTAO=${failure.subnetTao}`,
          `deficit=${failure.deficit}`
        );
      }
    }

    assert.equal(
      failures.length,
      0,
      `expected every subnet account free balance to be >= SubnetTAO; failures=${failures.length}`
    );

    console.log(`subnet account balance check: ok (${entries.length} subnets)`);
  } finally {
    await api?.disconnect();
  }
}

async function getSubnetAccount(netuid, docsSubnetAccounts, blockHash) {
  const rpcAccount = await getSubnetAccountFromRpc(netuid, blockHash);
  if (rpcAccount) {
    return rpcAccount;
  }

  const docsAccount = docsSubnetAccounts.get(netuid);
  assert.ok(
    docsAccount,
    `subnet account id unavailable for netuid=${netuid}; RPC missing and docs mapping has no entry`
  );
  return docsAccount;
}

async function getSubnetAccountFromRpc(netuid, blockHash) {
  const method = api.rpc.subnetInfo?.getSubnetAccountId;
  if (!method) {
    return undefined;
  }

  try {
    const encoded = await method(netuid, blockHash);
    const maybeAccount = api.createType("Option<AccountId32>", encoded);
    return maybeAccount.isSome ? maybeAccount.unwrap().toString() : undefined;
  } catch (err) {
    console.log(`subnetInfo_getSubnetAccountId unavailable for netuid=${netuid}:`, err.message);
    return undefined;
  }
}

function readDocsSubnetAccounts() {
  const docsUrl = new URL("../fixtures/special-account-ids.md", import.meta.url);
  const content = fs.readFileSync(docsUrl, "utf8");
  const accounts = new Map();

  for (const line of content.split("\n")) {
    const match = line.match(/^(\d+):\s+(\S+)$/);
    if (match) {
      accounts.set(Number(match[1]), match[2]);
    }
  }

  return accounts;
}

function parseNetwork() {
  const networkArgIndex = process.argv.findIndex((arg) => arg === "--network");
  const networkArg =
    process.argv.find((arg) => arg.startsWith("--network="))?.slice("--network=".length) ??
    (networkArgIndex >= 0 ? process.argv[networkArgIndex + 1] : undefined);
  const network = (networkArg ?? process.env.NETWORK ?? "testnet").toLowerCase();

  assert.ok(["testnet", "mainnet"].includes(network), `unsupported network: ${network}`);
  return network;
}

function defaultEndpoint(network) {
  if (network === "testnet") {
    return "wss://test.finney.opentensor.ai:443";
  }

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
