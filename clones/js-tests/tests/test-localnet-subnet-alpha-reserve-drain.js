import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const RUN_ID = process.env.LOCALNET_ALPHA_DRAIN_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const MIN_STAKE_TAO = 2_000_000n;
const MINIMUM_BUY_SEARCH_WINDOW = 100n;
const SWAP_MINIMUM_RESERVE = 1_000_000n;
const MAX_PRICE = 18_446_744_073_709_551_615n;
const LONG_TERM_WAIT_BLOCKS = Number(process.env.LONG_TERM_WAIT_BLOCKS ?? 30);
const ROOT_STAKE_TAO = 200_000_000n;
const LOCAL_VALIDATOR_ALPHA = 10_000_000n;
const ROOT_ALPHA_DIVIDEND_TARGET = BigInt(process.env.ROOT_ALPHA_DIVIDEND_TARGET ?? 800_000);
const NORMALIZED_TAO_RESERVE = 10_000_000_000n;
const I96F32_ONE = 1n << 32n;

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri("//Alice");
const ownerHotkey = keyring.addFromUri(`//AlphaReserveDrain//${RUN_ID}//owner-hotkey`);
const logger = createTempLogger("test-localnet-subnet-alpha-reserve-drain.log");
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
    console.log("run id:", RUN_ID);
    console.log("owner coldkey:", alice.address);
    console.log("owner hotkey:", ownerHotkey.address);

    assertMetadataAvailable();
    await assertAliceIsSudo();
    await prepareSubnetRegistration();

    const netuid = await registerSubnet();
    await enableSubtoken(netuid);
    await setupEmissionAndRootClaimFixture(netuid);
    const rootDividendsBeforeDrain = await waitForRootAlphaDividends(netuid, ROOT_ALPHA_DIVIDEND_TARGET);
    console.log("root alpha dividends before drain:", rootDividendsBeforeDrain.toString());
    await pauseSubnetEmissions(netuid);
    await normalizePoolForDrain(netuid);

    const initial = await readReserves(netuid);
    console.log("created netuid:", netuid);
    console.log("initial reserves:", formatReserves(initial));
    assert.ok(
      initial.alphaIn >= SWAP_MINIMUM_RESERVE,
      `fresh subnet alpha reserve should start at or above ${SWAP_MINIMUM_RESERVE}, got ${initial.alphaIn}`
    );

    const { amount: minimumDrainTao, result: drainResult, rejected } = await executeSmallestAcceptedBuy(netuid);
    const alphaBought = stakeAddedFromEvents(drainResult.events, ownerHotkey.address, netuid);
    const afterDrain = await readReserves(netuid);
    console.log("pre-drain rejected buy amounts:", rejected.map((amount) => amount.toString()).join(", "));
    console.log("minimum accepted drain buy:", `${minimumDrainTao} rao`);
    console.log("alpha bought:", alphaBought.toString());
    console.log("reserves after drain:", formatReserves(afterDrain));
    assert.ok(alphaBought > 0n, "minimum buy should buy positive alpha");
    assert.ok(
      afterDrain.alphaIn < SWAP_MINIMUM_RESERVE,
      `expected alpha reserve below ${SWAP_MINIMUM_RESERVE}, got ${afterDrain.alphaIn}`
    );

    await expectDispatchError(
      api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, MIN_STAKE_TAO, MAX_PRICE, false),
      "buy after alpha reserve below minimum",
      "ReservesTooLow"
    );
    console.log("post-drain buy rejected with reserve error:", `alpha_in=${afterDrain.alphaIn}`);

    const rootClaimedBeforeAuto = await readRootClaimed(netuid);
    await submitAndWait(alice, api.tx.sudo.sudo(api.tx.subtensorModule.sudoSetNumRootClaims(1)), "enable auto root claim");
    let rootClaimMode = "auto";
    let rootClaimedAfterAuto = await waitForRootClaimedIncrease(netuid, rootClaimedBeforeAuto, 12);
    if (rootClaimedAfterAuto === null) {
      console.log("auto root claim did not advance root claimed; submitting explicit claim_root");
      rootClaimMode = "manual";
      await submitAndWait(alice, api.tx.subtensorModule.claimRoot([netuid]), "claim_root");
      rootClaimedAfterAuto = await readRootClaimed(netuid);
      assert.ok(rootClaimedAfterAuto > rootClaimedBeforeAuto, "manual claim_root did not increase RootClaimed");
    }
    const afterAutoClaim = await readReserves(netuid);
    console.log(
      "root claim happened:",
      `mode=${rootClaimMode}`,
      `root_claimed_before=${rootClaimedBeforeAuto}`,
      `root_claimed_after=${rootClaimedAfterAuto}`,
      formatReserves(afterAutoClaim)
    );

    const rootClaimRestoredReserve = await checkPostRootClaimBuy(netuid, afterAutoClaim);

    const waitStart = await currentBlockNumber();
    await waitForBlocks(LONG_TERM_WAIT_BLOCKS);
    const waitEnd = await currentBlockNumber();
    const afterWait = await readReserves(netuid);
    console.log(
      "reserves after no-user-action wait:",
      `start_block=${waitStart}`,
      `end_block=${waitEnd}`,
      `waited_blocks=${waitEnd - waitStart}`,
      formatReserves(afterWait)
    );
    if (rootClaimRestoredReserve) {
      assert.ok(
        afterWait.alphaIn >= SWAP_MINIMUM_RESERVE,
        `expected auto root claim recovery to persist above ${SWAP_MINIMUM_RESERVE}, got ${afterWait.alphaIn}`
      );
      console.log("post-wait reserve remains above minimum after root claim:", `alpha_in=${afterWait.alphaIn}`);
    } else {
      assert.ok(
        afterWait.alphaIn < SWAP_MINIMUM_RESERVE,
        `expected alpha reserve to stay below ${SWAP_MINIMUM_RESERVE} without user action, got ${afterWait.alphaIn}`
      );

      await expectDispatchError(
        api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, MIN_STAKE_TAO, MAX_PRICE, false),
        "buy after waiting without user action",
        "ReservesTooLow"
      );
      console.log("post-wait buy still rejected with reserve error:", `alpha_in=${afterWait.alphaIn}`);
    }
    console.log("localnet subnet alpha reserve drain: ok");
  } finally {
    await api?.disconnect();
    await logger.flush();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Balances.forceSetBalance", api.tx.balances?.forceSetBalance],
    ["SubtensorModule.registerNetwork", api.tx.subtensorModule?.registerNetwork],
    ["SubtensorModule.rootRegister", api.tx.subtensorModule?.rootRegister],
    ["SubtensorModule.addStakeLimit", api.tx.subtensorModule?.addStakeLimit],
    ["SubtensorModule.claimRoot", api.tx.subtensorModule?.claimRoot],
    ["SubtensorModule.sudoSetNumRootClaims", api.tx.subtensorModule?.sudoSetNumRootClaims],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
    ["SubtensorModule.SubnetAlphaOut", api.query.subtensorModule?.subnetAlphaOut],
    ["SubtensorModule.SubnetLimit", api.query.subtensorModule?.subnetLimit],
    ["SubtensorModule.NetworkRateLimit", api.query.subtensorModule?.networkRateLimit],
    ["SubtensorModule.NetworkRegistrationStartBlock", api.query.subtensorModule?.networkRegistrationStartBlock],
    ["SubtensorModule.NetworkImmunityPeriod", api.query.subtensorModule?.networkImmunityPeriod],
    ["SubtensorModule.NetworkMinLockCost", api.query.subtensorModule?.networkMinLockCost],
    ["SubtensorModule.NetworkLastLockCost", api.query.subtensorModule?.networkLastLockCost],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.SubtokenEnabled", api.query.subtensorModule?.subtokenEnabled],
    ["SubtensorModule.FirstEmissionBlockNumber", api.query.subtensorModule?.firstEmissionBlockNumber],
    ["SubtensorModule.Tempo", api.query.subtensorModule?.tempo],
    ["SubtensorModule.TaoWeight", api.query.subtensorModule?.taoWeight],
    ["SubtensorModule.SubnetMovingPrice", api.query.subtensorModule?.subnetMovingPrice],
    ["SubtensorModule.SubnetEmissionEnabled", api.query.subtensorModule?.subnetEmissionEnabled],
    ["SubtensorModule.NetworkRegistrationAllowed", api.query.subtensorModule?.networkRegistrationAllowed],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.RootAlphaDividendsPerSubnet", api.query.subtensorModule?.rootAlphaDividendsPerSubnet],
    ["SubtensorModule.RootClaimed", api.query.subtensorModule?.rootClaimed],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `missing metadata: ${missing.map(([name]) => name).join(", ")}`);
}

