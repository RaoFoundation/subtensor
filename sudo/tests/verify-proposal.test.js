"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const { mapCalls, snakeCase } = require("../verify-proposal");
const { groups, params } = require("../lib/params");

function batch(calls) {
  return { call: { Utility: { batch_all: { calls } } } };
}

function basketCall() {
  return { AdminUtils: { sudo_set_basket_claim_dust: {
    row_cap_rao: 1, row_bps: 500, slice_rao: 2, forfeit_cap_rao: 3,
  } } };
}

test("the reviewed mapping derives all four atomic group members from its call", () => {
  const mapped = mapCalls(batch([basketCall()]));
  const fields = Object.keys(groups.basket_claim_dust.storage);
  assert.deepEqual(mapped.map((entry) => entry.key), fields.map((field) => `basket_claim_dust.${field}`));
  assert.deepEqual(mapped.map((entry) => entry.storage), fields.map((field) => groups.basket_claim_dust.storage[field]));
  assert.deepEqual(mapped.map((entry) => entry.proposed), ["1", "500", "2", "3"]);
  assert.ok(mapped.every((entry) => entry.setter === "AdminUtils.sudo_set_basket_claim_dust"));
});

test("unknown setters, substituted storage calls, and repeated calls are rejected", () => {
  assert.throws(() => mapCalls(batch([{ Sudo: { set_key: { new: "unauthorized" } } }])), /not in the reviewed allowlist/);
  assert.throws(() => mapCalls(batch([{ AdminUtils: { sudo_set_subnet_limit: { max_subnets: 161 } } }, {
    AdminUtils: { sudo_set_subnet_limit: { max_subnets: 162 } },
  }])), /Duplicate setter call/);
  assert.throws(() => mapCalls(batch([basketCall(), basketCall()])), /Duplicate setter call/);
});

test("grouped calls require every argument and cannot omit or add a member", () => {
  for (const argument of ["row_cap_rao", "row_bps", "slice_rao", "forfeit_cap_rao"]) {
    const call = basketCall();
    delete call.AdminUtils.sudo_set_basket_claim_dust[argument];
    assert.throws(() => mapCalls(batch([call])), new RegExp(argument));
  }
  const call = basketCall();
  call.AdminUtils.sudo_set_basket_claim_dust.extra_member = 4;
  assert.throws(() => mapCalls(batch([call])), /must contain exactly/);
});

test("single-value calls must match the exact setter argument and storage mapping", () => {
  const param = params.find((entry) => entry.name === "subnet_limit");
  assert.equal(snakeCase(param.argument), "max_subnets");
  assert.deepEqual(mapCalls(batch([{ AdminUtils: { sudo_set_subnet_limit: { max_subnets: 161 } } }])), [{
    key: "subnet_limit",
    proposed: "161",
    storage: ["SubtensorModule", "SubnetLimit"],
    setter: "AdminUtils.sudo_set_subnet_limit",
  }]);
  assert.throws(() => mapCalls(batch([{ AdminUtils: { sudo_set_subnet_limit: { default_take: 161 } } }])), /must contain exactly/);
  assert.throws(() => mapCalls(batch([{ AdminUtils: {
    sudo_set_subnet_limit: { max_subnets: 161 }, sudo_set_default_take: { default_take: 1 },
  } }])), /one function/);
});

test("the standalone mapper reads stdin and emits no executable helper or keys", () => {
  const mapper = path.join(__dirname, "..", "verify-proposal.js");
  const valid = spawnSync("node", [mapper], { input: JSON.stringify(batch([basketCall()])), encoding: "utf8" });
  assert.equal(valid.status, 0, valid.stderr);
  assert.equal(JSON.parse(valid.stdout).length, 4);
  const invalid = spawnSync("node", [mapper], { input: JSON.stringify(batch([{ Sudo: { sudo: {} } }])), encoding: "utf8" });
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /not in the reviewed allowlist/);
});
