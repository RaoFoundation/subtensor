'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { getEventListeners } = require('node:events')
const { Client } = require('../dist')

function deferred() {
  let resolve, reject
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
async function settles(promise) {
  let timer
  try {
    return await Promise.race([
      promise.then(value => ({ value }), error => ({ error })),
      new Promise(resolve => { timer = setTimeout(() => resolve({ stalled: true }), 1000) }),
    ])
  } finally { clearTimeout(timer) }
}
function fixture(t, options = {}) {
  const client = new Client('local', { endpoint: 'http://unused', archiveEndpoints: [], ...options })
  t.after(() => client.close())
  return client
}

test('Node RPC returns at its deadline while the native request is still pending', async t => {
  const client = fixture(t), entered = deferred(), result = deferred()
  let nativeOptions
  client.requireNativeClient = async () => ({ rpcValue: (_method, _params, timeout, cancellation) => {
    nativeOptions = { timeout, cancellation }; entered.resolve(); return result.promise
  } })
  t.after(() => result.resolve('late response'))
  const request = settles(client.rpc('system_health', [], { timeoutMs: 20 }))
  await entered.promise
  const outcome = await request
  assert.equal(outcome.error?.name, 'RequestTimeoutError')
  assert(nativeOptions.timeout > 0n && nativeOptions.timeout <= 20n)
  assert.equal(nativeOptions.cancellation.cancelled, true)
})

test('Node RPC aborts in flight and removes the caller signal listener', async t => {
  const client = fixture(t), controller = new AbortController(), entered = deferred(), result = deferred()
  let cancellation
  client.requireNativeClient = async () => ({ rpcValue: (_method, _params, _timeout, token) => {
    cancellation = token; entered.resolve(); return result.promise
  } })
  t.after(() => result.reject(new Error('late native failure')))
  const request = settles(client.rpc('state_getStorage', ['0x01'], { signal: controller.signal, timeoutMs: 0 }))
  await entered.promise
  controller.abort()
  assert.equal((await request).error?.name, 'RequestAbortedError')
  assert.equal(cancellation.cancelled, true)
  assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
})

test('Node RPC cancellation during connection cannot send a late request', async t => {
  for (const timeout of [false, true]) {
    const client = fixture(t), connected = deferred(), controller = new AbortController()
    let sends = 0
    const native = { rpcValue: async () => { sends++; return 'unexpected' } }
    client.requireNativeClient = () => connected.promise
    t.after(() => connected.resolve(native))
    const request = settles(client.rpc('system_health', [], { signal: controller.signal, timeoutMs: timeout ? 20 : 0 }))
    if (!timeout) controller.abort()
    assert.equal((await request).error?.name, timeout ? 'RequestTimeoutError' : 'RequestAbortedError')
    connected.resolve(native)
    await delay(0)
    assert.equal(sends, 0)
  }
})

test('Node RPC honors the configured deadline and an explicit zero disables it', async t => {
  const client = fixture(t, { requestTimeoutMs: 20 }), pending = deferred()
  client.requireNativeClient = async () => ({ rpcValue: () => pending.promise })
  t.after(() => pending.resolve('done'))
  assert.equal((await settles(client.rpc('system_health'))).error?.name, 'RequestTimeoutError')
  let completed = false
  const unlimited = client.rpc('system_health', [], { timeoutMs: 0 }).then(value => { completed = true; return value })
  await delay(40)
  assert.equal(completed, false)
  pending.resolve('done')
  assert.equal(await unlimited, 'done')
})

test('Node RPC keeps one deadline across the primary and archive fallback', async t => {
  const client = fixture(t, { archiveEndpoints: ['http://archive'] }), archive = deferred(), entered = deferred()
  let options
  client.requireNativeClient = async () => ({ rpcValue: async () => { await delay(20); throw new Error('State already discarded') } })
  client.genesisHash = async () => '0x' + '11'.repeat(32)
  client.nodeArchiveTransport = { close() {}, request: async (_method, _params, value) => { options = value; entered.resolve(); return archive.promise } }
  t.after(() => archive.resolve('late archive response'))
  const request = settles(client.rpc('state_getStorage', ['0x01'], { timeoutMs: 100 }))
  await entered.promise
  assert(options.timeoutMs > 0 && options.timeoutMs < 100)
  assert.equal((await request).error?.name, 'RequestTimeoutError')
  assert.equal(options.signal.aborted, true)
})

test('Node paginated reads honor aborts passed through RPC', async t => {
  const client = fixture(t), controller = new AbortController(), entered = deferred(), result = deferred()
  client.runtimeAt = async () => ({ storageKey: () => Buffer.from([0]), storageEntry: () => ({ valueType: 'u8' }) })
  client.requireNativeClient = async () => ({ rpcValue: () => { entered.resolve(); return result.promise } })
  t.after(() => result.resolve([]))
  const request = settles(client.queryMap('Example', 'Map', [], '0x' + '11'.repeat(32), { pageSize: 1, signal: controller.signal }))
  await entered.promise
  controller.abort()
  assert.equal((await request).error?.name, 'RequestAbortedError')
})

test('Node RPC forwards controls to real native HTTP and releases aborted I/O', async t => {
  const http = require('node:http'), { NativeChainClient } = require('../dist/transaction')
  const golden = require('../../python/tests/fixtures/golden.json')
  for (const kind of ['deadline', 'abort']) {
    const entered = deferred(), closed = deferred(), controller = new AbortController()
    let backups = 0, nativeResult
    async function server(backup) {
      const instance = http.createServer(async (request, response) => {
        let body = ''; for await (const chunk of request) body += chunk
        const { id, method } = JSON.parse(body)
        if (backup) backups++
        if (method === 'test_hold') {
          response.on('close', () => closed.resolve(true))
          entered.resolve()
          return // Deliberately never send headers or a response body.
        }
        const values = { system_properties: { ss58Format: 42 }, state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 }, state_call: golden.metadata.v15_hex, chain_getBlockHash: '0x' + '11'.repeat(32) }
        response.setHeader('content-type', 'application/json')
        response.end(JSON.stringify({ jsonrpc: '2.0', id, result: values[method] }))
      })
      await new Promise(resolve => instance.listen(0, '127.0.0.1', resolve))
      t.after(() => { instance.closeAllConnections(); return new Promise(resolve => instance.close(resolve)) })
      return `http://127.0.0.1:${instance.address().port}`
    }
    const primary = await server(false), backup = await server(true)
    const native = await NativeChainClient.connectEndpoints([primary, backup])
    const client = fixture(t)
    client.requireNativeClient = async () => ({ rpcValue: (...args) => {
      const result = native.rpcValue(...args)
      nativeResult = settles(result)
      return result
    } })
    const request = settles(client.rpc('test_hold', [], { timeoutMs: kind === 'deadline' ? 500 : 0, signal: controller.signal }))
    assert.equal((await settles(entered.promise)).stalled, undefined)
    if (kind === 'abort') controller.abort()
    assert.equal((await request).error?.name, kind === 'deadline' ? 'RequestTimeoutError' : 'RequestAbortedError')
    const nativeOutcome = await nativeResult
    assert(nativeOutcome.error, 'native I/O must end without waiting for a server response')
    assert.equal((await settles(closed.promise)).value, true, 'aborting releases the HTTP connection')
    assert.equal(backups, 0, 'cancellation or deadline must not start a fallback request')
  }
})
