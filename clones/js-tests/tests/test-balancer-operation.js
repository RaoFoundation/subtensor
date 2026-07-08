import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const TRANSFER_AMOUNT = 1_000_000_000n;
const STAKE_AMOUNT = 1_000_000_000n;
const LIMIT_STAKE_AMOUNT = 1_000_000_000n;
const MAX_PRICE = 18_446_744_073_709_551_615n;
const MIN_PRICE = 0n;
const HALF_PERQUINTILL = 500_000_000_000_000_000n;
const MIN_BALANCER_WEIGHT = 450_000_000_000_000_000n;
const MAX_BALANCER_WEIGHT = 550_000_000_000_000_000n;
const MAX_BLOCKS_TO_WAIT = Number(process.env.MAX_EPOCH_WAIT_BLOCKS ?? 360);
const TRANSFER_SOURCE_URI = process.env.TRANSFER_SOURCE_URI ?? "//Alice";
const TRANSFER_DEST_URI = process.env.TRANSFER_DEST_URI ?? "//Bob";

const keyring = new Keyring({ type: "sr25519" });
const transferSource = keyring.addFromUri(TRANSFER_SOURCE_URI);
const transferDest = keyring.addFromUri(TRANSFER_DEST_URI);
const logger = createTempLogger("test-balancer-operation.log");
logger.captureConsole();

