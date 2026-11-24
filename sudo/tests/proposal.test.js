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
  });
  const markdown = fs.readFileSync(path.join(output, "mainnet-proposal.md"), "utf8");
  const script = fs.readFileSync(path.join(output, "mainnet-countersign.py"), "utf8");
  assert.equal(proposal.diff.proposed[0].live, "160");
  assert.equal(proposal.diff.proposed[0].proposed, "161");
  assert.equal(proposal.signers.threshold, 2);
  assert.match(markdown, /"160"/);
  assert.match(markdown, /"161"/);
  assert.match(markdown, /python3 mainnet-countersign.py mainnet-proposal.json/);
  assert.match(script, /"threshold": 2/);
  assert.match(script, /threshold = config\["threshold"\]/);
  assert.match(script, /Set SUDO_WALLET/);
  assert.doesNotMatch(script, /Pass --wallet/);
  const parse = spawnSync("python3", ["-c", "import ast, sys; ast.parse(open(sys.argv[1]).read())", path.join(output, "mainnet-countersign.py")]);
  assert.equal(parse.status, 0);
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

test("optional values export as a TOML table and missing values remain reported", () => {
  const output = render({ tao_usd_price_oracle: null }, { network: "mainnet", ...context }, {
    subnet_limit: "storage unavailable",
  });
  assert.match(output, /\[tao_usd_price_oracle\][\s\S]*value = \{ none = true \}/);
  assert.match(output, /# UNREAD LIVE VALUES/);
  assert.match(output, /# subnet_limit: storage unavailable/);
});
