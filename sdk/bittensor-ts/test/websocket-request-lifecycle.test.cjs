const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { once } = require('node:events')
const { setTimeout: sleep } = require('node:timers/promises')
const { WebSocket, WebSocketServer } = require('ws')
const { JsonRpcTransport, isDefiniteSubmissionRejection } = require('../dist/transport')

const outcome = promise => promise.then(value => ({ value }), error => ({ error }))
async function settled(promise) {
  let timer
  try {
    return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('operation did not settle')), 1000) })])
  } finally { clearTimeout(timer) }
}

function controlled(t, onSend) {
  const sockets = []
  class Socket {
    readyState = 0
    listeners = new Map()
    sent = []
    constructor() { sockets.push(this) }
    addEventListener(type, listener) {
      const listeners = this.listeners.get(type) ?? []
      listeners.push(listener)
      this.listeners.set(type, listeners)
    }
    emit(type, event = {}) { for (const listener of this.listeners.get(type) ?? []) listener(event) }
    open() { this.readyState = 1; this.emit('open') }
    send(raw) { const message = JSON.parse(raw); this.sent.push(message); onSend?.(this, message) }
    message(value) { this.emit('message', { data: JSON.stringify(value) }) }
    close() { if (this.readyState === 3) return; this.readyState = 3; this.emit('close') }
  }
  const transport = new JsonRpcTransport('ws://controlled', [], false, {
    webSocketConstructor: Socket, requestTimeoutMs: 1000, maxRequestRetries: 0,
  })
  t.after(() => transport.close())
  return { transport, sockets }
}

// PROTECTED_REGRESSION: a transfer whose deadline expires during a shared handshake must never be broadcast later; changes require explicit human approval.
test('each shared connection waiter enforces its deadline before submission handoff', async t => {
  for (const submissionFirst of [false, true]) {
    const { transport, sockets } = controlled(t, (socket, message) => socket.message({ jsonrpc: '2.0', id: message.id, result: 'healthy' }))
    const submission = () => outcome(transport.request('author_submitExtrinsic', ['0x01'], { timeoutMs: 10 }))
    const healthy = () => outcome(transport.request('system_health', [], { timeoutMs: 1000 }))
    const first = submissionFirst ? submission() : healthy()
    await sleep(0)
    const later = submissionFirst ? healthy() : submission()
    await sleep(80)
    sockets[0].open()
    const result = await settled(submissionFirst ? first : later)
    assert.ok(isDefiniteSubmissionRejection(result.error), 'expired submission must be classified as not sent')
    assert.equal(result.error.cause.name, 'RequestTimeoutError')
    assert.equal((await (submissionFirst ? later : first)).value, 'healthy', 'another waiter retains the shared handshake')
    assert.deepEqual(sockets[0].sent.map(request => request.method), ['system_health'])
    assert.equal(sockets.length, 1)
    assert.equal(transport.pending.size, 0)
    transport.close()
  }
})

test('aborting either shared handshake waiter preserves the other request', async t => {
  for (const abortFirst of [true, false]) {
    const { transport, sockets } = controlled(t, (socket, message) => socket.message({ jsonrpc: '2.0', id: message.id, result: message.method }))
    const controller = new AbortController()
    const first = outcome(transport.request('system_health', [], { timeoutMs: 0, signal: abortFirst ? controller.signal : undefined }))
    await sleep(0)
    const second = outcome(transport.request('system_name', [], { timeoutMs: 0, signal: abortFirst ? undefined : controller.signal }))
    await sleep(0)
    controller.abort()
    const aborted = await settled(abortFirst ? first : second)
    assert.equal(aborted.error.name, 'RequestAbortedError')
    assert.equal(sockets[0].readyState, 0)
    sockets[0].open()
    assert.equal((await settled(abortFirst ? second : first)).value, abortFirst ? 'system_name' : 'system_health')
    assert.equal(sockets[0].sent.length, 1)
    transport.close()
  }
})

test('an abandoned or closed handshake closes its socket and cannot send afterward', async t => {
  for (const closeTransport of [false, true]) {
    const { transport, sockets } = controlled(t)
    const controller = new AbortController()
    const pending = outcome(transport.request('author_submitExtrinsic', ['0x01'], { timeoutMs: 0, signal: controller.signal }))
    await sleep(0)
    if (closeTransport) transport.close()
    else controller.abort()
    assert.ok(isDefiniteSubmissionRejection((await settled(pending)).error))
    assert.equal(sockets[0].readyState, 3)
    sockets[0].open()
    await sleep(0)
    assert.equal(sockets[0].sent.length, 0)
    assert.equal(transport.pending.size, 0)
  }
})