let api;
let lastFinalizedBlockNumber = null;

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

  const failures = [];
  await check("balance transfer", assertTransferWorks, failures);

  if (await check("balancer storage availability", assertBalancerStorageAvailable, failures)) {
    const balancerSummary = await assertBalancerWeights();
    const epochSummary = await assertEpochUpdatesReserves(balancerSummary.sampleNetuid);
    const injectionSummary = await assertProtocolLiquidityInjected(epochSummary.netuid);
    await withTemporaryTempo(injectionSummary.netuid, 1, () => assertValidatorsReceiveRewards(injectionSummary.netuid));
    await assertStakingSuiteWorks(injectionSummary.netuid);
  }

  assert.equal(failures.length, 0, `balancer operation test failed:\n${failures.join("\n")}`);
  } finally {
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

async function check(label, fn, failures) {
  try {
    await fn();
    console.log(`${label}: ok`);
    return true;
  } catch (error) {
    failures.push(`${label}: ${error.message}`);
    console.error(`${label}: ${error.message}`);
    return false;
  }
}

function assertBalancerStorageAvailable() {
  const missing = [
    ["Swap.SwapBalancer", api.query.swap?.swapBalancer],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
    ["Swap.FeeRate", api.query.swap?.feeRate],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
    ["SubtensorModule.SubnetAlphaInEmission", api.query.subtensorModule?.subnetAlphaInEmission],
    ["SubtensorModule.SubnetTaoInEmission", api.query.subtensorModule?.subnetTaoInEmission],
    ["SubtensorModule.SubnetAlphaOutEmission", api.query.subtensorModule?.subnetAlphaOutEmission],
    ["SubtensorModule.Emission", api.query.subtensorModule?.emission],
    ["SubtensorModule.Dividends", api.query.subtensorModule?.dividends],
    ["SubtensorModule.ValidatorPermit", api.query.subtensorModule?.validatorPermit],
    ["SubtensorModule.LastUpdate", api.query.subtensorModule?.lastUpdate],
    ["SubtensorModule.Tempo", api.query.subtensorModule?.tempo],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.addStakeLimit", api.tx.subtensorModule?.addStakeLimit],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.transferStake", api.tx.subtensorModule?.transferStake],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.TransferToggle", api.query.subtensorModule?.transferToggle],
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
  ].filter(([, query]) => !query);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run this test after upgrading the clone to the current subtensor-reference runtime`
  );
}

async function assertBalancerWeights() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const initializedNetuids = new Set(
    initializedEntries
      .filter(([, initialized]) => initialized.isTrue)
      .map(([key]) => key.args[0].toNumber())
  );

  assert.ok(initializedNetuids.size > 0, "no subnets have PalSwapInitialized=true");

  const balancers = (await api.query.swap.swapBalancer.entries())
    .filter(([key]) => initializedNetuids.has(key.args[0].toNumber()))
    .map(([key, value]) => ({
      netuid: key.args[0].toNumber(),
      weight: extractQuotePerquintill(value),
    }))
    .filter(({ weight }) => weight !== null);

  assert.ok(balancers.length > 0, "no initialized subnet balancer weights could be decoded");

  for (const { netuid, weight } of balancers) {
    assert.ok(
      weight >= MIN_BALANCER_WEIGHT && weight <= MAX_BALANCER_WEIGHT,
      `netuid ${netuid} balancer weight ${weight} is outside 0.45-0.55`
    );
  }

  const nonHalf = balancers.filter(({ weight }) => weight !== HALF_PERQUINTILL);
  assert.ok(nonHalf.length > 0, "expected at least one initialized subnet balancer weight not equal to 0.5");

  console.log("initialized balancers checked:", balancers.length);
  console.log(
    "non-0.5 balancer samples:",
    nonHalf.slice(0, 10).map(({ netuid, weight }) => `${netuid}:${formatPerquintill(weight)}`).join(", ")
  );

  return { sampleNetuid: nonHalf[0].netuid };
}

async function assertTransferWorks() {
  const senderBefore = (await api.query.system.account(transferSource.address)).data.free.toBigInt();
  assert.ok(
    senderBefore > TRANSFER_AMOUNT,
    `transfer source ${transferSource.address} has ${senderBefore}, cannot transfer ${TRANSFER_AMOUNT}; set TRANSFER_SOURCE_URI to a funded local test account`
  );

  const before = (await api.query.system.account(transferDest.address)).data.free.toBigInt();
  await submitAndWait(
    api,
    transferSource,
    balancesTransfer(api, transferDest.address, TRANSFER_AMOUNT),
    `balances transfer ${transferSource.address} -> ${transferDest.address}`
  );
  const after = (await api.query.system.account(transferDest.address)).data.free.toBigInt();

  assert.equal(after - before, TRANSFER_AMOUNT, "recipient free balance did not increase by transfer amount");
  console.log("transfer credited:", TRANSFER_AMOUNT.toString());
}

async function assertEpochUpdatesReserves(preferredNetuid) {
  let watched = await reserveSnapshots();
  assert.ok(watched.length > 0, "no initialized subnets have non-zero reserves to watch");

  watched.sort((a, b) => (a.netuid === preferredNetuid ? -1 : b.netuid === preferredNetuid ? 1 : a.netuid - b.netuid));
  console.log(
    "watching reserves:",
    watched.slice(0, 10).map(({ netuid, tao, alpha }) => `${netuid}:tao=${tao},alpha=${alpha}`).join("; ")
  );

  for (let blocks = 1; blocks <= MAX_BLOCKS_TO_WAIT; blocks++) {
    const header = await waitForFinalizedBlock();
    const latest = await reserveSnapshots();
    const changed = latest.find((current) => {
      const previous = watched.find(({ netuid }) => netuid === current.netuid);
      return previous && (previous.tao !== current.tao || previous.alpha !== current.alpha);
    });

    if (changed) {
      const previous = watched.find(({ netuid }) => netuid === changed.netuid);
      console.log("reserve update block:", header.number.toString());
      console.log(
        `reserve changed netuid ${changed.netuid}: tao ${previous.tao}->${changed.tao}, alpha ${previous.alpha}->${changed.alpha}`
      );
      return { netuid: changed.netuid, previous, current: changed, block: header.number.toString() };
    }

    if (blocks % 30 === 0) {
      console.log(`waited ${blocks}/${MAX_BLOCKS_TO_WAIT} finalized blocks for reserve update`);
    }
  }

  throw new Error(`no SubnetTAO/SubnetAlphaIn reserve changed within ${MAX_BLOCKS_TO_WAIT} finalized blocks`);
}

async function assertProtocolLiquidityInjected(preferredNetuid) {
  let watched = await injectionSnapshots();
  assert.ok(watched.length > 0, "no initialized subnets have reserve/emission state to watch");

  watched.sort((a, b) => (a.netuid === preferredNetuid ? -1 : b.netuid === preferredNetuid ? 1 : a.netuid - b.netuid));
  console.log(
    "watching protocol injection:",
    watched.slice(0, 10).map(({ netuid, alphaInEmission, taoInEmission }) => `${netuid}:alpha_in=${alphaInEmission},tao_in=${taoInEmission}`).join("; ")
  );

  for (let blocks = 1; blocks <= MAX_BLOCKS_TO_WAIT; blocks++) {
    const header = await waitForFinalizedBlock();
    const latest = await injectionSnapshots();
    const injected = latest.find((current) => {
      const previous = watched.find(({ netuid }) => netuid === current.netuid);
      return (
        previous &&
        (current.alphaInEmission > 0n || current.taoInEmission > 0n) &&
        (current.alpha !== previous.alpha || current.tao !== previous.tao) &&
        (current.alpha >= previous.alpha || current.tao >= previous.tao)
      );
    });

    if (injected) {
      const previous = watched.find(({ netuid }) => netuid === injected.netuid);
      console.log("protocol injection block:", header.number.toString());
      console.log(
        `protocol liquidity injected on netuid ${injected.netuid}: alpha_in_emission=${injected.alphaInEmission}, tao_in_emission=${injected.taoInEmission}, alpha_out_emission=${injected.alphaOutEmission}`
      );
      console.log(
        `injected pool reserves on netuid ${injected.netuid}: tao ${previous.tao}->${injected.tao}, alpha ${previous.alpha}->${injected.alpha}`
      );
      return injected;
    }

    if (blocks % 30 === 0) {
      console.log(`waited ${blocks}/${MAX_BLOCKS_TO_WAIT} finalized blocks for protocol liquidity injection`);
    }
  }

  throw new Error(`no protocol liquidity injection changed pool reserves within ${MAX_BLOCKS_TO_WAIT} finalized blocks`);
}

async function assertValidatorsReceiveRewards(preferredNetuid) {
  let watched = await validatorRewardSnapshots();
  assert.ok(watched.length > 0, "no initialized subnets have validator reward vectors to watch");

  watched.sort((a, b) => (a.netuid === preferredNetuid ? -1 : b.netuid === preferredNetuid ? 1 : a.netuid - b.netuid));
  console.log(
    "watching validator rewards:",
    watched.slice(0, 10).map(({ netuid, rewardedValidators, emissionTotal }) => `${netuid}:validators=${rewardedValidators},emission=${emissionTotal}`).join("; ")
  );

  for (let blocks = 1; blocks <= MAX_BLOCKS_TO_WAIT; blocks++) {
    const header = await waitForFinalizedBlock();
    const latest = await validatorRewardSnapshots();
    const changed = latest.find((current) => {
      const previous = watched.find(({ netuid }) => netuid === current.netuid);
      return previous && current.rewardedValidators > 0 && current.fingerprint !== previous.fingerprint;
    });

    if (changed) {
      console.log("validator reward epoch block:", header.number.toString());
      console.log(
        `validator rewards changed on netuid ${changed.netuid}: validators=${changed.rewardedValidators}, dividends_sum=${changed.dividendTotal}, emission_sum=${changed.emissionTotal}`
      );
      return changed;
    }

    if (blocks % 30 === 0) {
      console.log(`waited ${blocks}/${MAX_BLOCKS_TO_WAIT} finalized blocks for validator reward update`);
    }
  }

  throw new Error(`no subnet validator reward vector changed within ${MAX_BLOCKS_TO_WAIT} finalized blocks`);
}

async function withTemporaryTempo(netuid, tempo, fn) {
  const originalTempo = (await api.query.subtensorModule.tempo(netuid)).toNumber();

  try {
    await sudoSetStorage(
      [[api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", tempo)]],
      `sudo set temporary tempo ${tempo} for netuid ${netuid}`
    );
    console.log(`temporary tempo set on netuid ${netuid}: ${originalTempo}->${tempo}`);
    return await fn();
  } finally {
    await sudoSetStorage(
      [[api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", originalTempo)]],
      `sudo restore tempo for netuid ${netuid}`
    );
    console.log(`temporary tempo restored on netuid ${netuid}: ${tempo}->${originalTempo}`);
  }
}

async function assertStakingSuiteWorks(netuid) {
  const hotkey = await findExistingHotkey(netuid);
  const senderBefore = (await api.query.system.account(transferSource.address)).data.free.toBigInt();
  assert.ok(
    senderBefore > STAKE_AMOUNT,
    `staking source ${transferSource.address} has ${senderBefore}, cannot stake ${STAKE_AMOUNT}`
  );

  const alphaBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const result = await submitAndWait(
    api,
    transferSource,
    api.tx.subtensorModule.addStake(hotkey, netuid, STAKE_AMOUNT),
    `add stake on netuid ${netuid}`
  );
  const alphaAfter = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();

  assert.ok(alphaAfter > alphaBefore, `staking did not increase TotalHotkeyAlpha for netuid ${netuid}`);
  let ownedAlpha = assertStakeAddedEvent(result.events, hotkey, netuid);
  console.log(
    `stake added on epoch-updated netuid ${netuid}: hotkey=${hotkey}, alpha ${alphaBefore}->${alphaAfter}`
  );

  await assertBalancerWeightInRange(netuid, "after addStake on epoch-updated subnet");
  await waitForFinalizedBlock();
  ownedAlpha += await assertAddStakeLimitWorks(hotkey, netuid);
  await waitForFinalizedBlock();
  ownedAlpha -= await assertRemoveStakeLimitWorks(hotkey, netuid, ownedAlpha);
  await waitForFinalizedBlock();
  await assertCrossNetuidStakeTransferWorks(hotkey, netuid, ownedAlpha);
}

async function assertBalancerWeightInRange(netuid, label) {
  const weight = extractQuotePerquintill(await api.query.swap.swapBalancer(netuid));
  assert.ok(weight !== null, `netuid ${netuid} balancer weight could not be decoded ${label}`);
  assert.ok(
    weight >= MIN_BALANCER_WEIGHT && weight <= MAX_BALANCER_WEIGHT,
    `netuid ${netuid} balancer weight ${weight} is outside 0.45-0.55 ${label}`
  );
  console.log(`balancer weight ${label}: netuid ${netuid}=${formatPerquintill(weight)}`);
}

async function assertAddStakeLimitWorks(hotkey, netuid) {
  const alphaBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const result = await submitAndWait(
    api,
    transferSource,
    api.tx.subtensorModule.addStakeLimit(hotkey, netuid, LIMIT_STAKE_AMOUNT, MAX_PRICE, false),
    `add stake limit on netuid ${netuid}`
  );
  const alphaAfter = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();

  assert.ok(alphaAfter > alphaBefore, `addStakeLimit did not increase TotalHotkeyAlpha for netuid ${netuid}`);
  const alphaAdded = assertStakeAddedEvent(result.events, hotkey, netuid);
  console.log(`addStakeLimit increased alpha on netuid ${netuid}: ${alphaBefore}->${alphaAfter}`);
  return alphaAdded;
}

async function assertRemoveStakeLimitWorks(hotkey, netuid, ownedAlpha) {
  const alphaBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const reservesBefore = await reserveSnapshot(netuid);
  const amount = ownedAlpha / 4n;
  assert.ok(amount > 0n, `not enough alpha on netuid ${netuid} to test removeStakeLimit`);

  const result = await submitAndWait(
    api,
    transferSource,
    api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, amount, MIN_PRICE, false),
    `remove stake limit on netuid ${netuid}`
  );
  const alphaAfter = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const reservesAfter = await reserveSnapshot(netuid);

  assert.ok(alphaAfter < alphaBefore, `removeStakeLimit did not reduce TotalHotkeyAlpha for netuid ${netuid}`);
  const { alphaRemoved, feePaid } = assertStakeRemovedEvent(result.events, hotkey, netuid);
  assert.ok(feePaid > 0n, `removeStakeLimit did not report a positive swap fee on netuid ${netuid}`);
  assert.ok(
    reservesAfter.tao !== reservesBefore.tao || reservesAfter.alpha !== reservesBefore.alpha,
    `fee-bearing removeStakeLimit did not change swap pool reserves on netuid ${netuid}`
  );
  console.log(`removeStakeLimit reduced alpha on netuid ${netuid}: ${alphaBefore}->${alphaAfter}`);
  console.log(
    `removeStakeLimit fee and pool update on netuid ${netuid}: fee=${feePaid}, tao ${reservesBefore.tao}->${reservesAfter.tao}, alpha ${reservesBefore.alpha}->${reservesAfter.alpha}`
  );
  return alphaRemoved;
}

async function assertCrossNetuidStakeTransferWorks(hotkey, originNetuid, ownedAlpha) {
  const destinationNetuid = await findTransferDestinationNetuid(originNetuid);
  const originBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, originNetuid)).toBigInt();
  const destinationBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, destinationNetuid)).toBigInt();
  const amount = ownedAlpha / 3n;
  assert.ok(amount > 0n, `not enough alpha on netuid ${originNetuid} to test cross-netuid transferStake`);

  const result = await submitAndWait(
    api,
    transferSource,
    api.tx.subtensorModule.transferStake(
      transferDest.address,
      hotkey,
      originNetuid,
      destinationNetuid,
      amount
    ),
    `cross-netuid transfer stake ${originNetuid}->${destinationNetuid}`
  );

  const originAfter = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, originNetuid)).toBigInt();
  const destinationAfter = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, destinationNetuid)).toBigInt();

  assert.ok(originAfter < originBefore, `transferStake did not reduce origin alpha on netuid ${originNetuid}`);
  assert.ok(
    destinationAfter > destinationBefore,
    `transferStake did not increase destination alpha on netuid ${destinationNetuid}`
  );
  assertStakeTransferredEvent(result.events, hotkey, originNetuid, destinationNetuid);
  console.log(
    `transferStake moved stake ${originNetuid}->${destinationNetuid}: origin ${originBefore}->${originAfter}, destination ${destinationBefore}->${destinationAfter}`
  );
}

async function findExistingHotkey(netuid) {
  const keyEntries = await api.query.subtensorModule.keys.entries(netuid);
  assert.ok(keyEntries.length > 0, `netuid ${netuid} has no registered hotkeys in Keys storage`);

  const hotkey = keyEntries[0][1].toString();
  assert.notEqual(hotkey, "", `netuid ${netuid} first hotkey decoded to an empty address`);
  return hotkey;
}

function assertStakeAddedEvent(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeAdded") {
      return false;
    }

    const [, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return (
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      alphaStaked.toBigInt() > 0n
    );
  });

  assert.ok(event, `StakeAdded event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return event.event.data[3].toBigInt();
}

