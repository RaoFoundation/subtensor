import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { u8aToHex } from "@polkadot/util";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const RUN_ID = process.env.ROOT_REBORN_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const MAX_PRICE = 18_446_744_073_709_551_615n;
const I96F32_ONE = 1n << 32n;
const ROOT_STAKE_TAO = 2_000_000_000_000n;
const LATE_ROOT_STAKE_TAO = 4_000_000_000_000n;
const TEST_ACCOUNT_BALANCE = 10_000_000_000_000n;
const SUBNET_LOCK_COST = 1_000_000n;
const SUBNET_TAO_RESERVE = 40_000_000_000n;
const SUBNET_ALPHA_RESERVE = 40_000_000_000n;
const LOCAL_VALIDATOR_ALPHA = 20_000_000_000n;
const ROOT_ALPHA_DIVIDEND_TARGET = BigInt(process.env.ROOT_ALPHA_DIVIDEND_TARGET ?? 2_000_000);
const MAX_BASKET_WAIT_BLOCKS = Number(process.env.MAX_BASKET_WAIT_BLOCKS ?? 90);
const TX_TIMEOUT_MS = Number(process.env.TX_TIMEOUT_MS ?? 180_000);

const keyring = new Keyring({ type: "sr25519" });
const alice = keyring.addFromUri("//Alice");
const validatorHotkey = keyring.addFromUri(`//RootReborn//${RUN_ID}//validator-hotkey`);
const lateColdkey = keyring.addFromUri(`//RootReborn//${RUN_ID}//late-coldkey`);
const LOG_FILE = `test-root-reborn-basket-${RUN_ID}.log`;
const logger = createTempLogger(LOG_FILE);
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
    console.log("log file:", LOG_FILE);
    console.log("owner/root coldkey:", alice.address);
    console.log("validator hotkey:", validatorHotkey.address);
    console.log("late coldkey:", lateColdkey.address);

    assertMetadataAvailable();
    await assertAliceIsSudo();
    await fund(lateColdkey.address, TEST_ACCOUNT_BALANCE);
    await prepareSubnetRegistration();

    const netuid = await registerSubnet();
    await setupBasketFixture(netuid);
    await setRootWeights(netuid);
    await assertRootWeightValidation(netuid);

    const before = await readBasketState(netuid);
    console.log("basket before emission:", formatBasketState(before));

    await waitForBasketPrincipal(netuid, before.principal);

    const accrued = await readBasketState(netuid);
    console.log("basket after emission:", formatBasketState(accrued));
    assert.ok(accrued.principal > before.principal, "BasketPrincipal did not increase after root dividends");
    assert.ok(accrued.escrowAlpha > before.escrowAlpha, "escrow alpha stake did not increase");
    assert.ok(accrued.validatorNav > 0n, "validator NAV RPC returned zero after basket accrual");
    assert.ok(accrued.totalNav >= accrued.validatorNav, "total NAV RPC is below validator NAV");
    assert.ok(accrued.ownerOwed > 0n, "staker owed RPC returned zero after basket accrual");
    assert.ok(accrued.basket.length > 0, "validator basket RPC returned no rows");
    assert.equal(accrued.basket[0].netuid, netuid, "validator basket RPC used unexpected netuid");

    await assertClaimReducesOwnerOwed(netuid, accrued.ownerOwed, accrued.principal);
    await createSecondAccrualForDissolve(netuid);
    await assertDissolveRedistributionRisk(netuid);

    console.log("root reborn basket test: completed");
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
    ["SubtensorModule.setRootWeights", api.tx.subtensorModule?.setRootWeights],
    ["SubtensorModule.rootDissolveNetwork", api.tx.subtensorModule?.rootDissolveNetwork],
    ["SubtensorModule.sudoSetNumRootClaims", api.tx.subtensorModule?.sudoSetNumRootClaims],
    ["SubtensorModule.BasketPrincipal", api.query.subtensorModule?.basketPrincipal],
    ["SubtensorModule.RootClaimable", api.query.subtensorModule?.rootClaimable],
    ["SubtensorModule.RootClaimed", api.query.subtensorModule?.rootClaimed],
    ["SubtensorModule.Weights", api.query.subtensorModule?.weights],
    ["SubtensorModule.WeightsSetRateLimit", api.query.subtensorModule?.weightsSetRateLimit],
    ["SubtensorModule.AlphaV2", api.query.subtensorModule?.alphaV2],
    ["SubtensorModule.TotalHotkeyAlpha", api.query.subtensorModule?.totalHotkeyAlpha],
    ["SubtensorModule.TotalHotkeySharesV2", api.query.subtensorModule?.totalHotkeySharesV2],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
    ["SubtensorModule.SubnetAlphaOut", api.query.subtensorModule?.subnetAlphaOut],
    ["SubtensorModule.SubnetLimit", api.query.subtensorModule?.subnetLimit],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.NetworkRegistrationAllowed", api.query.subtensorModule?.networkRegistrationAllowed],
    ["SubtensorModule.RootAlphaDividendsPerSubnet", api.query.subtensorModule?.rootAlphaDividendsPerSubnet],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.Uids", api.query.subtensorModule?.uids],
    ["betaBasket_getStakerOwed", api._rpcCore.provider],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `missing metadata: ${missing.map(([name]) => name).join(", ")}`);
}

