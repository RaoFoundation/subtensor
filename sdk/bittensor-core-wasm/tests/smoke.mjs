// Node smoke test for the wasm binding, driven by the same golden fixtures
// the Python SDK tests use. Build first:
//   wasm-pack build sdk/bittensor-core-wasm --target nodejs --out-dir pkg-node
// Then: node sdk/bittensor-core-wasm/tests/smoke.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const here = dirname(fileURLToPath(import.meta.url));
const core = await import(join(here, "..", "pkg-node", "bittensor_core_wasm.js"));

const golden = JSON.parse(
  readFileSync(join(here, "..", "..", "python", "tests", "fixtures", "golden.json"), "utf8"),
);

const fromHex = (hex) => Uint8Array.from(Buffer.from(hex.replace(/^0x/, ""), "hex"));
const toHex = (bytes) => "0x" + Buffer.from(bytes).toString("hex");

// The fixture stores the raw `Metadata_metadata_at_version` response:
// Option<OpaqueMetadata>, i.e. 0x01 + compact byte length + the blob.
const unwrapOpaque = (bytes) => {
  assert.equal(bytes[0], 1, "fixture metadata is Some");
  const mode = bytes[1] & 3;
  const skip = mode === 0 ? 1 : mode === 1 ? 2 : mode === 2 ? 4 : (bytes[1] >> 2) + 5;
  return bytes.subarray(1 + skip);
};
const metadataBytes = unwrapOpaque(fromHex(golden.metadata.v15_hex));

// --- keys --------------------------------------------------------------------

assert.equal(typeof core.coreVersion(), "string");

const alice = core.Keypair.fromUri("//Alice");
assert.equal(alice.ss58Address, "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY");

const message = new TextEncoder().encode("hello, bittensor!");
const signature = alice.sign(message);
assert.equal(signature.length, 64);
assert.equal(alice.verify(message, signature), true);
assert.equal(core.verifySignature(message, signature, alice.ss58Address), true);

for (const entry of golden.ss58) {
  assert.equal(toHex(core.ss58Decode(entry.address)), entry.public_key_hex);
  assert.equal(core.ss58Encode(fromHex(entry.public_key_hex), entry.ss58_format), entry.address);
}

// --- runtime -----------------------------------------------------------------

const net = golden.network;
const runtime = new core.Runtime(
  metadataBytes,
  net.spec_version,
  net.transaction_version,
  net.ss58_format,
);
assert.equal(runtime.specVersion, net.spec_version);
assert.equal(runtime.isV15, true);

// Sudo/Utility/Proxy fixtures embed inner calls via harness-specific string
// aliases; exercise Sudo by composing the inner call to bytes explicitly and
// skip the alias-based ones.
for (const call of golden.calls) {
  let params = call.params;
  if (call.module === "Sudo") {
    const inner = call.params.call;
    const { module: m, function: f, ...args } = inner;
    params = { call: runtime.composeCall(m, f, args) };
  } else if (call.module === "Utility" || call.module === "Proxy") {
    continue;
  }
  const data = runtime.composeCall(call.module, call.function, params);
  assert.equal(toHex(data), call.data_hex, `${call.module}.${call.function}`);
  const decoded = runtime.decodeCall(data);
  assert.equal(decoded.call_module, call.module);
  assert.equal(decoded.call_function, call.function);
}

for (const sk of golden.storage_keys) {
  const key = runtime.storageKey(sk.pallet, sk.storage_function, sk.params);
  assert.equal(toHex(key), sk.key_hex, `${sk.pallet}.${sk.storage_function}`);
}

for (const sv of golden.storage_values) {
  if (sv.raw_hex == null) continue; // recorded storage misses
  const entry = runtime.storageEntry(sv.pallet, sv.storage_function);
  const decoded = runtime.decode(entry.valueType, fromHex(sv.raw_hex));
  assert.ok(decoded !== undefined, `${sv.pallet}.${sv.storage_function} decodes`);
}

for (const c of golden.constants) {
  const decoded = runtime.constant(c.module, c.name);
  if (typeof c.decoded === "number") {
    assert.equal(Number(decoded), c.decoded, `${c.module}.${c.name}`);
  }
}

