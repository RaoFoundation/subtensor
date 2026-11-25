"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { stringify: stringifyToml } = require("smol-toml");
const { loadParams, compare, encodeProposal, normalize } = require("../lib/diff");
const { params, groups } = require("../lib/params");

function fixture() {
  const data = { metadata: { network: "test" } };
  for (const [i, param] of params.entries()) {
    data[param.name] = {
      setter: `${param.callPallet}.${param.call}`,
      value: ["net_tao_flow_enabled", "hashed_accounts_enabled", "gamma_enabled"].includes(param.name) ? false :
        param.name === "basket_trade_price_reference" ? "StrictestEmaAndSpot" :
        param.name === "tao_usd_price_oracle" ? "5E7RCRrPVS8TckCDjr92B5ciGziwz2kfvxe4URy3L7AgirGJ" :
        param.name === "subnet_moving_alpha" ? { bits: "100000" } : String(i + 1),
    };
  }
  for (const [name, group] of Object.entries(groups)) {
    data[name] = { setter: `${group.callPallet}.${group.call}` };
    for (const field of Object.keys(group.storage)) data[name][field] = "1";
  }
  return { toml: stringifyToml(data), values: loadParams(stringifyToml(data)).values };
}

function fakeApi() {
  const calls = [];
  const snakeCase = (name) => name.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
  const setterName = (name) => name.replace(/_([a-z0-9])/g, (_, char) => char.toUpperCase());
  const definitions = new Map([
    ...params.map((param) => [`${param.callPallet}.${setterName(param.call)}`, [param.argument]]),
    ...Object.values(groups).map((group) => [
      `${group.callPallet}.${setterName(group.call)}`, group.callArguments,
    ]),
  ]);
  const fakeCall = (target, args) => {
    const raw = Buffer.from(JSON.stringify({ target, args }));
    return {
      method: {
        toU8a: () => raw,
        toHex: () => "0x" + raw.toString("hex"),
        hash: { toHex: () => "0x" + require("node:crypto").createHash("sha256").update(raw).digest("hex") },
      },
      toJSON: () => ({ target, args }),
    };
  };
  return {
    calls,
    tx: new Proxy({}, {
      get: (target, pallet) => {
        if (Reflect.has(target, pallet)) return Reflect.get(target, pallet);
        return new Proxy({}, {
          get: (innerTarget, method) => {
            if (Reflect.has(innerTarget, method)) return Reflect.get(innerTarget, method);
            const argumentNames = definitions.get(`${String(pallet).charAt(0).toUpperCase()}${String(pallet).slice(1)}.${String(method)}`);
            const isBatch = pallet === "utility" && method === "batchAll";
            const isSudo = pallet === "sudo" && method === "sudo";
            if (!argumentNames && !isBatch && !isSudo) return undefined;
            const fn = (...values) => {
              const args = argumentNames ? Object.fromEntries(argumentNames.map((name, i) => [name, values[i]])) : values[0];
              calls.push({ pallet, method, args });
              return fakeCall(`${pallet}.${method}`, args);
            };
            fn.meta = {
              args: (argumentNames || []).map((name) => ({ name: { toString: () => name } })),
              fields: (argumentNames || []).map((name) => ({ name: { toString: () => snakeCase(name) } })),
            };
            return fn;
          },
        });
      },
    }),
    registry: {
      createType: (_type, raw) => ({
        args: Object.values(JSON.parse(Buffer.from(raw).toString()).args).map((value) => ({
          toPrimitive: () => value,
        })),
        toHex: () => "0x" + Buffer.from(raw).toString("hex"),
        toHuman: () => {
          const { target, args } = JSON.parse(Buffer.from(raw).toString());
          const [section, method] = target.split(".");
          const snakeKeys = (value) => {
            if (value?.method) return snakeKeys(value.method);
            if (Array.isArray(value)) return value.map(snakeKeys);
            if (value && typeof value === "object") {
              return Object.fromEntries(Object.entries(value).map(([key, item]) => [snakeCase(key), snakeKeys(item)]));
            }
            return String(value);
          };
          return { section, method, args: snakeKeys(args) };
        },
      }),
    },
  };
}

test("loads every declared parameter and rejects unknown tables", () => {
  const { toml, values } = fixture();
  assert.equal(Object.keys(values).length, 51);
  assert.equal(loadParams(toml).values.subnet_limit, "6");
  assert.throws(() => loadParams(toml + "\n[not_a_global]\nvalue = 1\n"), /Unknown tables/);
});