async function assertAliceIsSudo() {
  const sudoKey = await api.query.sudo.key();
  assert.equal(sudoKey.toString(), alice.address, `Alice is not sudo; sudo key is ${sudoKey.toString()}`);
}

async function fund(address, amount) {
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(api.tx.balances.forceSetBalance(address, amount)),
    `fund ${address}`
  );
}

async function prepareSubnetRegistration() {
  const activeCount = await activeNonRootSubnetCount();
  const subnetLimit = (await api.query.subtensorModule.subnetLimit()).toNumber();
  const targetLimit = Math.max(subnetLimit, activeCount + 2);
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(
      api.tx.system.setStorage([
        [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", targetLimit)],
        [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkRegistrationStartBlock.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", SUBNET_LOCK_COST)],
        [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", SUBNET_LOCK_COST)],
        [api.query.subtensorModule.weightsSetRateLimit.key(0), storageValueHex("u64", 0n)],
      ])
    ),
    "sudo prepare subnet registration"
  );
  console.log("registration prepared:", `subnet_limit=${targetLimit}`);
}

async function activeNonRootSubnetCount() {
  const entries = await api.query.subtensorModule.networksAdded.entries();
  return entries.filter(([key, value]) => value.isTrue && key.args[0].toNumber() !== 0).length;
}

async function registerSubnet() {
  const result = await submitAndWait(
    alice,
    api.tx.subtensorModule.registerNetwork(validatorHotkey.address),
    "registerNetwork"
  );
  const event = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkAdded"
  );
  assert.ok(event, "NetworkAdded event not found");
  const netuid = event.event.data[0].toNumber();
  console.log("registered subnet:", netuid);
  return netuid;
}

async function setupBasketFixture(netuid) {
  const block = await currentBlockNumber();
  const subnetAccount = await getSubnetAccountId(netuid);
  const entries = [
    [api.query.subtensorModule.subtokenEnabled.key(0), storageValueHex("bool", true)],
    [api.query.subtensorModule.subtokenEnabled.key(netuid), storageValueHex("bool", true)],
    [api.query.subtensorModule.firstEmissionBlockNumber.key(netuid), storageValueHex("Option<u64>", block)],
    [api.query.subtensorModule.tempo.key(netuid), storageValueHex("u16", 1)],
    [api.query.subtensorModule.taoWeight.key(), storageValueHex("u64", MAX_PRICE)],
    [api.query.subtensorModule.subnetMovingPrice.key(netuid), storageValueHex("i128", 2n * I96F32_ONE)],
    [api.query.subtensorModule.subnetEmissionEnabled.key(netuid), storageValueHex("bool", true)],
    [api.query.subtensorModule.networkRegistrationAllowed.key(netuid), storageValueHex("bool", true)],
    [api.query.subtensorModule.totalHotkeyAlpha.key(validatorHotkey.address, netuid), storageValueHex("u64", LOCAL_VALIDATOR_ALPHA)],
    [api.query.subtensorModule.subnetTAO.key(netuid), storageValueHex("u64", SUBNET_TAO_RESERVE)],
    [api.query.subtensorModule.subnetAlphaIn.key(netuid), storageValueHex("u64", SUBNET_ALPHA_RESERVE)],
    [api.query.subtensorModule.subnetAlphaOut.key(netuid), storageValueHex("u64", SUBNET_ALPHA_RESERVE)],
  ];

  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.system.setStorage(entries)), "sudo setup basket fixture");
  await submitAndWait(
    alice,
    api.tx.sudo.sudo(api.tx.balances.forceSetBalance(subnetAccount, SUBNET_TAO_RESERVE)),
    "fund subnet account"
  );
  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.subtensorModule.sudoSetNumRootClaims(0)), "disable auto root claim");
  await submitAndWait(
    alice,
    api.tx.subtensorModule.addStakeLimit(validatorHotkey.address, 0, ROOT_STAKE_TAO, MAX_PRICE, false),
    "owner add root stake"
  );
  await submitAndWait(alice, api.tx.subtensorModule.rootRegister(validatorHotkey.address), "rootRegister");

  console.log(
    "basket fixture:",
    `netuid=${netuid}`,
    `first_emission_block=${block}`,
    `root_stake=${ROOT_STAKE_TAO}`,
    `local_validator_alpha=${LOCAL_VALIDATOR_ALPHA}`
  );
}