for (const sp of golden.signature_payloads) {
  const eraBlockHash = sp.era_birth_block_hash ?? net.genesis_hash;
  const payload = runtime.signaturePayload(
    fromHex(sp.call_data_hex),
    sp.era,
    sp.nonce,
    sp.tip,
    null,
    fromHex(net.genesis_hash),
    fromHex(eraBlockHash),
  );
  assert.equal(toHex(payload), sp.payload_hex);
}

// Sign a real payload and assemble a structurally valid extrinsic.
{
  const sp = golden.signature_payloads[0];
  const payload = runtime.signaturePayload(
    fromHex(sp.call_data_hex),
    sp.era,
    sp.nonce,
    sp.tip,
    null,
    fromHex(net.genesis_hash),
    fromHex(net.genesis_hash),
  );
  const sig = alice.sign(payload);
  const [extrinsic, hash] = runtime.encodeSignedExtrinsic(
    fromHex(sp.call_data_hex),
    alice.publicKey,
    sig,
    1, // sr25519
    sp.era,
    sp.nonce,
    sp.tip,
    null,
  );
  assert.ok(extrinsic.length > 100);
  assert.equal(hash.length, 32);
  const decoded = runtime.decodeExtrinsic(extrinsic);
  assert.equal(decoded.call.call_module, "Balances");
}

// --- marshalling boundaries ------------------------------------------------------

// The golden fixtures stay inside the f64-safe range; exercise the BigInt,
// u256-decimal, and depth-limit paths explicitly so they stay enforced.
{
  const roundTrip = (type, value) => runtime.decode(type, runtime.encode(type, value));

  // Safe-integer boundary: 2^53 - 1 passes as a number, 2^53 must be BigInt.
  assert.equal(roundTrip("u64", 9007199254740991), 9007199254740991);
  assert.throws(() => runtime.encode("u64", 9007199254740992), /use BigInt/);
  assert.equal(roundTrip("u64", 9007199254740992n), 9007199254740992n);

  // u128 extremes round-trip digit-exact through BigInt.
  const u128Max = 340282366920938463463374607431768211455n;
  assert.equal(roundTrip("u128", u128Max), u128Max);
  assert.equal(roundTrip("u128", 2n ** 100n), 2n ** 100n);

  // Negative i128 and out-of-range rejections.
  assert.equal(roundTrip("i128", -(2n ** 100n)), -(2n ** 100n));
  assert.throws(() => runtime.encode("u128", 2n ** 256n));
  assert.throws(() => runtime.encode("i128", -(2n ** 200n)), /below i128 range/);

  // Non-integer numbers never silently truncate.
  assert.throws(() => runtime.encode("u64", 1.5), /use BigInt/);

  // Decode side of the same boundary: values above 2^53 - 1 come back BigInt.
  assert.equal(typeof runtime.decode("u128", runtime.encode("u128", 1000)), "number");
  assert.equal(typeof runtime.decode("u128", runtime.encode("u128", u128Max)), "bigint");

  // The recursion ceiling fails cleanly instead of overflowing the stack.
  let nested = [];
  for (let i = 0; i < 300; i++) nested = [nested];
  assert.throws(() => runtime.encode("Vec<u8>", nested), /nesting exceeds/);
}

// --- digest --------------------------------------------------------------------

const digest = core.metadataDigest(metadataBytes, net.spec_version, "node-subtensor");
assert.equal(digest.length, 32);

// --- timelock / mlkem ----------------------------------------------------------

const [ciphertext, round] = core.encryptAtRound(new TextEncoder().encode("secret"), 17200000);
assert.equal(round, 17200000);
assert.equal(core.revealRound(ciphertext), 17200000);
assert.ok(core.mlkemKdfId().length > 0);

const [commit, commitRound] = core.getEncryptedCommitV2(
  new Uint16Array([1, 2, 3]),
  new Uint16Array([100, 200, 300]),
  42, // versionKey
  100, // lastEpochBlock
  0, // pendingEpochAt
  0, // subnetEpochIndex
  50, // tempo
  0, // blocksSinceLastStep
  120, // currentBlock
  1, // subnetRevealPeriodEpochs
  12.0,
  new Uint8Array([1, 2, 3]),
);
assert.ok(commit.length > 0 && commitRound > 0);

console.log("smoke: all assertions passed");
