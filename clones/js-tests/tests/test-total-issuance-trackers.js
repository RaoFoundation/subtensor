import assert from "node:assert/strict";

import { Keyring } from "@polkadot/api";
import { hexToU8a, u8aConcat, u8aToHex } from "@polkadot/util";
import { blake2AsU8a, encodeAddress } from "@polkadot/util-crypto";
import { ethers } from "ethers";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const ETH_RPC_ENDPOINT = process.env.ETH_RPC_ENDPOINT ?? "http://127.0.0.1:9944";
const RUN_ID = process.env.TOTAL_ISSUANCE_RUN_ID ?? `run${Date.now()}p${process.pid}`;
const FUND_SOURCE_URI = process.env.TOTAL_ISSUANCE_FUND_SOURCE_URI ?? "//Alice";
const FUND_AMOUNT = BigInt(process.env.TOTAL_ISSUANCE_FUND_AMOUNT ?? "5000000000000");
const STAKE_AMOUNT = BigInt(process.env.TOTAL_ISSUANCE_STAKE_AMOUNT ?? "10000000000");
const TRANSFER_AMOUNT = BigInt(process.env.TOTAL_ISSUANCE_TRANSFER_AMOUNT ?? "1000000000");
const NEURON_BURN = BigInt(process.env.TOTAL_ISSUANCE_NEURON_BURN ?? "1000000");
const NETWORK_LOCK_COST = BigInt(process.env.TOTAL_ISSUANCE_NETWORK_LOCK_COST ?? "1000000000");
const MAX_PRICE = 18_446_744_073_709_551_615n;
const MIN_PRICE = 0n;
const SS58_PREFIX = Number(process.env.SS58_PREFIX ?? 42);
const SIMPLE_RETURN_42_BYTECODE = "0x600a600c600039600a6000f3602a60005260206000f3";
const I96F32_ZERO_STORAGE = `0x${"00".repeat(16)}`;
const I96F32_ONE_STORAGE = `0x${"00".repeat(4)}01000000${"00".repeat(8)}`;

const keyring = new Keyring({ type: "sr25519" });
const fundSource = keyring.addFromUri(FUND_SOURCE_URI);
const stakeColdkey = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//stake-coldkey`);
const stakeDest = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//stake-destination`);
const burnColdkey = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//burn-coldkey`);
const burnHotkey = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//burn-hotkey`);
const subnetOwner = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//subnet-owner`);
const subnetOwnerHotkey = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//subnet-owner-hotkey`);
const replacementOwner = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//replacement-owner`);
const replacementHotkey = keyring.addFromUri(`//TotalIssuanceTrackers//${RUN_ID}//replacement-hotkey`);
const logger = createTempLogger("test-total-issuance-trackers.log");
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

    assertMetadataAvailable();
    await repairIssuanceMirrorIfNeeded("pre-test setup");
    await fundTestAccounts();
    await assertIssuanceMatch("initial");

    const { netuid, hotkey } = await findTransferEnabledSubnet();
    console.log("working subnet:", netuid);
    console.log("working hotkey:", hotkey);

    await exerciseStakeUnstakeBalanceTransferAndStakeTransfer(netuid, hotkey);
    await exerciseBurnedRegistration(netuid);
    await exerciseSubnetDeregistrationByRegistration();
    await exerciseEvmContractFees();

    await assertIssuanceMatch("final");
    console.log("total issuance tracker scenarios: ok");
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