async function setRootWeights(netuid) {
  await submitAndWait(
    validatorHotkey,
    api.tx.subtensorModule.setRootWeights([netuid], [65_535], await rootVersionKey()),
    "set_root_weights"
  );
  const uid = (await api.query.subtensorModule.uids(0, validatorHotkey.address)).unwrap().toNumber();
  const weights = await api.query.subtensorModule.weights(0, uid);
  console.log("root weights stored:", `uid=${uid}`, weights.toString());
  assert.match(weights.toString(), new RegExp(`\\b${netuid}\\b`));
}

async function assertRootWeightValidation(netuid) {
  await assert.rejects(
    async () => submitAndWait(validatorHotkey, api.tx.subtensorModule.setRootWeights([0], [65_535], await rootVersionKey()), "set invalid root weight"),
    (error) => {
      assert.match(error.message, /\bUidVecContainInvalidOne\b/);
      return true;
    }
  );
  await assert.rejects(
    async () => submitAndWait(validatorHotkey, api.tx.subtensorModule.setRootWeights([netuid, netuid], [1, 2], await rootVersionKey()), "set duplicate root weight"),
    (error) => {
      assert.match(error.message, /\bDuplicateUids\b/);
      return true;
    }
  );
  console.log("root weight validation rejects root destination and duplicates");
}

async function waitForBasketPrincipal(netuid, previousPrincipal) {
  const start = await currentBlockNumber();
  console.log("waiting for basket accrual:", `start_block=${start}`, `netuid=${netuid}`);

  for (let i = 0; i < MAX_BASKET_WAIT_BLOCKS; i++) {
    await sleep(1000);
    const state = await readBasketState(netuid);
    const rootDividends = await readRootAlphaDividends(netuid);
    if (state.principal > previousPrincipal) {
      console.log(
        "basket accrued:",
        `block=${await currentBlockNumber()}`,
        `principal=${state.principal}`,
        `escrow_alpha=${state.escrowAlpha}`,
        `root_dividends=${rootDividends}`
      );
      return;
    }
    if (i % 15 === 14) {
      console.log(
        "still waiting for basket accrual:",
        `waited_blocks~${i + 1}`,
        `principal=${state.principal}`,
        `root_dividends=${rootDividends}`
      );
    }
  }

  throw new Error(`BasketPrincipal did not increase within ${MAX_BASKET_WAIT_BLOCKS} blocks`);
}

async function assertClaimReducesOwnerOwed(netuid, owedBefore, principalBefore) {
  await submitAndWait(alice, api.tx.subtensorModule.claimRoot([netuid]), "owner claim_root");
  const after = await readBasketState(netuid);
  console.log(
    "claim_root result:",
    `owed_before=${owedBefore}`,
    `owed_after=${after.ownerOwed}`,
    `principal_before=${principalBefore}`,
    `principal_after=${after.principal}`,
    `escrow_alpha_after=${after.escrowAlpha}`
  );
  assert.ok(after.ownerOwed < owedBefore, "claim_root did not reduce owner owed RPC amount");
  assert.ok(after.principal < principalBefore, "claim_root did not reduce BasketPrincipal");
}

async function createSecondAccrualForDissolve(netuid) {
  const before = await readBasketState(netuid);
  await waitForBasketPrincipal(netuid, before.principal);
  const after = await readBasketState(netuid);
  assert.ok(after.ownerOwed > 0n, "second accrual did not create owner owed amount");
  console.log("second accrual for dissolve:", formatBasketState(after));
}

