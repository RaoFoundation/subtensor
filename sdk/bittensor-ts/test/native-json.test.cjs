const test = require('node:test')
const assert = require('node:assert/strict')
const http = require('node:http')
const { NativeChainClient } = require('../dist/transaction')
const golden = require('../../python/tests/fixtures/golden.json')

test('native asynchronous RPC preserves ordinary, arbitrary precision, and prototype-key JSON values', async () => {
  const server = http.createServer(async (request, response) => {
    let body = ''; for await (const chunk of request) body += chunk
    const { id, method } = JSON.parse(body)
    const values = {
      system_properties: { ss58Format: 42 },
      state_getRuntimeVersion: { specVersion: 419, transactionVersion: 1 },
      state_call: golden.metadata.v15_hex,
      chain_getBlockHash: '0x' + '11'.repeat(32),
    }
    const result = method === 'test_numbers'
      ? '{"small":486,"large":9007199254740993,"u256":115792089237316195423570985008687907853269984665640564039457584007913129639935,"negative":-9007199254740993,"fraction":0.25,"nested":[7],"__proto__":{"safe":true},"a\\u0000b":2}'
      : JSON.stringify(values[method])
    response.setHeader('content-type', 'application/json')
    response.end(`{"jsonrpc":"2.0","id":${id},"result":${result}}`)
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  try {
    const client = await NativeChainClient.connect(`http://127.0.0.1:${server.address().port}`)
    const value = await client.rpcValue('test_numbers')
    assert.equal(value.small, 486)
    assert.equal(value.large, 9007199254740993n)
    assert.equal(value.u256, (1n << 256n) - 1n)
    assert.equal(value.negative, -9007199254740993n)
    assert.equal(value.fraction, 0.25)
    assert.deepEqual(value.nested, [7])
    assert.equal(Object.hasOwn(value, '__proto__'), true)
    assert.equal(Object.getPrototypeOf(value), Object.prototype)
    assert.equal(value['a\0b'], 2)
  } finally { await new Promise(resolve => server.close(resolve)) }
})
