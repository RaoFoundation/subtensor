import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const TARGET_NETUIDS = [86, 76, 72] as const;
const RUN_ID = process.env.DISSOLUTION_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const NETWORK_LOCK_COST = BigInt(process.env.DISSOLUTION_NETWORK_LOCK_COST ?? "1000000000");
const OWNER_BALANCE = BigInt(process.env.DISSOLUTION_OWNER_BALANCE ?? "5000000000000");
const CLEANUP_TIMEOUT_MS = Number(process.env.DISSOLUTION_CLEANUP_TIMEOUT_MS ?? 45 * 60 * 1000);
const POLL_INTERVAL_MS = Number(process.env.DISSOLUTION_POLL_INTERVAL_MS ?? 1000);
const MAX_U64 = (1n << 64n) - 1n;

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri("//Alice");
const replacements = TARGET_NETUIDS.map((netuid) => ({
  netuid,
  owner: keyring.addFromUri(`//MainnetDissolution//${RUN_ID}//${netuid}//owner`),
  hotkey: keyring.addFromUri(`//MainnetDissolution//${RUN_ID}//${netuid}//hotkey`),
}));
const logger = createTempLogger("test-mainnet-subnet-dissolutions.log");
logger.captureConsole();

let api;
let originalSettings;
let settingsModified = false;
const completedTargets = new Set<number>();
const replacementRegisteredAt = new Map<number, bigint>();

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const [chain, runtimeVersion, startHeader] = await Promise.all([
      api.rpc.system.chain(),
      api.rpc.state.getRuntimeVersion(),
      api.rpc.chain.getHeader(),
    ]);
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), runtimeVersion.specVersion.toString());
    console.log("start block:", startHeader.number.toString());
    console.log("run id:", RUN_ID);
    console.log("targets:", TARGET_NETUIDS.join(","));

    assertMetadataAvailable();
    await assertAliceIsSudo();
    await assertCleanOrchestratorState("before test");

    originalSettings = await readRegistrationSettings();
    const activeCount = (await activeNonRootNetuids()).length;
    await configureRegistration(activeCount);
    await fundReplacementOwners();

    const initialPriority = await deregistrationPriority();
    console.log("initial deregistration priority:", formatPriority(initialPriority.slice(0, 10)));
    assert.deepEqual(
      initialPriority.slice(0, TARGET_NETUIDS.length).map(({ netuid }) => netuid),
      [...TARGET_NETUIDS],
      "fresh mainnet state does not have the expected top-three deregistration priority"
    );

    const results = [];
    for (const replacement of replacements) {
      const priority = await deregistrationPriority();
      console.log(`priority before netuid ${replacement.netuid}:`, formatPriority(priority.slice(0, 5)));
      assert.equal(priority[0]?.netuid, replacement.netuid, `netuid ${replacement.netuid} is not next to prune`);
      results.push(await pruneAndReplace(replacement));
    }

    await assertCleanOrchestratorState("after all dissolutions");
    console.log("DISSOLUTION_BLOCK_COUNTS", JSON.stringify(results));
    console.log("mainnet subnet dissolution test: ok");
  } finally {
    if (api && settingsModified && originalSettings) {
      try {
        await restoreRegistrationSettings(originalSettings);
      } catch (error) {
        await logger.error(`failed to restore registration settings: ${formatError(error)}`);
      }
    }
    await api?.disconnect();
    await logger.flush();
  }
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["Balances.forceSetBalance", api.tx.balances?.forceSetBalance],
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Utility.batchAll", api.tx.utility?.batchAll],
    ["SubtensorModule.registerNetwork", api.tx.subtensorModule?.registerNetwork],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.NetworkRegisteredAt", api.query.subtensorModule?.networkRegisteredAt],
    ["SubtensorModule.LastMechansimStepBlock", api.query.subtensorModule?.lastMechansimStepBlock],
    ["SubtensorModule.SubnetMovingPrice", api.query.subtensorModule?.subnetMovingPrice],
    ["SubtensorModule.SubnetMechanism", api.query.subtensorModule?.subnetMechanism],
    ["SubtensorModule.DissolveCleanupQueue", api.query.subtensorModule?.dissolveCleanupQueue],
    ["SubtensorModule.CurrentDissolveCleanupStatus", api.query.subtensorModule?.currentDissolveCleanupStatus],
    ["SubtensorModule.NetworkRegistrationQueue", api.query.subtensorModule?.networkRegistrationQueue],
    ["SubtensorModule.SubnetOwner", api.query.subtensorModule?.subnetOwner],
    ["SubtensorModule.SubnetOwnerHotkey", api.query.subtensorModule?.subnetOwnerHotkey],
    ["SubtensorModule.RegisteredSubnetCounter", api.query.subtensorModule?.registeredSubnetCounter],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.Uids", api.query.subtensorModule?.uids],
    ["SubtensorModule.IsNetworkMember", api.query.subtensorModule?.isNetworkMember],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.HotkeyLock", api.query.subtensorModule?.hotkeyLock],
    ["SubtensorModule.DecayingHotkeyLock", api.query.subtensorModule?.decayingHotkeyLock],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to the current runtime`
  );
}

async function assertAliceIsSudo() {
  const sudoKey = await api.query.sudo.key();
  assert.equal(sudoKey.toString(), alice.address, `Alice is not sudo; sudo key is ${sudoKey.toString()}`);
}

async function readRegistrationSettings() {
  const netuids = await activeNonRootNetuids();
  const [subnetLimit, networkRateLimit, registrationStartBlock, immunityPeriod, minLockCost, lastLockCost] =
    await Promise.all([
      api.query.subtensorModule.subnetLimit(),
      api.query.subtensorModule.networkRateLimit(),
      api.query.subtensorModule.networkRegistrationStartBlock(),
      api.query.subtensorModule.networkImmunityPeriod(),
      api.query.subtensorModule.networkMinLockCost(),
      api.query.subtensorModule.networkLastLockCost(),
    ]);
  const registeredAt = new Map<number, bigint>();
  for (const netuid of netuids) {
    registeredAt.set(netuid, (await api.query.subtensorModule.networkRegisteredAt(netuid)).toBigInt());
  }
  const mechanismStepEntries = await api.query.subtensorModule.lastMechansimStepBlock.entries();
  const snapshotBlock = mechanismStepEntries.reduce(
    (maximum, [, value]) => (value.toBigInt() > maximum ? value.toBigInt() : maximum),
    0n
  );
  assert.ok(snapshotBlock > 0n, "could not derive the imported mainnet snapshot block");
  return {
    subnetLimit: subnetLimit.toNumber(),
    networkRateLimit: networkRateLimit.toBigInt(),
    registrationStartBlock: registrationStartBlock.toBigInt(),
    immunityPeriod: immunityPeriod.toBigInt(),
    minLockCost: minLockCost.toBigInt(),
    lastLockCost: lastLockCost.toBigInt(),
    registeredAt,
    snapshotBlock,
  };
}

async function configureRegistration(activeCount) {
  let mature = 0;
  let immune = 0;
  const normalizedRegisteredAt = [...originalSettings.registeredAt].map(([netuid, registeredAt]) => {
    const age = originalSettings.snapshotBlock >= registeredAt ? originalSettings.snapshotBlock - registeredAt : 0n;
    const normalized = age >= originalSettings.immunityPeriod ? 0n : MAX_U64;
    if (normalized === 0n) mature += 1;
    else immune += 1;
    return [api.query.subtensorModule.networkRegisteredAt.key(netuid), storageValueHex("u64", normalized)];
  });
  await sudoSetStorage(
    [
      [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", activeCount)],
      [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", 0n)],
      [api.query.subtensorModule.networkRegistrationStartBlock.key(), storageValueHex("u64", 0n)],
      [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", 0n)],
      [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", NETWORK_LOCK_COST)],
      [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", NETWORK_LOCK_COST)],
      ...normalizedRegisteredAt,
    ],
    "configure priority-pruning registrations"
  );
  settingsModified = true;
  console.log(
    "registration configured:",
    `active=${activeCount}`,
    `limit=${activeCount}`,
    "immunity=0",
    `snapshotBlock=${originalSettings.snapshotBlock}`,
    `mature=${mature}`,
    `immune=${immune}`,
    `lock=${NETWORK_LOCK_COST}`
  );
}

async function restoreRegistrationSettings(settings) {
  const registeredAtEntries = [];
  for (const [netuid, registeredAt] of settings.registeredAt) {
    if (!(await api.query.subtensorModule.networksAdded(netuid)).isTrue) continue;
    const value = completedTargets.has(netuid) ? replacementRegisteredAt.get(netuid) : registeredAt;
    if (value === undefined) continue;
    registeredAtEntries.push([
      api.query.subtensorModule.networkRegisteredAt.key(netuid),
      storageValueHex("u64", value),
    ]);
  }
  await sudoSetStorage(
    [
      [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", settings.subnetLimit)],
      [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", settings.networkRateLimit)],
      [
        api.query.subtensorModule.networkRegistrationStartBlock.key(),
        storageValueHex("u64", settings.registrationStartBlock),
      ],
      [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", settings.immunityPeriod)],
      [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", settings.minLockCost)],
      [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", settings.lastLockCost)],
      ...registeredAtEntries,
    ],
    "restore mainnet registration settings"
  );
  settingsModified = false;
  console.log("registration settings restored");
}

async function fundReplacementOwners() {
  const calls = replacements.map(({ owner }) => api.tx.balances.forceSetBalance(owner.address, OWNER_BALANCE));
  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.utility.batchAll(calls)), "fund replacement owners");
  for (const { owner } of replacements) {
    const free = (await api.query.system.account(owner.address)).data.free.toBigInt();
    assert.equal(free, OWNER_BALANCE, `failed to fund ${owner.address}`);
  }
}

async function deregistrationPriority() {
  const currentBlock = (await api.rpc.chain.getHeader()).number.toBigInt();
  const immunity = (await api.query.subtensorModule.networkImmunityPeriod()).toBigInt();
  const netuids = await activeNonRootNetuids();
  const candidates = await Promise.all(
    netuids.map(async (netuid) => {
      const [registeredAtCodec, priceCodec, mechanismCodec] = await Promise.all([
        api.query.subtensorModule.networkRegisteredAt(netuid),
        api.query.subtensorModule.subnetMovingPrice(netuid),
        api.query.subtensorModule.subnetMechanism(netuid),
      ]);
      const registeredAt = registeredAtCodec.toBigInt();
      const rawPrice = priceCodec.bits.toBigInt();
      const mechanism = mechanismCodec.toNumber();
      const price = mechanism === 0 ? 1n << 32n : rawPrice < 0n ? 0n : rawPrice;
      return { netuid, registeredAt, rawPrice, price, eligible: currentBlock >= registeredAt + immunity };
    })
  );

  return candidates
    .filter(({ eligible }) => eligible)
    .sort((a, b) => compareBigInt(a.price, b.price) || compareBigInt(a.registeredAt, b.registeredAt));
}

async function pruneAndReplace({ netuid, owner, hotkey }) {
  const before = await captureSubnetState(netuid);
  assert.equal(before.networksAdded, true, `target subnet ${netuid} is not active`);
  assert.ok(before.hotkeys.length > 0, `target subnet ${netuid} has no registered hotkeys`);
  console.log(
    `netuid ${netuid} before:`,
    `owner=${before.owner}`,
    `hotkeys=${before.hotkeys.length}`,
    `n=${before.subnetworkN}`,
    `tao=${before.subnetTao}`,
    `alphaIn=${before.subnetAlphaIn}`,
    `alphaOut=${before.subnetAlphaOut}`,
    `counter=${before.registeredSubnetCounter}`
  );

  const result = await submitAndWait(owner, api.tx.subtensorModule.registerNetwork(hotkey.address), `prune netuid ${netuid}`);
  const removed = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkRemoved"
  );
  const queued = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkRegistrationQueued"
  );
  assert.ok(removed, `registerNetwork did not emit NetworkRemoved for ${netuid}`);
  assert.equal(removed.event.data[0].toNumber(), netuid, `registerNetwork pruned the wrong subnet`);
  assert.ok(queued, `replacement for ${netuid} was not queued behind cleanup`);

  const removalBlock = (await api.rpc.chain.getHeader(result.blockHash)).number.toNumber();
  console.log(`netuid ${netuid} removed: block=${removalBlock}, waiting for cleanup and reuse`);
  const completion = await waitForCleanupAndReplacement(netuid, owner.address, removalBlock);
  const after = await assertReplacementState(netuid, owner.address, hotkey.address, before);
  replacementRegisteredAt.set(netuid, after.registeredAt);
  completedTargets.add(netuid);
  await sudoSetStorage(
    [[api.query.subtensorModule.networkRegisteredAt.key(netuid), storageValueHex("u64", MAX_U64)]],
    `keep replacement netuid ${netuid} immune during test`
  );

  const blocksToComplete = completion.cleanupBlock - removalBlock;
  console.log(
    `netuid ${netuid} complete:`,
    `removedBlock=${removalBlock}`,
    `cleanupBlock=${completion.cleanupBlock}`,
    `addedBlock=${completion.addedBlock}`,
    `blocks=${blocksToComplete}`
  );
  return { netuid, removalBlock, cleanupBlock: completion.cleanupBlock, addedBlock: completion.addedBlock, blocksToComplete };
}

async function waitForCleanupAndReplacement(netuid, ownerAddress, removalBlock) {
  const deadline = Date.now() + CLEANUP_TIMEOUT_MS;
  let nextBlock = removalBlock;
  let cleanupBlock;
  let addedBlock;
  let lastProgress = "";

  while (Date.now() < deadline) {
    const latest = (await api.rpc.chain.getHeader()).number.toNumber();
    while (nextBlock <= latest) {
      const blockHash = await api.rpc.chain.getBlockHash(nextBlock);
      const events = await api.query.system.events.at(blockHash);
      for (const { event } of events) {
        if (
          event.section === "subtensorModule" &&
          event.method === "NetworkDissolveCleanupCompleted" &&
          event.data[0].toNumber() === netuid
        ) {
          cleanupBlock = nextBlock;
        }
        if (event.section === "subtensorModule" && event.method === "NetworkAdded" && event.data[0].toNumber() === netuid) {
          const owner = await api.query.subtensorModule.subnetOwner.at(blockHash, netuid);
          if (owner.toString() === ownerAddress) addedBlock = nextBlock;
        }
      }
      nextBlock += 1;
    }

    if (cleanupBlock !== undefined && addedBlock !== undefined) return { cleanupBlock, addedBlock };

    const status = await api.query.subtensorModule.currentDissolveCleanupStatus();
    const progress = status.isSome
      ? `netuid=${status.unwrap().netuid.toString()} phase=${status.unwrap().phase.toString()}`
      : "idle";
    if (progress !== lastProgress) {
      console.log(`netuid ${netuid} cleanup progress: block=${latest} ${progress}`);
      lastProgress = progress;
    }
    await sleep(POLL_INTERVAL_MS);
  }

  throw new Error(
    `netuid ${netuid} did not clean up and re-register within ${CLEANUP_TIMEOUT_MS}ms ` +
      `(cleanupBlock=${cleanupBlock}, addedBlock=${addedBlock})`
  );
}

async function captureSubnetState(netuid) {
  const [
    networksAdded,
    owner,
    ownerHotkey,
    subnetworkN,
    subnetTao,
    subnetAlphaIn,
    subnetAlphaOut,
    registeredSubnetCounter,
    registeredAt,
    keyEntries,
  ] = await Promise.all([
    api.query.subtensorModule.networksAdded(netuid),
    api.query.subtensorModule.subnetOwner(netuid),
    api.query.subtensorModule.subnetOwnerHotkey(netuid),
    api.query.subtensorModule.subnetworkN(netuid),
    api.query.subtensorModule.subnetTAO(netuid),
    api.query.subtensorModule.subnetAlphaIn(netuid),
    api.query.subtensorModule.subnetAlphaOut(netuid),
    api.query.subtensorModule.registeredSubnetCounter(netuid),
    api.query.subtensorModule.networkRegisteredAt(netuid),
    api.query.subtensorModule.keys.entries(netuid),
  ]);
  return {
    networksAdded: networksAdded.isTrue,
    owner: owner.toString(),
    ownerHotkey: ownerHotkey.toString(),
    subnetworkN: subnetworkN.toNumber(),
    subnetTao: subnetTao.toBigInt(),
    subnetAlphaIn: subnetAlphaIn.toBigInt(),
    subnetAlphaOut: subnetAlphaOut.toBigInt(),
    registeredSubnetCounter: registeredSubnetCounter.toBigInt(),
    registeredAt: registeredAt.toBigInt(),
    hotkeys: keyEntries.map(([, value]) => value.toString()),
  };
}

async function assertReplacementState(netuid, ownerAddress, hotkeyAddress, before) {
  const [
    after,
    uidEntries,
    cleanupQueue,
    cleanupStatus,
    registrationQueue,
    identity,
    emissionEnabled,
    volume,
    swapInitialized,
  ] = await Promise.all([
    captureSubnetState(netuid),
    api.query.subtensorModule.uids.entries(netuid),
    api.query.subtensorModule.dissolveCleanupQueue(),
    api.query.subtensorModule.currentDissolveCleanupStatus(),
    api.query.subtensorModule.networkRegistrationQueue(),
    api.query.subtensorModule.subnetIdentitiesV3(netuid),
    api.query.subtensorModule.subnetEmissionEnabled(netuid),
    api.query.subtensorModule.subnetVolume(netuid),
    api.query.swap.palSwapInitialized(netuid),
  ]);

  assert.equal(after.networksAdded, true, `reused netuid ${netuid} is not active`);
  assert.equal(after.owner, ownerAddress, `reused netuid ${netuid} has the wrong owner`);
  assert.equal(after.ownerHotkey, hotkeyAddress, `reused netuid ${netuid} has the wrong owner hotkey`);
  assert.equal(after.subnetworkN, 1, `reused netuid ${netuid} did not reset SubnetworkN`);
  assert.deepEqual(after.hotkeys, [hotkeyAddress], `reused netuid ${netuid} retained old Keys entries`);
  assert.equal(uidEntries.length, 1, `reused netuid ${netuid} retained old Uids entries`);
  assert.equal(uidEntries[0][0].args[1].toString(), hotkeyAddress, `reused netuid ${netuid} has an unexpected UID hotkey`);
  assert.equal(after.subnetAlphaOut, 0n, `reused netuid ${netuid} did not reset SubnetAlphaOut`);
  assert.equal(volume.toBigInt(), 0n, `reused netuid ${netuid} did not reset SubnetVolume`);
  assert.equal(emissionEnabled.isFalse, true, `reused netuid ${netuid} did not reset SubnetEmissionEnabled`);
  assert.equal(identity.isNone, true, `reused netuid ${netuid} retained its old subnet identity`);
  assert.equal(swapInitialized.isFalse, true, `reused netuid ${netuid} retained swap initialization state`);
  assert.equal(
    after.registeredSubnetCounter,
    before.registeredSubnetCounter + 1n,
    `reused netuid ${netuid} did not advance RegisteredSubnetCounter`
  );
  assert.ok(![...cleanupQueue].some((value: any) => value.toNumber() === netuid), `netuid ${netuid} remains in cleanup queue`);
  assert.equal(cleanupStatus.isNone, true, `cleanup status was not cleared after netuid ${netuid}`);
  assert.equal(registrationQueue.length, 0, `registration queue was not drained after netuid ${netuid}`);

  await assertOldHotkeyStateCleared(netuid, before.hotkeys);
  await assertRootWeightsCleared(netuid);
  console.log(`netuid ${netuid} reset checks: ok (${before.hotkeys.length} old hotkeys checked)`);
  return after;
}

async function assertOldHotkeyStateCleared(netuid, hotkeys) {
  for (let offset = 0; offset < hotkeys.length; offset += 40) {
    const batch = hotkeys.slice(offset, offset + 40);
    const results = await Promise.all(
      batch.map(async (hotkey) => {
        const [uid, member, alpha, lock, decayingLock] = await Promise.all([
          api.query.subtensorModule.uids(netuid, hotkey),
          api.query.subtensorModule.isNetworkMember(hotkey, netuid),
          api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid),
          api.query.subtensorModule.hotkeyLock(netuid, hotkey),
          api.query.subtensorModule.decayingHotkeyLock(netuid, hotkey),
        ]);
        return { hotkey, uid, member, alpha, lock, decayingLock };
      })
    );
    for (const result of results) {
      assert.equal(result.uid.isNone, true, `old hotkey ${result.hotkey} retained Uids(${netuid})`);
      assert.equal(result.member.isFalse, true, `old hotkey ${result.hotkey} retained IsNetworkMember(${netuid})`);
      assert.equal(result.alpha.toBigInt(), 0n, `old hotkey ${result.hotkey} retained TotalHotkeyAlpha(${netuid})`);
      assert.equal(result.lock.isNone, true, `old hotkey ${result.hotkey} retained HotkeyLock(${netuid})`);
      assert.equal(result.decayingLock.isNone, true, `old hotkey ${result.hotkey} retained DecayingHotkeyLock(${netuid})`);
    }
  }
}

async function assertRootWeightsCleared(netuid) {
  const rootWeights = await api.query.subtensorModule.weights.entries(0);
  for (const [key, value] of rootWeights) {
    const destinations = value.toJSON() as unknown[];
    assert.ok(
      !destinations.some((row: any) => Number(row[0]) === netuid),
      `root UID ${key.args[1].toString()} retained a weight for netuid ${netuid}`
    );
  }
}

async function assertCleanOrchestratorState(label) {
  const [cleanupQueue, cleanupStatus, registrationQueue] = await Promise.all([
    api.query.subtensorModule.dissolveCleanupQueue(),
    api.query.subtensorModule.currentDissolveCleanupStatus(),
    api.query.subtensorModule.networkRegistrationQueue(),
  ]);
  assert.equal(cleanupQueue.length, 0, `${label}: dissolve cleanup queue is not empty`);
  assert.equal(cleanupStatus.isNone, true, `${label}: current dissolve cleanup status is set`);
  assert.equal(registrationQueue.length, 0, `${label}: network registration queue is not empty`);
}

async function activeNonRootNetuids() {
  const entries = await api.query.subtensorModule.networksAdded.entries();
  return entries
    .filter(([key, value]) => value.isTrue && key.args[0].toNumber() !== 0)
    .map(([key]) => key.args[0].toNumber())
    .sort((a, b) => a - b);
}

async function sudoSetStorage(entries, label) {
  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), label);
}

async function submitAndWait(signer, txOrPromise, label) {
  const tx = await txOrPromise;
  console.log("submit:", label);
  return new Promise<any>((resolve, reject) => {
    let unsubscribe;
    let settled = false;
    const timeout = setTimeout(() => finish(reject, new Error(`${label} timed out after 180000ms`)), 180_000);
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
            finish(reject, new Error(`${label} failed: ${formatDispatchError(event.data[0])}`));
            return;
          }
        }
      }
      if (status.isFinalized) finish(resolve, { blockHash: status.asFinalized.toString(), events });
    })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
}

function formatDispatchError(dispatchError) {
  if (dispatchError.isModule) {
    const decoded = api.registry.findMetaError(dispatchError.asModule);
    return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
  }
  return dispatchError.toString();
}

function storageValueHex(type, value) {
  return u8aToHex(api.createType(type, value).toU8a());
}

function compareBigInt(a: bigint, b: bigint) {
  return a < b ? -1 : a > b ? 1 : 0;
}

function formatPriority(rows) {
  return rows.map(({ netuid, rawPrice, registeredAt }) => `${netuid}(price=${rawPrice},at=${registeredAt})`).join(" ");
}

function formatError(error) {
  return error instanceof Error ? error.stack ?? error.message : String(error);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
