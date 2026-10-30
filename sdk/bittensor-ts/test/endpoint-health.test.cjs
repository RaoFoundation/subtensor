const test = require('node:test')
const assert = require('node:assert/strict')
const { probeEndpoints } = require('../dist/endpoint-health.js')
const { JsonRpcTransport } = require('../dist/transport.js')

test('endpoint health rejects coercible malformed heights and accepts canonical headers', async () => {
  const request = JsonRpcTransport.prototype.request
  try {
    JsonRpcTransport.prototype.request = async function () {
      return { number: this.endpoint.includes('healthy') ? '0x7b' : this.endpoint.includes('null') ? null : false }
    }
    const results = await probeEndpoints(['https://null.test', 'https://false.test', 'https://healthy.test', 'https://healthy.test'])
    assert.equal(results.length, 3)
    assert.equal(results[0].endpoint, 'https://healthy.test')
    assert.equal(results[0].blockNumber, 123)
    assert.equal(results.filter(result => result.ok).length, 1)
    assert.ok(results.slice(1).every(result => /invalid header/.test(result.error)))
  } finally { JsonRpcTransport.prototype.request = request }
})
