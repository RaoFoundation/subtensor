#!/usr/bin/env node
"use strict";

// Map authenticated btcli call arguments through the reviewed allowlist. This
// program reads JSON on stdin and writes only the storage/precondition shape;
// it has no chain connection, signing path, or artifact-code execution.
const fs = require("node:fs");
const { params, groups } = require("./lib/params");
const { normalize } = require("./lib/diff");

function table(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be a JSON object`);
  }
  return value;
}

function exactKeys(value, expected, label) {
  table(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} must contain exactly: ${wanted.join(", ")}`);
  }
}

function snakeCase(name) {
  return name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
}

const singles = new Map(params.map((param) => [
  `${param.callPallet}.${param.call}`,
  param,
]));
const grouped = new Map(Object.entries(groups).map(([name, group]) => [
  `${group.callPallet}.${group.call}`,
  { name, ...group },
]));

function mapCalls(callArgs) {
  exactKeys(callArgs, ["call"], "Outer arguments");
  exactKeys(callArgs.call, ["Utility"], "Outer call");
  exactKeys(callArgs.call.Utility, ["batch_all"], "Utility call");
  const batch = callArgs.call.Utility.batch_all;
  exactKeys(batch, ["calls"], "utility.batch_all");
  if (!Array.isArray(batch.calls) || batch.calls.length === 0) {
    throw new Error("utility.batch_all.calls must be a nonempty array");
  }
  const affected = [];
  const seenSetters = new Set();
  const seenStorage = new Set();
  for (const [index, call] of batch.calls.entries()) {
    table(call, `Batch call ${index}`);
    if (Object.keys(call).length !== 1) throw new Error(`Batch call ${index} must have one pallet`);
    const [pallet, functions] = Object.entries(call)[0];
    table(functions, `Batch call ${index} pallet`);
    if (Object.keys(functions).length !== 1) throw new Error(`Batch call ${index} must have one function`);
    const [callFunction, args] = Object.entries(functions)[0];
    const target = `${pallet}.${callFunction}`;
    if (seenSetters.has(target)) throw new Error(`Duplicate setter call: ${target}`);
    seenSetters.add(target);
    let entries;
    if (singles.has(target)) {
      const param = singles.get(target);
      const argument = snakeCase(param.argument);
      exactKeys(args, [argument], target);
      entries = [{
        key: param.name,
        proposed: normalize(args[argument]),
        storage: [param.pallet, param.apiStorage || param.storage],
        setter: target,
      }];
    } else if (grouped.has(target)) {
      const group = grouped.get(target);
      const fields = Object.keys(group.storage);
      const argumentsByField = group.callArguments.map(snakeCase);
      exactKeys(args, argumentsByField, target);
      entries = fields.map((field, fieldIndex) => ({
        key: `${group.name}.${field}`,
        proposed: normalize(args[argumentsByField[fieldIndex]]),
        storage: group.storage[field],
        setter: target,
      }));
    } else {
      throw new Error(`Setter is not in the reviewed allowlist: ${target}`);
    }
    for (const entry of entries) {
      if (seenStorage.has(entry.key)) throw new Error(`Duplicate affected storage: ${entry.key}`);
      seenStorage.add(entry.key);
      affected.push(entry);
    }
  }
  return affected;
}

if (require.main === module) {
  try {
    const input = fs.readFileSync(0, "utf8");
    process.stdout.write(JSON.stringify(mapCalls(JSON.parse(input))));
  } catch (error) {
    process.stderr.write(`Invalid reviewed batch: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { mapCalls, snakeCase };
