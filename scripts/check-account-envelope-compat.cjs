// Generate fixtures with ACCOUNT_ENVELOPE_FIXTURES=/tmp/envelopes.json cargo test
// -p node-subtensor-runtime --lib export_client_envelope_fixtures, then run:
// node scripts/check-account-envelope-compat.cjs /tmp/envelopes.json
// Uses the installed, UNMODIFIED Polkadot.js packages. Never connects to a node.
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const req = createRequire(resolve(__dirname, '../ts-tests/package.json'));
const { TypeRegistry, Metadata } = req('@polkadot/types');
const { blake2AsHex } = req('@polkadot/util-crypto');
const fixture = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const sdk = JSON.parse(readFileSync(resolve(__dirname, '../runtime/tests/fixtures/hashed-python-v4.json'), 'utf8'));
const transactions = fixture.extrinsics.concat(sdk.wallets.flatMap(wallet => wallet.extrinsics.map(bytes => `0x${bytes}`)));
for (const [version, encoded] of Object.entries(fixture.metadata)) {
  const registry = new TypeRegistry();
  registry.setMetadata(new Metadata(registry, encoded));
  const block = registry.createType('SignedBlock', {
    block: {
      header: {parentHash: new Uint8Array(32), number: 1, stateRoot: new Uint8Array(32), extrinsicsRoot: new Uint8Array(32), digest: {logs: []}},
      extrinsics: transactions,
    },
    justifications: null,
  });
  assert.equal(block.block.extrinsics.length, transactions.length);
  block.block.extrinsics.forEach((tx, index) => {
    assert.equal(tx.toHex(), transactions[index]);
    assert.equal(tx.hash.toHex(), blake2AsHex(transactions[index]));
    assert.equal(tx.isSigned, true);
    if (index >= 2) {
      assert.equal(tx.signer.toHuman().Raw.length, 2 + 106 * 2);
      const transfer = index >= 4 && (index - 4) % 3 !== 0;
      assert.equal(tx.method.section, transfer ? 'balances' : 'system');
      assert.equal(tx.method.method, transfer ? 'transferKeepAlive' : 'remark');
      if (index >= 4) {
        const account = `0x${sdk.wallets[Math.floor((index - 4) / 3)].account}`;
        // The explorer displays a MultiAddress signer through AccountId32.
        // Check that its existing conversion shows the permanent account.
        assert.equal(registry.createType('AccountId', tx.signer.toString()).toHex(), account);
      }
    }
  });
  console.log(`metadata v${version}: mixed standard and hashed Ed/Sr block decoded; bytes and hashes preserved`);
}
