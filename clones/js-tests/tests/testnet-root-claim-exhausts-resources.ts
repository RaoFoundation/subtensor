import assert from "node:assert/strict";
import { ApiPromise, Keyring, WsProvider } from "@polkadot/api";
import { cryptoWaitReady } from "@polkadot/util-crypto";
import { createTempLogger } from "../lib/file-log.js";

const logger = createTempLogger("testnet-root-claim-exhausts-resources.log");
logger.captureConsole();

async function main() {
  await logger.start();
  await cryptoWaitReady();
  const provider = new WsProvider("wss://test.finney.opentensor.ai:443");
  const api = await ApiPromise.create({ provider });
  try {
    await api.isReady;
    const hash = await api.rpc.chain.getFinalizedHead();
    const at = await api.at(hash);
    console.log("UTC:", new Date().toISOString());
    console.log("finalized hash:", hash.toHex());
    console.log("header:", (await api.rpc.chain.getHeader(hash)).toString());
    console.log("genesis:", api.genesisHash.toHex());
    console.log("runtime:", (await api.rpc.state.getRuntimeVersion(hash)).toString());
    console.log("block weights:", at.consts.system.blockWeights.toString());
    const keyring = new Keyring({ type: "sr25519" });
    let signer = keyring.addFromUri("//TestnetFunded");
    for (const uri of ["//TestnetFunded", "//Alice", "//Bob"]) {
      const candidate = keyring.addFromUri(uri);
      const state = await at.query.system.account(candidate.address);
      console.log("candidate:", candidate.address, state.toString());
      if (!state.data.free.isZero()) {
        signer = candidate;
        break;
      }
    }
    const account = await at.query.system.account(signer.address);
    console.log("account:", signer.address, account.toString());
    const methods = await api.rpc.rpc.methods();
    console.log("dry run available:", methods.methods.some((m) => m.toString() === "system_dryRun"));
    const tx = api.tx.subtensorModule.claimRootWithHotkey(signer.address);
    console.log("call:", tx.method.toHex());
    console.log("payment info:", (await tx.paymentInfo(signer)).toString());
    await tx.signAsync(signer, { nonce: account.nonce, era: 0 });
    // Execute against finalized live state without broadcasting or charging fees.
    // The API decorator treats the opaque extrinsic as Bytes and adds another length.
    // Runtime API input is SCALE(TransactionSource, Extrinsic, BlockHash).
    const input = `0x02${tx.toHex().slice(2)}${hash.toHex().slice(2)}`;
    const output = await api.rpc.state.call("TaggedTransactionQueue_validate_transaction", input, hash);
    const result = api.registry.createType("TransactionValidity", output);
    console.log("runtime transaction validation:", result.toString());
    assert.ok(result.isErr, "expected transaction validity failure");
    assert.ok(result.asErr.isInvalid, "expected Invalid transaction");
    assert.ok(result.asErr.asInvalid.isExhaustsResources, "expected ExhaustsResources");
    console.log("REPRODUCED: Invalid: ExhaustsResources (no transaction broadcast)");
  } finally {
    await api.disconnect();
  }
}

main().catch(async (err) => {
  await logger.error(err);
  process.exitCode = 1;
});
