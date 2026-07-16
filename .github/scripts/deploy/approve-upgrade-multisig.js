const { ApiPromise, WsProvider, Keyring } = require("@polkadot/api");
const { read } = require("read");
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

async function getSudoMultisigProposal(
  api,
  multisigProposal,
  signerNum,
  remainingSignatories,
  sudoThreshold,
  timePointAndBlockHeight
) {
  // Return the sudo multi-sig proposal for the given signer number
  if (signerNum === 0) {
    // First signer doesn't need to upload the actual call data
    // or the timepoint
    const sudoMultiApprove = api.tx.multisig.approveAsMulti(
      // We are the last signer
      sudoThreshold, // 1. The number of required signatures (2 of 2)
      remainingSignatories, // 2. List of other signatories in lexicographic order
      null, // 3. Timepoint (null for the first submission)
      multisigProposal.method.hash.toHex(), // 4. The call to be executed (proxy call of runtime upgrade)
      // Must cover the finalizing as_multi's declared weight: its own pinned
      // 60e9/10k max_weight argument plus multisig+proxy overhead (measured
      // ~65e9/21k on v431). Too-low values fail the executing approval with
      // MaxWeightTooLow.
      { refTime: 80_000_000_000, proofSize: 50_000 } // 5. Maximum weight as an object
    );

    return sudoMultiApprove;
  } else {
    if (signerNum > 0 && signerNum < sudoThreshold - 1) {
      // Next signers don't need the call data, but do need the timepoint
      const sudoMultiApprove = api.tx.multisig.approveAsMulti(
        // We are the last signer
        sudoThreshold, // 1. The number of required signatures (2 of 2)
        remainingSignatories, // 2. List of other signatories in lexicographic order
        timePointAndBlockHeight, // 3. Timepoint (null for the first submission)
        multisigProposal.method.hash.toHex(), // 4. The call to be executed (proxy call of runtime upgrade)
        { refTime: 80_000_000_000, proofSize: 50_000 } // 5. Maximum weight as an object
      );

      return sudoMultiApprove;
    } else {
      // Last signer needs the call data and timepoint
      const sudoMultiApprove = api.tx.multisig.asMulti(
        // We are the last signer
        sudoThreshold, // 1. The number of required signatures (2 of 2)
        remainingSignatories, // 2. List of other signatories in lexicographic order
        timePointAndBlockHeight, // 3. Timepoint (null for the first submission)
        multisigProposal, // 4. The call to be executed (proxy call of runtime upgrade)
        // The executing approval: max_weight must cover the finalizing
        // as_multi's declared weight (its pinned 60e9/10k argument plus
        // multisig+proxy overhead — measured ~65e9/21k on v431), or the
        // chain rejects it with MaxWeightTooLow. The 60e9/10k literal at
        // the deploymentMultisigProposal below is different: it is *inside*
        // the finalizing call's encoding and must never change.
        { refTime: 80_000_000_000, proofSize: 50_000 } // 5. Maximum weight as an object
      );

      return sudoMultiApprove;
    }
  }
}

async function ask_for_seed_phrase() {
  const seedPhrase = await read({
    prompt: "mnemonic: ",
    silent: true,
    replace: "*"
  });
  return seedPhrase;
}

