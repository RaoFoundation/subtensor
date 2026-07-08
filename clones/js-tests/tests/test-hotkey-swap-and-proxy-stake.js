import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const RUN_ID = process.env.HOTKEY_PROXY_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const FUND_SOURCE_URI = process.env.HOTKEY_PROXY_FUND_SOURCE_URI ?? "//Alice";
const FUND_AMOUNT = BigInt(process.env.HOTKEY_PROXY_FUND_AMOUNT ?? "5000000000000");
const REGISTRATION_BURN = BigInt(process.env.HOTKEY_PROXY_REGISTRATION_BURN ?? "1000000");
const STAKE_AMOUNT = BigInt(process.env.HOTKEY_PROXY_STAKE_AMOUNT ?? "10000000000");
const MAX_REGISTRATIONS_PER_BLOCK = 10_000;
const PROXY_TYPE_STAKING = "Staking";

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const swapColdkey = keyring.addFromUri(`//HotkeyProxy//${RUN_ID}//swap-coldkey`);
const oldHotkey = keyring.addFromUri(`//HotkeyProxy//${RUN_ID}//old-hotkey`);
const newHotkey = keyring.addFromUri(`//HotkeyProxy//${RUN_ID}//new-hotkey`);
const proxyColdkey = keyring.addFromUri(`//HotkeyProxy//${RUN_ID}//proxy-coldkey`);
const proxyDelegate = keyring.addFromUri(`//HotkeyProxy//${RUN_ID}//proxy-delegate`);
const logger = createTempLogger("test-hotkey-swap-and-proxy-stake.log");
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

    const { netuid, hotkey: existingHotkey } = await findTransferEnabledSubnet();
    console.log("working subnet:", netuid);
    console.log("existing subnet hotkey:", existingHotkey);

    await fundTestAccounts();
    await prepareRegistration(netuid);
    await exerciseHotkeySwap(netuid);
    await exerciseProxyStake(netuid);

    console.log("hotkey swap and proxy staking scenarios: ok");
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
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Utility.batch", api.tx.utility?.batch],
    ["Proxy.addProxy", api.tx.proxy?.addProxy],
    ["Proxy.proxy", api.tx.proxy?.proxy],
    ["SubtensorModule.burnedRegister", api.tx.subtensorModule?.burnedRegister],
    ["SubtensorModule.swapHotkeyV2", api.tx.subtensorModule?.swapHotkeyV2],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.Uids", api.query.subtensorModule?.uids],
    ["SubtensorModule.Owner", api.query.subtensorModule?.owner],
    ["SubtensorModule.IsNetworkMember", api.query.subtensorModule?.isNetworkMember],
    ["SubtensorModule.Burn", api.query.subtensorModule?.burn],
    ["SubtensorModule.MinBurn", api.query.subtensorModule?.minBurn],
    ["SubtensorModule.MaxBurn", api.query.subtensorModule?.maxBurn],
    ["SubtensorModule.NetworkRegistrationAllowed", api.query.subtensorModule?.networkRegistrationAllowed],
    ["SubtensorModule.MaxRegistrationsPerBlock", api.query.subtensorModule?.maxRegistrationsPerBlock],
    ["SubtensorModule.RegistrationsThisBlock", api.query.subtensorModule?.registrationsThisBlock],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run against the current runtime`
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

async function fundTestAccounts() {
  const calls = [swapColdkey, proxyColdkey, proxyDelegate].map((account) =>
    balancesTransfer(account.address, FUND_AMOUNT)
  );
  await signedBatch(fundSource, calls, "fund hotkey/proxy test accounts");

  for (const [account, label] of [
    [swapColdkey, "swap coldkey"],
    [proxyColdkey, "proxy coldkey"],
    [proxyDelegate, "proxy delegate"],
  ]) {
    const free = (await api.query.system.account(account.address)).data.free.toBigInt();
    assert.ok(free >= FUND_AMOUNT, `${label} funding failed: free=${free}`);
    console.log(`${label} funded:`, account.address, free.toString());
  }
}

async function prepareRegistration(netuid) {
  await sudoSetStorage(
    [
      [api.query.subtensorModule.minBurn.key(netuid), storageValueHex("u64", REGISTRATION_BURN)],
      [api.query.subtensorModule.maxBurn.key(netuid), storageValueHex("u64", REGISTRATION_BURN)],
      [api.query.subtensorModule.burn.key(netuid), storageValueHex("u64", REGISTRATION_BURN)],
      [api.query.subtensorModule.networkRegistrationAllowed.key(netuid), storageValueHex("bool", true)],
      [api.query.subtensorModule.maxRegistrationsPerBlock.key(netuid), storageValueHex("u16", MAX_REGISTRATIONS_PER_BLOCK)],
      [api.query.subtensorModule.registrationsThisBlock.key(netuid), storageValueHex("u16", 0)],
    ],
    `prepare registration settings for netuid ${netuid}`
  );

  const burn = (await api.query.subtensorModule.burn(netuid)).toBigInt();
  assert.equal(burn, REGISTRATION_BURN, `Burn(${netuid}) was not updated`);
  console.log("registration settings prepared:", `netuid=${netuid}`, `burn=${burn}`);
}

async function exerciseHotkeySwap(netuid) {
  await submitAndWait(
    swapColdkey,
    api.tx.subtensorModule.burnedRegister(netuid, oldHotkey.address),
    "register old hotkey"
  );
  const oldUid = await requireUid(netuid, oldHotkey.address, "old hotkey after registration");
  assert.equal((await api.query.subtensorModule.owner(oldHotkey.address)).toString(), swapColdkey.address);
  console.log("old hotkey registered:", `uid=${oldUid}`, `hotkey=${oldHotkey.address}`);

  const result = await submitAndWait(
    swapColdkey,
    api.tx.subtensorModule.swapHotkeyV2(oldHotkey.address, newHotkey.address, null, false),
    "swap hotkey v2"
  );
  assertEvent(result.events, "subtensorModule", "HotkeySwapped");

  const oldMembership = await api.query.subtensorModule.isNetworkMember(oldHotkey.address, netuid);
  const newMembership = await api.query.subtensorModule.isNetworkMember(newHotkey.address, netuid);
  assert.equal(oldMembership.isFalse, true, "old hotkey should no longer be a subnet member");
  assert.equal(newMembership.isTrue, true, "new hotkey should be a subnet member");

  const maybeOldUid = await api.query.subtensorModule.uids(netuid, oldHotkey.address);
  assert.equal(maybeOldUid.isNone, true, "old hotkey uid should be removed after swap");
  const newUid = await requireUid(netuid, newHotkey.address, "new hotkey after swap");
  assert.equal(newUid, oldUid, "new hotkey should keep the old subnet uid");
  assert.equal((await api.query.subtensorModule.keys(netuid, newUid)).toString(), newHotkey.address);
  assert.equal((await api.query.subtensorModule.owner(newHotkey.address)).toString(), swapColdkey.address);

  console.log(
    "hotkey swap v2 scenario:",
    `netuid=${netuid}`,
    `uid=${newUid}`,
    `old=${oldHotkey.address}`,
    `new=${newHotkey.address}`
  );
}

async function exerciseProxyStake(netuid) {
  const alphaBefore = (await api.query.subtensorModule.totalHotkeyAlpha(newHotkey.address, netuid)).toBigInt();

  const addProxyResult = await submitAndWait(
    proxyColdkey,
    api.tx.proxy.addProxy(proxyDelegate.address, PROXY_TYPE_STAKING, 0),
    "add staking proxy"
  );
  assertEvent(addProxyResult.events, "proxy", "ProxyAdded");
  console.log(
    "staking proxy added:",
    `real=${proxyColdkey.address}`,
    `delegate=${proxyDelegate.address}`,
    `type=${PROXY_TYPE_STAKING}`
  );

  const call = api.tx.subtensorModule.addStake(newHotkey.address, netuid, STAKE_AMOUNT);
  const proxyResult = await submitAndWait(
    proxyDelegate,
    api.tx.proxy.proxy(proxyColdkey.address, PROXY_TYPE_STAKING, call),
    "stake through proxy"
  );
  assertProxyExecutedOk(proxyResult.events, "stake through proxy");
  const alphaAdded = assertStakeAddedEvent(proxyResult.events, proxyColdkey.address, newHotkey.address, netuid);
  const alphaAfter = (await api.query.subtensorModule.totalHotkeyAlpha(newHotkey.address, netuid)).toBigInt();
  assert.ok(alphaAfter > alphaBefore, `total hotkey alpha did not increase: before=${alphaBefore} after=${alphaAfter}`);

  console.log(
    "proxy stake scenario:",
    `netuid=${netuid}`,
    `hotkey=${newHotkey.address}`,
    `stake_rao=${STAKE_AMOUNT}`,
    `alpha_added=${alphaAdded}`,
    `alpha_before=${alphaBefore}`,
    `alpha_after=${alphaAfter}`
  );
}

async function findTransferEnabledSubnet() {
  const networkEntries = await api.query.subtensorModule.networksAdded.entries();
  for (const [key, added] of networkEntries) {
    if (!added.isTrue) continue;
    const netuid = key.args[0].toNumber();
    if (netuid === 0) continue;
    if ((await api.query.subtensorModule.transferToggle(netuid)).isFalse) continue;
    const keys = await api.query.subtensorModule.keys.entries(netuid);
    const hotkey = keys.find(([, value]) => value.toString())?.[1]?.toString();
    if (hotkey) {
      return { netuid, hotkey };
    }
  }
  throw new Error("no initialized transfer-enabled subnet with at least one hotkey found");
}

async function requireUid(netuid, hotkey, label) {
  const maybeUid = await api.query.subtensorModule.uids(netuid, hotkey);
  assert.ok(maybeUid.isSome, `${label}: expected uid for ${hotkey} on netuid ${netuid}`);
  return maybeUid.unwrap().toNumber();
}

async function signedBatch(signer, calls, label) {
  const batched = api.tx.utility.batchAll ? api.tx.utility.batchAll(calls) : api.tx.utility.batch(calls);
  await submitAndWait(signer, batched, label);
}

async function sudoSetStorage(entries, label) {
  await submitAndWait(fundSource, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), label);
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

function assertEvent(events, section, method) {
  const event = events.find(({ event }) => event.section === section && event.method === method);
  assert.ok(event, `${section}.${method} event not found`);
  return event;
}

function assertProxyExecutedOk(events, label) {
  const event = assertEvent(events, "proxy", "ProxyExecuted");
  const result = event.event.data.result ?? event.event.data[0];
  assert.ok(result.isOk, `${label}: ProxyExecuted result was not Ok: ${result.toString()}`);
}

function assertStakeAddedEvent(events, coldkey, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeAdded") return false;
    const [eventColdkey, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return (
      eventColdkey.toString() === coldkey &&
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      alphaStaked.toBigInt() > 0n
    );
  });
  assert.ok(event, `StakeAdded event not found for coldkey ${coldkey}, hotkey ${hotkey}, netuid ${netuid}`);
  return event.event.data[3].toBigInt();
}

function storageValueHex(type, value) {
  return u8aToHex(api.createType(type, value).toU8a());
}

function formatDispatchError(error) {
  if (!error.isModule) {
    return error.toString();
  }
  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
