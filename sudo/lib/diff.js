"use strict";

const { parse: parseToml } = require("smol-toml");
const { params, groups } = require("./params");

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be a table`);
  }
}

function loadParams(text) {
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
    if (entry === undefined) throw new Error(`Missing [${name}]`);
    assertPlainObject(entry, name);
    const expected = ["setter", ...Object.keys(group.storage)].sort().join(",");
    if (Object.keys(entry).sort().join(",") !== expected) {
      throw new Error(`[${name}] must contain setter and ${Object.keys(group.storage).join(", ")}`);
    }
    if (entry.setter !== `${group.callPallet}.${group.call}`) {
      throw new Error(`[${name}].setter must be ${group.callPallet}.${group.call}`);
    }
    for (const key of Object.keys(group.storage)) loaded[`${name}.${key}`] = entry[key];
    root.delete(name);
  }
  if (root.size) throw new Error(`Unknown tables: ${[...root.keys()].join(", ")}`);
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
    if (typeof bits !== "string" || !/^(0x[0-9a-fA-F]+|[0-9]+)$/.test(bits)) {
      throw new Error(`Invalid fixed-point bits: ${bits}`);
    }
    return { bits: bits.startsWith("0x") ? BigInt(bits).toString() : bits };
  }
  if (Array.isArray(value)) return value.map(normalize);
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, normalize(item)]));
}

function storagePlan() {
  return [
    ...params.map((param) => ({
      key: param.name,
      pallet: param.pallet,
      item: param.apiStorage || param.storage,
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
    unchanged: entries.filter((entry) => equalValue(entry.desired, entry.chain)),
    changed: entries.filter((entry) => !equalValue(entry.desired, entry.chain) &&
      (!baseline || !equalValue(entry.desired, entry.baseline))),
    drift: entries.filter((entry) => baseline && !equalValue(entry.desired, entry.chain) &&
      equalValue(entry.desired, entry.baseline)),
  };
}

function apiName(name) {
  return name.charAt(0).toLowerCase() + name.slice(1);
}

function setterName(name) {
  return name.replace(/_([a-z0-9])/g, (_, char) => char.toUpperCase());
}

function setterCalls(diff, api) {
  function numericArgs(value) {
    if (typeof value === "string" && /^[0-9]+$/.test(value) && BigInt(value) <= BigInt(Number.MAX_SAFE_INTEGER)) {
      return Number(value);
    }
    if (Array.isArray(value)) return value.map(numericArgs);
    if (value !== null && typeof value === "object") {
      if ("bits" in value && Object.keys(value).length === 1) {
        const bits = BigInt(value.bits);
        return { bits: bits <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(bits) : "0x" + bits.toString(16).padStart(32, "0") };
      }
      return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, numericArgs(item)]));
    }
    return value;
  }
  // Grouped settings have one all-or-nothing call even if only one member
  // changed. Including all values also makes the call's full effect explicit.
  const changes = new Map();
  for (const entry of diff.changed) {
    const id = `${entry.callPallet}.${entry.call}`;
    if (!changes.has(id)) changes.set(id, entry);
  }
  return [...changes.values()].map((entry) => {
    const builder = api.tx[apiName(entry.callPallet)]?.[setterName(entry.call)];
    if (typeof builder !== "function") {
      throw new Error(`Live metadata does not contain ${entry.callPallet}.${entry.call}`);
    }
    if (!builder.meta?.args || !builder.meta?.fields ||
        builder.meta.args.length !== builder.meta.fields.length) {
      throw new Error(`Malformed live metadata for ${entry.callPallet}.${entry.call}`);
    }
    const args = entry.args(normalize(entry.desired), normalize(diff.values || Object.fromEntries(diff.changed.map((item) => [item.key, item.desired]))));
    const numeric = numericArgs(args);
    const snakeArgs = Object.fromEntries(builder.meta.fields.map((field, i) => [
      field.name.toString(),
      numeric[builder.meta.args[i].name.toString()],
    ]));
    // Compose through the live registry; this validates both call existence
    // and SCALE argument types. It does not sign or submit an extrinsic.
    const call = builder(args);
    return { key: entry.key, target: `${entry.callPallet}.${entry.call}`, args, snakeArgs, call };
  });
}

function encodeProposal(diff, api) {
  const calls = setterCalls(diff, api);
  if (!calls.length) return null;
  const batch = api.tx.utility.batchAll(calls.map(({ call }) => call));
  const outer = api.tx.sudo.sudo(batch);
  const decoded = outer.toJSON();
  // Round-trip the outer call and every nested call against live metadata.
  const roundTrip = api.registry.createType("Call", outer.method.toU8a());
  if (roundTrip.toHex() !== outer.method.toHex()) {
    throw new Error("Outer call round-trip failed against live runtime metadata");
  }
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
    setters: calls.map(({ key, target, args, snakeArgs, call }) => ({
      key, target, args, btcliArgs: snakeArgs, callData: call.method.toHex(), callHash: call.method.hash.toHex(),
    })),
  };
}

module.exports = { apiName, loadParams, compare, encodeProposal, equalValue, normalize, setterCalls, storagePlan };
