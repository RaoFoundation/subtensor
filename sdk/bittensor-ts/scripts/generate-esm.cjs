'use strict'

const { readFileSync, writeFileSync } = require('node:fs')
const { dirname, join, resolve } = require('node:path')

const root = join(__dirname, '..')
const identifier = /^[A-Za-z_$][A-Za-z0-9_$]*$/

function collectExports(filePath, seen, names) {
  const normalized = resolve(filePath)
  if (seen.has(normalized)) return
  seen.add(normalized)

  const source = readFileSync(normalized, 'utf8')
  for (const match of source.matchAll(/\bexports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=/g)) {
    names.add(match[1])
  }
  for (const match of source.matchAll(/Object\.defineProperty\(exports,\s*["']([^"']+)["']/g)) {
    if (match[1] !== '__esModule') names.add(match[1])
  }
  for (const match of source.matchAll(/__exportStar\(require\(["'](.+?)["']\),\s*exports\)/g)) {
    collectExports(resolve(dirname(normalized), `${match[1]}.js`), seen, names)
  }
}

function generate(entry) {
  const cjsPath = join(root, 'dist', `${entry}.js`)
  const esmPath = join(root, 'dist', `${entry}.mjs`)
  const seen = new Set()
  const names = new Set()

  collectExports(cjsPath, seen, names)
  names.delete('default')

  const sorted = [...names].sort()
  for (const name of sorted) {
    if (!identifier.test(name)) {
      throw new Error(`Cannot emit ESM named export for ${JSON.stringify(name)}`)
    }
  }

  const lines = [
    `import * as sdk from './${entry}.js'`,
    '',
    ...(entry === 'browser'
      ? [
          "async function __loadBittensorCoreWasm() {",
          "  const wasmBindings = await import('./wasm/bittensor_core_wasm_bg.js')",
          "  const wasmAsset = await import('./wasm/bittensor_core_wasm_bg.wasm')",
          "  const wasmUrl = wasmAsset.default ?? wasmAsset",
          "  const response = await fetch(wasmUrl)",
          "  if (!response.ok) throw new Error(`failed to fetch bittensor-core WASM: ${response.status}`)",
          "  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {",
          "    './bittensor_core_wasm_bg.js': wasmBindings,",
          "  })",
          "  wasmBindings.__wbg_set_wasm(instance.exports)",
          "  if (typeof instance.exports.__wbindgen_start === 'function') instance.exports.__wbindgen_start()",
          "  const module = { ...wasmBindings }",
          "  delete module.default",
          "  return module",
          "}",
          "sdk.setDefaultBrowserWasmLoader(__loadBittensorCoreWasm)",
          '',
        ]
      : []),
    ...sorted.map((name) => `export const ${name} = sdk.${name}`),
    '',
    'export default sdk',
    '',
  ]
  writeFileSync(esmPath, lines.join('\n'))
}

generate('index')
generate('browser')
