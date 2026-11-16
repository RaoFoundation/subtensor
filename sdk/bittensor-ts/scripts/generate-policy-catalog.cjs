'use strict'
const fs = require('node:fs')
const path = require('node:path')
const native = require('../native.cjs')
const catalog = Object.fromEntries(['protected', 'transaction', 'financial', 'validation'].map(domain => {
  const result = JSON.parse(native.policyEvaluate(domain + '.constants', '{}'))
  if (result.version !== 1 || result.error) throw new Error(result.error?.message ?? 'invalid Rust policy catalog')
  return [domain, result.value]
}))
const rendered = JSON.stringify(catalog, null, 2)
const outputs = [
  [path.join(__dirname, '../src/policy-catalog.ts'), '// Generated from bittensor-core policy constants; do not edit by hand.\nexport const POLICY_CATALOG = ' + rendered + ' as const\n'],
  [path.join(__dirname, '../../python/bittensor/_policy_catalog.py'), '# Generated from bittensor-core policy constants; do not edit by hand.\n# ruff: noqa\nimport json\n\nPOLICY_CATALOG = json.loads(r"""\n' + rendered + '\n""")\n'],
]
for (const [file, output] of outputs) {
  if (process.argv.includes('--check')) {
    if (!fs.existsSync(file) || fs.readFileSync(file, 'utf8') !== output) throw new Error('Rust policy catalog drift: ' + file + '; run npm run policy:generate after building native')
  } else fs.writeFileSync(file, output)
}
