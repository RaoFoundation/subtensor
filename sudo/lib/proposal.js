"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { sortAddresses } = require("@polkadot/util-crypto");

function display(value) {
  if (typeof value === "bigint") return value.toString();
  return JSON.stringify(value);
}

function changesMarkdown(diff) {
  const lines = [
    "| Parameter | Live value | Proposed value | Setter |",
    "| --- | ---: | ---: | --- |",
    ...diff.changed.map((entry) => `| \`${entry.key}\` | \`${display(entry.chain)}\` | \`${display(entry.desired)}\` | \`${entry.callPallet}.${entry.call}\` |`),
  ];
  if (!diff.changed.length) lines.push("| _No proposed changes_ | | | |");
  if (diff.drift.length) {
    lines.push("", "### Unrelated live drift (excluded from the proposal)", "",
      "These values were not edited in the PR. Review them separately; they are not part of the batch.", "");
    for (const entry of diff.drift) {
      lines.push(`- \`${entry.key}\`: file \`${display(entry.desired)}\`; live \`${display(entry.chain)}\``);
    }
  }
  return lines.join("\n");
}

function signerConfiguration() {
  const file = path.join(__dirname, "..", "..", ".github", "scripts", "deploy", "sudo-signatories.json");
  const { signatories, threshold } = JSON.parse(fs.readFileSync(file, "utf8"));
  if (!Array.isArray(signatories) || !Number.isInteger(threshold) || threshold < 2 ||
      threshold > signatories.length || new Set(signatories).size !== signatories.length) {
    throw new Error("Invalid triumvirate signer configuration");
  }
  return { signatories: sortAddresses(signatories), threshold };
}

function signerCommand(network) {
  return `SUDO_WALLET=<WALLET> python3 ${network}-countersign.py ${network}-proposal.json`;
}

function renderMarkdown(proposal, file, network) {
  const lines = [
    `# ${network} sudo parameter proposal`, "",
    "**Proposal only. CI has not signed or submitted any extrinsic.**", "",
    `Source file: \`${file}\``,
    `Finalized block: \`${proposal.context.blockNumber}\` (\`${proposal.context.blockHash}\`)`,
    `Runtime spec version: \`${proposal.context.specVersion}\``, "",
    `Call: \`Sudo.sudo(Utility.batch_all(...))\``, "",
    ...changesMarkdown(proposal.diff).split("\n"), "",
  ];
  if (!proposal.encoded) {
    lines.push("No call data was produced because there are no proposed changes.");
    return lines.join("\n");
  }
  lines.push(
    "## Encoded call", "",
    `Call hash (BLAKE2-256): \`${proposal.encoded.callHash}\``, "",
    "```text", proposal.encoded.callData, "```", "",
    "Each setter was encoded and round-tripped against live runtime metadata.", "",
    "The accompanying `args.json` lists every batch member and its decoded arguments.",
    "The proposal snapshot is not a substitute for a fresh live-state check.", "",
  );
  if (network !== "mainnet") {
    lines.push(
      "## Apply status", "",
      "**TODO: devnet/testnet parameter application is not configured.**",
      "This artifact is proposal-only. A separate reviewed workflow may use the existing protected network environment and deployment credential, but no such apply job is enabled here.", "",
      "Do not paste a seed into a workflow or artifact. Keep application disabled by default until that separate authorization and review are complete.",
    );
    return lines.join("\n");
  }
  lines.push(
    "## Independent verification", "",
    "Re-run the diff from the reviewed PR. Compare the decoded calls and hash before signing.", "",
    "```bash",
    `node sudo/diff.js mainnet --base-file /path/to/base/mainnet.toml --output-dir /tmp/sudo-proposal`,
    "```", "",
    "## Triumvirate countersigning", "",
    `The script derives the ${proposal.signers.threshold}-of-${proposal.signers.signatories.length} legacy multisig and verifies it against live \`Sudo.Key\`.`,
    "It verifies the call hash, composes the same call from live metadata, and uses the repository's standard `btcli` multisig approval path.", "",
    "Run the following on your own trusted signer machine after reviewing the artifacts. Replace `<WALLET>` with your own local member wallet. No key is included here.", "",
    "```bash", signerCommand(network), "```", "",
    "The script checks the expected call hash against `mainnet-proposal.json`. After the first approval, the CLI discovers the pending timepoint. The final approval executes the batch.", "",
    "**Do not reuse this artifact after a runtime upgrade or out-of-band change without regenerating it.**", "",
    "### Individual command templates", "",
  );
  for (const signer of proposal.signers.signatories) {
    lines.push(`Signer: \`${signer}\``, "", "```bash", signerCommand(network), "```", "");
  }
  return lines.join("\n");
}

