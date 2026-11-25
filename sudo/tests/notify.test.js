"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { compactValue, notifyProposal, renderPayload, trustedContext } = require("../lib/notify");
const { loadParams } = require("../lib/diff");

const WEBHOOK = "https://discord.invalid/api/webhooks/never-a-real-secret/DO_NOT_LOG_THIS";
const TOKEN = "test-only-github-token-that-must-not-be-logged";
const SHA = "0123456789abcdef0123456789abcdef01234567";
const PR_URL = "https://github.com/RaoFoundation/subtensor/pull/3361";
const RUN_URL = "https://github.com/RaoFoundation/subtensor/actions/runs/123456";
const NOW = new Date("2026-11-24T12:34:56.000Z");
const context = {
  prUrl: PR_URL,
  commitUrl: `https://github.com/RaoFoundation/subtensor/commit/${SHA}`,
  runUrl: RUN_URL,
  timestamp: NOW.toISOString(),
};

function proposal(changes = [{ key: "subnet_limit", live: "160", proposed: "161" }], decodedCall) {
  return {
    network: "mainnet",
    call_data: "0x0c000b02041325a100",
    call_hash: `0x${"bf".repeat(32)}`,
    decoded_call: decodedCall || {
      section: "sudo", method: "sudo",
      args: { call: { section: "utility", method: "batchAll", args: { calls: [{ section: "adminUtils", method: "sudoSetSubnetLimit", args: { max_subnets: "161" } }] } } },
    },
    diff: { proposed: changes, drift: [{ key: "other", live: "1", desired: "1" }] },
  };
}

function environment(overrides = {}) {
  return {
    GITHUB_EVENT_NAME: "push",
    GITHUB_REF: "refs/heads/main",
    GITHUB_REPOSITORY: "RaoFoundation/subtensor",
    GITHUB_SHA: SHA,
    GITHUB_RUN_ID: "123456",
    GITHUB_TOKEN: TOKEN,
    SUDO_DISCORD_WEBHOOK: WEBHOOK,
    ...overrides,
  };
}

function transport(t, { pulls = [{ html_url: PR_URL, merged_at: "2026-11-24T00:00:00Z" }] } = {}) {
  const calls = [];
  const fetchImpl = async (url, options = {}) => {
    const request = { url, options };
    calls.push(request);
    if (url === WEBHOOK) return { ok: true, status: 204 };
    if (url.endsWith(`/commits/${SHA}/pulls`)) return { ok: true, status: 200, json: async () => pulls };
    throw new Error("Unexpected network request");
  };
  t.after(() => {
    const output = calls.map((call) => call.options.method === "POST" ? call.options.body : "").join("\n");
    assert.doesNotMatch(output, /DO_NOT_LOG_THIS/);
  });
  return { calls, fetchImpl };
}

async function notify(t, options = {}) {
  const { calls, fetchImpl } = transport(t, options);
  const logs = [];
  const result = await notifyProposal({
    proposal: options.proposal || proposal(),
    env: environment(options.env),
    fetchImpl,
    now: () => NOW,
    log: (line) => logs.push(line),
  });
  return { calls, logs, result };
}

test("one changed network resolves its merged PR and posts one complete Discord message", async (t) => {
  const { calls, logs, result } = await notify(t);
  assert.equal(result.status, "posted");
  assert.equal(calls.length, 2);
  assert.equal(calls[0].url, `https://api.github.com/repos/RaoFoundation/subtensor/commits/${SHA}/pulls`);
  assert.equal(calls[0].options.headers.authorization, `Bearer ${TOKEN}`);
  assert.equal(calls[1].url, WEBHOOK);
  assert.equal(calls[1].options.method, "POST");
  const payload = JSON.parse(calls[1].options.body);
  assert.deepEqual(payload, result.payload);
  assert.match(payload.content, /Sudo parameter proposal: mainnet/);
  assert.match(payload.content, /subnet_limit: "160" -> "161"/);
  assert.match(payload.content, /sudo\.sudo\(\{call: utility\.batchAll\(\{calls: \[adminUtils\.sudoSetSubnetLimit\(\{max_subnets: "161"\}\)\]\}\)\}\)/);
  assert.match(payload.content, new RegExp(`Call hash: \`0x${"bf".repeat(32)}\``));
  assert.match(payload.content, new RegExp(`Merged PR: <${PR_URL}>`));
  assert.match(payload.content, new RegExp(`Run / countersign command: <${RUN_URL}>`));
  assert.match(payload.content, /Timestamp \(UTC\): 2026-11-24T12:34:56\.000Z/);
  assert.doesNotMatch(payload.content, /Source commit:/);
  assert.equal(payload.content.length <= 2000, true);
  assert.deepEqual(payload.allowed_mentions, { parse: [] });
  assert.match(logs.join("\n"), /notification posted for mainnet/);
  assert.doesNotMatch(logs.join("\n"), /DO_NOT_LOG_THIS/);
});

