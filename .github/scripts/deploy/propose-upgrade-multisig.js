// Mainnet runtime upgrade proposal. The CI key is one half of a 2-of-2
// deployment multisig (CI key + sudo multisig) which holds a
// SudoUncheckedSetCode proxy on the chain's sudo key. This script submits
// CI's approveAsMulti half; the triumvirate completes the other half via
// approve-upgrade-multisig.js / PolkadotJS Apps.
//
// Usage: CI_SEED=<mnemonic-or-uri> node propose-upgrade-multisig.js <wsUrl> <wasmPath> [blobOutPath]
const { ApiPromise, WsProvider, Keyring } = require("@polkadot/api");
const {
  cryptoWaitReady,
  createKeyMulti,
  encodeAddress,
  sortAddresses,
} = require("@polkadot/util-crypto");
const fs = require("fs");

const SS58Prefix = 42;

function getMultisigAddress(addresses, threshold) {
  // Address as a byte array.
  const multiAddress = createKeyMulti(addresses, threshold);

  // Convert byte array to SS58 encoding.
  const Ss58Address = encodeAddress(multiAddress, SS58Prefix);
  return Ss58Address;
}

async function main() {
  const wsUrl = process.argv[2];
  const wasmPath = process.argv[3];
  const blobFilePath = process.argv[4] || "./proxy_proxy_blob.hex";
  const seedPhrase = process.env.CI_SEED;

  if (!wsUrl || !wasmPath) {
    console.error(
      "Usage: CI_SEED=<seed> node propose-upgrade-multisig.js <wsUrl> <wasmPath> [blobOutPath]"
    );
    process.exit(1);
  }
  if (!seedPhrase) {
    console.error("CI_SEED environment variable is not set");
    process.exit(1);
  }

  await cryptoWaitReady();

  // Connect to the Substrate node
  const provider = new WsProvider(wsUrl);
  const api = await ApiPromise.create({ provider });

  // Create a keyring and add the private key (the wallet submitting the proposal)
  const keyring = new Keyring({ type: "sr25519" });
  const pair = keyring.addFromUri(seedPhrase); // CI's signing key
  const ciKeyAddress = pair.address;
  console.log(`CI key address (fee payer + multisig depositor): ${ciKeyAddress}`);

  // Grab the sudo key from the chain
  const sudoKey = (await api.query.sudo.key()).toString();

  // Grab multi-sig address using sudo and CI key
  const multisigAddress = getMultisigAddress(
    sortAddresses([ciKeyAddress, sudoKey]),
    2
  );

  // Verify the CI key has the correct permissions
  const proxies = (await api.query.proxy.proxies(sudoKey)).toJSON()[0];
  if (proxies.length === 0) {
    throw Error(`Sudo key ${sudoKey} does not have any proxies`);
  }
  let correctProxies = proxies.filter(
    (x) =>
      x.delegate === multisigAddress &&
      x.proxyType === "SudoUncheckedSetCode" &&
      x.delay === 0
  );
  if (correctProxies.length === 0) {
    throw Error(
      `Sudo key ${sudoKey} does not have a proxy matching the deployment multi-sig address ${multisigAddress}: ${JSON.stringify(
        proxies
      )}`
    );
  }

  // The CI key pays the extrinsic fee and the multisig deposit
  // (depositBase + threshold * depositFactor), so an unfunded key fails with
  // an opaque 1010 at submission time. Check up front and name the account.
  const account = await api.query.system.account(ciKeyAddress);
  const free = account.data.free.toBigInt();
  const depositBase = api.consts.multisig.depositBase.toBigInt();
  const depositFactor = api.consts.multisig.depositFactor.toBigInt();
  const required = depositBase + 2n * depositFactor;
  console.log(
    `CI key free balance: ${free} rao; multisig deposit needed: ${required} rao (+ fee)`
  );
  if (free < required) {
    throw Error(
      `CI key ${ciKeyAddress} has ${free} rao free but needs at least ` +
        `${required} rao for the multisig deposit plus fees — fund it and re-run`
    );
  }

  // Read the WASM file and convert it to hex string
  const wasm = fs.readFileSync(wasmPath).toString("hex");
  console.log(`WASM file size (hex): ${wasm.length / 2} bytes`);

  // Print the current spec version
  const specVersionBefore = api.runtimeVersion.specVersion.toNumber();
  console.log(`Spec version before: ${specVersionBefore}`);

  // Create the setCode call with hex bytes prefixed by '0x'
  const setCodeCall = api.tx.system.setCode(`0x${wasm}`);

  // Create the unchecked weight call
  const uncheckedWeightCall = api.tx.sudo.sudoUncheckedWeight(setCodeCall, {
    refTime: 50000000000,
    proofSize: 0,
  });

  // Wrap in a proxy call
  const proxyProxyCall = api.tx.proxy.proxy(sudoKey, null, uncheckedWeightCall);

  // Fetch the call hash (to provide to signatories for approval)
  const callHash = proxyProxyCall.method.hash.toHex();
  console.log(`Call hash for multi-sig: ${callHash}`);

  // Encode the unchecked weight call for the final asMulti as raw bytes
  const encodedBlob = proxyProxyCall.method.toHex();

  // Write the raw bytes to a file for GitHub artifact upload
  fs.writeFileSync(blobFilePath, encodedBlob);
  console.log(`Raw bytes of uncheckedWeight blob written to: ${blobFilePath}`);

  // Submit the multi-sig proposal on behalf of the multi-sig account, signed by the CI
  const multisigProposal = api.tx.multisig.approveAsMulti(
    // First signer doesn't need to upload the actual call data
    2, // 1. The number of required signatures (2 of 2)
    [sudoKey], // 2. List of other signatories in lexicographic order
    null, // 3. Timepoint (null for the first submission)
    callHash, // 4. The call hash to be executed (proxy call of runtime upgrade)
    { refTime: 50000000000, proofSize: 0 } // 5. Maximum weight as an object
  );

  console.log(
    `Submitting multi-sig proposal on behalf of ${multisigAddress}...`
  );

  // Sign the proposal with the CI wallet and gather all needed output details
  await multisigProposal.signAndSend(pair, async (result) => {
    console.log(`Current status is ${result.status}`);

    if (result.status.isInBlock) {
      const blockHash = result.status.asInBlock.toHex();
      console.log(`Transaction included at blockHash: ${blockHash}`);

      // Fetch the extrinsic index (timepoint)
      const block = await api.rpc.chain.getBlock(blockHash);
      const extrinsicIndex = block.block.extrinsics.findIndex(
        (ext) => ext.hash.toHex() === multisigProposal.hash.toHex()
      );

      // Fetch the block header to get the block number (height)
      const blockHeader = await api.rpc.chain.getHeader(blockHash);
      const blockHeight = blockHeader.number.toNumber();

      console.log(`Extrinsic index for timepoint: ${extrinsicIndex}`);
      console.log(`Block height: ${blockHeight}`);

      // Output all needed information for multi-sig approval
      const summary = `
## Multi-sig Transaction Details

| **Name** | **Value** |
| ---------| ----------|
| **Multi-sig Address** | \`${multisigAddress}\` |
| **Call Hash** | \`${callHash}\` |
| **Block Hash** | \`${blockHash}\` |
| **Block Height** | \`${blockHeight}\` |
| **Extrinsic Index** | \`${extrinsicIndex}\` |


## Info for Signers

### Call Hash
\`\`\`
${callHash}
\`\`\`

### Included in Block

| **Name** | **Value** |
| ---------| ----------|
| **Block Hash** | \`${blockHash}\` |
| **Block Height** | \`${blockHeight}\` |


### Max Weight
\`\`\`json
{
"refTime": 50000000000,
"proofSize": 0
}
\`\`\`

## Next Steps
1. First signer uses call hash, max weight, and null timepoint to approveAsMulti, noting the resulting timepoint by
checking Developer -> Chain info, selecting multi-sig, and entering the call hash to see the current expected timepoint.
3. Any interior signers use the call hash and the timepoint of the first signer to approveAsMulti.
4. Once everyone has signed, a signer uses the call hash and timepoint _of the first approval_ to \`asMulti\` the call.
Note that this should be Multi-sig > AsMulti > sudo > sudoUncheckedWeight > system > setCode, and should use the subtensor
blob artifact provided by the CI.
5. If everything worked, spec version will bump.
`;

      if (process.env.GITHUB_STEP_SUMMARY) {
        fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
      }

      // Machine-readable record for the release watcher and upgrade manifest
      const pendingRelease = {
        callHash,
        blockHash,
        blockHeight,
        extrinsicIndex,
        multisigAddress,
        ciAddress: ciKeyAddress,
        sudoKey,
        specVersionBefore,
      };
      fs.writeFileSync(
        "pending-release.json",
        JSON.stringify(pendingRelease, null, 2)
      );

      if (process.env.GITHUB_OUTPUT) {
        fs.appendFileSync(
          process.env.GITHUB_OUTPUT,
          `call_hash=${callHash}\nblock_height=${blockHeight}\nextrinsic_index=${extrinsicIndex}\n`
        );
      }
    } else if (result.status.isFinalized) {
      console.log(
        `Transaction finalized at blockHash ${result.status.asFinalized}`
      );
      process.exit(0);
    } else if (result.isError) {
      throw Error(`Transaction failed with error: ${result.status}`);
    }
  });
}

main().catch((error) => {
  console.error(error.stack);
  process.exit(1);
});
