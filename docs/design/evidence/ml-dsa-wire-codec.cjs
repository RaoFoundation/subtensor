'use strict';
// Legacy codec regression using runtime-exported transactions plus a malformed
// carrier control. This script checks decoding; runtime tests verify authority.
// Usage: node ml-dsa-wire-codec.cjs FIXTURE RESULTS [SDK_CALL_RESULTS]
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { readFileSync, writeFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const { resolve } = require('node:path');
if (!process.argv[2] || !process.argv[3]) {
  throw new Error('Usage: node ml-dsa-wire-codec.cjs fixture.json results.json');
}
const req = createRequire(resolve(__dirname, '../../../ts-tests/package.json'));
const { TypeRegistry, Metadata } = req('@polkadot/types');
const { blake2AsHex } = req('@polkadot/util-crypto');
const { compactAddLength, u8aConcat } = req('@polkadot/util');
const fixturePath = resolve(process.argv[2]);
const resultsPath = resolve(process.argv[3]);
const fixtureBytes = readFileSync(fixturePath);
const fixture = JSON.parse(fixtureBytes);
const operations = process.argv[4] ? JSON.parse(readFileSync(resolve(process.argv[4]))).extrinsics : [];
const packageInfo = req('@polkadot/types/packageInfo').packageInfo;
const output = {
  test: 'Runtime ML-DSA signed-v4 transactions and synthetic carrier control; legacy codec compatibility',
  fixturePath,
  scriptSha256: createHash("sha256").update(readFileSync(__filename)).digest("hex"),
  sdkCallEvidenceSha256: process.argv[4] ? createHash("sha256").update(readFileSync(resolve(process.argv[4]))).digest("hex") : null,
  fixtureSha256: createHash('sha256').update(fixtureBytes).digest('hex'),
  package: {name: packageInfo.name, version: packageInfo.version, path: packageInfo.path},
  limitations: ['This script checks decoding, not cryptographic verification', 'Metadata and signed transaction provenance is supplied by the runtime test fixture'],
  versions: [],
};
for (const [version, encoded] of Object.entries(fixture.metadata)) {
  const registry = new TypeRegistry();
  registry.setMetadata(new Metadata(registry, encoded));
  for (const operation of operations) {
    const decoded = registry.createType('Extrinsic', operation.hex);
    assert.equal(decoded.isSigned, true);
    assert.equal(decoded.signer.type, 'Raw');
    assert.equal(decoded.signer.value.length, 5367);
    assert.equal(decoded.toHex(), operation.hex);
    assert.equal(decoded.hash.toHex(), operation.hash);
    assert.equal(decoded.method.meta.name.toString(), operation.function);
  }
  output.consolidatedCallsPerMetadataVersion = operations.length;
  const standardAndHashed = fixture.extrinsics;
  const mixedOperations = [];
  for (let i = 0; i < Math.max(standardAndHashed.length, operations.length); i++) {
    if (i < standardAndHashed.length) mixedOperations.push(standardAndHashed[i]);
    if (i < operations.length) mixedOperations.push(operations[i].hex);
  }
  const operationBlock = registry.createType('SignedBlock', {
    block: {
      header: {parentHash: new Uint8Array(32), number: 1, stateRoot: new Uint8Array(32), extrinsicsRoot: new Uint8Array(32), digest: {logs: []}},
      extrinsics: mixedOperations,
    },
    justifications: null,
  });
  operationBlock.block.extrinsics.forEach((tx, index) => {
    assert.equal(tx.toHex(), mixedOperations[index]);
    assert.equal(tx.hash.toHex(), blake2AsHex(mixedOperations[index]));
  });
  output.mixedConsolidatedBlockExtrinsics = mixedOperations.length;
  const decoded = standardAndHashed.map(wire => registry.createType('Extrinsic', wire));
  const real = decoded.find(tx => tx.isSigned && tx.signer.type === 'Raw' && tx.signer.value.length === 5367);
  assert.ok(real, 'fixture must contain a real runtime-signed ML-DSA transaction');
  const template = decoded.find(tx => tx.isSigned && tx.signer.type === 'Raw' && tx.signer.value.length === 106);
  assert.ok(template, 'fixture must retain legacy hashed-account coverage');
  assert.equal(template.signer.type, 'Raw');
  const account = template.signer.value.subarray(0, 32);
  const envelope = new Uint8Array(5367);
  envelope.set(account, 0); // permanent AccountId32 must stay first
  envelope[32] = 2; // proposed envelope version
  envelope[33] = 4; // proposed algorithm; opaque to legacy codecs
  envelope[34] = 7; // generation = 7, little endian u64
  envelope.fill(0x23, 42, 1994); // synthetic ML-DSA-65 public key, 1952 bytes
  envelope.fill(0x37, 1994, 2058); // synthetic next commitment, 64 bytes
  envelope.fill(0x49, 2058, 5367); // synthetic ML-DSA-65 proof, 3309 bytes
  const signer = registry.createType('Address', { Raw: '0x' + Buffer.from(envelope).toString('hex') });
  const extras = Object.keys(registry.getSignedExtensionTypes()).map(k => template.inner.signature.get(k).toU8a());
  function makeExtrinsic(carrier) {
    const sig = registry.createType('ExtrinsicSignature', { Ed25519: carrier }).toU8a();
    return '0x' + Buffer.from(compactAddLength(u8aConcat(Uint8Array.of(0x84), signer.toU8a(), sig, ...extras, template.method.toU8a()))).toString('hex');
  }
  const ms = makeExtrinsic(new Uint8Array(64).fill(0x4d));
  const tx = registry.createType('Extrinsic', ms);
  assert.equal(tx.isSigned, true);
  assert.equal(tx.signer.type, 'Raw');
  assert.equal(tx.signer.value.length, 5367);
  assert.equal(tx.toHex(), ms);
  assert.equal(tx.hash.toHex(), blake2AsHex(ms));
  assert.equal(tx.method.toHex(), template.method.toHex());
  assert.equal(registry.createType('AccountId', tx.signer.toString()).toHex(), '0x' + Buffer.from(account).toString('hex'));
  for (const key of Object.keys(registry.getSignedExtensionTypes())) {
    assert.equal(tx.inner.signature.get(key).toHex(), template.inner.signature.get(key).toHex());
  }
  const positions = [];
  const otherTransactions = standardAndHashed.filter(wire => wire !== real.toHex());
  for (let position = 0; position <= otherTransactions.length; position++) {
    const transactions = [...otherTransactions];
    transactions.splice(position, 0, real.toHex());
    const block = registry.createType('SignedBlock', {
      block: {
        header: {parentHash: new Uint8Array(32), number: 1, stateRoot: new Uint8Array(32), extrinsicsRoot: new Uint8Array(32), digest: {logs: []}},
        extrinsics: transactions,
      },
      justifications: null,
    });
    assert.equal(block.block.extrinsics.length, transactions.length);
    block.block.extrinsics.forEach((item, index) => {
      assert.equal(item.toHex(), transactions[index]);
      assert.equal(item.hash.toHex(), blake2AsHex(transactions[index]));
      assert.equal(item.method.toHex(), registry.createType('Extrinsic', transactions[index]).method.toHex());
      const source = decoded.find(original => original.toHex() === transactions[index]);
      assert.ok(source);
      assert.equal(item.isSigned, source.isSigned);
      if (source.isSigned) {
        for (const key of Object.keys(registry.getSignedExtensionTypes())) {
          assert.equal(item.inner.signature.get(key).toHex(), source.inner.signature.get(key).toHex());
        }
      }
    });
    const blockRoundTrip = registry.createType('SignedBlock', block.toU8a());
    assert.equal(blockRoundTrip.toHex(), block.toHex());
    positions.push(position);
  }
  let zeroError;
  try { registry.createType('Extrinsic', makeExtrinsic(new Uint8Array(64))); }
  catch (error) { zeroError = error.message; }
  assert.ok(zeroError, 'all-zero carrier must reproduce the legacy decoder regression');
  output.versions.push({
    metadataVersion: Number(version),
    positive: 'PASS: exact extrinsic and SignedBlock round trip, signed flag, call, extension fields, tx hash, account prefix',
    rawLength: tx.signer.value.length,
    runtimeExtrinsicHash: real.hash.toHex(),
    runtimeExtrinsic: real.toHex(),
    mixedBlockInsertionPositions: positions,
    negativeZeroCarrier: zeroError,
    syntheticExtrinsic: ms,
  });
}
writeFileSync(resultsPath, JSON.stringify(output, null, 2) + '\n');
console.log(JSON.stringify({ package: output.package, fixtureSha256: output.fixtureSha256, resultsPath, versions: output.versions.map(({syntheticExtrinsic, runtimeExtrinsic, ...rest}) => rest) }, null, 2));
