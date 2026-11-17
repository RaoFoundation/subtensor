const test = require('node:test')
const assert = require('node:assert/strict')
const { once } = require('node:events')
const { WebSocket } = require('ws')
const { BridgeServer, BridgeClient, connectBridgeSigner } = require('../dist/extension-bridge')

async function browser(server, handler = () => ({ accounts: [] })) {
  const socket = new WebSocket(server.wsUrl, { origin: new URL(server.httpUrl).origin })
  await once(socket, 'open')
  socket.send(JSON.stringify({ role: 'bridge', session: server.sessionId }))
  socket.on('message', async raw => {
    const message = JSON.parse(raw)
    const result = await handler(message)
    if (result !== undefined && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ id: message.id, result }))
  })
  await server.waitUntilReady(1000)
  return socket
}

test('extension bridge serves shared UI and authenticates clients before forwarding', async () => {
  const server = new BridgeServer({ port: 0 })
  await server.start()
  const client = new BridgeClient(server.wsUrl, server.clientToken)
  let page
  try {
    const response = await fetch(server.browserUrl)
    assert.equal(response.status, 200)
    assert.match(await response.text(), /injectedWeb3|web3Enable/)
    const bad = new BridgeClient(server.wsUrl, 'incorrect-token')
    await assert.rejects(bad.connect(), /rejected the client token/)
    await bad.close()
    page = await browser(server, message => {
      assert.equal(message.method, 'accounts.list')
      return { accounts: [{ address: 'account', type: 'sr25519', meta: { name: 'Alice', source: 'test-wallet' } }] }
    })
    assert.deepEqual(await client.listAccounts(), [{ address: 'account', type: 'sr25519', name: 'Alice', source: 'test-wallet', genesisHash: undefined }])
    const status = await client.request('bridge.status')
    assert.equal(status.browser_connected, true)
    assert.equal(status.account_count, 1)
  } finally { await client.close(); page?.terminate(); await server.stop() }
})

test('extension bridge rejects foreign origins and stale browser capabilities', async () => {
  const server = new BridgeServer({ port: 0 })
  await server.start()
  try {
    const foreign = new WebSocket(server.wsUrl, { origin: 'https://attacker.example' })
    await assert.rejects(once(foreign, 'open'), /403/)
    const stale = new WebSocket(server.wsUrl, { origin: new URL(server.httpUrl).origin })
    await once(stale, 'open')
    stale.send(JSON.stringify({ role: 'bridge', session: 'old-session' }))
    const [code] = await once(stale, 'close')
    assert.equal(code, 1008)
    assert.equal(server.browserConnected, false)
  } finally { await server.stop() }
})

test('concurrent bridge requests get unique browser IDs; replacing tab rejects only old requests', async () => {
  const server = new BridgeServer({ port: 0, requestTimeoutMs: 2000 })
  await server.start()
  const one = new BridgeClient(server.wsUrl, server.clientToken)
  const two = new BridgeClient(server.wsUrl, server.clientToken)
  let page, replacement
  const ids = new Set()
  try {
    page = await browser(server, message => { ids.add(message.id); return { signature: message.params.data } })
    const results = await Promise.all([one.signBytes('a', '0x01'), two.signBytes('b', '0x02')])
    assert.deepEqual(results, [{ signature: '0x01' }, { signature: '0x02' }])
    assert.equal(ids.size, 2)
    page.removeAllListeners('message')
    const pending = one.signBytes('a', '0x03')
    const rejected = assert.rejects(pending, /disconnected/)
    await once(page, 'message')
    replacement = await browser(server, () => ({ signature: '0x04' }))
    await rejected
    assert.deepEqual(await two.signBytes('a', '0x04'), { signature: '0x04' })
  } finally { await one.close(); await two.close(); page?.terminate(); replacement?.terminate(); await server.stop() }
})

test('extension bridge bounds requests, times out, and creates an account-bound staged signer', async () => {
  const server = new BridgeServer({ port: 0, requestTimeoutMs: 30 })
  await server.start()
  let page
  const stages = []
  try {
    page = await browser(server, message => {
      if (message.method === 'accounts.list') return { accounts: [{ address: 'account', type: 'ed25519', source: 'test-wallet' }] }
      if (message.method === 'extrinsic.sign') { stages.push(message.params); return { signature: '0x' + '11'.repeat(64) } }
      return undefined
    })
    const signer = await connectBridgeSigner(server.wsUrl, server.clientToken, { address: 'account' })
    try {
      assert.equal(signer.cryptoType, 0)
      signer.twoStage = true
      await signer.signPayload({ address: 'account' })
      await signer.signPayload({ address: 'account' })
      assert.equal(stages[0].moreComing, true)
      assert.equal(stages[1].moreComing, false)
      signer.beginTwoStageSigning()
      await signer.signPayload({ address: 'account' })
      assert.equal(stages[2].moreComing, true)
      assert.throws(() => signer.signPayload({ address: 'someone-else' }), /address mismatch/)
      await assert.rejects(server.request('bytes.sign', {}), /timed out/)
      await assert.rejects(server.request('unknown.method'), /unsupported/)
    } finally { await signer.close() }
  } finally { page?.terminate(); await server.stop() }
})

test('extension bridge rejects invalid timeout budgets', () => {
  for (const timeoutMs of [0, -1, NaN, Infinity, 1.5]) assert.throws(() => new BridgeClient('ws://127.0.0.1:1', 'token', timeoutMs), /timeout/)
})
