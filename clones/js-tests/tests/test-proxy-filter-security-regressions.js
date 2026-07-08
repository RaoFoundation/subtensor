import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const RUN_ID = process.env.PROXY_FILTER_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const FUND_SOURCE_URI = process.env.PROXY_FILTER_FUND_SOURCE_URI ?? "//Alice";
const FUND_AMOUNT = BigInt(process.env.PROXY_FILTER_FUND_AMOUNT ?? "5000000000000");
const ZERO_HASH = `0x${"00".repeat(32)}`;

const PROXY_TYPES = ["NonFungible", "SwapHotkey", "NonTransfer", "Owner"];

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const real = keyring.addFromUri(`//ProxyFilterSecurity//${RUN_ID}//real`);
const delegate = keyring.addFromUri(`//ProxyFilterSecurity//${RUN_ID}//delegate`);
const dummyHotkey = keyring.addFromUri(`//ProxyFilterSecurity//${RUN_ID}//dummy-hotkey`);
const replacementHotkey = keyring.addFromUri(`//ProxyFilterSecurity//${RUN_ID}//replacement-hotkey`);
const logger = createTempLogger("test-proxy-filter-security-regressions.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    await assertMetadataAvailable();
    await waitForBlockProduction();

    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const startHeader = await api.rpc.chain.getHeader();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("start block:", startHeader.number.toString());
    console.log("run id:", RUN_ID);
    console.log("real:", real.address);
    console.log("delegate:", delegate.address);

    await fundProxyAccounts();
    await addProxyRelationships();

    await expectProxyTypeDenied(
      "NonFungible denies swapHotkeyV2",
      "NonFungible",
      api.tx.subtensorModule.swapHotkeyV2(dummyHotkey.address, replacementHotkey.address, null, false)
    );

    await expectProxyTypeAllowed(
      "SwapHotkey allows swapHotkeyV2",
      "SwapHotkey",
      api.tx.subtensorModule.swapHotkeyV2(dummyHotkey.address, replacementHotkey.address, null, false)
    );

    await expectProxyTypeDenied(
      "NonTransfer denies announceColdkeySwap",
      "NonTransfer",
      api.tx.subtensorModule.announceColdkeySwap(ZERO_HASH)
    );

    await expectProxyTypeDenied(
      "NonFungible denies announceColdkeySwap",
      "NonFungible",
      api.tx.subtensorModule.announceColdkeySwap(ZERO_HASH)
    );

    await expectProxyTypeDenied(
      "Owner denies sudoSetSubnetOwnerHotkey",
      "Owner",
      api.tx.adminUtils.sudoSetSubnetOwnerHotkey(0, replacementHotkey.address)
    );

    console.log("proxy filter security regressions: ok");
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

async function assertMetadataAvailable() {
  const missing = [
    ["Balances.transfer", api.tx.balances?.transferKeepAlive ?? api.tx.balances?.transferAllowDeath ?? api.tx.balances?.transfer],
    ["Utility.batch", api.tx.utility?.batch],
    ["Proxy.addProxy", api.tx.proxy?.addProxy],
    ["Proxy.proxy", api.tx.proxy?.proxy],
    ["SubtensorModule.swapHotkeyV2", api.tx.subtensorModule?.swapHotkeyV2],
    ["SubtensorModule.announceColdkeySwap", api.tx.subtensorModule?.announceColdkeySwap],
    ["AdminUtils.sudoSetSubnetOwnerHotkey", api.tx.adminUtils?.sudoSetSubnetOwnerHotkey],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run against a runtime with these calls in metadata`
  );
}

async function waitForBlockProduction() {
  const observed = [];
  let previous = (await api.rpc.chain.getHeader()).number.toBigInt();
  const deadline = Date.now() + 180_000;

  while (Date.now() < deadline && observed.length < 2) {
    await sleep(6_000);
    const current = (await api.rpc.chain.getHeader()).number.toBigInt();
    if (current > previous) {
      observed.push(current);
      previous = current;
      console.log("observed produced block:", current.toString());
    }
  }

  assert.equal(observed.length, 2, `block production did not advance twice; observed ${observed.length} advances`);
}

async function fundProxyAccounts() {
  await signedBatch(
    fundSource,
    [balancesTransfer(real.address, FUND_AMOUNT), balancesTransfer(delegate.address, FUND_AMOUNT)],
    "fund proxy filter test accounts"
  );

  for (const [account, label] of [
    [real, "real"],
    [delegate, "delegate"],
  ]) {
    const free = (await api.query.system.account(account.address)).data.free.toBigInt();
    assert.ok(free >= FUND_AMOUNT, `${label} funding failed: free=${free}`);
    console.log(`${label} funded:`, account.address, free.toString());
  }
}

async function addProxyRelationships() {
  const calls = PROXY_TYPES.map((proxyType) => api.tx.proxy.addProxy(delegate.address, proxyType, 0));
  const result = await signedBatch(real, calls, "add proxy filter relationships");
  for (const proxyType of PROXY_TYPES) {
    assertEvent(result.events, "proxy", "ProxyAdded", (event) => event.data[2].toString() === proxyType);
    console.log("proxy added:", `type=${proxyType}`);
  }
}

async function expectProxyTypeDenied(label, proxyType, call) {
  const result = await proxyCall(proxyType, call, label);
  assert.equal(
    result.error,
    "system.CallFiltered",
    `${label}: expected system.CallFiltered, got ${result.error ?? "Ok"}`
  );
  console.log(`${label}: denied by proxy filter`);
}

async function expectProxyTypeAllowed(label, proxyType, call) {
  const result = await proxyCall(proxyType, call, label);
  assert.notEqual(result.error, "system.CallFiltered", `${label}: unexpectedly denied by proxy filter`);
  console.log(`${label}: passed proxy filter with inner result ${result.error ?? "Ok"}`);
}

async function proxyCall(proxyType, call, label) {
  const result = await submitAndWait(
    delegate,
    api.tx.proxy.proxy(real.address, proxyType, call),
    `proxy ${label}`
  );
  const event = assertEvent(result.events, "proxy", "ProxyExecuted");
  const dispatchResult = event.event.data.result ?? event.event.data[0];
  if (dispatchResult.isOk) {
    return { error: null };
  }
  return { error: formatDispatchError(dispatchResult.asErr) };
}

async function signedBatch(signer, calls, label) {
  const batched = api.tx.utility.batchAll ? api.tx.utility.batchAll(calls) : api.tx.utility.batch(calls);
  return submitAndWait(signer, batched, label);
}

async function submitAndWait(signer, tx, label) {
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

function balancesTransfer(dest, amount) {
  if (api.tx.balances.transferKeepAlive) {
    return api.tx.balances.transferKeepAlive(dest, amount);
  }
  if (api.tx.balances.transferAllowDeath) {
    return api.tx.balances.transferAllowDeath(dest, amount);
  }
  return api.tx.balances.transfer(dest, amount);
}

function assertEvent(events, section, method, predicate = () => true) {
  const event = events.find((record) => {
    return record.event.section === section && record.event.method === method && predicate(record.event);
  });
  assert.ok(event, `${section}.${method} event not found`);
  return event;
}

function formatDispatchError(error) {
  if (!error.isModule) {
    return error.toString();
  }
  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}`;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
