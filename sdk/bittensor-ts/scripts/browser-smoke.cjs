'use strict'

const assert = require('node:assert/strict')
const { spawn, spawnSync } = require('node:child_process')
const fs = require('node:fs')
const http = require('node:http')
const os = require('node:os')
const path = require('node:path')

const esbuild = require('esbuild')

const root = path.join(__dirname, '..')
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'bittensor-sdk-browser-'))
const isCi = process.env.CI === 'true' || process.env.GITHUB_ACTIONS === 'true'

function compactLength(buffer, offset) {
  const first = buffer[offset]
  const mode = first & 0b11
  if (mode === 0) return { length: first >> 2, offset: offset + 1 }
  if (mode === 1) return { length: buffer.readUInt16LE(offset) >> 2, offset: offset + 2 }
  if (mode === 2) return { length: buffer.readUInt32LE(offset) >>> 2, offset: offset + 4 }
  const bytes = (first >> 2) + 4
  let length = 0
  for (let i = 0; i < bytes; i += 1) length += buffer[offset + 1 + i] * (256 ** i)
  return { length, offset: offset + 1 + bytes }
}

function goldenMetadataHex() {
  const golden = JSON.parse(
    fs.readFileSync(
      path.join(root, '..', 'python', 'tests', 'fixtures', 'golden.json'),
      'utf8',
    ),
  )
  const raw = Buffer.from(golden.metadata.v15_hex.slice(2), 'hex')
  assert.equal(raw[0], 1)
  const decoded = compactLength(raw, 1)
  return raw.subarray(decoded.offset, decoded.offset + decoded.length).toString('hex')
}

function modulePath(filePath) {
  const relative = path.relative(tmp, filePath).replace(/\\/g, '/')
  return relative.startsWith('.') ? relative : `./${relative}`
}

function browserEntry(metadataHex, mode) {
  const useCustomLoader = mode === 'custom'
  return `
import * as sdk from ${JSON.stringify(modulePath(path.join(root, 'dist', 'browser.mjs')))}
${useCustomLoader ? `import * as wasmBindings from ${JSON.stringify(modulePath(path.join(root, 'dist', 'wasm', 'bittensor_core_wasm_bg.js')))}
import wasmUrl from ${JSON.stringify(modulePath(path.join(root, 'dist', 'wasm', 'bittensor_core_wasm_bg.wasm')))}` : ''}

const metadataHex = ${JSON.stringify(metadataHex)}
const mnemonic = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about'
const mode = ${JSON.stringify(mode)}
const useCustomLoader = ${JSON.stringify(useCustomLoader)}

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2)
  for (let i = 0; i < out.length; i += 1) out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16)
  return out
}

${useCustomLoader ? `async function loadWasm() {
  const response = await fetch(wasmUrl)
  if (!response.ok) throw new Error('failed to fetch WASM: ' + response.status)
  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {
    './bittensor_core_wasm_bg.js': wasmBindings,
  })
  wasmBindings.__wbg_set_wasm(instance.exports)
  instance.exports.__wbindgen_start()
  const module = { ...wasmBindings }
  delete module.default
  return module
}` : ''}

try {
  if (useCustomLoader) sdk.configureBrowserWasm(() => loadWasm())
  const wasm = await sdk.initBrowser()
  assert(typeof wasm.Runtime === 'function', 'WASM Runtime constructor is available')
  assert(typeof sdk.coreVersion() === 'string' && sdk.coreVersion().length > 0, 'WASM initialized')

  const keypair = sdk.Keypair.fromMnemonic(mnemonic)
  const message = new TextEncoder().encode('browser signing smoke')
  const signature = keypair.sign(message)
  assert(signature.length === 64, 'sr25519 signature length')
  assert(keypair.verify(message, signature), 'keypair verifies its own signature')
  assert(sdk.verifySignature(message, signature, keypair.ss58Address), 'browser verifySignature works')
  assert(sdk.ss58FromPublic(keypair.publicKey, 42) === keypair.ss58Address, 'SS58 roundtrip works')

  const metadata = hexToBytes(metadataHex)
  const runtime = new sdk.Runtime(metadata, 419, 1, 42)
  assert(runtime.specVersion === 419, 'runtime spec version')
  assert(runtime.transactionVersion === 1, 'runtime transaction version')
  assert(runtime.metadataIr().pallets.some((pallet) => pallet.name === 'System'), 'runtime metadata parsed')

  const call = runtime.composeCall('System', 'remark', { remark: new Uint8Array([1, 2, 3]) })
  const decodedCall = runtime.decodeCall(call)
  assert(decodedCall.call_module === 'System', 'call module decoded')
  assert(decodedCall.call_function === 'remark', 'call function decoded')

  const genesisHash = new Uint8Array(32)
  const eraBlockHash = new Uint8Array(32)
  genesisHash.fill(1)
  eraBlockHash.fill(2)
  const txParams = {
    era: '00',
    nonce: 0,
    tip: 0,
    tipAssetId: null,
    genesisHash,
    eraBlockHash,
  }
  const payloadParts = runtime.signaturePayloadParts(txParams)
  assert(payloadParts.includedInExtrinsic.length > 0, 'signature payload extrinsic parts')
  assert(payloadParts.includedInSignedData.length > 0, 'signature payload signed parts')
  const payload = runtime.signaturePayload(call, txParams)
  const extrinsicSignature = keypair.sign(payload)
  const signed = runtime.encodeSignedExtrinsic(
    call,
    keypair.publicKey,
    extrinsicSignature,
    keypair.cryptoType,
    { era: '00', nonce: 0, tip: 0, tipAssetId: null, metadataHashEnabled: false },
  )
  assert(signed.bytes.length > call.length, 'signed extrinsic bytes')
  assert(signed.hash.length === 32, 'signed extrinsic hash')
  const decodedExtrinsic = runtime.decodeExtrinsic(signed.bytes, false)
  assert(decodedExtrinsic.call.call_module === 'System', 'extrinsic call module decoded')
  assert(decodedExtrinsic.call.call_function === 'remark', 'extrinsic call function decoded')

  window.__SDK_BROWSER_RESULT__ = {
    ok: true,
    mode,
    address: keypair.ss58Address,
    callLength: call.length,
    extrinsicLength: signed.bytes.length,
  }
} catch (error) {
  window.__SDK_BROWSER_ERROR__ = {
    message: String(error?.message ?? error),
    stack: String(error?.stack ?? ''),
  }
}
`
}

