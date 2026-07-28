// Direct sudo runtime upgrade. Used for chains where CI holds the sudo key
// (testnet, local mainnet clones).
//
// Usage: DEPLOY_SEED=<mnemonic-or-uri> node deploy-wasm.js <wsUrl> <wasmPath>
const { ApiPromise, WsProvider, Keyring } = require("@polkadot/api");
const fs = require("fs");

async function main() {
  const wsUrl = process.argv[2];
  const wasmPath = process.argv[3];
  const seedPhrase = process.env.DEPLOY_SEED;

  if (!wsUrl || !wasmPath) {
    console.error("Usage: DEPLOY_SEED=<seed> node deploy-wasm.js <wsUrl> <wasmPath>");
    process.exit(1);
  }
  if (!seedPhrase) {
    console.error("DEPLOY_SEED environment variable is not set");
    process.exit(1);
  }

  // Connect to the Substrate node
  const provider = new WsProvider(wsUrl);
  const api = await ApiPromise.create({ provider });

  // Create a keyring and add the private key
  const keyring = new Keyring({ type: "sr25519" });
  const pair = keyring.addFromUri(seedPhrase);

  // Check account balance
  const {
    data: { free: balance },
  } = await api.query.system.account(pair.address);
  console.log(`Balance of ${pair.address}: ${balance}`);

  if (balance.isZero()) {
    console.error(
      "Account balance is zero. Please ensure the correct key is used and the account has sufficient funds."
    );
    process.exit(1);
  }

  // Read the WASM file
  const wasm = fs.readFileSync(wasmPath).toString("hex");
  console.log(`WASM file size (hex): ${wasm.length / 2} bytes`);

  // Print the current spec version
  const specVersionBefore = api.runtimeVersion.specVersion.toNumber();
  console.log(`Spec version before: ${specVersionBefore}`);

  const setCodeCall = api.tx.system.setCode(`0x${wasm}`);
  const uncheckedWeightCall = api.tx.sudo.sudoUncheckedWeight(setCodeCall, {
    refTime: 0,
    proofSize: 0,
  });
  const sudoCall = api.tx.sudo.sudo(uncheckedWeightCall);

  await sudoCall.signAndSend(pair, async (result) => {
    console.log(`Current status is ${result.status}`);

    if (result.status.isInBlock) {
      console.log(`Transaction included at blockHash ${result.status.asInBlock}`);
    } else if (result.status.isFinalized) {
      console.log(`Transaction finalized at blockHash ${result.status.asFinalized}`);

      await api.rpc.system.syncState();
      const specVersionAfter = api.runtimeVersion.specVersion.toNumber();
      console.log(`Spec version after: ${specVersionAfter}`);
      process.exit(0);
    } else if (result.isError) {
      console.error(`Transaction failed with error: ${result.status}`);
      process.exit(1);
    }
  });
}

main().catch((error) => {
  console.error(`Unhandled error: ${error.message}`);
  process.exit(1);
});
