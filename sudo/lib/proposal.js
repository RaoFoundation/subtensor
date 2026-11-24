"use strict";

const fs = require("node:fs");
const path = require("node:path");
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
  return `SUDO_WALLET=<WALLET> python3 sudo/countersign.py /path/to/${network}-proposal.json --expected-call-hash <INDEPENDENTLY_VERIFIED_HASH>`;
}

function renderMarkdown(proposal, file, network) {
  const lines = [
    `# ${network} sudo parameter proposal`, "",
    "**Proposal only. CI has not signed or submitted any extrinsic.**", "",
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
  lines.push(
    "## Encoded call", "",
    `Call hash (BLAKE2-256): \`${proposal.encoded.callHash}\``, "",
    "```text", proposal.encoded.callData, "```", "",
    "Each setter was encoded and round-tripped against live runtime metadata.", "",
    "The `decoded_call` and `setter_calls` JSON fields list every batch member and its decoded arguments.", "",
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
    "Re-run the diff from the same reviewed PR commit and exact reviewed baseline. Independently confirm the decoded calls and hash before signing. Never use call data from an unreviewed checkout or artifact.", "",
    "```bash",
    "node sudo/diff.js mainnet --file /path/to/reviewed-source/mainnet.toml --base-file /path/to/reviewed-base/mainnet.toml --output-dir /tmp/sudo-proposal \\",
    "  --source-commit <RECORDED_SOURCE_COMMIT> --base-commit <RECORDED_BASELINE_COMMIT>",
    "```", "",
    "## Triumvirate countersigning", "",
    `The checked-in verifier derives the ${proposal.signers.threshold}-of-${proposal.signers.signatories.length} legacy multisig and verifies it against live \`Sudo.Key\`.`,
    "It recomposes the call from the data-only proposal, verifies the independently supplied call hash and every affected old value, and uses the repository's standard `btcli` multisig approval path.", "",
    "Run the checked-in script from a separately reviewed repository checkout on your own trusted signer machine. Do not execute any script downloaded from an artifact. Replace `<WALLET>` with your own local member wallet and `<INDEPENDENTLY_VERIFIED_HASH>` with the freshly reproduced hash. No key is included here.", "",
    "```bash", signerCommand(network), "```", "",
    "The verifier checks the independently supplied hash against the artifact and recomposed call. After the first approval, the CLI discovers the pending timepoint. The final approval executes the batch.", "",
    "**Do not reuse this artifact after a runtime upgrade or out-of-band change without regenerating it.**", "",
    "### Individual command templates", "",
  );
  for (const signer of proposal.signers.signatories) {
    lines.push(`Signer: \`${signer}\``, "", "```bash", signerCommand(network), "```", "");
  }
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
    provenance: options.provenance || null,
    call_data: encoded?.callData || null,
    call_hash: encoded?.callHash || null,
    decoded_call: encoded?.decoded || null,
    btcli_args: encoded?.btcliArgs || null,
    setter_calls: encoded?.setters || [],
  };
  fs.mkdirSync(outputDir, { recursive: true });
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.json`), JSON.stringify(proposal, null, 2) + "\n");
  const markdown = renderMarkdown({ ...proposal, encoded, context }, options.file, network);
  fs.writeFileSync(path.join(outputDir, `${network}-proposal.md`), markdown + "\n");
  if (encoded) {
    fs.writeFileSync(path.join(outputDir, `${network}-args.json`), JSON.stringify(encoded.btcliArgs, null, 2) + "\n");
  }
  return proposal;
}

module.exports = { changesMarkdown, renderMarkdown, signerCommand, writeProposal };