function countersignScript(network, callHash, signers) {
  const config = JSON.stringify(signers, null, 2);
  return `#!/usr/bin/env python3
"""Mainnet parameter countersigner.

No keys or seed material are embedded. btcli handles local wallet decryption.
The operator reviews a live diff before approving; this script never exports
keys or accepts seed material. Only the standard btcli approval command submits.
"""
import argparse
import asyncio
import json
import os
import subprocess
from pathlib import Path

import bittensor as bt

parser = argparse.ArgumentParser()
parser.add_argument("proposal", type=Path)
args = parser.parse_args()
proposal_path = args.proposal.resolve()
proposal = json.loads(proposal_path.read_text())
if proposal.get("network") != "mainnet":
    raise SystemExit("Countersigning is only available for a mainnet proposal")
call_args_path = proposal_path.with_name(proposal_path.name.replace("proposal.json", "args.json"))
call_args = json.loads(call_args_path.read_text())
expected = "${callHash}"
if proposal["call_hash"] != expected:
    raise SystemExit("Artifact call hash differs from the reviewed script")
config = ${config}
signatories = config["signatories"]
threshold = config["threshold"]
wallet = os.environ.get("SUDO_WALLET")
if not wallet or wallet.startswith("<"):
    raise SystemExit("Set SUDO_WALLET to your own local triumvirate member wallet")
def sdk_call(value):
    """Convert a pallet/function variant call to the SDK's Call type."""
    if isinstance(value, list):
        return [sdk_call(item) for item in value]
    if isinstance(value, dict) and len(value) == 1:
        pallet, function_map = next(iter(value.items()))
        if isinstance(function_map, dict) and len(function_map) == 1:
            function, arguments = next(iter(function_map.items()))
            if callable(getattr(getattr(bt.calls, pallet, None), function, None)):
                return bt.calls.Call(pallet, function, {
                    key: sdk_call(item) for key, item in arguments.items()
                })
    return value

def normalize(value):
    if isinstance(value, dict) and set(value) == {"none"} and value["none"] is True:
        return None
    if isinstance(value, dict) and set(value) == {"bits"}:
        return {"bits": int(value["bits"], 0) if isinstance(value["bits"], str) else value["bits"]}
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items()}
    if isinstance(value, list):
        return [normalize(item) for item in value]
    if isinstance(value, str) and value.isdecimal():
        return int(value)
    return value

async def verify():
    async with bt.Subtensor("finney", fallback_endpoints=[], archive_endpoints=[]) as client:
        batch = call_args["call"]["Utility"]["batch_all"]["calls"]
        inner_calls = [await client.compose(sdk_call(value)) for value in batch]
        batch_call = await client.compose(bt.calls.Utility.batch_all(calls=inner_calls))
        sudo_key = str(await client.query(bt.storage.Sudo.Key))
        multisig = await client.multisig(signatories, threshold, account_mode="legacy")
        if multisig.address != sudo_key:
            raise SystemExit("Signer set does not derive the live sudo key")
        call = await client.compose(bt.calls.Sudo.sudo(call=batch_call))
        actual = "0x" + bytes(call.call_hash).hex()
        if actual != expected:
            raise SystemExit(f"Refusing approval: live call hash {actual} != {expected}")
        for entry in proposal["diff"]["proposed"]:
            live = await client.query(tuple(entry["storage"]))
            if normalize(live) != normalize(entry["live"]):
                raise SystemExit(f"Live value changed for {entry['key']}; regenerate the proposal")
        print("Verified live sudo key, call hash, and proposed old values.")
        print("Call hash:", actual)

asyncio.run(verify())
print("Review the displayed batch, then approve in the standard btcli flow.")
command = [
    "btcli", "call", "Sudo.sudo", "--args-file", str(call_args_path),
    "--multisig-account-mode", "legacy",
    "--multisig-threshold", str(threshold),
    "--signatories", ",".join(signatories),
    "-w", wallet, "--network", "finney",
]
raise SystemExit(subprocess.run(command).returncode)
`;
}
function writeProposal(diff, encoded, network, context, outputDir, options = {}) {
  const signers = network === "mainnet" ? signerConfiguration() : null;
  const proposal = {
    network,
    source_file: options.file,
    base_file: options.baseFile || null,
    ...context,
    diff: {
      proposed: diff.changed.map(({ key, chain, desired, callPallet, call, pallet, item }) => ({ key, live: chain, proposed: desired, storage: [pallet, item], setter: `${callPallet}.${call}` })),
      drift: diff.drift.map(({ key, chain, desired }) => ({ key, file: desired, live: chain })),
    },
    signers,
    call_data: encoded?.callData || null,
    call_hash: encoded?.callHash || null,
    decoded_call: encoded?.decoded || null,
    setter_calls: encoded?.setters || [],
  };
  fs.mkdirSync(outputDir, { recursive: true });
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.json`), JSON.stringify(proposal, null, 2) + "\n");
  const markdown = renderMarkdown({ diff, encoded, signers, context }, options.file, network);
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.md`), markdown + "\n");
  if (encoded) {
    fs.writeFileSync(path.join(outputDir, `${network}-args.json`), JSON.stringify(encoded.btcliArgs, null, 2) + "\n");
    if (network === "mainnet") {
      const script = countersignScript(network, encoded.callHash, signers);
      fs.writeFileSync(path.join(outputDir, `${network}-countersign.py`), script, { mode: 0o755 });
    }
  }
  return proposal;
}

module.exports = { changesMarkdown, countersignScript, renderMarkdown, signerCommand, writeProposal };
