#!/usr/bin/env node
'use strict'

// This is a source-change alarm, not a claim that name coverage proves behavior.
// Update only after reviewing changed upstream workflows and running their tests.
const fs = require('node:fs')
const path = require('node:path')
const { createHash } = require('node:crypto')
const root = path.resolve(__dirname, '../../..')
const sdk = path.join(root, 'sdk/bittensor-ts')
const manifestPath = path.join(sdk, 'parity-manifest.json')
function walk(relative, extension) {
  const output = []
  for (const item of fs.readdirSync(path.join(root, relative), { withFileTypes: true })) {
    const file = `${relative}/${item.name}`
    if (item.isDirectory() && !item.name.startsWith('.') && item.name !== '__pycache__') output.push(...walk(file, extension))
    else if (item.isFile() && item.name.endsWith(extension)) output.push(file)
  }
  return output
}
const sources = [
  ...walk('sdk/python/bittensor', '.py'),
  ...walk('sdk/bittensor-core/src', '.rs'),
  ...walk('sdk/bittensor-core-wasm/src', '.rs'),
  ...walk('precompiles/src/solidity', '.abi'),
  'sdk/python/bittensor/extension/static/bridge.html',
  // Semantic admission also mirrors these runtime rules directly.
  'runtime/src/lib.rs',
  'runtime/src/staking_fee.rs',
  'runtime/src/transaction_payment_wrapper.rs',
  'pallets/subtensor/src/lib.rs',
  'pallets/subtensor/src/staking/claim_root.rs',
  'pallets/subtensor/src/weights.rs',
  'primitives/hashed/src/lib.rs',
  'primitives/hashed/src/mldsa.rs',
  'primitives/hashed/src/falcon.rs',
  'pallets/hashed-accounts/src/lib.rs',
  'pallets/hashed-accounts/src/mldsa.rs',
  'pallets/limit-orders/src/lib.rs',
  'pallets/limit-orders/src/authorization.rs',
  'pallets/limit-orders/src/v2.rs',
  'pallets/proxy/src/lib.rs',
  'pallets/multisig/src/lib.rs',
].sort()
const generated = [
  'descriptors.ts', 'read-catalog.ts', 'read-namespaces.ts', 'intent-catalog.ts',
  'intent-hyperparameter-catalog.ts', 'intent-inputs.ts', 'evm-catalog.ts', 'extension-page.ts', 'error-catalog.ts',
].map(name => `sdk/bittensor-ts/src/${name}`)
if (fs.existsSync(path.join(sdk, 'src/basket-index-data.ts'))) generated.push('sdk/bittensor-ts/src/basket-index-data.ts')
const digest = file => createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex')
const snapshot = { version: 1, sources: Object.fromEntries(sources.map(file => [file, digest(file)])), generated: Object.fromEntries(generated.map(file => [file, digest(file)])) }
if (process.argv.includes('--write')) {
  fs.writeFileSync(manifestPath, JSON.stringify(snapshot, null, 2) + '\n')
  console.log(`Recorded ${sources.length} reviewed upstream source files and ${generated.length} generated catalogs`)
} else {
  const expected = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  const changed = []
  for (const group of ['sources', 'generated']) {
    for (const file of new Set([...Object.keys(expected[group]), ...Object.keys(snapshot[group])])) if (expected[group][file] !== snapshot[group][file]) changed.push(file)
  }
  if (changed.length) {
    console.error('SDK parity review required for changed upstream sources/catalogs:\n' + changed.map(file => `  ${file}`).join('\n'))
    console.error('Review the feature changes, regenerate affected catalogs, run behavior tests, then npm run parity:record.')
    process.exitCode = 1
  } else console.log(`Source parity guard OK: ${sources.length} upstream files, ${generated.length} catalogs`)
}
