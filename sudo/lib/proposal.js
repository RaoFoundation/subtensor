"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { safeJsonValues } = require("./diff");
const { sortAddresses } = require("@polkadot/util-crypto");

function display(value) {
  if (typeof value === "bigint") return value.toString();
  return JSON.stringify(value);
}

function changesMarkdown(diff) {
  const proposed = Object.hasOwn(diff, "proposed") ? diff.proposed : diff.changed.flatMap((entry) => (entry.affected || [entry]).map((affected) => ({
    key: affected.key,
    live: affected.live ?? affected.chain,
    proposed: affected.proposed ?? affected.desired,
    setter: affected.setter || `${entry.callPallet}.${entry.call}`,
  })));
  const lines = [
    `Reviewed baseline: ${diff.reviewed ? "provided" : "**missing; report only**"}`, "",
    "| Parameter | Live value | Proposed value | Setter |",
    "| --- | ---: | ---: | --- |",
    ...proposed.map((affected) => `| \`${affected.key}\` | \`${display(affected.live)}\` | \`${display(affected.proposed)}\` | \`${affected.setter}\` |`),
  ];
  if (!proposed.length) lines.push("| _No proposed changes_ | | | |");
  if (diff.drift.length) {
    lines.push("", "### Unrelated live drift (excluded from the proposal)", "",
      "These values were not edited in the PR or cannot be separated without a baseline. Review them separately; they are not part of the batch.", "");
    for (const entry of diff.drift) {
      lines.push(`- \`${entry.key}\`: file \`${display(entry.file)}\`; live \`${display(entry.live)}\``);
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

function signerCommand(proposal) {
  if (!/^0x[0-9a-f]{64}$/.test(proposal.call_hash)) throw new Error("Cannot publish an invalid call hash");
  return `SUDO_WALLET=<WALLET> python3 sudo/countersign.py <ARTIFACT_JSON> <REPRODUCED_JSON> --expected-call-hash ${proposal.call_hash} --operation <OPEN_OR_HEIGHT:INDEX>`;
}

function reviewBlock(proposal) {
  return [
    "```text",
    ...proposal.diff.proposed.map((affected) => `${affected.key}: ${display(affected.live)} -> ${display(affected.proposed)}`),
    "```", "",
    "Decoded call:", "",
    "```json", JSON.stringify(proposal.decoded_call, null, 2), "```", "",
    `Call data: \`${proposal.call_data}\``, "",
    `Call hash (BLAKE2-256): \`${proposal.call_hash}\``,
  ].join("\n");
}

function renderMarkdown(proposal, file, network) {
  const lines = [
    `# ${network} sudo parameter proposal`, "",
    proposal.preview
      ? "**Preview only. CI has not signed or submitted any extrinsic.**"
      : "**Proposal only. CI has not signed or submitted any extrinsic.**", "",
    `Source file: \`${file}\``,
    ...(proposal.provenance ? [
      `Source commit: \`${proposal.provenance.sourceCommit}\``,
      `Reviewed baseline commit: \`${proposal.provenance.baseCommit || "none; report only"}\``,
      `Source SHA-256: \`${proposal.provenance.sourceSha256}\``,
      `Baseline SHA-256: \`${proposal.provenance.baseSha256 || "none; report only"}\``,
    ] : []),
    `Finalized block: \`${proposal.context.blockNumber}\` (\`${proposal.context.blockHash}\`)`,
    `Runtime spec version: \`${proposal.context.specVersion}\``, "",
    `Call: \`Sudo.sudo(Utility.batch_all(...))\``, "",
    ...changesMarkdown(proposal.diff).split("\n"), "",
  ];
  if (!proposal.encoded) {
    lines.push("No call data was produced because there are no proposed changes or no reviewed baseline. Mismatches without a baseline are reported as unclassified live drift.");
    return lines.join("\n");
  }
  if (!proposal.countersign_command) {
    lines.push(
      "## Encoded call", "",
      ...reviewBlock(proposal).split("\n"), "",
      "Each setter was encoded and round-tripped against live runtime metadata.", "",
      proposal.preview
        ? "**Preview only. Do not sign using this run. Use a trusted main-branch run.**"
        : "The proposal snapshot is not a substitute for a fresh live-state check.",
    );
    return lines.join("\n");
  }
  lines.push(
    "## Triumvirate countersigning", "",
    `Inspect the decoded changes, decoded call, call data, and call hash immediately below. Confirm that the command is exactly the change you reviewed before allowing it to use your wallet. It uses the existing ${proposal.signers.threshold}-of-${proposal.signers.signatories.length} legacy multisig.`, "",
    "Use the verifier from a trusted checkout, not from a pull request or downloaded artifact. Independently reproduce the proposal from the listed source and baseline before approving. Only the designated initiator uses `open`. Every subsequent signer must use the exact opening `HEIGHT:INDEX`; if it is no longer pending, the approval refuses to sign.", "",
    "**Known limitation:** old-value checks are not atomic with execution. The verifier rechecks every affected old value immediately before handing off to btcli. If any value changed, stop, regenerate, and restart review.", "",
    "### Review this exact call, then verify and approve", "",
    ...reviewBlock(proposal).split("\n"), "",
    "```bash", proposal.countersign_command, "```", "",
    "**Do not reuse this artifact after a runtime upgrade or out-of-band change without regenerating it.**",
  );
  return lines.join("\n");
}

function writeProposal(diff, encoded, network, context, outputDir, options = {}) {
  const signers = network === "mainnet" ? signerConfiguration() : null;
  const proposal = {
    network,
    source_file: options.file,
    base_file: options.baseFile || null,
    ...context,
    diff: {
      reviewed: diff.reviewed,
      proposed: encoded?.setters.flatMap((setter) => setter.affected) || [],
      drift: diff.drift.map(({ key, chain, desired }) => ({ key, file: desired, live: chain })),
    },
    signers,
    preview: Boolean(options.preview),
    provenance: options.provenance || null,
    call_data: encoded?.callData || null,
    call_hash: encoded?.callHash || null,
    decoded_call: encoded?.decoded || null,
    btcli_args: encoded?.btcliArgs || null,
    countersign_command: null,
    setter_calls: encoded?.setters || [],
  };
  if (network === "mainnet" && encoded && !options.preview && options.provenance?.baseCommit) proposal.countersign_command = signerCommand(proposal);
  fs.mkdirSync(outputDir, { recursive: true });
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.json`), JSON.stringify(proposal, null, 2) + "\n");
  const markdown = renderMarkdown({ ...proposal, encoded, context }, options.file, network);
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.md`), markdown + "\n");
  if (encoded) {
    fs.writeFileSync(path.join(outputDir, `${network}-args.json`), JSON.stringify(safeJsonValues(encoded.btcliArgs, true), null, 2) + "\n");
  }
  return proposal;
}

module.exports = { changesMarkdown, renderMarkdown, signerCommand, writeProposal };