async function assertDissolveRedistributionRisk(netuid) {
  const beforeLateRootStake = await rootStakeOf(lateColdkey.address);
  await submitAndWait(
    lateColdkey,
    api.tx.subtensorModule.addStakeLimit(validatorHotkey.address, 0, LATE_ROOT_STAKE_TAO, MAX_PRICE, false),
    "late coldkey add root stake"
  );
  const before = {
    ownerRootStake: await rootStakeOf(alice.address),
    lateRootStake: await rootStakeOf(lateColdkey.address),
    ownerOwed: await rpcU64("betaBasket_getStakerOwed", [alice.address, null]),
    lateOwed: await rpcU64("betaBasket_getStakerOwed", [lateColdkey.address, null]),
    principal: await basketPrincipal(netuid),
  };
  console.log(
    "before dissolve:",
    `owner_root=${before.ownerRootStake}`,
    `late_root=${before.lateRootStake}`,
    `owner_owed=${before.ownerOwed}`,
    `late_owed=${before.lateOwed}`,
    `principal=${before.principal}`
  );
  assert.ok(before.ownerOwed > 0n, "owner should have accrued basket owed before dissolve");
  assert.ok(before.lateRootStake > beforeLateRootStake, "late root stake was not added");

  await submitAndWait(alice, api.tx.sudo.sudo(api.tx.subtensorModule.rootDissolveNetwork(netuid)), "root_dissolve_network");
  const after = {
    ownerRootStake: await rootStakeOf(alice.address),
    lateRootStake: await rootStakeOf(lateColdkey.address),
    ownerOwed: await rpcU64("betaBasket_getStakerOwed", [alice.address, null]),
    lateOwed: await rpcU64("betaBasket_getStakerOwed", [lateColdkey.address, null]),
    principal: await basketPrincipal(netuid),
    networkExists: (await api.query.subtensorModule.networksAdded(netuid)).isTrue,
  };
  console.log(
    "after dissolve:",
    `owner_root=${after.ownerRootStake}`,
    `late_root=${after.lateRootStake}`,
    `owner_owed=${after.ownerOwed}`,
    `late_owed=${after.lateOwed}`,
    `principal=${after.principal}`,
    `network_exists=${after.networkExists}`
  );

  const ownerRootGain = after.ownerRootStake - before.ownerRootStake;
  const lateRootGain = after.lateRootStake - before.lateRootStake;
  console.log("dissolve root stake gains:", `owner=${ownerRootGain}`, `late=${lateRootGain}`);

  assert.equal(after.networkExists, false, "subnet still exists after root dissolve");
  assert.equal(after.principal, 0n, "BasketPrincipal was not cleared on dissolve");
  assert.equal(after.ownerOwed, 0n, "owner owed RPC was not cleared on dissolve");
  assert.equal(after.lateOwed, 0n, "late owed RPC was not cleared on dissolve");
  assert.ok(lateRootGain > 0n, "late/current root staker did not receive any dissolved basket value");
  assert.ok(
    lateRootGain * 2n > ownerRootGain,
    "late/current root staker did not receive the majority of dissolved value despite having the majority current root stake"
  );
  console.log("dissolve redistribution risk reproduced: late/current root staker received dissolved basket value");
}

async function readBasketState(netuid) {
  const [principal, escrowAlpha, ownerOwed, validatorNav, totalNav, basket] = await Promise.all([
    basketPrincipal(netuid),
    escrowAlphaStake(netuid),
    rpcU64("betaBasket_getStakerOwed", [alice.address, null]),
    rpcU64("betaBasket_getValidatorNav", [validatorHotkey.address, null]),
    rpcU64("betaBasket_getTotalNav", [null]),
    rpcBasket(validatorHotkey.address),
  ]);
  return { principal, escrowAlpha, ownerOwed, validatorNav, totalNav, basket };
}

async function basketPrincipal(netuid) {
  return (await api.query.subtensorModule.basketPrincipal(validatorHotkey.address, netuid)).toBigInt();
}

async function escrowAlphaStake(netuid) {
  const escrow = await betaEscrowAccount();
  return readPairStake(validatorHotkey.address, escrow, netuid);
}

async function rootStakeOf(coldkey) {
  return readPairStake(validatorHotkey.address, coldkey, 0);
}

async function betaEscrowAccount() {
  const candidates = await api.query.subtensorModule.alphaV2.entries(validatorHotkey.address);
  const entry = candidates.find(([key]) => key.args[2].toNumber() !== 0 && key.args[1].toString() !== alice.address);
  if (entry) {
    return entry[0].args[1].toString();
  }

  const basket = await rpcBasket(validatorHotkey.address);
  assert.ok(basket.length === 0, "basket exists but escrow stake entry was not found");
  return alice.address;
}

