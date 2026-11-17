const test = require('node:test')
const assert = require('node:assert/strict')
const { spawn } = require('node:child_process')

// Run the server in its own process so the pre-fix uncaught URL error is
// observable without crashing the test runner.
async function probe(upgrade) {
  const assert = require('node:assert/strict')
  const net = require('node:net')
  const { BridgeServer, BridgeClient } = require('./dist/extension-bridge')
  const server = new BridgeServer({ port: 0 })
  await server.start()
  const url = new URL(server.httpUrl)
  try {
    const response = await new Promise((resolve, reject) => {
      const socket = net.connect(Number(url.port), url.hostname)
      let received = ''
      socket.setTimeout(2000, () => socket.destroy(new Error('malformed request was not rejected')))
      socket.on('error', reject)
      socket.on('data', chunk => { received += chunk })
      socket.on('end', () => resolve(received))
      socket.on('connect', () => socket.write([
        'GET //[ HTTP/1.1', `Host: ${url.host}`,
        ...(upgrade ? ['Connection: Upgrade', 'Upgrade: websocket', 'Sec-WebSocket-Version: 13', 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=='] : ['Connection: close']),
        '', '',
      ].join('\r\n')))
    })
    assert.match(response, /^HTTP\/1\.1 400 /)
    assert.equal((await fetch(server.httpUrl)).status, 200)
    const client = new BridgeClient(server.wsUrl, server.clientToken)
    try { assert.equal((await client.request('bridge.status')).browser_connected, false) }
    finally { await client.close() }
  } finally { await server.stop() }
}

for (const upgrade of [false, true]) {
  // PROTECTED_REGRESSION: an unauthenticated malformed request must not terminate the wallet bridge host; changes require explicit human approval.
  test(`malformed ${upgrade ? 'upgrade' : 'HTTP'} target is rejected and the bridge remains usable`, async t => {
    const source = `(${probe.toString()})(${upgrade}).catch(error => { console.error(error); process.exitCode = 1 })`
    const child = spawn(process.execPath, ['-e', source], { cwd: require('node:path').resolve(__dirname, '..'), stdio: ['ignore', 'pipe', 'pipe'] })
    t.after(() => { if (child.exitCode == null) child.kill() })
    let output = ''
    child.stdout.on('data', chunk => { output += chunk })
    child.stderr.on('data', chunk => { output += chunk })
    const code = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { child.kill(); reject(new Error('bridge child timed out')) }, 5000)
      child.once('error', error => { clearTimeout(timer); reject(error) })
      child.once('exit', code => { clearTimeout(timer); resolve(code) })
    })
    assert.equal(code, 0, output)
  })
}