async function main(sudoSignatories, sudoThreshold) {
  const wsUrl = process.argv[2]; // Substrate node URL
  const ciKey = process.argv[3]; // CI key public address
  const signerNum = parseInt(process.argv[4]); // Signer number (which signer in-order)
  const deployHeight = parseInt(process.argv[5]); // Block height of deployment multisig approval
  const deployIndex = parseInt(process.argv[6]); // Index of deployment multisig approval

  const deployTimepoint = { height: deployHeight, index: deployIndex };

  let index, height, blobPath, seedPhrase;
  if (signerNum === 0) {
    // First signer doesn't need timepoint or blob
    height = null;
    index = null;
    blobPath = process.argv[7]; // Path to tx blob file
    seedPhrase = await ask_for_seed_phrase(); // Signer's seed phrase
  } else {
    height = parseInt(process.argv[7]); // Timepoint height
    index = parseInt(process.argv[8]); // Timepoint index
    blobPath = process.argv[9]; // Path to tx blob file
    seedPhrase = await ask_for_seed_phrase(); // Signer's seed phrase
  }

  const timepoint = { index, height };

  await cryptoWaitReady();

  // Connect to the Substrate node
  const provider = new WsProvider(wsUrl);
  const api = await ApiPromise.create({ provider });

  // Create a keyring and add the private key (the wallet submitting the proposal)
  const keyring = new Keyring({ type: "sr25519" });
  const pair = keyring.addFromUri(seedPhrase); // CI's signing key

  // CI's address
  const ciKeyAddress = ciKey;

  // Grab the sudo key from the chain
  const sudoKey = (await api.query.sudo.key()).toString();
  const sudoMulti = getMultisigAddress(
    sortAddresses(sudoSignatories),
    sudoThreshold
  );
  if (sudoMulti !== sudoKey) {
    throw Error("Sudo multi-sig address does not match the expected address");
  }

  // Read the txBlob file and convert it to hex string
  const txBlobHex = fs.readFileSync(blobPath).toString("utf8");
  const txBlob = Buffer.from(txBlobHex.replace(/^0x/, ""), "hex");
  console.log(`txBlob file size (hex): ${txBlob.length / 2} bytes`); // Display file size in bytes

  // Convert txBlob into call
  const call = api.registry.createType("Call", txBlob);

  const remainingSignatories = sudoSignatories.filter(
    (signatory) => signatory !== pair.address
  );

  // Submit a final multi-sig approval, with call-data, on behalf of the deployment multi-sig
  const deploymentMultisigProposal = api.tx.multisig.asMulti(
    // We are the last signer
    2, // 1. The number of required signatures (2 of 2)
    [ciKeyAddress], // 2. List of other signatories in lexicographic order
    deployTimepoint, // 3. Timepoint (null for the first submission)
    call, // 4. The call to be executed (proxy call of runtime upgrade)
    { refTime: 60_000_000_000, proofSize: 10_000 } // 5. Maximum weight as an object
  );

  // Write to file
  fs.writeFileSync(
    "deployment-multisig-proposal.hex",
    deploymentMultisigProposal.method.toHex()
  );

  const multisigProposal = await getSudoMultisigProposal(
    api,
    deploymentMultisigProposal,
    signerNum,
    remainingSignatories,
    sudoThreshold,
    timepoint
  );

  console.log(`Submitting multi-sig proposal on behalf of ${sudoMulti}...`);

  // Sign the proposal with the CI wallet and gather all needed output details
  const unsub = await multisigProposal.signAndSend(pair, async (result) => {
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
      const blockHeight = blockHeader.number.toNumber(); // Extract the block height

      console.log(`Extrinsic index for timepoint: ${extrinsicIndex}`);
      console.log(`Block height: ${blockHeight}`);

      const callHash = multisigProposal.method.hash.toHex();

      // Output all needed information for multi-sig approval
      const summary = `
## Multi-sig Transaction Details

| **Name** | **Value** |
| ---------| ----------|
| **Multi-sig Address** | \`${sudoMulti}\` |
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
4. Once everyone has signed, a signer uses the call data from the outputted file \"deployment-multisig-proposal.hex\" and timepoint _of the first approval_ to \`asMulti\` the call.
Note: steps 3 and 4 can be done on the PolkadotJS Apps (Oracle) under the multi-sig address.
5. If everything worked, spec version will bump.
`;

      if (process.env.GITHUB_STEP_SUMMARY) {
        fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
      }
    } else if (result.status.isFinalized) {
      console.log(
        `Transaction finalized at blockHash ${result.status.asFinalized}`
      );
      process.exit(0); // Exit with success
    } else if (result.isError) {
      throw Error(`Transaction failed with error: ${result.status}`);
    }
  });
}

module.exports = { main };