function assertMetadataAvailable() {
  const missing = [
    ["Balances.TotalIssuance", api.query.balances?.totalIssuance],
    ["SubtensorModule.TotalIssuance", api.query.subtensorModule?.totalIssuance],
    ["Sudo.sudo", api.tx.sudo?.sudo],
    ["System.setStorage", api.tx.system?.setStorage],
    ["Utility.batch", api.tx.utility?.batch],
    ["Balances.transfer", api.tx.balances?.transferKeepAlive ?? api.tx.balances?.transferAllowDeath ?? api.tx.balances?.transfer],
    ["SubtensorModule.addStake", api.tx.subtensorModule?.addStake],
    ["SubtensorModule.removeStakeLimit", api.tx.subtensorModule?.removeStakeLimit],
    ["SubtensorModule.transferStake", api.tx.subtensorModule?.transferStake],
    ["SubtensorModule.burnedRegister", api.tx.subtensorModule?.burnedRegister],
    ["SubtensorModule.registerNetwork", api.tx.subtensorModule?.registerNetwork],
    ["SubtensorModule.SubnetLimit", api.query.subtensorModule?.subnetLimit],
    ["SubtensorModule.NetworkRegisteredAt", api.query.subtensorModule?.networkRegisteredAt],
    ["SubtensorModule.NetworkRegistrationStartBlock", api.query.subtensorModule?.networkRegistrationStartBlock],
    ["SubtensorModule.NetworkImmunityPeriod", api.query.subtensorModule?.networkImmunityPeriod],
    ["SubtensorModule.NetworkRateLimit", api.query.subtensorModule?.networkRateLimit],
    ["SubtensorModule.NetworkMinLockCost", api.query.subtensorModule?.networkMinLockCost],
    ["SubtensorModule.NetworkLastLockCost", api.query.subtensorModule?.networkLastLockCost],
    ["SubtensorModule.NetworksAdded", api.query.subtensorModule?.networksAdded],
    ["SubtensorModule.SubnetMovingPrice", api.query.subtensorModule?.subnetMovingPrice],
    ["SubtensorModule.Keys", api.query.subtensorModule?.keys],
    ["SubtensorModule.Burn", api.query.subtensorModule?.burn],
    ["SubtensorModule.MinBurn", api.query.subtensorModule?.minBurn],
    ["SubtensorModule.MaxBurn", api.query.subtensorModule?.maxBurn],
    ["EVM.DisableWhitelistCheck", api.query.evm?.disableWhitelistCheck],
    ["EVM.disableWhitelist", api.tx.evm?.disableWhitelist],
  ].filter(([, value]) => !value);

  assert.equal(
    missing.length,
    0,
    `${missing.map(([name]) => name).join(", ")} unavailable; run after upgrading the clone to the current runtime`
  );
}

async function fundTestAccounts() {
  const evmWallet = getEvmWallet();
  const accounts = [
    [stakeColdkey.address, "stake coldkey"],
    [stakeDest.address, "stake destination"],
    [burnColdkey.address, "burn coldkey"],
    [subnetOwner.address, "subnet owner"],
    [replacementOwner.address, "replacement owner"],
    [evmAddressToSs58(evmWallet.address), "EVM mapped account"],
  ];
  const calls = accounts.map(([address]) => balancesTransfer(address, FUND_AMOUNT));
  await signedBatch(fundSource, calls, "batch transfer test funding");
  await assertIssuanceMatch("after setup funding transfers");

  for (const [address, label] of accounts) {
    const free = (await api.query.system.account(address)).data.free.toBigInt();
    assert.ok(free >= FUND_AMOUNT, `${label} funding failed: free=${free}`);
    console.log(`${label} funded:`, address, free.toString());
  }
}

async function exerciseStakeUnstakeBalanceTransferAndStakeTransfer(netuid, hotkey) {
  const alphaBefore = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  const addResult = await submitAndWait(
    stakeColdkey,
    api.tx.subtensorModule.addStake(hotkey, netuid, STAKE_AMOUNT),
    "add stake"
  );
  const alphaAdded = stakeAddedFromEvents(addResult.events, hotkey, netuid);
  assert.ok(alphaAdded > 0n, "addStake should mint alpha for the tested coldkey/hotkey pair");
  await assertIssuanceMatch("after addStake");

  const unstakeAmount = alphaAdded / 4n;
  assert.ok(unstakeAmount > 0n, "not enough added alpha to unstake");
  await submitAndWait(
    stakeColdkey,
    api.tx.subtensorModule.removeStakeLimit(hotkey, netuid, unstakeAmount, MIN_PRICE, false),
    "remove stake limit"
  );
  await assertIssuanceMatch("after removeStakeLimit");

  await submitAndWait(
    stakeColdkey,
    balancesTransfer(stakeDest.address, TRANSFER_AMOUNT),
    "balance transfer"
  );
  await assertIssuanceMatch("after balances transfer");

  const alphaAfterUnstake = (await api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid)).toBigInt();
  assert.ok(alphaAfterUnstake < alphaBefore + alphaAdded, "unstake should reduce total hotkey alpha");
  const transferStakeAmount = alphaAdded / 3n;
  assert.ok(transferStakeAmount > 0n, "not enough added alpha to transfer stake");
  await submitAndWait(
    stakeColdkey,
    api.tx.subtensorModule.transferStake(stakeDest.address, hotkey, netuid, netuid, transferStakeAmount),
    "same-subnet transferStake"
  );
  await assertIssuanceMatch("after transferStake");

  console.log(
    "stake/unstake/transfer/transferStake scenario:",
    `alphaBefore=${alphaBefore}`,
    `alphaAdded=${alphaAdded}`,
    `unstaked=${unstakeAmount}`,
    `stakeTransferred=${transferStakeAmount}`
  );
}