async function assertAliceIsSudo() {
  const sudoKey = await api.query.sudo.key();
  assert.equal(sudoKey.toString(), alice.address, `Alice is not sudo; sudo key is ${sudoKey.toString()}`);
}

async function prepareSubnetRegistration() {
  const activeCount = await activeNonRootSubnetCount();
  const subnetLimit = (await api.query.subtensorModule.subnetLimit()).toNumber();
  const targetLimit = Math.max(subnetLimit, activeCount + 1);
  const entries = [
    [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", targetLimit)],
    [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", 0n)],
    [api.query.subtensorModule.networkRegistrationStartBlock.key(), storageValueHex("u64", 0n)],
    [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", 0n)],
    [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", SWAP_MINIMUM_RESERVE)],
    [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", SWAP_MINIMUM_RESERVE)],
  ];

  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), "sudo prepare subnet registration");
  console.log(
    "registration settings:",
    `subnet_limit=${targetLimit}`,
    `lock_cost=${SWAP_MINIMUM_RESERVE}`
  );
}

async function activeNonRootSubnetCount() {
  const entries = await api.query.subtensorModule.networksAdded.entries();
  return entries.filter(([key, value]) => value.isTrue && key.args[0].toNumber() !== 0).length;
}

async function registerSubnet() {
  const result = await submitAndWait(
    alice,
    api.tx.subtensorModule.registerNetwork(ownerHotkey.address),
    "registerNetwork"
  );
  const event = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkAdded"
  );
  assert.ok(event, "NetworkAdded event not found");
  const netuid = event.event.data[0].toNumber();
  assert.equal((await api.query.subtensorModule.networksAdded(netuid)).isTrue, true, `${netuid} was not added`);
  return netuid;
}

async function setupEmissionAndRootClaimFixture(netuid) {
  const block = await currentBlockNumber();
  const entries = [
    [api.query.subtensorModule.subtokenEnabled.key(0), storageValueHex("bool", true)],
    [api.query.subtensorModule.firstEmissionBlockNumber.key(netuid), storageValueHex("Option<u64>", block)],
    [api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", 1)],
    [api.query.subtensorModule.taoWeight.key(), storageValueHex("u64", MAX_PRICE)],
    [api.query.subtensorModule.subnetMovingPrice.key(netuid), storageValueHex("i128", 2n * I96F32_ONE)],
    [api.query.subtensorModule.subnetEmissionEnabled.key(netuid), storageValueHex("bool", true)],
    [api.query.subtensorModule.networkRegistrationAllowed.key(netuid), storageValueHex("bool", true)],
    [api.query.subtensorModule.totalHotkeyAlpha.key(ownerHotkey.address, netuid), storageValueHex("u64", LOCAL_VALIDATOR_ALPHA)],
  ];

  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), "sudo setup emissions");
  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.subtensorModule.sudoSetNumRootClaims(0)), "disable auto root claim");
  await submitAndWait(alice, api.tx.subtensorModule.rootRegister(ownerHotkey.address), "rootRegister");

  const rootStakeResult = await submitAndWait(
    alice,
    api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, 0, ROOT_STAKE_TAO, MAX_PRICE, false),
    "add root stake"
  );
  const rootAlpha = stakeAddedFromEvents(rootStakeResult.events, ownerHotkey.address, 0);
  console.log(
    "emission/root-claim fixture:",
    `first_emission_block=${block}`,
    `root_stake_tao=${ROOT_STAKE_TAO}`,
    `root_alpha=${rootAlpha}`,
    `local_validator_alpha=${LOCAL_VALIDATOR_ALPHA}`,
    `auto_claim=disabled`
  );
}