test("every changed parameter and every part of a nested decoded call are rendered", () => {
  const payload = renderPayload(proposal([
    { key: "subnet_limit", live: "160", proposed: "161" },
    { key: "network_registration_allowed", live: false, proposed: true },
    { key: "basket_claim_dust.min_rao", live: { bits: "1" }, proposed: { bits: "2" } },
  ]), context);
  for (const change of ['subnet_limit: "160" -> "161"', "network_registration_allowed: false -> true", 'basket_claim_dust.min_rao: {"bits":"1"} -> {"bits":"2"}']) {
    assert.match(payload.content, new RegExp(change.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
  assert.equal(compactValue({ section: "utility", method: "batchAll", args: { calls: [] } }), "utility.batchAll({calls: []})");
});

test("zero changes and drift-only output do not call GitHub or Discord", async (t) => {
  for (const report of [proposal([]), { ...proposal([]), diff: { proposed: [], drift: [] } }, { network: "mainnet", diff: { proposed: [], drift: [{ key: "unrelated" }] } }]) {
    const { calls, logs, result } = await notify(t, { proposal: report });
    assert.equal(result.status, "skipped-no-changes");
    assert.equal(calls.length, 0);
    assert.match(logs.join("\n"), /no encoded proposed changes/);
  }
});

test("pull requests, manual comparisons and non-main refs never post, even if secrets are supplied", async (t) => {
  for (const env of [
    { GITHUB_EVENT_NAME: "pull_request" },
    { GITHUB_EVENT_NAME: "pull_request_target" },
    { GITHUB_EVENT_NAME: "push", GITHUB_REF: "refs/heads/sudo-params-as-code" },
    { GITHUB_EVENT_NAME: "workflow_dispatch", GITHUB_REF: "refs/heads/sudo-params-as-code" },
    { GITHUB_EVENT_NAME: "workflow_dispatch" },
    { GITHUB_EVENT_NAME: "schedule" },
  ]) {
    const { calls, result } = await notify(t, { env });
    assert.equal(result.status, "skipped-untrusted");
    assert.equal(calls.length, 0);
  }
  assert.equal(trustedContext(environment()), true);
});

test("missing merged PRs, unavailable tokens, and GitHub failures fall back to the commit link", async (t) => {
  for (const options of [
    { pulls: [] },
    { pulls: [{ html_url: PR_URL, merged_at: null }] },
    { env: { GITHUB_TOKEN: "" } },
  ]) {
    const { calls, result } = await notify(t, options);
    assert.equal(result.status, "posted");
    assert.match(result.payload.content, new RegExp(`Source commit: <https://github.com/RaoFoundation/subtensor/commit/${SHA}>`));
    assert.doesNotMatch(result.payload.content, /Merged PR:/);
    assert.equal(calls.at(-1).url, WEBHOOK);
  }
});

test("GitHub lookup request and HTTP failures are sanitized and do not block the fallback", async (t) => {
  for (const mode of ["throw", "http"]) {
    const calls = [];
    const logs = [];
    const fetchImpl = async (url, options = {}) => {
      calls.push({ url, options });
      if (url === WEBHOOK) return { ok: true, status: 204 };
      if (mode === "throw") throw new Error(`transport exposed ${WEBHOOK} and ${TOKEN}`);
      return { ok: false, status: 403 };
    };
    const result = await notifyProposal({ proposal: proposal(), env: environment(), fetchImpl, now: () => NOW, log: (line) => logs.push(line) });
    assert.equal(result.status, "posted");
    assert.match(result.payload.content, /Source commit:/);
    assert.doesNotMatch(logs.join("\n"), /DO_NOT_LOG_THIS|test-only-github-token/);
  }
});

test("long decoded calls truncate with a run pointer and stay within content limits", () => {
  const nested = { section: "sudo", method: "sudo", args: { call: { section: "utility", method: "batchAll", args: { calls: [{ section: "adminUtils", method: "sudoSetSubnetLimit", args: { max_subnets: "1".repeat(5000) } }] } } } };
  const payload = renderPayload(proposal(undefined, nested), context);
  assert.equal(payload.content.length, 2000);
  assert.match(payload.content, /\.\.\. \[decoded call truncated; see run URL\]/);
  assert.match(payload.content, new RegExp(RUN_URL.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  assert.match(payload.content, new RegExp(`0x${"bf".repeat(32)}`));
  assert.match(payload.content, /2026-11-24T12:34:56\.000Z/);
  assert.deepEqual(payload.allowed_mentions, { parse: [] });
  assert.doesNotMatch(payload.content, new RegExp(WEBHOOK.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
});

test("all supported parameters fit one embed within Discord's aggregate limits", () => {
  const values = loadParams(fs.readFileSync(path.join(__dirname, "..", "mainnet.toml"), "utf8")).values;
  const changes = Object.entries(values).map(([key, value]) => ({ key, live: value, proposed: value }));
  const decoded = { section: "sudo", method: "sudo", args: { call: { section: "utility", method: "batchAll", args: { calls: changes.map(({ key }) => ({ section: "adminUtils", method: key, args: { value: "1".repeat(100) } })) } } } };
  const payload = renderPayload(proposal(changes, decoded), context);
  assert.equal(payload.content, "**Sudo parameter proposal: mainnet**");
  assert.equal(payload.embeds.length, 1);
  for (const { key } of changes) assert.match(payload.embeds[0].description, new RegExp(`^${key}:`, "m"));
  assert.ok(payload.embeds[0].description.length <= 4096);
  assert.ok(payload.embeds[0].fields.every((field) => field.name.length <= 256 && field.value.length <= 1024));
  const aggregate = payload.embeds.reduce((sum, embed) => sum + (embed.title?.length || 0) + (embed.description?.length || 0) +
    (embed.footer?.text.length || 0) + (embed.author?.name.length || 0) +
    (embed.fields || []).reduce((total, field) => total + field.name.length + field.value.length, 0), 0);
  assert.ok(aggregate <= 6000);
  assert.match(payload.embeds[0].description, /decoded call truncated; see run URL/);
  assert.ok(payload.embeds[0].fields.some((field) => field.value === `[Open workflow summary](${RUN_URL})`));
});

test("webhook transport failures never echo the URL or GitHub token", async (t) => {
  const logs = [];
  for (const response of ["throw", "http"]) {
    const fetchImpl = async (url) => {
      if (url !== WEBHOOK) return { ok: true, status: 200, json: async () => [] };
      if (response === "throw") throw new Error(`request URL was ${WEBHOOK}; token ${TOKEN}`);
      return { ok: false, status: 429 };
    };
    await assert.rejects(
      notifyProposal({ proposal: proposal(), env: environment(), fetchImpl, now: () => NOW, log: (line) => logs.push(line) }),
      (error) => {
        assert.doesNotMatch(error.message, /DO_NOT_LOG_THIS|test-only-github-token|discord\.invalid/);
        assert.match(error.message, /Discord webhook/);
        return true;
      },
    );
  }
});

test("CLI missing-secret warning succeeds without any network access, including blank input", (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-notify-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, "proposal.json");
  fs.writeFileSync(file, JSON.stringify(proposal()));
  for (const webhook of [undefined, "", "   "]) {
    const result = spawnSync(process.execPath, [path.join(__dirname, "..", "notify.js"), file], {
      env: { ...environment(), SUDO_DISCORD_WEBHOOK: webhook, PATH: process.env.PATH },
      encoding: "utf8",
      timeout: 10000,
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /::warning::SUDO_DISCORD_WEBHOOK is unset or empty/);
    assert.doesNotMatch(result.stdout + result.stderr, /DO_NOT_LOG_THIS|test-only-github-token/);
  }
});

test("CLI never logs a supplied webhook when skipping a zero-change report", (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-notify-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, "proposal.json");
  fs.writeFileSync(file, JSON.stringify(proposal([])));
  const result = spawnSync(process.execPath, [path.join(__dirname, "..", "notify.js"), file], {
    env: { ...environment(), PATH: process.env.PATH },
    encoding: "utf8",
    timeout: 10000,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.stdout, /::add-mask::/);
  assert.match(result.stdout, /no encoded proposed changes/);
  assert.doesNotMatch(result.stdout + result.stderr, /DO_NOT_LOG_THIS|test-only-github-token|discord\.invalid/);
});