async function exerciseBurnedRegistration(netuid) {
  await setBurnBounds(netuid, NEURON_BURN);
  await submitAndWait(
    burnColdkey,
    api.tx.subtensorModule.burnedRegister(netuid, burnHotkey.address),
    "burned neuron registration"
  );
  await assertIssuanceMatch("after burnedRegister");

  const registered = await api.query.subtensorModule.uids(netuid, burnHotkey.address);
  assert.ok(registered.isSome, `burn hotkey ${burnHotkey.address} was not registered on netuid ${netuid}`);
  console.log("burned registration scenario:", `netuid=${netuid}`, `hotkey=${burnHotkey.address}`, `burn=${NEURON_BURN}`);
}

async function exerciseSubnetDeregistrationByRegistration() {
  const originalLimit = (await api.query.subtensorModule.subnetLimit()).toNumber();
  const originalNetworkRateLimit = (await api.query.subtensorModule.networkRateLimit()).toBigInt();
  const originalRegistrationStartBlock = (await api.query.subtensorModule.networkRegistrationStartBlock()).toBigInt();
  const originalNetworkImmunityPeriod = (await api.query.subtensorModule.networkImmunityPeriod()).toBigInt();
  const originalMinLockCost = (await api.query.subtensorModule.networkMinLockCost()).toBigInt();
  const originalLastLockCost = (await api.query.subtensorModule.networkLastLockCost()).toBigInt();
  const initialActiveCount = await countNonRootSubnets();

  try {
    await sudoSetStorage(
      [
        [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", initialActiveCount + 1)],
        [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkRegistrationStartBlock.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", 0n)],
        [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", NETWORK_LOCK_COST)],
        [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", NETWORK_LOCK_COST)],
      ],
      "sudo enable register_network for test"
    );

    const firstNetuid = await registerSubnet(subnetOwner, subnetOwnerHotkey, "subnet registration before prune");
    await assertIssuanceMatch("after initial test subnet registration");

    const activeNetuids = await activeNonRootNetuids();
    const activeCount = activeNetuids.length;
    const pruneBlock = 0n;
    await sudoSetStorage(
      [
        [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", activeCount)],
        [api.query.subtensorModule.networkRegisteredAt.key(firstNetuid), storageValueHex("u64", pruneBlock)],
        [api.query.subtensorModule.subnetMovingPrice.key(firstNetuid), I96F32_ZERO_STORAGE],
        ...activeNetuids
          .filter((netuid) => netuid !== firstNetuid)
          .map((netuid) => [api.query.subtensorModule.subnetMovingPrice.key(netuid), I96F32_ONE_STORAGE]),
      ],
      "sudo move test subnet to front of deregistration line"
    );

    assert.equal((await api.query.subtensorModule.subnetLimit()).toNumber(), activeCount, "SubnetLimit was not set");
    assert.equal(
      (await api.query.subtensorModule.networkRegisteredAt(firstNetuid)).toBigInt(),
      pruneBlock,
      "NetworkRegisteredAt was not moved for pruning"
    );

    const replacementNetuid = await registerSubnet(replacementOwner, replacementHotkey, "replacement subnet registration");
    assert.equal(replacementNetuid, firstNetuid, "replacement registration should reuse the pruned netuid");
    await assertIssuanceMatch("after subnet prune and replacement registration");

    console.log("subnet deregistration scenario:", `prunedAndReusedNetuid=${replacementNetuid}`);
  } finally {
    await sudoSetStorage(
      [
        [api.query.subtensorModule.subnetLimit.key(), storageValueHex("u16", originalLimit)],
        [api.query.subtensorModule.networkRateLimit.key(), storageValueHex("u64", originalNetworkRateLimit)],
        [
          api.query.subtensorModule.networkRegistrationStartBlock.key(),
          storageValueHex("u64", originalRegistrationStartBlock),
        ],
        [api.query.subtensorModule.networkImmunityPeriod.key(), storageValueHex("u64", originalNetworkImmunityPeriod)],
        [api.query.subtensorModule.networkMinLockCost.key(), storageValueHex("u64", originalMinLockCost)],
        [api.query.subtensorModule.networkLastLockCost.key(), storageValueHex("u64", originalLastLockCost)],
      ],
      "sudo restore subnet registration test settings"
    );
  }
}

async function exerciseEvmContractFees() {
  await ensureEvmWhitelistDisabled();
  const wallet = getEvmWallet();
  const provider = new ethers.JsonRpcProvider(ETH_RPC_ENDPOINT);
  const connectedWallet = wallet.connect(provider);
  await provider.getBlockNumber();
  await assertIssuanceMatch("before EVM deployment");

  const factory = new ethers.ContractFactory([], SIMPLE_RETURN_42_BYTECODE, connectedWallet);
  const contract = await factory.deploy({ gasLimit: 150_000 });
  const deployReceipt = await contract.deploymentTransaction().wait();
  assert.equal(deployReceipt.status, 1, "contract deployment failed");
  await assertIssuanceMatch("after EVM contract deployment");

  const contractAddress = await contract.getAddress();
  for (let index = 1; index <= 3; index++) {
    const tx = await connectedWallet.sendTransaction({
      to: contractAddress,
      data: "0x",
      gasLimit: 50_000,
    });
    const receipt = await tx.wait();
    assert.equal(receipt.status, 1, `EVM contract call ${index} failed`);
    await assertIssuanceMatch(`after EVM contract call ${index}`);
  }

  console.log("EVM contract fee scenario:", `contract=${contractAddress}`, `deployer=${wallet.address}`);
}

async function registerSubnet(owner, hotkey, label) {
  const result = await submitAndWait(owner, api.tx.subtensorModule.registerNetwork(hotkey.address), label);
  const event = result.events.find(
    ({ event }) => event.section === "subtensorModule" && event.method === "NetworkAdded"
  );
  assert.ok(event, `${label} did not emit NetworkAdded`);
  const netuid = event.event.data[0].toNumber();
  assert.equal((await api.query.subtensorModule.networksAdded(netuid)).isTrue, true, `${netuid} was not added`);
  console.log(`${label}:`, `netuid=${netuid}`, `owner=${owner.address}`, `hotkey=${hotkey.address}`);
  return netuid;
}

async function setBurnBounds(netuid, amount) {
  await sudoSetStorage(
    [
      [api.query.subtensorModule.minBurn.key(netuid), storageValueHex("u64", amount)],
      [api.query.subtensorModule.maxBurn.key(netuid), storageValueHex("u64", amount)],
      [api.query.subtensorModule.burn.key(netuid), storageValueHex("u64", amount)],
    ],
    `sudo set burn bounds for netuid ${netuid}`
  );
  const burn = (await api.query.subtensorModule.burn(netuid)).toBigInt();
  assert.equal(burn, amount, `Burn(${netuid}) was not updated`);
}

async function ensureEvmWhitelistDisabled() {
  const disabled = await api.query.evm.disableWhitelistCheck();
  if (disabled.isTrue) {
    console.log("EVM whitelist check already disabled");
    return;
  }
  await submitAndWait(fundSource, api.tx.sudo.sudo(api.tx.evm.disableWhitelist(true)), "sudo disable EVM whitelist");
  assert.equal((await api.query.evm.disableWhitelistCheck()).isTrue, true, "EVM whitelist did not disable");
  console.log("EVM whitelist check disabled");
}

async function repairIssuanceMirrorIfNeeded(label) {
  for (let attempt = 1; attempt <= 3; attempt++) {
    const balances = (await api.query.balances.totalIssuance()).toBigInt();
    const subtensor = (await api.query.subtensorModule.totalIssuance()).toBigInt();
    const diff = balances - subtensor;
    if (diff === 0n) {
      console.log(`${label}: issuance matched`, balances.toString());
      return;
    }

    const target = balances + diff;
    assert.ok(target > 0n, `cannot repair issuance mirror: computed target ${target}`);
    await submitAndWait(
      fundSource,
      api.tx.sudo.sudo(api.tx.system.setStorage([
        [api.query.subtensorModule.totalIssuance.key(), storageValueHex("u64", target)],
      ])),
      `sudo repair Subtensor TotalIssuance mirror attempt ${attempt}`
    );
  }

  await assertIssuanceMatch(`${label} repaired`);
}

async function assertIssuanceMatch(label) {
  const [balancesIssuance, subtensorIssuance] = await Promise.all([
    api.query.balances.totalIssuance(),
    api.query.subtensorModule.totalIssuance(),
  ]);
  const balances = balancesIssuance.toBigInt();
  const subtensor = subtensorIssuance.toBigInt();
  assert.equal(
    subtensor,
    balances,
    `${label}: SubtensorModule.TotalIssuance ${subtensor} does not match Balances.TotalIssuance ${balances}`
  );
  console.log(`${label}: issuance match`, balances.toString());
}

async function findTransferEnabledSubnet() {
  const networkEntries = await api.query.subtensorModule.networksAdded.entries();
  for (const [key, added] of networkEntries) {
    if (!added.isTrue) continue;
    const netuid = key.args[0].toNumber();
    if (netuid === 0) continue;
    if ((await api.query.subtensorModule.transferToggle(netuid)).isFalse) continue;
    const keys = await api.query.subtensorModule.keys.entries(netuid);
    if (keys.length === 0) continue;
    return { netuid, hotkey: keys[0][1].toString() };
  }
  throw new Error("no initialized transfer-enabled subnet with at least one hotkey found");
}

async function countNonRootSubnets() {
  return (await activeNonRootNetuids()).length;
}

async function activeNonRootNetuids() {
  const entries = await api.query.subtensorModule.networksAdded.entries();
  return entries
    .filter(([key, added]) => key.args[0].toNumber() !== 0 && added.isTrue)
    .map(([key]) => key.args[0].toNumber());
}

async function sudoBatch(calls, label) {
  const batched = api.tx.utility.batchAll ? api.tx.utility.batchAll(calls) : api.tx.utility.batch(calls);
  await submitAndWait(fundSource, api.tx.sudo.sudo(batched), label);
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

function balancesTransfer(dest, amount) {
  if (api.tx.balances.transferKeepAlive) {
    return api.tx.balances.transferKeepAlive(dest, amount);
  }
  if (api.tx.balances.transferAllowDeath) {
    return api.tx.balances.transferAllowDeath(dest, amount);
  }
  return api.tx.balances.transfer(dest, amount);
}

function stakeAddedFromEvents(events, hotkey, netuid) {
  const event = events.find(({ event }) => {
    if (event.section !== "subtensorModule" || event.method !== "StakeAdded") return false;
    const [, eventHotkey, , alphaStaked, eventNetuid] = event.data;
    return eventHotkey.toString() === hotkey && eventNetuid.toNumber() === netuid && alphaStaked.toBigInt() > 0n;
  });
  assert.ok(event, `StakeAdded event not found for hotkey ${hotkey} on netuid ${netuid}`);
  return event.event.data[3].toBigInt();
}

function getEvmWallet() {
  return new ethers.Wallet(ethers.id(`total-issuance-trackers-${RUN_ID}`));
}

function evmAddressToSs58(address) {
  const addressBytes = hexToU8a(address.startsWith("0x") ? address : `0x${address}`);
  const accountId = blake2AsU8a(u8aConcat(new TextEncoder().encode("evm:"), addressBytes));
  return encodeAddress(accountId, SS58_PREFIX);
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