test("rejects setter substitution and missing values", () => {
  const { toml } = fixture();
  assert.throws(() => loadParams(toml.replace("sudo_set_subnet_limit", "sudo_set_default_take")), /setter must/);
  assert.throws(() => loadParams(toml.replace(/\[default_take\][\s\S]*?\n\n/, "")), /Missing \[default_take\]/);
});

test("represents a disabled optional oracle without TOML null", () => {
  const { toml, values } = fixture();
  const address = values.tao_usd_price_oracle;
  assert.equal(typeof address, "string");
  const original = `value = ${JSON.stringify(address)}`;
  assert.ok(toml.includes(original));
  const disabled = toml.replace(original, "value = { none = true }");
  assert.equal(loadParams(disabled).values.tao_usd_price_oracle, null);
  const changed = compare({ ...values, tao_usd_price_oracle: null }, values, values);
  const encoded = encodeProposal(changed, fakeApi());
  assert.equal(encoded.setters[0].args.oracle, null);
  assert.equal(encoded.btcliArgs.call.Utility.batch_all.calls[0].AdminUtils.sudo_set_tao_usd_price_oracle.oracle, null);
  assert.throws(() => loadParams(toml.replace(original, "value = 123")), /SS58 string/);
});

test("normalizes polkadot-js fixed-point, optional, and large integer values", () => {
  assert.deepEqual(normalize({ bits: "0x100" }), { bits: "256" });
  assert.deepEqual(normalize({ bits: 1288490 }), { bits: "1288490" });
  assert.equal(normalize(100n), "100");
  assert.equal(normalize({ none: true }), null);
  assert.equal(normalize(null), null);
  assert.throws(() => normalize({ bits: "not-a-number" }), /Invalid fixed-point/);
});

test("skips unchanged values and proposes only deliberate file edits", () => {
  const { values } = fixture();
  const chain = { ...values, subnet_limit: "99", tx_rate_limit: "99" };
  const desired = { ...values, subnet_limit: "160" };
  const diff = compare(desired, chain, values);
  assert.equal(diff.unchanged.length, 49);
  assert.deepEqual(diff.changed.map((entry) => entry.key), ["subnet_limit"]);
  assert.deepEqual(diff.drift.map((entry) => entry.key), ["tx_rate_limit"]);
});

test("without a base file, mismatches are unclassified drift and produce no call", () => {
  const { values } = fixture();
  const diff = compare({ ...values, subnet_limit: "161" }, values);
  assert.equal(diff.reviewed, false);
  assert.deepEqual(diff.changed.map((entry) => entry.key), ["subnet_limit"]);
  assert.deepEqual(diff.drift.map((entry) => entry.key), ["subnet_limit"]);
  assert.equal(encodeProposal(diff, fakeApi()), null);
});

test("grouped basket dust is one atomic call using all four fields", () => {
  const { values } = fixture();
  const diff = compare({ ...values, "basket_claim_dust.row_bps": "10" }, values, values);
  const api = fakeApi();
  const proposal = encodeProposal(diff, api);
  const dust = proposal.setters.filter((call) => call.target.endsWith("sudo_set_basket_claim_dust"));
  assert.equal(dust.length, 1);
  assert.deepEqual(dust[0].args, { rowCapRao: "1", rowBps: "10", sliceRao: "1", forfeitCapRao: "1" });
  assert.deepEqual(dust[0].btcliArgs, { row_cap_rao: 1, row_bps: 10, slice_rao: 1, forfeit_cap_rao: 1 });
  assert.deepEqual(dust[0].affected.map((entry) => entry.key), [
    "basket_claim_dust.row_cap_rao", "basket_claim_dust.row_bps",
    "basket_claim_dust.slice_rao", "basket_claim_dust.forfeit_cap_rao",
  ]);
});

test("a grouped proposal rejects unrelated member drift", () => {
  const { values } = fixture();
  const chain = { ...values, "basket_claim_dust.row_cap_rao": "999" };
  const diff = compare({ ...values, "basket_claim_dust.row_bps": "10" }, chain, values);
  assert.throws(() => encodeProposal(diff, fakeApi()), /unrelated member drift.*row_cap_rao/);
});