async function pauseSubnetEmissions(netuid) {
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(
      api.tx.system.setStorage([
        [api.query.subtensorModule.networkRegistrationAllowed.key(netuid), storageValueHex("bool", false)],
      ])
    ),
    "pause subnet emissions"
  );
  console.log("subnet emissions paused:", netuid);
}

async function normalizePoolForDrain(netuid) {
  const subnetAccount = await getSubnetAccountId(netuid);
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(
      api.tx.system.setStorage([
        [api.query.subtensorModule.subnetTAO.key(netuid), storageValueHex("u64", NORMALIZED_TAO_RESERVE)],
        [api.query.subtensorModule.subnetAlphaIn.key(netuid), storageValueHex("u64", SWAP_MINIMUM_RESERVE)],
      ])
    ),
    "normalize pool reserves before drain"
  );
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(api.tx.balances.forceSetBalance(subnetAccount, NORMALIZED_TAO_RESERVE)),
    "fund subnet account for root claim"
  );
  console.log("pool reserves normalized before drain:", formatReserves(await readReserves(netuid)));
  console.log("subnet account funded:", `account=${subnetAccount}`, `free=${NORMALIZED_TAO_RESERVE}`);
}

async function getSubnetAccountId(netuid) {
  const encoded = await api._rpcCore.provider.send("subnetInfo_getSubnetAccountId", [netuid, null]);
  const account = api.createType("Option<AccountId32>", Uint8Array.from(encoded));
  assert.ok(account.isSome, `subnet account id not found for netuid ${netuid}`);
  return account.unwrap().toString();
}