test('late callbacks from a rotated handshake cannot cancel the replacement connection', async t => {
  const { transport, sockets } = controlled(t, (socket, message) => socket.message({ jsonrpc: '2.0', id: message.id, result: message.method }))
  const old = outcome(transport.request('system_health', [], { timeoutMs: 0 }))
  await sleep(0)
  transport.rotateEndpoint()
  const replacement = outcome(transport.request('system_name', [], { timeoutMs: 0 }))
  await sleep(0)
  assert.match((await settled(old)).error.message, /endpoint rotated/)
  sockets[0].open()
  sockets[0].close()
  assert.equal(transport.connecting.generation, 1)
  sockets[1].open()
  assert.equal((await settled(replacement)).value, 'system_name')
  assert.deepEqual(sockets[0].sent, [])
  assert.equal(sockets.length, 2)
})

test('the response timeout uses the budget remaining after the handshake', async t => {
  const { transport, sockets } = controlled(t)
  const pending = outcome(transport.request('system_health', [], { timeoutMs: 80 }))
  await sleep(50)
  sockets[0].open()
  await sleep(50)
  const request = sockets[0].sent[0]
  sockets[0].message({ jsonrpc: '2.0', id: request.id, result: 'too late' })
  assert.equal((await settled(pending)).error?.name, 'RequestTimeoutError')
  assert.equal(transport.pending.size, 0)
})

test('subscription routing is installed before synchronous notifications and stays generation-bound', async t => {
  let count = 0
  const { transport, sockets } = controlled(t, (socket, request) => {
    if (request.method === 'state_subscribeStorage') {
      count++
      socket.message({ jsonrpc: '2.0', id: request.id, result: 'reused-id' })
      socket.message({ jsonrpc: '2.0', method: 'state_storage', params: { subscription: 'reused-id', result: count } })
    } else socket.message({ jsonrpc: '2.0', id: request.id, result: true })
  })
  const initial = transport.subscribe('state_subscribeStorage', [], 'state_unsubscribeStorage')
  await sleep(0)
  sockets[0].open()
  const subscription = await initial, iterator = subscription[Symbol.asyncIterator]()
  assert.deepEqual(await settled(iterator.next()), { done: false, value: 1 })
  sockets[0].close()
  await sleep(0)
  sockets[1].open()
  assert.deepEqual(await settled(iterator.next()), { done: false, value: 2 })
  sockets[0].message({ jsonrpc: '2.0', method: 'state_storage', params: { subscription: 'reused-id', result: 'stale' } })
  sockets[1].message({ jsonrpc: '2.0', method: 'state_storage', params: { subscription: 'reused-id', result: 3 } })
  assert.deepEqual(await settled(iterator.next()), { done: false, value: 3 })
  await subscription.unsubscribe()
  assert.equal(transport.subscriptionsById.size, 0)
})

async function realServer(t, injected, onConnection) {
  const server = http.createServer(), wss = new WebSocketServer({ server })
  wss.on('connection', onConnection)
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const transport = new JsonRpcTransport(`ws://127.0.0.1:${server.address().port}`, [], false, {
    ...(injected ? { webSocketConstructor: WebSocket } : {}), requestTimeoutMs: 1000, maxRequestRetries: 0,
  })
  t.after(async () => {
    transport.close()
    for (const socket of wss.clients) socket.terminate()
    await new Promise(resolve => wss.close(resolve))
    await new Promise(resolve => server.close(resolve))
  })
  return transport
}

for (const injected of [false, true]) {
  // PROTECTED_REGRESSION: losing an initial finalized status must not leave a successful payment looking unconfirmed and encourage a duplicate retry; changes require explicit human approval.
  test(`${injected ? 'injected ws' : 'Node default WebSocket'} retains first storage and finalized updates sent with subscription responses`, async t => {
    const finalized = { finalized: '0x' + '42'.repeat(32) }
    const storage = { block: '0x' + '11'.repeat(32), changes: [['0x01', '0x02']] }
    const transport = await realServer(t, injected, socket => {
      socket.on('message', bytes => {
        const request = JSON.parse(bytes)
        const subscription = request.method === 'state_subscribeStorage' ? 'storage' : 'transaction'
        if (request.method.startsWith('state_unsubscribe') || request.method === 'author_unwatchExtrinsic') {
          socket.send(JSON.stringify({ jsonrpc: '2.0', id: request.id, result: true })); return
        }
        socket._socket.cork()
        socket.send(JSON.stringify({ jsonrpc: '2.0', id: request.id, result: subscription }))
        socket.send(JSON.stringify({ jsonrpc: '2.0', method: subscription === 'storage' ? 'state_storage' : 'author_extrinsicUpdate', params: { subscription, result: subscription === 'storage' ? storage : finalized } }))
        socket._socket.uncork()
      })
    })
    const storageSub = await transport.subscribe('state_subscribeStorage', [['0x01']], 'state_unsubscribeStorage')
    assert.deepEqual(await settled(storageSub[Symbol.asyncIterator]().next()), { done: false, value: storage })
    await storageSub.unsubscribe()
    const transactionSub = await transport.subscribe('author_submitAndWatchExtrinsic', ['0x01'], 'author_unwatchExtrinsic')
    assert.deepEqual(await settled(transactionSub[Symbol.asyncIterator]().next()), { done: false, value: finalized })
    await transactionSub.unsubscribe()
  })
}
