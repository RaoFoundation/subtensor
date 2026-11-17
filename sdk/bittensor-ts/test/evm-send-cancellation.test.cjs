const test = require('node:test')
const assert = require('node:assert/strict')
const { Wallet: EthereumWallet, keccak256 } = require('ethers')
const { sendEvmTransaction, EvmRpc, EvmSubmissionUncertainError } = require('../dist/evm')
const account = new EthereumWallet('0x' + '11'.repeat(32))
const preview = { sender: account.address.toLowerCase(), to: '0x' + '22'.repeat(20), valueWei: 1n, data: '0x', gas: 21000n, gasPriceWei: 1n, nonce: 7, chainId: 945n }
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done }); return { promise, resolve } }

function fixture() {
  const state = { chains: 0, signs: 0, sends: 0, waits: 0, raw: undefined }
  const signer = { address: account.address, async signTransaction(tx) { state.signs++; return account.signTransaction(tx) } }
  const rpc = {
    async chainId() { state.chains++; return preview.chainId },
    async sendRawTransaction(raw) { state.sends++; state.raw = raw; return keccak256(raw) },
    async waitForReceipt() { state.waits++; return { status: '0x1', blockNumber: '0x2', gasUsed: '0x5208' } },
  }
  return { state, signer, rpc }
}

// PROTECTED_REGRESSION: cancelling an EVM payment before broadcast must never send funds; changes require explicit human approval.
test('pre-aborted EVM sends neither sign nor broadcast, with or without receipt waiting', async () => {
  for (const wait of [true, false]) {
    const f = fixture(), controller = new AbortController(), reason = Error('cancel payment')
    controller.abort(reason)
    await assert.rejects(sendEvmTransaction(f.rpc, f.signer, preview, { wait, signal: controller.signal }), error => error === reason)
    assert.deepEqual([f.state.chains, f.state.signs, f.state.sends, f.state.waits], [0, 0, 0, 0])
  }
})

// PROTECTED_REGRESSION: cancellation during asynchronous EVM preparation or signing cannot trigger a later broadcast; changes require explicit human approval.
test('EVM cancellation interrupts a pending chain read or signer and never broadcasts their late result', async () => {
  for (const stage of ['chain', 'sign']) {
    const f = fixture(), controller = new AbortController(), entered = deferred(), pending = deferred(), reason = Error(`cancel ${stage}`)
    if (stage === 'chain') f.rpc.chainId = async () => { f.state.chains++; entered.resolve(); await pending.promise; return preview.chainId }
    else f.signer.signTransaction = async tx => { f.state.signs++; entered.resolve(); await pending.promise; return account.signTransaction(tx) }
    const sending = sendEvmTransaction(f.rpc, f.signer, preview, { wait: false, signal: controller.signal })
    await entered.promise
    controller.abort(reason)
    await assert.rejects(sending, error => error === reason)
    pending.resolve()
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(f.state.sends, 0)
    assert.equal(f.state.signs, stage === 'chain' ? 0 : 1)
  }
})

// PROTECTED_REGRESSION: a signer that observes cancellation must not hand its completed payment to the transport; changes require explicit human approval.
test('EVM checks cancellation again immediately after signing', async () => {
  const f = fixture(), controller = new AbortController(), reason = Error('stop before handoff')
  f.signer.signTransaction = async tx => { const raw = await account.signTransaction(tx); controller.abort(reason); return raw }
  await assert.rejects(sendEvmTransaction(f.rpc, f.signer, preview, { signal: controller.signal }), error => error === reason)
  assert.equal(f.state.sends, 0)
})

// PROTECTED_REGRESSION: interrupted EVM submission retains its local transaction hash so retry cannot silently duplicate a payment; changes require explicit human approval.
test('EVM cancellation after handoff retains the local hash for broadcast and receipt uncertainty', async () => {
  for (const stage of ['broadcast', 'receipt']) {
    const f = fixture(), controller = new AbortController(), entered = deferred(), pending = deferred(), reason = Error(`cancel ${stage}`)
    if (stage === 'broadcast') f.rpc.sendRawTransaction = async raw => { f.state.sends++; f.state.raw = raw; entered.resolve(); await pending.promise; return keccak256(raw) }
    else f.rpc.waitForReceipt = async () => { f.state.waits++; entered.resolve(); await pending.promise; return { status: '0x1', blockNumber: '0x2', gasUsed: '0x5208' } }
    const sending = sendEvmTransaction(f.rpc, f.signer, preview, { wait: stage !== 'broadcast', signal: controller.signal })
    await entered.promise
    controller.abort(reason)
    await assert.rejects(sending, error => {
      assert.ok(error instanceof EvmSubmissionUncertainError)
      assert.equal(error.txHash, keccak256(f.state.raw))
      assert.equal(error.extrinsicHash, error.txHash)
      assert.equal(error.submissionUncertain, true)
      assert.equal(error.cause, reason)
      return true
    })
    pending.resolve()
    assert.equal(f.state.sends, 1)
  }
})

test('EVM send reports successful sends and retains a hash on transport failures', async () => {
  for (const wait of [false, true]) {
    const f = fixture(), result = await sendEvmTransaction(f.rpc, f.signer, preview, { wait })
    assert.equal(result.txHash, keccak256(f.state.raw))
    assert.equal(f.state.sends, 1)
    assert.equal(f.state.waits, Number(wait))
    if (wait) assert.equal(result.success, true)
  }
  for (const outcome of ['disconnect', 'wrong_hash']) {
    const f = fixture()
    f.rpc.sendRawTransaction = async raw => { f.state.raw = raw; if (outcome === 'disconnect') throw Error('response lost'); return '0x' + '00'.repeat(32) }
    await assert.rejects(sendEvmTransaction(f.rpc, f.signer, preview), error => error.submissionUncertain === true && error.txHash === keccak256(f.state.raw))
  }
})

test('EVM raw RPC sends honor an already aborted signal before fetch', async () => {
  const controller = new AbortController(), reason = Error('not sent')
  controller.abort(reason)
  const rpc = new EvmRpc('http://unused', 30000, async () => assert.fail('cancelled request reached fetch'))
  await assert.rejects(rpc.sendRawTransaction('0x01', controller.signal), error => error === reason)
})
