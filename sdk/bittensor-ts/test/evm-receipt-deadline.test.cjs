const test = require('node:test')
const assert = require('node:assert/strict')
const { getEventListeners, once } = require('node:events')
const http = require('node:http')
const { setTimeout: sleep } = require('node:timers/promises')
const { EvmRpc } = require('../dist/evm')

const hash = '0x' + '11'.repeat(32)
const receipt = { status: '0x1', blockNumber: '0x2a', gasUsed: '0x5208' }
const outcome = promise => promise.then(value => ({ value }), error => ({ error }))
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done }); return { promise, resolve } }
function response(init, result = receipt) { return { ok: true, json: async () => ({ jsonrpc: '2.0', id: JSON.parse(init.body).id, result }) } }
async function settled(promise, timeout = 500) {
  let timer
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('receipt wait did not settle promptly')), timeout) })]) }
  finally { clearTimeout(timer) }
}

for (const stage of ['headers', 'body']) {
  test(`receipt deadline rejects a stalled ${stage} and ignores a late receipt`, async t => {
    const gate = deferred(), reached = deferred(), controller = new AbortController()
    let calls = 0, requestSignal
    const rpc = new EvmRpc('http://mock', 5000, async (_, init) => {
      calls++; requestSignal = init.signal
      const reply = response(init)
      if (stage === 'headers') { reached.resolve(); await gate.promise; return reply }
      return { ...reply, json: async () => { reached.resolve(); await gate.promise; return reply.json() } }
    })
    t.after(() => gate.resolve())
    const result = outcome(rpc.waitForReceipt(hash, { timeoutMs: 20, signal: controller.signal }))
    await reached.promise
    const failed = await settled(result)
    assert.match(failed.error?.message ?? '', /receipt timeout/)
    assert.equal(requestSignal.aborted, true)
    assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
    gate.resolve(); await sleep(0)
    assert.equal(calls, 1)
  })
}

test('one receipt deadline spans RPC calls and polling sleep', async () => {
  let calls = 0
  const rpc = new EvmRpc('http://mock', 5000, async (_, init) => { calls++; await sleep(30); return response(init, calls === 1 ? null : receipt) })
  const result = await settled(outcome(rpc.waitForReceipt(hash, { timeoutMs: 50, pollIntervalMs: 30 })))
  assert.match(result.error?.message ?? '', /receipt timeout/)
  assert.equal(calls, 1, 'no new poll may start after the shared deadline')
})

test('abort during receipt polling sleep is prompt and removes caller listeners', async () => {
  let calls = 0
  const controller = new AbortController(), reason = new Error('cancel receipt wait')
  const rpc = new EvmRpc('http://mock', 5000, async (_, init) => { calls++; return response(init, null) })
  const result = outcome(rpc.waitForReceipt(hash, { timeoutMs: 5000, pollIntervalMs: 1000, signal: controller.signal }))
  await sleep(0)
  controller.abort(reason)
  assert.equal((await settled(result)).error, reason)
  assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
  assert.equal(calls, 1)
})

test('abort during in-flight receipt headers or body preserves the caller reason', async t => {
  for (const stage of ['headers', 'body']) {
    const gate = deferred(), reached = deferred(), controller = new AbortController()
    const reason = new Error(`cancel ${stage}`)
    let requestSignal
    const rpc = new EvmRpc('http://mock', 5000, async (_, init) => {
      requestSignal = init.signal
      const reply = response(init)
      if (stage === 'headers') { reached.resolve(); await gate.promise; return reply }
      return { ...reply, json: async () => { reached.resolve(); await gate.promise; return reply.json() } }
    })
    t.after(() => gate.resolve())
    const result = outcome(rpc.waitForReceipt(hash, { timeoutMs: 5000, signal: controller.signal }))
    await reached.promise; controller.abort(reason)
    assert.equal((await settled(result)).error, reason)
    assert.equal(requestSignal.aborted, true)
    assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
    gate.resolve(); await sleep(0)
  }
})

test('pre-abort, exhausted budgets and invalid receipt wait limits send no RPC', async () => {
  let calls = 0
  const rpc = new EvmRpc('http://mock', 5000, async (_, init) => { calls++; return response(init) })
  const controller = new AbortController(), reason = new Error('cancel before polling')
  controller.abort(reason)
  await assert.rejects(rpc.waitForReceipt(hash, { signal: controller.signal }), error => error === reason)
  await assert.rejects(rpc.waitForReceipt(hash, { timeoutMs: 0 }), /receipt timeout/)
  for (const key of ['timeoutMs', 'pollIntervalMs']) {
    for (const value of [-1, NaN, Infinity]) await assert.rejects(rpc.waitForReceipt(hash, { [key]: value }), RangeError)
  }
  assert.equal(calls, 0)
})

test('successful receipt wait cleans its deadline and caller abort listener', async () => {
  const controller = new AbortController()
  let requestSignal
  const rpc = new EvmRpc('http://mock', 5000, async (_, init) => { requestSignal = init.signal; return response(init) })
  assert.deepEqual(await rpc.waitForReceipt(hash, { timeoutMs: 30.5, pollIntervalMs: 0.5, signal: controller.signal }), receipt)
  assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
  await sleep(50)
  assert.equal(requestSignal.aborted, false, 'the completed wait must cancel its deadline timer')
})

for (const bodyStarted of [false, true]) {
  test(`receipt timeout aborts real HTTP ${bodyStarted ? 'body' : 'header'} I/O`, async t => {
    const received = deferred(), closed = deferred()
    const server = http.createServer((request, reply) => {
      request.resume()
      reply.once('close', () => closed.resolve())
      if (bodyStarted) { reply.writeHead(200, { 'content-type': 'application/json' }); reply.write('{"jsonrpc":"2.0",') }
      received.resolve()
    })
    server.listen(0, '127.0.0.1'); await once(server, 'listening')
    t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)) })
    const rpc = new EvmRpc(`http://127.0.0.1:${server.address().port}`, 5000)
    const result = outcome(rpc.waitForReceipt(hash, { timeoutMs: 200 }))
    await settled(received.promise)
    assert.match((await settled(result)).error?.message ?? '', /receipt timeout/)
    await settled(closed.promise)
  })
}

test('browser EVM receipt polling uses the same in-flight deadline', async () => {
  const browser = await import('../dist/browser.mjs')
  const rpc = new browser.EvmRpc('http://mock', 5000, async (_, init) => { await sleep(60); return response(init) })
  const result = await settled(outcome(rpc.waitForReceipt(hash, { timeoutMs: 10 })))
  assert.match(result.error?.message ?? '', /receipt timeout/)
})