async function bundleBrowserEntry() {
  const metadataHex = goldenMetadataHex()
  const customEntry = path.join(tmp, 'custom-entry.js')
  const defaultEntry = path.join(tmp, 'default-entry.js')
  fs.writeFileSync(customEntry, browserEntry(metadataHex, 'custom'))
  fs.writeFileSync(defaultEntry, browserEntry(metadataHex, 'default'))
  const outdir = path.join(tmp, 'site')
  fs.mkdirSync(outdir, { recursive: true })
  await esbuild.build({
    entryPoints: [customEntry, defaultEntry],
    bundle: true,
    format: 'esm',
    platform: 'browser',
    target: ['chrome110'],
    outdir,
    entryNames: '[name]',
    assetNames: 'assets/[name]-[hash]',
    loader: { '.wasm': 'file' },
    logLevel: 'silent',
  })
  fs.writeFileSync(
    path.join(outdir, 'custom.html'),
    '<!doctype html><meta charset="utf-8"><title>Bittensor SDK Browser Smoke</title><script type="module" src="/custom-entry.js"></script>',
  )
  fs.writeFileSync(
    path.join(outdir, 'default.html'),
    '<!doctype html><meta charset="utf-8"><title>Bittensor SDK Browser Smoke</title><script type="module" src="/default-entry.js"></script>',
  )
  return outdir
}

function serve(directory) {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1')
    const pathname = url.pathname === '/' ? '/index.html' : url.pathname
    const filePath = path.join(directory, pathname)
    if (!filePath.startsWith(directory) || !fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
      response.writeHead(404)
      response.end('not found')
      return
    }
    const extension = path.extname(filePath)
    const contentType = extension === '.html'
      ? 'text/html; charset=utf-8'
      : extension === '.js'
        ? 'text/javascript; charset=utf-8'
        : extension === '.wasm'
          ? 'application/wasm'
          : 'application/octet-stream'
    response.writeHead(200, {
      'content-type': contentType,
      'content-security-policy': "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; base-uri 'none'; object-src 'none'",
    })
    response.end(fs.readFileSync(filePath))
  })
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      server.off('error', reject)
      resolve({
        server,
        url: `http://127.0.0.1:${server.address().port}/`,
      })
    })
  })
}

function findChrome() {
  const candidates = [
    process.env.CHROME_BIN,
    process.env.CHROMIUM_BIN,
    'chromium',
    'chromium-browser',
    'google-chrome',
    'google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/snap/bin/chromium',
    '/opt/google/chrome/chrome',
  ].filter(Boolean)
  for (const candidate of candidates) {
    const result = spawnSync(candidate, ['--version'], { stdio: 'ignore' })
    if (result.status === 0) return candidate
  }
  if (!isCi) return undefined
  throw new Error('Chromium is required for browser smoke tests in CI; install chromium or set CHROME_BIN')
}

