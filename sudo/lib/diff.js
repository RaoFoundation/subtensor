"use strict";

const { parse: parseToml } = require("smol-toml");
const { stringCamelCase } = require("@polkadot/util");
const { params, groups } = require("./params");

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be a table`);
  }
}

function loadParams(text, { historical = false } = {}) {
  let parsed;
  try {
    parsed = parseToml(text, { integersAsBigInt: "asNeeded" });
  } catch (error) {
    throw new Error(`Invalid TOML: ${error.message}`);
  }
  const root = new Map(Object.entries(parsed));
  const metadata = root.get("metadata");
  root.delete("metadata");
  if (metadata !== undefined) assertPlainObject(metadata, "metadata");
  const loaded = {};
  for (const param of params) {
    const entry = root.get(param.name);
    if (historical && entry === undefined) continue;
    if (entry === undefined) throw new Error(`Missing [${param.name}]`);
    assertPlainObject(entry, param.name);
    if (entry.setter !== `${param.callPallet}.${param.call}`) {
      throw new Error(`[${param.name}].setter must be ${param.callPallet}.${param.call}`);
    }
    if (Object.keys(entry).sort().join(",") !== "setter,value") {
      throw new Error(`[${param.name}] must contain only setter and value`);
    }
    const value = normalize(entry.value);
    if (param.optional && value !== null && typeof entry.value !== "string") {
      throw new Error(`[${param.name}].value must be an SS58 string or { none = true }`);
    }
    loaded[param.name] = value;
    root.delete(param.name);
  }
  for (const [name, group] of Object.entries(groups)) {
    const entry = root.get(name);
    if (historical && entry === undefined) continue;
    if (entry === undefined) throw new Error(`Missing [${name}]`);
    assertPlainObject(entry, name);
    const expected = ["setter", ...Object.keys(group.storage)].sort().join(",");
    if (!historical && Object.keys(entry).sort().join(",") !== expected) {
      throw new Error(`[${name}] must contain setter and ${Object.keys(group.storage).join(", ")}`);
    }
    if (entry.setter !== `${group.callPallet}.${group.call}`) {
      throw new Error(`[${name}].setter must be ${group.callPallet}.${group.call}`);
    }
    for (const key of Object.keys(group.storage)) {
      if (Object.hasOwn(entry, key)) loaded[`${name}.${key}`] = normalize(entry[key]);
    }
    root.delete(name);
  }
  if (!historical && root.size) throw new Error(`Unknown tables: ${[...root.keys()].join(", ")}`);
  return { metadata: metadata || {}, values: loaded };
}

function normalize(value) {
  if (value !== null && typeof value === "object" && !Array.isArray(value) &&
      Object.keys(value).length === 1 && value.none === true) return null;
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) throw new Error(`Unsafe numeric chain value: ${value}`);
    return value.toString();
  }
  if (value === null || typeof value !== "object") return value;
  if (!Array.isArray(value) && Object.keys(value).length === 1 && "bits" in value) {
    const bits = typeof value.bits === "number" || typeof value.bits === "bigint" ? value.bits.toString() : value.bits;
    if (typeof bits !== "string" || !/^(0x[0-9a-fA-F]+|-?[0-9]+)$/.test(bits)) {
      throw new Error(`Invalid fixed-point bits: ${bits}`);
    }
    if (typeof value.bits === "number" && !Number.isSafeInteger(value.bits)) throw new Error("Unsafe fixed-point bits");
    return { bits: BigInt(bits).toString() };
  }
  if (Array.isArray(value)) return value.map(normalize);
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, normalize(item)]));
}

function storagePlan() {
  return [
    ...params.map((param) => ({
      key: param.name,
      pallet: param.pallet,
      item: param.storage,
      callPallet: param.callPallet,
      call: param.call,
      args: (value) => ({ [param.argument]: value }),
    })),
    ...Object.entries(groups).flatMap(([name, group]) =>
      Object.entries(group.storage).map(([field, [pallet, item]]) => ({
        key: `${name}.${field}`,
        pallet,
        item,
        callPallet: group.callPallet,
        call: group.call,
        group: name,
        args: (_value, all) => Object.fromEntries(group.callArguments.map((arg, i) => {
          const field = Object.keys(group.storage)[i];
          return [arg, all[`${name}.${field}`]];
        })),
      })),
    ),
  ];
}

function equalValue(left, right) {
  // BigInt has no JSON representation; stable strings also normalize numeric
  // codec results such as u64 -> number or string before comparison.
  left = normalize(left);
  right = normalize(right);
  if (left === null || right === null) return left === right;
  if (left === right) return true;
  if (typeof left !== "object" || typeof right !== "object") return false;
  if (Array.isArray(left) !== Array.isArray(right)) return false;
  if (Array.isArray(left)) {
    return left.length === right.length && left.every((value, i) => equalValue(value, right[i]));
  }
  const a = Object.keys(left).sort();
  const b = Object.keys(right).sort();
  return a.length === b.length && a.every((key, i) => key === b[i] && equalValue(left[key], right[key]));
}

function compare(desired, chain, baseline = null) {
  const plan = storagePlan();
  const entries = plan.map((entry) => {
    if (!(entry.key in chain)) throw new Error(`No chain value for ${entry.key}`);
    return {
      ...entry,
      desired: normalize(desired[entry.key]),
      chain: normalize(chain[entry.key]),
      ...(baseline && entry.key in baseline ? { baseline: normalize(baseline[entry.key]) } : {}),
    };
  });
  return {
    values: normalize(desired),
    chainValues: normalize(chain),
    reviewed: Boolean(baseline),
    unchanged: entries.filter((entry) => equalValue(entry.desired, entry.chain)),
    changed: entries.filter((entry) => !equalValue(entry.desired, entry.chain) &&
      (!baseline || !equalValue(entry.desired, entry.baseline))),
    drift: entries.filter((entry) => !equalValue(entry.desired, entry.chain) &&
      (!baseline || equalValue(entry.desired, entry.baseline))),
  };
}

function apiName(name) {
  return stringCamelCase(name);
}

function setterName(name) {
  return name.replace(/_([a-z0-9])/g, (_, char) => char.toUpperCase());
}

// Keep unsafe integers as exact decimal strings in machine-readable artifacts.
// At the CLI boundary, use JSON number tokens without a JS Number conversion.
function safeJsonValues(value, raw = false) {
  if (typeof value === "string" && /^-?(0|[1-9][0-9]*)$/.test(value)) {
    const n = Number(value);
    return Number.isSafeInteger(n) ? n : raw ? JSON.rawJSON(value) : value;
  }
  if (Array.isArray(value)) return value.map((item) => safeJsonValues(item, raw));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, safeJsonValues(item, raw)]));
  }
  return value;
}

function setterCalls(diff, api) {
  // Grouped settings have one all-or-nothing call even if only one member
  // changed. Including all values also makes the call's full effect explicit.
  const changes = new Map();
  for (const entry of diff.changed) {
    const id = `${entry.callPallet}.${entry.call}`;
    if (!changes.has(id)) changes.set(id, entry);
  }
  return [...changes.values()].map((entry) => {
    // A grouped call writes every member. Do not let an unedited live drift
    // enter the call under the guise of a single reviewed member edit.
    const members = entry.group ? diff.drift.filter((item) => item.group === entry.group) : [];
    if (members.length) {
      throw new Error(`Grouped proposal ${entry.group} also contains unrelated member drift: ${members.map((item) => item.key).join(", ")}; reconcile every member first`);
    }
    const builder = api.tx[apiName(entry.callPallet)]?.[setterName(entry.call)];
    if (typeof builder !== "function") {
      throw new Error(`Live metadata does not contain ${entry.callPallet}.${entry.call}`);
    }
    if (!builder.meta?.args || !builder.meta?.fields ||
        builder.meta.args.length !== builder.meta.fields.length) {
      throw new Error(`Malformed live metadata for ${entry.callPallet}.${entry.call}`);
    }
    const args = entry.args(normalize(entry.desired), normalize(diff.values || Object.fromEntries(diff.changed.map((item) => [item.key, item.desired]))));
    const affected = entry.group ? storagePlan().filter((item) => item.group === entry.group).map((item) => ({
      key: item.key,
      live: normalize(diff.chainValues[item.key]),
      proposed: normalize(diff.values[item.key]),
      storage: [item.pallet, item.item],
      setter: `${item.callPallet}.${item.call}`,
    })) : [{
      key: entry.key,
      live: normalize(entry.chain),
      proposed: normalize(entry.desired),
      storage: [entry.pallet, entry.item],
      setter: `${entry.callPallet}.${entry.call}`,
    }];
    const names = builder.meta.fields.map((field) => field.name.toString());
    const expected = Object.keys(args).map((name) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
    if (names.length !== expected.length || names.some((name, i) => name !== expected[i])) {
      throw new Error(`Argument mapping differs from live metadata for ${entry.callPallet}.${entry.call}: ${names.join(", ")}`);
    }
    const values = Object.values(args);
    const call = builder(...values);
    const decoded = api.registry.createType("Call", call.method.toU8a());
    const decodedArgs = decoded.args.map((arg) => normalize(arg.toPrimitive()));
    if (decodedArgs.length !== values.length || values.some((value, i) => !equalValue(value, decodedArgs[i]))) {
      throw new Error(`Decoded arguments differ from requested values for ${entry.callPallet}.${entry.call}`);
    }
    const snakeArgs = Object.fromEntries(names.map((name, i) => [name, safeJsonValues(decodedArgs[i])]));
    return { key: entry.key, target: `${entry.callPallet}.${entry.call}`, args, snakeArgs, affected, call };
  });
}

function encodeProposal(diff, api) {
  if (diff.changed.length && !diff.reviewed) return null;
  const calls = setterCalls(diff, api);
  if (!calls.length) return null;
  const batch = api.tx.utility.batchAll(calls.map(({ call }) => call));
  const outer = api.tx.sudo.sudo(batch);
  // Round-trip the outer call and every nested call against live metadata.
  const roundTrip = api.registry.createType("Call", outer.method.toU8a());
  if (roundTrip.toHex() !== outer.method.toHex()) {
    throw new Error("Outer call round-trip failed against live runtime metadata");
  }
  const decoded = roundTrip.toHuman();
  for (const { call } of calls) {
    if (api.registry.createType("Call", call.method.toU8a()).toHex() !== call.method.toHex()) {
      throw new Error("Setter call round-trip failed against live runtime metadata");
    }
  }
  return {
    callData: outer.method.toHex(),
    callHash: outer.method.hash.toHex(),
    decoded: decoded,
    batchData: batch.method.toHex(),
    btcliArgs: {
      call: {
        Utility: {
          batch_all: {
            calls: calls.map(({ target, snakeArgs }) => {
              const [call_module, call_function] = target.split(".");
              return { [call_module]: { [call_function]: snakeArgs } };
            }),
          },
        },
      },
    },
    setters: calls.map(({ key, target, args, snakeArgs, affected, call }) => ({
      key, target, args, btcliArgs: snakeArgs, affected, callData: call.method.toHex(), callHash: call.method.hash.toHex(),
    })),
  };
}

module.exports = { apiName, loadParams, compare, encodeProposal, equalValue, normalize, setterCalls, storagePlan, safeJsonValues };