function assertStakeRemovedEvent(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeRemoved") {
      return false;
    }

    const [, eventHotkey, , alphaUnstaked, eventNetuid] = event.data;
    return (
      eventHotkey.toString() === hotkey &&
      eventNetuid.toNumber() === netuid &&
      alphaUnstaked.toBigInt() > 0n
    );
  });

  assert.ok(event, `StakeRemoved event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return {
    alphaRemoved: event.event.data[3].toBigInt(),
    feePaid: event.event.data[5]?.toBigInt() ?? 0n,
  };
}

function assertStakeTransferredEvent(events, hotkey, originNetuid, destinationNetuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeTransferred") {
      return false;
    }

    const [, , eventHotkey, eventOriginNetuid, eventDestinationNetuid, taoMoved] = event.data;
    return (
      eventHotkey.toString() === hotkey &&
      eventOriginNetuid.toNumber() === originNetuid &&
      eventDestinationNetuid.toNumber() === destinationNetuid &&
      taoMoved.toBigInt() > 0n
    );
  });

  assert.ok(
    event,
    `StakeTransferred event not found for hotkey ${hotkey} from netuid ${originNetuid} to ${destinationNetuid}`
  );
}

async function findTransferDestinationNetuid(originNetuid) {
  const originTransferEnabled = await api.query.subtensorModule.transferToggle(originNetuid);
  assert.ok(originTransferEnabled.isTrue, `transferStake is disabled for origin netuid ${originNetuid}`);

  const snapshots = await reserveSnapshots();
  for (const { netuid } of snapshots) {
    if (netuid === originNetuid) continue;
    const transferEnabled = await api.query.subtensorModule.transferToggle(netuid);
    if (transferEnabled.isTrue) {
      return netuid;
    }
  }

  throw new Error(`no initialized transfer-enabled destination netuid found for origin netuid ${originNetuid}`);
}

async function reserveSnapshots() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber());

  const snapshots = [];
  for (const netuid of initializedNetuids) {
    const [tao, alpha] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
    ]);
    const snapshot = {
      netuid,
      tao: tao.toBigInt(),
      alpha: alpha.toBigInt(),
    };
    if (snapshot.tao > 0n || snapshot.alpha > 0n) {
      snapshots.push(snapshot);
    }
  }
  return snapshots;
}

async function reserveSnapshot(netuid) {
  const [tao, alpha] = await Promise.all([
    api.query.subtensorModule.subnetTAO(netuid),
    api.query.subtensorModule.subnetAlphaIn(netuid),
  ]);
  return {
    netuid,
    tao: tao.toBigInt(),
    alpha: alpha.toBigInt(),
  };
}

async function injectionSnapshots() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber());

  const snapshots = [];
  for (const netuid of initializedNetuids) {
    const [tao, alpha, alphaInEmission, taoInEmission, alphaOutEmission] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
      api.query.subtensorModule.subnetAlphaInEmission(netuid),
      api.query.subtensorModule.subnetTaoInEmission(netuid),
      api.query.subtensorModule.subnetAlphaOutEmission(netuid),
    ]);
    snapshots.push({
      netuid,
      tao: tao.toBigInt(),
      alpha: alpha.toBigInt(),
      alphaInEmission: alphaInEmission.toBigInt(),
      taoInEmission: taoInEmission.toBigInt(),
      alphaOutEmission: alphaOutEmission.toBigInt(),
    });
  }
  return snapshots;
}

async function validatorRewardSnapshots() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const initializedNetuids = initializedEntries
    .filter(([, initialized]) => initialized.isTrue)
    .map(([key]) => key.args[0].toNumber());

  const snapshots = [];
  for (const netuid of initializedNetuids) {
    const [emission, dividends, validatorPermit, lastUpdate] = await Promise.all([
      api.query.subtensorModule.emission(netuid),
      api.query.subtensorModule.dividends(netuid),
      api.query.subtensorModule.validatorPermit(netuid),
      api.query.subtensorModule.lastUpdate(netuid),
    ]);
    const emissionValues = codecVecToBigInts(emission);
    const dividendValues = codecVecToBigInts(dividends);
    const permitValues = codecVecToBools(validatorPermit);
    const lastUpdateValues = codecVecToBigInts(lastUpdate);
    const rewardedValidators = dividendValues.filter((value, index) => value > 0n && permitValues[index]).length;
    snapshots.push({
      netuid,
      rewardedValidators,
      dividendTotal: sumBigInts(dividendValues),
      emissionTotal: sumBigInts(emissionValues),
      fingerprint: [
        emissionValues.join(","),
        dividendValues.join(","),
        permitValues.join(","),
        lastUpdateValues.join(","),
      ].join("|"),
    });
  }
  return snapshots;
}

function codecVecToBigInts(value) {
  return Array.from(value).map((item) => item.toBigInt());
}

function codecVecToBools(value) {
  return Array.from(value).map((item) => item.isTrue ?? Boolean(item.toJSON()));
}

function sumBigInts(values) {
  return values.reduce((sum, value) => sum + value, 0n);
}

function balancesTransfer(api, dest, amount) {
  if (api.tx.balances.transferKeepAlive) {
    return api.tx.balances.transferKeepAlive(dest, amount);
  }
  if (api.tx.balances.transferAllowDeath) {
    return api.tx.balances.transferAllowDeath(dest, amount);
  }
  return api.tx.balances.transfer(dest, amount);
}

async function submitAndWait(api, signer, tx, label) {
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

async function sudoSetStorage(entries, label) {
  await submitAndWait(api, transferSource, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), label);
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
      .subscribeFinalizedHeads((header) => {
        const blockNumber = header.number.toNumber();
        if (lastFinalizedBlockNumber !== null && blockNumber <= lastFinalizedBlockNumber) {
          return;
        }
        lastFinalizedBlockNumber = blockNumber;
        finish(resolve, header);
      })
      .then((unsub) => {
        unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });
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

function extractQuotePerquintill(value) {
  const json = value.toJSON();

  if (typeof json === "number" || typeof json === "string") {
    return BigInt(json);
  }

  if (json && typeof json === "object") {
    const quote = json.quote ?? json.Quote;
    if (typeof quote === "number" || typeof quote === "string") {
      return BigInt(quote);
    }
  }

  const human = value.toHuman();
  if (human && typeof human === "object") {
    const quote = human.quote ?? human.Quote;
    if (typeof quote === "string") {
      return BigInt(quote.replaceAll(",", ""));
    }
  }

  return null;
}

function formatPerquintill(value) {
  const integer = value / 1_000_000_000_000_000_000n;
  const fractional = (value % 1_000_000_000_000_000_000n).toString().padStart(18, "0");
  return `${integer}.${fractional.slice(0, 6)}`;
}