async function readPairStake(hotkey, coldkey, netuid) {
  const [share, totalHotkeyStake, totalHotkeyShares] = await Promise.all([
    readAlphaShare(hotkey, coldkey, netuid),
    api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid),
    readTotalHotkeyShares(hotkey, netuid),
  ]);

  if (share.numerator === 0n || totalHotkeyStake.toBigInt() === 0n || totalHotkeyShares.numerator === 0n) {
    return 0n;
  }

  return fixedMulDivToBigInt(share, totalHotkeyStake.toBigInt(), totalHotkeyShares);
}

async function readAlphaShare(hotkey, coldkey, netuid) {
  if (api.query.subtensorModule?.alpha) {
    const legacyShare = await readOptionalStorage(api.query.subtensorModule.alpha, hotkey, coldkey, netuid);
    if (legacyShare) {
      return decodeFixedRational(legacyShare);
    }
  }

  const shareV2 = await api.query.subtensorModule.alphaV2(hotkey, coldkey, netuid);
  return decodeFixedRational(shareV2);
}

async function readTotalHotkeyShares(hotkey, netuid) {
  if (api.query.subtensorModule?.totalHotkeyShares) {
    const legacyShares = await readOptionalStorage(api.query.subtensorModule.totalHotkeyShares, hotkey, netuid);
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

async function readRootAlphaDividends(netuid) {
  return (await api.query.subtensorModule.rootAlphaDividendsPerSubnet(netuid, validatorHotkey.address)).toBigInt();
}

async function getSubnetAccountId(netuid) {
  const encoded = await api._rpcCore.provider.send("subnetInfo_getSubnetAccountId", [netuid, null]);
  const account = api.createType("Option<AccountId32>", Uint8Array.from(encoded));
  assert.ok(account.isSome, `subnet account id not found for netuid ${netuid}`);
  return account.unwrap().toString();
}

async function rootVersionKey() {
  if (api.query.subtensorModule.weightsVersionKey) {
    return (await api.query.subtensorModule.weightsVersionKey(0)).toBigInt();
  }
  if (api.query.subtensorModule.weightsVersionKeyV2) {
    return (await api.query.subtensorModule.weightsVersionKeyV2(0)).toBigInt();
  }
  return 0n;
}

async function currentBlockNumber() {
  const header = await api.rpc.chain.getHeader();
  return header.number.toNumber();
}

async function rpcBasket(hotkey) {
  const encoded = await api._rpcCore.provider.send("betaBasket_getValidatorBasket", [hotkey, null]);
  const bytes = rpcBytes(encoded);
  const decoded = api.createType("Vec<(u16,u64,u64)>", bytes);
  return decoded.map(([netuid, alpha, tao]) => ({
    netuid: netuid.toNumber(),
    alpha: alpha.toBigInt(),
    tao: tao.toBigInt(),
  }));
}

async function rpcU64(method, params) {
  const value = await api._rpcCore.provider.send(method, params);
  if (typeof value === "number") return BigInt(value);
  if (typeof value === "bigint") return value;
  if (typeof value === "string") {
    if (value.startsWith("0x")) {
      return api.createType("u64", value).toBigInt();
    }
    return BigInt(value);
  }
  return api.createType("u64", Uint8Array.from(value)).toBigInt();
}

function rpcBytes(value) {
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return Uint8Array.from(value);
  if (typeof value === "string" && value.startsWith("0x")) {
    return api.createType("Bytes", value).toU8a(true);
  }
  return Uint8Array.from(value);
}

async function submitAndWait(signer, txOrPromise, label) {
  const tx = await txOrPromise;
  console.log("submit:", label);
  return new Promise((resolve, reject) => {
    let unsubscribe;
    let settled = false;
    const timeout = setTimeout(() => {
      finish(reject, new Error(`${label} timed out after ${TX_TIMEOUT_MS}ms`));
    }, TX_TIMEOUT_MS);

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
            const [error] = event.data;
            finish(reject, new Error(`${label} failed: ${formatDispatchError(error)}`));
            return;
          }
        }
      }

      if (status.isFinalized) {
        console.log("finalized:", label, status.asFinalized.toString());
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
  throw new Error(`could not decode bigint from ${value}`);
}

function formatBasketState(state) {
  return [
    `principal=${state.principal}`,
    `escrow_alpha=${state.escrowAlpha}`,
    `owner_owed=${state.ownerOwed}`,
    `validator_nav=${state.validatorNav}`,
    `total_nav=${state.totalNav}`,
    `basket=${state.basket.map((row) => `${row.netuid}:${row.alpha}/${row.tao}`).join(",")}`,
  ].join(" ");
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
