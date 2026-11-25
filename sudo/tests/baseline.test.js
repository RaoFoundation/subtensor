"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

function git(repo, ...args) {
  return execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();
}

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sudo-baseline-test-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const repo = path.join(root, "repo");
  const output = path.join(root, "out");
  fs.mkdirSync(path.join(repo, "sudo"), { recursive: true });
  git(repo, "init", "-q");
  git(repo, "config", "user.name", "test");
  git(repo, "config", "user.email", "test@example.invalid");
  fs.writeFileSync(path.join(repo, "tracked"), "base\n");
  git(repo, "add", "tracked");
  git(repo, "commit", "-qm", "base");
  const base = git(repo, "rev-parse", "HEAD");
  const file = path.join(repo, "sudo", "mainnet.toml");
  const initial = fs.readFileSync(path.join(__dirname, "..", "mainnet.toml"), "utf8");
  fs.writeFileSync(file, initial);
  git(repo, "add", "sudo/mainnet.toml");
  git(repo, "commit", "-qm", "initial live export");
  const head = git(repo, "rev-parse", "HEAD");
  function resolve(baseRef = base) {
    return Object.fromEntries(execFileSync("bash", [path.join(__dirname, "..", "baseline.sh")], {
      cwd: repo,
      encoding: "utf8",
      env: { ...process.env, NETWORK: "mainnet", BASE_REF: baseRef, EVENT_NAME: "pull_request", OUTPUT_DIR: output },
    }).trimEnd().split("\n").map((line) => {
      const separator = line.indexOf("=");
      return [line.slice(0, separator), line.slice(separator + 1)];
    }));
  }
  return { repo, output, file, initial, base, head, resolve };
}

test("initial export stays report-only without a historical baseline", (t) => {
  const state = fixture(t);
  const result = state.resolve();
  assert.equal(result.BASE_COMMIT, "");
  assert.equal(fs.existsSync(result.BASE_FILE), false);
});

test("PR uses its branch point instead of a later base-tip edit", (t) => {
  const state = fixture(t);
  git(state.repo, "switch", "-qc", "main-tip", state.head);
  fs.writeFileSync(state.file, state.initial.replace('value = "160"', 'value = "161"'));
  git(state.repo, "commit", "-qam", "main changes subnet limit");
  const mainTip = git(state.repo, "rev-parse", "HEAD");
  git(state.repo, "switch", "-qc", "pr-head", state.head);
  git(state.repo, "commit", "-qm", "PR changes other setting", "--allow-empty");
  const result = state.resolve(mainTip);
  assert.equal(result.BASE_COMMIT, state.head);
  assert.equal(fs.readFileSync(result.BASE_FILE, "utf8"), state.initial);
});

test("an edited committed initial export cannot bootstrap itself", (t) => {
  const state = fixture(t);
  fs.writeFileSync(state.file, state.initial.replace('value = "160"', 'value = "161"'));
  git(state.repo, "add", "sudo/mainnet.toml");
  git(state.repo, "commit", "-qm", "edited initial export");
  assert.equal(state.resolve().BASE_COMMIT, "");
});

test("a real baseline commit is used and working-tree edits remain visible", (t) => {
  const state = fixture(t);
  git(state.repo, "commit", "-qm", "unrelated change", "--allow-empty");
  const result = state.resolve(state.head);
  assert.equal(result.BASE_COMMIT, state.head);
  fs.writeFileSync(state.file, state.initial.replace('value = "160"', 'value = "161"'));
  assert.equal(state.resolve(state.head).BASE_COMMIT, state.head);
});