async function executeSmallestAcceptedBuy(netuid) {
  const sim = await simSwapTaoForAlpha(netuid, MIN_STAKE_TAO);
  const simulatedMinimum = MIN_STAKE_TAO + sim.taoFee;
  const reserves = await readReserves(netuid);
  const targetAlphaOut = reserves.alphaIn - SWAP_MINIMUM_RESERVE + 1n;
  const rejected = [];
  const firstRejectedAttempt = simulatedMinimum > 2n ? simulatedMinimum - 2n : MIN_STAKE_TAO;
  let low = simulatedMinimum;
  let high = simulatedMinimum;

  console.log(
    "minimum buy simulation:",
    `tao_amount=${sim.taoAmount}`,
    `alpha_amount=${sim.alphaAmount}`,
    `tao_fee=${sim.taoFee}`,
    `simulated_minimum=${simulatedMinimum}`,
    `target_alpha_out=${targetAlphaOut}`
  );

  while ((await simSwapTaoForAlpha(netuid, high)).alphaAmount < targetAlphaOut) {
    high *= 2n;
  }

  while (low < high) {
    const mid = (low + high) / 2n;
    const midSim = await simSwapTaoForAlpha(netuid, mid);
    if (midSim.alphaAmount >= targetAlphaOut) {
      high = mid;
    } else {
      low = mid + 1n;
    }
  }

  const drainAmount = low;
  for (let amount = firstRejectedAttempt; amount < simulatedMinimum; amount += 1n) {
    try {
      await submitAndWait(
        alice,
        api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, amount, MAX_PRICE, false),
        `buy candidate ${amount} rao`
      );
      throw new Error(`buy candidate ${amount} unexpectedly passed below simulated minimum ${simulatedMinimum}`);
    } catch (error) {
      if (!/\bAmountTooLow\b/.test(error.message)) {
        throw error;
      }
      rejected.push(amount);
    }
  }

  const result = await submitAndWait(
    alice,
    api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, drainAmount, MAX_PRICE, false),
    `minimum drain buy ${drainAmount} rao`
  );
  return { amount: drainAmount, result, rejected };
}

async function simSwapTaoForAlpha(netuid, tao) {
  const bytes = await api._rpcCore.provider.send("swap_simSwapTaoForAlpha", [
    netuid,
    Number(tao),
    null,
  ]);
  const decoded = api.createType("(u64,u64,u64,u64,u64,u64)", Uint8Array.from(bytes));
  const [taoAmount, alphaAmount, taoFee, alphaFee, taoSlippage, alphaSlippage] = decoded;

  return {
    taoAmount: taoAmount.toBigInt(),
    alphaAmount: alphaAmount.toBigInt(),
    taoFee: taoFee.toBigInt(),
    alphaFee: alphaFee.toBigInt(),
    taoSlippage: taoSlippage.toBigInt(),
    alphaSlippage: alphaSlippage.toBigInt(),
  };
}

