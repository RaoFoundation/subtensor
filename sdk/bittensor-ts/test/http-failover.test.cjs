'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { JsonRpcTransport, JsonRpcError, isDefiniteSubmissionRejection } = require('../dist/transport.js')

async function endpoint(t, handler) {
  const calls = []
  const server = http.createServer(async (request, response) => {
    let body = ''
    for await (const chunk of request) body += chunk
    const rpc = JSON.parse(body)
    calls.push(rpc)
    handler(rpc, response)
  })
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve) })
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections() }))
  return { url: `http://127.0.0.1:${server.address().port}`, calls }
}
function success(rpc, response) {
  response.setHeader('content-type', 'application/json')
  response.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result: 'healthy backup' }))
}
function client(t, primary, backup, options = {}, forever = false) {
  const transport = new JsonRpcTransport(primary.url, [backup.url], forever, {
    requestTimeoutMs: 1000, maxRequestRetries: 2, retryBackoffMs: 0, ...options,
  })
  t.after(() => transport.close())
  return transport
}

for (const status of [408, 429, 500, 502, 503, 504]) {
  test(`HTTP ${status} fails over a read to a healthy endpoint and cancels the failed body`, { timeout: 5000 }, async t => {
    let bodyClosed
    const closed = new Promise(resolve => { bodyClosed = resolve })
    const primary = await endpoint(t, (_rpc, response) => {
      response.once('close', bodyClosed)
      response.writeHead(status, { 'content-type': 'text/plain' })
      response.write('upstream temporarily unavailable')
      // Deliberately never end: failover must not wait to consume this body.
    })
    const backup = await endpoint(t, success), transport = client(t, primary, backup)
    assert.equal(await transport.request('state_getStorage', ['0x1234']), 'healthy backup')
    await closed
    assert.equal(transport.endpoint, backup.url)
    assert.equal(primary.calls.length, 1)
    assert.equal(backup.calls.length, 1)
    assert.equal(backup.calls[0].method, 'state_getStorage')
    assert.deepEqual(backup.calls[0].params, ['0x1234'])
  })
}

test('HTTP retry exhaustion and explicit no-retry options bound the total attempts', { timeout: 5000 }, async t => {
  for (const maxRetries of [0, 2]) {
    const fail = (_rpc, response) => { response.writeHead(503); response.end('unavailable') }
    const primary = await endpoint(t, fail), backup = await endpoint(t, fail)
    const transport = client(t, primary, backup, {}, true)
    await assert.rejects(transport.request('chain_getHeader', [], { maxRetries, retryForever: false }), error => {
      assert.equal(error.name, 'HttpTransportError')
      assert.equal(error.status, 503)
      assert.equal(error instanceof JsonRpcError, false)
      return true
    })
    assert.equal(primary.calls.length + backup.calls.length, maxRetries + 1)
    assert.equal(backup.calls.length, maxRetries === 0 ? 0 : 1)
  }
})

test('JSON-RPC application errors and permanent HTTP failures do not fail over', { timeout: 5000 }, async t => {
  for (const status of [200, 400, 401, 403, 404]) {
    const primary = await endpoint(t, (rpc, response) => {
      response.writeHead(status, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, error: { code: -32000, message: 'application rejected request', data: { detail: 'not a gateway failure' } } }))
    })
    const backup = await endpoint(t, success), transport = client(t, primary, backup)
    await assert.rejects(transport.request('state_getStorage', []), error => {
      if (status === 200) {
        assert(error instanceof JsonRpcError)
        assert.equal(error.code, -32000)
        assert.deepEqual(error.data, { detail: 'not a gateway failure' })
      } else {
        assert.equal(error.name, 'HttpTransportError')
        assert.equal(error.status, status)
      }
      return true
    })
    assert.equal(primary.calls.length, 1)
    assert.equal(backup.calls.length, 0)
  }
})

test('HTTP failures never replay author_submit calls and do not retry other writes by default', { timeout: 5000 }, async t => {
  for (const method of ['author_submitExtrinsic', 'author_submitAndWatchExtrinsic', 'author_submitCustom', 'author_insertKey', 'engine_createBlock']) {
    const primary = await endpoint(t, (_rpc, response) => { response.writeHead(503); response.end('request may have reached upstream') })
    const backup = await endpoint(t, success), transport = client(t, primary, backup, { maxRequestRetries: 4 }, true)
    const options = method.startsWith('author_submit') ? { maxRetries: 4, retryForever: true } : {}
    await assert.rejects(transport.request(method, ['0x1234'], options), error => {
      assert.equal(error.name, 'HttpTransportError')
      assert.equal(isDefiniteSubmissionRejection(error), false)
      return true
    })
    assert.equal(primary.calls.length, 1)
    assert.equal(backup.calls.length, 0)
  }
})

test('caller cancellation and an overall abort deadline stop HTTP failover during backoff', { timeout: 5000 }, async t => {
  for (const deadline of [false, true]) {
    let failed
    const firstRequest = new Promise(resolve => { failed = resolve })
    const primary = await endpoint(t, (_rpc, response) => { response.writeHead(429); response.end('rate limited'); failed() })
    const backup = await endpoint(t, success), transport = client(t, primary, backup, { retryBackoffMs: 1000 }, true)
    const controller = new AbortController()
    const signal = deadline ? AbortSignal.timeout(150) : controller.signal
    const result = transport.request('state_getMetadata', [], { signal })
    const rejected = assert.rejects(result, error => error.name === 'RequestAbortedError')
    await firstRequest
    if (!deadline) controller.abort()
    await rejected
    assert.equal(primary.calls.length, 1)
    assert.equal(backup.calls.length, 0)
    await assert.rejects(transport.request('state_getMetadata', [], { signal }), error => error.name === 'RequestAbortedError')
    assert.equal(primary.calls.length + backup.calls.length, 1)
  }
})

test('HTTP request timeouts retain the configured retry budget', { timeout: 5000 }, async t => {
  const primary = await endpoint(t, () => {}), backup = await endpoint(t, () => {})
  const transport = client(t, primary, backup, { requestTimeoutMs: 40, maxRequestRetries: 1 })
  await assert.rejects(transport.request('chain_getHeader'), error => error.name === 'RequestTimeoutError')
  assert.equal(primary.calls.length, 1)
  assert.equal(backup.calls.length, 1)
})
