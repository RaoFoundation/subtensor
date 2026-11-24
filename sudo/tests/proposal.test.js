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
};
const liveValues = loadParams(fs.readFileSync(path.join(__dirname, "..", "mainnet.toml"), "utf8")).values;

function changedDiff() {
  const api = {
    tx: {
      adminUtils: {
        sudoSetSubnetLimit: Object.assign(
          ({ maxSubnets }) => call("AdminUtils.sudo_set_subnet_limit", { maxSubnets }),
          {
            meta: {
              args: [{ name: { toString: () => "maxSubnets" } }],
              fields: [{ name: { toString: () => "max_subnets" } }],
            },
          },
        ),
      },
      utility: { batchAll: (calls) => call("Utility.batch_all", { calls }) },
      sudo: { sudo: (inner) => call("Sudo.sudo", { call: inner }) },
    },
    registry: {
      createType: (_type, bytes) => ({ toHex: () => `0x${Buffer.from(bytes).toString("hex")}` }),
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
  assert.match(markdown, /python3 sudo\/countersign.py \/path\/to\/mainnet-proposal.json --expected-call-hash <INDEPENDENTLY_VERIFIED_HASH>/);
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
  const wallet = spawnSync("python3", [path.join(__dirname, "..", "countersign.py"), path.join(output, "mainnet-proposal.json"), "--expected-call-hash", proposal.call_hash], {
    env: { ...process.env, SUDO_WALLET: "" },
  });
  assert.notEqual(wallet.status, 0);
  assert.match(wallet.stderr.toString(), /SUDO_WALLET/);
  const wrongHash = spawnSync("python3", [path.join(__dirname, "..", "countersign.py"), path.join(output, "mainnet-proposal.json"), "--expected-call-hash", `0x${"44".repeat(32)}`], {
    env: { ...process.env, SUDO_WALLET: "test-only-wallet" },
  });
  assert.notEqual(wrongHash.status, 0);
  assert.match(wrongHash.stderr.toString(), /independently reproduced hash/);
  const unreviewed = { ...proposal, provenance: { ...proposal.provenance, baseCommit: null } };
  fs.writeFileSync(path.join(output, "unreviewed.json"), JSON.stringify(unreviewed));
  const noBaseline = spawnSync("python3", [path.join(__dirname, "..", "countersign.py"), path.join(output, "unreviewed.json"), "--expected-call-hash", proposal.call_hash], {
    env: { ...process.env, SUDO_WALLET: "test-only-wallet" },
  });
  assert.notEqual(noBaseline.status, 0);
  assert.match(noBaseline.stderr.toString(), /no exact reviewed baseline/);
});

test("non-mainnet proposals skip countersigning and explain the disabled apply TODO", (t) => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-devnet-test-"));
  t.after(() => fs.rmSync(output, { recursive: true, force: true }));
  const { diff, encoded } = changedDiff();
  writeProposal(diff, encoded, "devnet", context, output, { file: "devnet.toml" });
  const markdown = fs.readFileSync(path.join(output, "devnet-proposal.md"), "utf8");
  assert.match(markdown, /TODO: devnet\/testnet parameter application is not configured/);
  assert.match(markdown, /Keep application disabled by default/);
  assert.equal(fs.existsSync(path.join(output, "devnet-countersign.py")), false);
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