async function enableSubtoken(netuid) {
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(
      api.tx.system.setStorage([
        [api.query.subtensorModule.subtokenEnabled.key(netuid), storageValueHex("bool", true)],
      ])
    ),
    `sudo enable subtoken trading for netuid ${netuid}`
  );

  assert.equal(
    (await api.query.subtensorModule.subtokenEnabled(netuid)).isTrue,
    true,
    `subtoken trading was not enabled for netuid ${netuid}`
  );
  console.log("subtoken trading enabled:", netuid);
}

async function readReserves(netuid) {
  const [tao, alphaIn, alphaOut] = await Promise.all([
    api.query.subtensorModule.subnetTAO(netuid),
    api.query.subtensorModule.subnetAlphaIn(netuid),
    api.query.subtensorModule.subnetAlphaOut(netuid),
  ]);
  return {
    tao: tao.toBigInt(),
    alphaIn: alphaIn.toBigInt(),
    alphaOut: alphaOut.toBigInt(),
  };
}

async function readRootClaimed(netuid) {
  return (await api.query.subtensorModule.rootClaimed(netuid, ownerHotkey.address, alice.address)).toBigInt();
}

async function readRootAlphaDividends(netuid) {
  return (await api.query.subtensorModule.rootAlphaDividendsPerSubnet(netuid, ownerHotkey.address)).toBigInt();
}

async function waitForRootAlphaDividends(netuid, minimum) {
  const start = await currentBlockNumber();
  const target = start + 60;
  console.log("waiting for root alpha dividends:", `start_block=${start}`, `target_minimum=${minimum}`);

  while ((await currentBlockNumber()) < target) {
    const dividends = await readRootAlphaDividends(netuid);
    if (dividends >= minimum) {
      console.log("root alpha dividends reached:", `block=${await currentBlockNumber()}`, `amount=${dividends}`);
      return dividends;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }

  throw new Error(`root alpha dividends did not reach ${minimum}; current=${await readRootAlphaDividends(netuid)}`);
}

async function waitForRootClaimedIncrease(netuid, previous, blocks) {
  const start = await currentBlockNumber();
  const target = start + blocks;
  console.log("waiting for auto root claim:", `start_block=${start}`, `target_block=${target}`);

  while ((await currentBlockNumber()) < target) {
    const claimed = await readRootClaimed(netuid);
    if (claimed > previous) {
      return claimed;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }

  return null;
}

async function checkPostRootClaimBuy(netuid, reserves) {
  if (reserves.alphaIn >= SWAP_MINIMUM_RESERVE) {
    console.log("root claim restored alpha reserve above minimum:", `alpha_in=${reserves.alphaIn}`);
    return true;
  }

  try {
    await submitAndWait(
        alice,
        api.tx.subtensorModule.addStakeLimit(ownerHotkey.address, netuid, MIN_STAKE_TAO, MAX_PRICE, false),
        "buy after auto root claim"
      );
    throw new Error(`buy after auto root claim unexpectedly succeeded with alpha_in=${reserves.alphaIn}`);
  } catch (error) {
    assert.match(error.message, /\bReservesTooLow\b/);
    console.log("post-root-claim buy still rejected with reserve error:", `alpha_in=${reserves.alphaIn}`);
    return false;
  }
}

async function currentBlockNumber() {
  const header = await api.rpc.chain.getHeader();
  return header.number.toNumber();
}

async function waitForBlocks(blocks) {
  assert.ok(Number.isInteger(blocks) && blocks > 0, `invalid wait block count: ${blocks}`);

  const start = await currentBlockNumber();
  const target = start + blocks;
  console.log("waiting without user action:", `start_block=${start}`, `target_block=${target}`);

  while ((await currentBlockNumber()) < target) {
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}

function stakeAddedFromEvents(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeAdded") return false;
    const [, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && alphaStaked.toBigInt() > 0n;
  });
  assert.ok(event, `StakeAdded event not found for ${hotkey} on netuid ${netuid}`);
  return event.event.data[3].toBigInt();
}

async function expectDispatchError(tx, label, expectedName) {
  await assert.rejects(
    () => submitAndWait(alice, tx, label),
    (error) => {
      assert.match(error.message, new RegExp(`\\b${expectedName}\\b`));
      return true;
    }
  );
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

function formatReserves(reserves) {
  return `tao=${reserves.tao} alpha_in=${reserves.alphaIn} alpha_out=${reserves.alphaOut}`;
}
