'use strict'
const assert = require('node:assert/strict')
const test = require('node:test')
const { Timelocked, TimelockNotReady, createTimelock, parseDuration, formatDuration, currentRound, roundAt, revealTime } = require('../dist/timelock-workflow.js')

function envelope(round, inner = new Uint8Array([7, 8, 9])) {
  const data = new Uint8Array(inner.length + 9)
  data[0] = inner.length * 4; data.set(inner, 1)
  new DataView(data.buffer).setBigUint64(inner.length + 1, round, true)
  return data
}

test('timelock duration and beacon arithmetic use UTC and exact rounds', () => {
  assert.equal(parseDuration('1h30m 0.5s'), 5400.5)
  assert.equal(parseDuration('2w'), 1209600)
  assert.equal(formatDuration(183930), '2d 3h 5m 30s')
  assert.throws(() => parseDuration('1h nonsense'), /parse/)
  assert.throws(() => parseDuration(Infinity), /finite/)
  assert.equal(roundAt(1692803367), 1n)
  assert.equal(roundAt(1692803367.001), 2n)
  assert.equal(currentRound(1692803370), 2n)
  assert.equal(revealTime(2n).toISOString(), '2023-08-23T15:09:30.000Z')
  assert.equal(roundAt('2023-08-23T15:09:30'), 2n)
})

test('Timelocked parses exact SCALE envelope and returns independent byte copies', () => {
  const sealed = Timelocked.parse(envelope(5n))
  assert.equal(sealed.revealRound, 5n)
  assert.deepEqual(sealed.encrypted, new Uint8Array([7, 8, 9]))
  assert.equal(Timelocked.parse(sealed.hex()).revealRound, 5n)
  sealed.ciphertext.fill(0)
  assert.equal(sealed.revealRound, Timelocked.parse(sealed.ciphertext).revealRound)
  assert.throws(() => Timelocked.parse(new Uint8Array([...sealed.ciphertext, 0])), /length/)
  assert.throws(() => Timelocked.parse('invalid'), /hex/)
})

test('timelock encrypt supports absolute, relative, and explicit moments through injected crypto', async () => {
  const captured = []
  const service = createTimelock({
    async encryptAtRound(data, round) { captured.push({ data, round }); return { ciphertext: envelope(round, data), revealRound: round } },
    async decrypt(data) { return Timelocked.parse(data).encrypted },
  })
  const round = currentRound() + 10n
  const sealed = await service.encrypt('answer', { revealRound: round })
  assert.equal(sealed.revealRound, round)
  assert.equal(new TextDecoder().decode(captured[0].data), 'answer')
  await assert.rejects(sealed.decrypt(), TimelockNotReady)
  await assert.rejects(sealed.decrypt({ wait: true, timeout: 0.01 }), TimelockNotReady)
  await assert.rejects(service.encrypt('answer', { revealIn: 1, revealRound: round }), /exactly one/)
  await assert.rejects(service.encrypt('answer', { revealRound: currentRound() }), /future/)
  assert.ok((await service.encrypt('answer', { revealIn: '1m' })).revealRound > currentRound())
})

test('timelock decrypt handles published rounds, bounded failures, and cancellation', async () => {
  const data = envelope(2n)
  const success = createTimelock({ async encryptAtRound() { throw new Error('unused') }, async decrypt() { return new Uint8Array([42]) } })
  assert.deepEqual(await success.decrypt(data, { timeout: 0 }), new Uint8Array([42]))
  const fail = createTimelock({ async encryptAtRound() { throw new Error('unused') }, async decrypt() { throw new Error('bad proof') } })
  await assert.rejects(fail.decrypt(data, { timeout: 0 }), /failed to decrypt/)
  const controller = new AbortController(); controller.abort(new Error('cancelled'))
  await assert.rejects(success.decrypt(data, { signal: controller.signal }), /cancelled/)
})