test("composes batch and sudo, validates round-trip, and emits snake-case btcli args", () => {
  const { values } = fixture();
  const diff = compare({ ...values, subnet_limit: "160" }, values, values);
  const proposal = encodeProposal(diff, fakeApi());
  assert.match(proposal.callData, /^0x/);
  assert.match(proposal.callHash, /^0x[0-9a-f]{64}$/);
  const subnet = proposal.setters.find((call) => call.target.endsWith("sudo_set_subnet_limit"));
  assert.equal(subnet.btcliArgs.max_subnets, 160);
  assert.equal(proposal.btcliArgs.call.Utility.batch_all.calls.length, proposal.setters.length);
});

test("encodes fixed-point values as their raw bits", () => {
  const { values } = fixture();
  const diff = compare({ ...values, subnet_moving_alpha: { bits: "18446744073709551616" } }, values, values);
  const encoded = encodeProposal(diff, fakeApi());
  assert.deepEqual(encoded.setters[0].args, { alpha: { bits: "18446744073709551616" } });
  assert.deepEqual(encoded.setters[0].btcliArgs, { alpha: { bits: "18446744073709551616" } });
});

test("no changes and drift-only comparisons produce no call", () => {
  const { values } = fixture();
  assert.equal(encodeProposal(compare(values, values, values), fakeApi()), null);
  const chain = { ...values, subnet_limit: "99" };
  assert.equal(encodeProposal(compare(values, chain, values), fakeApi()), null);
});

test("malformed metadata and failed call round trips are rejected", () => {
  const { values } = fixture();
  const diff = compare({ ...values, subnet_limit: "160" }, values, values);
  const malformed = fakeApi();
  malformed.tx.adminUtils = new Proxy({}, {
    get: (_target, property) => property === "sudoSetSubnetLimit" ? (() => ({})) : undefined,
  });
  assert.throws(() => encodeProposal(diff, malformed), /Malformed live metadata/);
  const mismatched = fakeApi();
  const roundTrip = mismatched.registry.createType;
  mismatched.registry.createType = (type, raw) => ({
    ...roundTrip(type, raw),
    toHex: () => "0xdeadbeef",
  });
  assert.throws(() => encodeProposal(diff, mismatched), /Outer call round-trip failed/);
});

test("live metadata missing a setter fails before proposal", () => {
  const { values } = fixture();
  const diff = compare({ ...values, subnet_limit: "160" }, values, values);
  const api = fakeApi();
  api.tx.adminUtils = {};
  assert.throws(() => encodeProposal(diff, api), /Live metadata does not contain/);
});

test("historical schemas can add and retire settings without treating existing values as edits", () => {
  const { toml, values } = fixture();
  const historical = toml.replace(/\[default_take\][\s\S]*?\n\n/, "") +
    "\n[retired_parameter]\nvalue = 1\n";
  const baseline = loadParams(historical, { historical: true }).values;
  assert.deepEqual(compare(values, { ...values, default_take: "0", tx_rate_limit: "999" }, baseline)
    .changed.map((entry) => entry.key), ["default_take"]);
});

test("valid SCALE bytes with the wrong value are refused", () => {
  const { values } = fixture();
  const api = fakeApi();
  const original = api.registry.createType;
  api.registry.createType = (...args) => ({
    ...original(...args),
    args: [{ toPrimitive: () => true }],
  });
  const old = { ...values, gamma_enabled: true };
  assert.throws(() => encodeProposal(compare(values, old, old), api), /Decoded arguments differ/);
});

test("signed cutoff bits round-trip and don't block unrelated changes", () => {
  const { values, toml } = fixture();
  const signed = loadParams(toml.replace(/(\[tao_flow_cutoff\][^\[]*?value = )"\d+"/,
    '$1{ bits = "-1" }')).values;
  assert.deepEqual(signed.tao_flow_cutoff, { bits: "-1" });
  assert.deepEqual(normalize({ bits: "-170141183460469231731687303715884105728" }),
    { bits: "-170141183460469231731687303715884105728" });
  const proposal = encodeProposal(compare(signed, values, values), fakeApi());
  assert.deepEqual(proposal.setters[0].btcliArgs.flow_cutoff, { bits: -1 });
  const unrelated = encodeProposal(compare({ ...signed, subnet_limit: "160" }, signed, signed), fakeApi());
  assert.deepEqual(unrelated.setters.map(({ key }) => key), ["subnet_limit"]);
});