function launchChrome(chrome, url) {
  const profile = path.join(tmp, 'chrome-profile')
  fs.mkdirSync(profile, { recursive: true })
  const args = [
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--disable-dev-shm-usage',
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    url,
  ]
  const child = spawn(chrome, args, { stdio: ['ignore', 'ignore', 'pipe'] })
  let stderr = ''
  const devtools = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`timed out waiting for Chromium DevTools\n${stderr}`)), 15_000)
    child.once('exit', (code, signal) => {
      clearTimeout(timeout)
      reject(new Error(`Chromium exited before DevTools was ready: ${code ?? signal}\n${stderr}`))
    })
    child.stderr.on('data', (chunk) => {
      stderr += String(chunk)
      const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/)
      if (match == null) return
      clearTimeout(timeout)
      resolve(match[1])
    })
  })
  return { child, devtools }
}

function stopChrome(child) {
  if (child == null || child.exitCode != null) return Promise.resolve()
  return new Promise((resolve) => {
    let settled = false
    let force
    let giveUp
    const finish = () => {
      if (settled) return
      settled = true
      clearTimeout(force)
      clearTimeout(giveUp)
      resolve()
    }
    child.once('exit', finish)
    force = setTimeout(() => {
      if (child.exitCode == null) child.kill('SIGKILL')
    }, 2_000)
    giveUp = setTimeout(finish, 5_000)
    child.kill()
  })
}

function cdp(wsUrl) {
  const socket = new WebSocket(wsUrl)
  let nextId = 1
  const pending = new Map()
  const events = []

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', () => reject(new Error('failed to connect to Chromium DevTools')), { once: true })
  })

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(String(event.data))
    if (message.id != null) {
      const entry = pending.get(message.id)
      if (entry == null) return
      pending.delete(message.id)
      if (message.error) entry.reject(new Error(message.error.message))
      else entry.resolve(message.result)
      return
    }
    events.push(message)
  })

  return {
    ready,
    events,
    send(method, params = {}) {
      const id = nextId++
      const payload = JSON.stringify({ id, method, params })
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject })
        socket.send(payload)
      })
    },
    close() {
      socket.close()
    },
  }
}

async function pageWebSocketUrl(browserWsUrl) {
  const httpUrl = new URL(browserWsUrl.replace(/^ws:/, 'http:').replace(/^wss:/, 'https:'))
  const response = await fetch(`${httpUrl.origin}/json/list`)
  const targets = await response.json()
  const page = targets.find((target) => target.type === 'page')
  if (page?.webSocketDebuggerUrl == null) {
    throw new Error(`could not find Chromium page target: ${JSON.stringify(targets)}`)
  }
  return page.webSocketDebuggerUrl
}

async function waitForBrowserResult(client) {
  const deadline = Date.now() + 30_000
  for (;;) {
    const result = await client.send('Runtime.evaluate', {
      expression: 'window.__SDK_BROWSER_RESULT__ || window.__SDK_BROWSER_ERROR__ || null',
      returnByValue: true,
      awaitPromise: false,
    })
    const value = result.result?.value
    if (value?.ok) return value
    if (value?.message) {
      throw new Error(`${value.message}\n${value.stack ?? ''}`)
    }
    const exception = client.events.find((event) => event.method === 'Runtime.exceptionThrown')
    if (exception != null) throw new Error(JSON.stringify(exception.params?.exceptionDetails ?? exception.params))
    if (Date.now() > deadline) throw new Error('timed out waiting for browser smoke result')
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
}

async function runBrowserPage(chromePath, url, mode) {
  let chrome
  let client
  try {
    chrome = launchChrome(chromePath, url)
    const browserWsUrl = await chrome.devtools
    client = cdp(await pageWebSocketUrl(browserWsUrl))
    await client.ready
    await client.send('Runtime.enable')
    await client.send('Page.enable')
    const result = await waitForBrowserResult(client)
    assert.equal(result.ok, true)
    assert.equal(result.mode, mode)
    assert.equal(typeof result.address, 'string')
    assert.ok(result.callLength > 0)
    assert.ok(result.extrinsicLength > result.callLength)
  } finally {
    client?.close()
    await stopChrome(chrome?.child)
  }
}

async function main() {
  let server
  try {
    const site = await bundleBrowserEntry()
    const chromePath = findChrome()
    if (chromePath == null) {
      console.warn('Skipping browser smoke tests because Chromium is not installed; CI requires Chromium.')
      return
    }
    const served = await serve(site)
    server = served.server
    await runBrowserPage(chromePath, new URL('/custom.html', served.url).href, 'custom')
    await runBrowserPage(chromePath, new URL('/default.html', served.url).href, 'default')
  } finally {
    server?.close()
    fs.rmSync(tmp, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 })
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
