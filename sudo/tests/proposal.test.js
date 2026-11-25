"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { compare, encodeProposal, loadParams } = require("../lib/diff");
const { writeProposal } = require("../lib/proposal");
const { render } = require("../export");

const context = {
  blockNumber: 42,
  blockHash: `0x${"11".repeat(32)}`,
  genesisHash: `0x${"22".repeat(32)}`,
  specVersion: 495,
  sudoKey: "test-only-sudo-key",
};
const liveValues = loadParams(fs.readFileSync(path.join(__dirname, "..", "mainnet.toml"), "utf8")).values;

function changedDiff() {
  const api = {
    tx: {
      adminUtils: {
        sudoSetSubnetLimit: Object.assign(
          (maxSubnets) => call("AdminUtils.sudoSetSubnetLimit", { maxSubnets }),
          {
            meta: {
              args: [{ name: { toString: () => "maxSubnets" } }],
              fields: [{ name: { toString: () => "max_subnets" } }],
            },
          },
        ),
      },
      utility: { batchAll: (calls) => call("Utility.batchAll", { calls }) },
      sudo: { sudo: (inner) => call("Sudo.sudo", { call: inner }) },
    },
    registry: {
      createType: (_type, bytes) => ({
        args: Object.values(JSON.parse(Buffer.from(bytes).toString()).args).map(
          (value) => ({ toPrimitive: () => value }),
        ),
        toHex: () => `0x${Buffer.from(bytes).toString("hex")}`,
        toHuman: () => {
          const { target, args } = JSON.parse(Buffer.from(bytes).toString());
          const lowerFirst = (value) => value.charAt(0).toLowerCase() + value.slice(1);
          const readableArgs = (value) => {
            if (value?.target && value?.args) {
              const [section, method] = value.target.split(".");
              return { section: lowerFirst(section), method, args: readableArgs(value.args) };
            }
            if (Array.isArray(value)) return value.map(readableArgs);
            if (value && typeof value === "object") {
              return Object.fromEntries(Object.entries(value).map(([key, item]) => [key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`), readableArgs(item)]));
            }
            return String(value);
          };
          const [section, method] = target.split(".");
          return { section: lowerFirst(section), method, args: readableArgs(args) };
        },
      }),
    },
  };
  const diff = compare({ ...liveValues, subnet_limit: "161" }, liveValues, liveValues);
  return { diff, encoded: encodeProposal(diff, api) };
}

function call(target, args) {
  const data = Buffer.from(JSON.stringify({ target, args }));
  return {
    method: {
      toU8a: () => data,
      toHex: () => `0x${data.toString("hex")}`,
      hash: { toHex: () => `0x${"33".repeat(32)}` },
    },
    toJSON: () => ({ target, args }),
  };
}

test("mainnet artifacts contain call data, old-to-new values, and a countersign command", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-mainnet-test-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { diff, encoded } = changedDiff();
  const proposal = writeProposal(diff, encoded, "mainnet", context, output, {
    file: "/reviewed/mainnet.toml",
    baseFile: "/base/mainnet.toml",
    provenance: { sourceCommit: "abc", baseCommit: "def", sourceSha256: "123", baseSha256: "456" },
  });
  const markdown = fs.readFileSync(path.join(output, "mainnet-proposal.md"), "utf8");
  const artifact = path.join(output, "mainnet-proposal.json");
  const reproduced = path.join(output, "reproduced-mainnet-proposal.json");
  fs.copyFileSync(artifact, reproduced);
  const script = fs.readFileSync(path.join(__dirname, "..", "countersign.py"), "utf8");
  assert.equal(proposal.diff.proposed[0].live, "160");
  assert.equal(proposal.diff.proposed[0].proposed, "161");
  assert.equal(proposal.signers.threshold, 2);
  assert.match(markdown, /"160"/);
  assert.match(markdown, /"161"/);
  assert.match(markdown, /Source commit: `abc`/);
  assert.match(markdown, /Reviewed baseline commit: `def`/);
  assert.match(markdown, /Source SHA-256: `123`/);
  assert.match(markdown, /Baseline SHA-256: `456`/);
  const expectedArgs = JSON.stringify(proposal.btcli_args);
  const command = `SUDO_WALLET=<WALLET> python3 sudo/countersign.py <ARTIFACT_JSON> <REPRODUCED_JSON> --expected-call-hash ${proposal.call_hash} --operation <OPEN_OR_HEIGHT:INDEX>`;
  assert.equal(proposal.countersign_command, command);
  assert.equal((markdown.match(/```bash\nSUDO_WALLET=/g) || []).length, 1);
  assert.match(markdown, /```text\nsubnet_limit: "160" -> "161"\n```/);
  assert.match(markdown, new RegExp(`Decoded call:\\n\\n\`\`\`json\\n${JSON.stringify(proposal.decoded_call, null, 2).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\n\`\`\``));
  assert.match(markdown, new RegExp(`Call data: \`${proposal.call_data}\``));
  assert.match(markdown, new RegExp(`Call hash \\(BLAKE2-256\\): \`${proposal.call_hash}\``));
  const review = markdown.slice(markdown.indexOf("### Review this exact call, then verify and approve"));
  const commandIndex = review.indexOf(command);
  assert.ok(commandIndex > 0);
  for (const text of ['subnet_limit: "160" -> "161"', "Decoded call:", `Call data: \`${proposal.call_data}\``, `Call hash (BLAKE2-256): \`${proposal.call_hash}\``]) {
    assert.ok(review.indexOf(text) >= 0 && review.indexOf(text) < commandIndex, text);
  }
  assert.equal(proposal.decoded_call.section, "sudo");
  assert.equal(proposal.decoded_call.method, "sudo");
  assert.deepEqual(proposal.decoded_call.args.call.args.calls, [{ section: "adminUtils", method: "sudoSetSubnetLimit", args: { max_subnets: "161" } }]);
  assert.match(markdown, /"max_subnets": "161"/);
  assert.match(expectedArgs, /^\{"call":\{"Utility":\{"batch_all":\{"calls":\[\{"AdminUtils":\{"sudo_set_subnet_limit":\{"max_subnets":161\}\}\}\]\}\}\}\}$/);
  assert.match(markdown, /old-value checks are not atomic with execution/);
  assert.equal(fs.existsSync(path.join(output, "mainnet-countersign.py")), false);
  assert.equal(fs.readdirSync(output).some((name) => name.endsWith(".py") || name.endsWith(".sh")), false);
  assert.equal(proposal.btcli_args.call.Utility.batch_all.calls.length, 1);
  assert.equal(proposal.provenance.baseCommit, "def");
  assert.equal(proposal.diff.reviewed, true);
  assert.match(script, /threshold = config\["threshold"\]/);
  assert.match(script, /Set SUDO_WALLET/);
  assert.doesNotMatch(script, /Pass --wallet/);
  const parse = spawnSync("python3", ["-c", "import ast, sys; ast.parse(open(sys.argv[1]).read())", path.join(__dirname, "..", "countersign.py")]);
  assert.equal(parse.status, 0);
  function offlineVerifier(first, second, hash) {
    return spawnSync("python3", [
      path.join(__dirname, "countersign_driver.py"),
      path.join(__dirname, "..", "countersign.py"),
      first, second, "--expected-call-hash", hash, "--operation", "open",
    ], { env: { ...process.env, SUDO_WALLET: "test-only-wallet" }, timeout: 10000 });
  }
  const valid = offlineVerifier(artifact, reproduced, proposal.call_hash);
  assert.equal(valid.status, 1);
  assert.match(valid.stderr.toString(), /offline test reached live verification or btcli/);
  const wallet = spawnSync("python3", [
    path.join(__dirname, "..", "countersign.py"), artifact, reproduced,
    "--expected-call-hash", proposal.call_hash, "--operation", "open",
  ], { env: { ...process.env, SUDO_WALLET: "" } });
  assert.notEqual(wallet.status, 0);
  assert.match(wallet.stderr.toString(), /SUDO_WALLET/);
  const wrongHash = offlineVerifier(artifact, reproduced, `0x${"44".repeat(32)}`);
  assert.notEqual(wrongHash.status, 0);
  assert.match(wrongHash.stderr.toString(), /independently reproduced hash/);
  const unreviewed = { ...proposal, provenance: { ...proposal.provenance, baseCommit: null } };
  fs.writeFileSync(path.join(output, "unreviewed.json"), JSON.stringify(unreviewed));
  const noBaseline = offlineVerifier(path.join(output, "unreviewed.json"), reproduced, proposal.call_hash);
  assert.notEqual(noBaseline.status, 0);
  assert.match(noBaseline.stderr.toString(), /no exact reviewed baseline/);
  const samePath = offlineVerifier(artifact, artifact, proposal.call_hash);
  assert.match(samePath.stderr.toString(), /must be a separate file/);
  for (const [name, mutate, message] of [
    ["old-value", (value) => { value.diff.proposed[0].live = "999"; }, /Artifact old values differ/],
    ["storage", (value) => { value.diff.proposed[0].storage[1] = "DefaultTake"; }, /Artifact old values differ/],
    ["calls", (value) => {
      value.btcli_args.call.Utility.batch_all.calls[0].AdminUtils.sudo_set_subnet_limit.max_subnets = 999;
    }, /Artifact calls differ/],
    ["provenance", (value) => { value.provenance.sourceCommit = "tampered"; }, /provenance differs/],
    ["signers", (value) => { value.signers.threshold = 3; }, /signers differ/],
    ["call-data", (value) => { value.call_data = "0xdeadbeef"; }, /call data differs/],
    ["decoded-call", (value) => { value.decoded_call = {}; }, /decoded call differs/],
    ["sudo-key", (value) => { value.sudoKey = "substituted"; }, /sudo key differs/],
  ]) {
    const changed = JSON.parse(JSON.stringify(proposal));
    mutate(changed);
    const file = path.join(output, `tampered-${name}.json`);
    fs.writeFileSync(file, JSON.stringify(changed));
    const result = offlineVerifier(file, reproduced, proposal.call_hash);
    assert.notEqual(result.status, 0, name);
    assert.match(result.stderr.toString(), message, name);
  }
  const unknown = JSON.parse(JSON.stringify(proposal));
  unknown.btcli_args.call.Utility.batch_all.calls = [{ Sudo: { set_key: { new: "attacker" } } }];
  unknown.diff.proposed = [{ key: "not-allowed", live: "1", proposed: "2", storage: ["Sudo", "Key"], setter: "Sudo.set_key" }];
  fs.writeFileSync(reproduced, JSON.stringify(unknown));
  fs.copyFileSync(reproduced, path.join(output, "unknown-artifact.json"));
  const rejected = offlineVerifier(path.join(output, "unknown-artifact.json"), reproduced, proposal.call_hash);
  assert.match(rejected.stderr.toString(), /not in the reviewed allowlist/);
});

test("an omitted grouped artifact member is rejected against the independent proposal", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-group-verifier-test-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const group = require("../lib/params").groups.basket_claim_dust;
  const changed = { ...liveValues, "basket_claim_dust.row_bps": "500" };
  const affected = Object.entries(group.storage).map(([key, storage], index) => ({
    key: `basket_claim_dust.${key}`,
    live: liveValues[`basket_claim_dust.${key}`],
    proposed: [1, 500, 1, 1][index].toString(),
    storage,
    setter: "AdminUtils.sudo_set_basket_claim_dust",
  }));
  const btcliArgs = { call: { Utility: { batch_all: { calls: [{ AdminUtils: { sudo_set_basket_claim_dust: {
    row_cap_rao: 1, row_bps: 500, slice_rao: 1, forfeit_cap_rao: 1,
  } } }] } } } };
  const proposal = {
    network: "mainnet",
    provenance: { sourceCommit: "abc", baseCommit: "def" },
    signers: {
      threshold: 2,
      signatories: JSON.parse(fs.readFileSync(path.join(__dirname, "..", "..", ".github/scripts/deploy/sudo-signatories.json"), "utf8")).signatories,
    },
    sudoKey: "test-only-sudo-key",
    call_data: "0x00",
    call_hash: `0x${"33".repeat(32)}`,
    decoded_call: {},
    btcli_args: btcliArgs,
    diff: { reviewed: true, proposed: affected, drift: [] },
  };
  const reproduced = path.join(output, "reproduced.json");
  const artifact = path.join(output, "artifact.json");
  fs.writeFileSync(reproduced, JSON.stringify(proposal));
  const omitted = JSON.parse(JSON.stringify(proposal));
  omitted.diff.proposed.splice(1, 1);
  fs.writeFileSync(artifact, JSON.stringify(omitted));
  const result = spawnSync("python3", [
    path.join(__dirname, "countersign_driver.py"),
    path.join(__dirname, "..", "countersign.py"),
    artifact, reproduced, "--expected-call-hash", proposal.call_hash, "--operation", "open",
  ], { env: { ...process.env, SUDO_WALLET: "test-only-wallet" }, timeout: 10000 });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr.toString(), /Artifact old values differ/);

  fs.copyFileSync(reproduced, artifact);
  fs.copyFileSync(reproduced, path.join(output, "second-reproduction.json"));
  const valid = spawnSync("python3", [
    path.join(__dirname, "countersign_driver.py"),
    path.join(__dirname, "..", "countersign.py"),
    artifact, path.join(output, "second-reproduction.json"), "--expected-call-hash", proposal.call_hash, "--operation", "open",
  ], { env: { ...process.env, SUDO_WALLET: "test-only-wallet" }, timeout: 10000 });
  assert.match(valid.stderr.toString(), /offline test reached live verification or btcli/);

  const mismatched = JSON.parse(JSON.stringify(proposal));
  mismatched.diff.proposed = mismatched.diff.proposed.slice(1);
  fs.writeFileSync(reproduced, JSON.stringify(mismatched));
  const other = path.join(output, "other-reproduction.json");
  fs.writeFileSync(other, JSON.stringify(mismatched));
  const wrongMapping = spawnSync("python3", [
    path.join(__dirname, "countersign_driver.py"),
    path.join(__dirname, "..", "countersign.py"),
    reproduced, other, "--expected-call-hash", proposal.call_hash, "--operation", "open",
  ], { env: { ...process.env, SUDO_WALLET: "test-only-wallet" }, timeout: 10000 });
  assert.notEqual(wrongMapping.status, 0);
  assert.match(wrongMapping.stderr.toString(), /Affected storage differs from the reviewed setter mapping/);
});
test("non-mainnet proposals contain the decoded diff and call without apply instructions", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-devnet-test-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { diff, encoded } = changedDiff();
  const proposal = writeProposal(diff, encoded, "devnet", context, output, { file: "devnet.toml" });
  const markdown = fs.readFileSync(path.join(output, "devnet-proposal.md"), "utf8");
  assert.match(markdown, /## Encoded call\n\n```text\nsubnet_limit: "160" -> "161"\n```/);
  assert.match(markdown, /Decoded call:/);
  assert.match(markdown, new RegExp(`Call data: \`${proposal.call_data}\``));
  assert.match(markdown, new RegExp(`Call hash \\(BLAKE2-256\\): \`${proposal.call_hash}\``));
  assert.doesNotMatch(markdown, /TODO|Apply status|apply job|secret-gat|deployment credential|protected network environment|countersign/i);
  assert.equal(proposal.countersign_command, null);
  assert.equal(fs.readdirSync(output).some((name) => name.endsWith(".py") || name.endsWith(".sh")), false);
});

test("Markdown lists every affected group member and its live value", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-group-test-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { groups } = require("../lib/params");
  const group = groups.basket_claim_dust;
  const changed = { ...liveValues, "basket_claim_dust.row_bps": "500" };
  const encoded = {
    callData: "0x00",
    callHash: `0x${"33".repeat(32)}`,
    decoded: {},
    btcliArgs: { call: { Utility: { batch_all: { calls: [] } } } },
    setters: [{
      key: "basket_claim_dust.row_bps",
      target: "AdminUtils.sudo_set_basket_claim_dust",
      args: {},
      btcliArgs: {},
      callData: "0x01",
      callHash: `0x${"44".repeat(32)}`,
      affected: Object.entries(group.storage).map(([key, storage]) => ({
        key: `basket_claim_dust.${key}`,
        live: liveValues[`basket_claim_dust.${key}`],
        proposed: changed[`basket_claim_dust.${key}`],
        storage,
        setter: "AdminUtils.sudo_set_basket_claim_dust",
      })),
    }],
  };
  const proposal = writeProposal({ ...compare(changed, liveValues, liveValues), changed }, encoded, "devnet", context, output, { file: "devnet.toml" });
  const markdown = fs.readFileSync(path.join(output, "devnet-proposal.md"), "utf8");
  assert.equal(proposal.diff.proposed.length, 4);
  for (const key of Object.keys(group.storage)) {
    assert.match(markdown, new RegExp(`basket_claim_dust\\.${key}`));
  }
  assert.equal(markdown.match(/AdminUtils\.sudo_set_basket_claim_dust/g).length, 4);
  assert.match(markdown, /`"500"`/);
});

test("optional values export as a TOML table and missing values remain reported", () => {
  const output = render({ tao_usd_price_oracle: null }, { network: "mainnet", ...context }, {
    subnet_limit: "storage unavailable",
  });
  assert.match(output, /\[tao_usd_price_oracle\][\s\S]*value = \{ none = true \}/);
  assert.match(output, /# UNREAD LIVE VALUES/);
  assert.match(output, /# subnet_limit: storage unavailable/);
});

test("a PR preview provides exact review data but no command, and displays drift", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-preview-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { diff, encoded } = changedDiff();
  diff.drift.push({ key: "tx_rate_limit", desired: "400", chain: "500" });
  const proposal = writeProposal(diff, encoded, "mainnet", context, output, {
    file: "mainnet.toml", preview: true, provenance: { sourceCommit: "abc", baseCommit: "def" },
  });
  const text = fs.readFileSync(path.join(output, "mainnet-proposal.md"), "utf8");
  assert.match(text, /Preview only/);
  assert.match(text, /tx_rate_limit.*file `"400"`; live `"500"`/);
  assert.doesNotMatch(text, /countersign\.py|btcli call/);
  assert.equal(proposal.countersign_command, null);
  assert.ok(proposal.call_hash);
});

test("args JSON preserves large integers as exact JSON number tokens", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-large-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { diff, encoded } = changedDiff();
  encoded.btcliArgs = { max: "18446744073709551615", fp: { bits: "36893488147419103232" } };
  writeProposal(diff, encoded, "devnet", context, output, { file: "devnet.toml" });
  const args = fs.readFileSync(path.join(output, "devnet-args.json"), "utf8");
  assert.match(args, /"max": 18446744073709551615/);
  assert.match(args, /"bits": 36893488147419103232/);
});
