const test = require('node:test')
const assert = require('node:assert/strict')
const { classifyError, classifyChainError, chainErrorFromDispatch, ErrorCode, CHAIN_ERROR_CATALOG } = require('../dist/chain-errors')
const { ChainError, JsonRpcError } = require('../dist/transport')
const { mapNativeError, RpcError, PolicyError } = require('../dist/errors')

test('all current module error names have a semantic classification', () => {
  assert.equal(CHAIN_ERROR_CATALOG.length, 421)
  for (const item of CHAIN_ERROR_CATALOG) assert.notEqual(classifyError('', item.name), ErrorCode.UNKNOWN, item.name)
  assert.equal(classifyError('insufficient balance', 'UnknownNewFailure'), ErrorCode.UNKNOWN)
  assert.equal(classifyError('', 'NewRateLimitExceeded'), ErrorCode.RATE_LIMITED)
  assert.equal(classifyError('', '__proto__'), ErrorCode.UNKNOWN)
  assert.equal(classifyChainError('SubtensorModule.UnknownNewFailure: insufficient balance').code, ErrorCode.UNKNOWN)
  assert.equal(classifyChainError('UnknownNewFailure: insufficient balance').code, ErrorCode.UNKNOWN)
})

test('pool custom, nonce, and watch failures carry structured semantics', () => {
  assert.equal(classifyChainError('Invalid transaction with custom error: 8').name, 'SlippageTooHigh')
  assert.equal(classifyChainError(new JsonRpcError('Invalid Transaction', 1010, { Custom: 7 })).code, ErrorCode.INSUFFICIENT_LIQUIDITY)
  assert.equal(new JsonRpcError('Invalid Transaction', 1010, { Custom: 7 }).code, 1010)
  assert.equal(classifyChainError('Transaction is outdated').name, 'Stale')
  assert.equal(classifyChainError('extrinsic usurped').name, 'Usurped')
  assert.equal(classifyChainError('SubtensorModule.NotEnoughStake: detail').code, ErrorCode.INSUFFICIENT_BALANCE)
  assert.equal(new ChainError('Invalid transaction with custom error: 3').code, ErrorCode.SUBNET_NOT_EXISTS)
  const rpc = mapNativeError(new Error('[BITTENSOR_CORE:RPC] Invalid transaction with custom error: 7'))
  assert.ok(rpc instanceof RpcError)
  assert.equal(rpc.chainError.code, ErrorCode.INSUFFICIENT_LIQUIDITY)
  assert.ok(mapNativeError(new Error('[BITTENSOR_CORE:POLICY] spend limit')) instanceof PolicyError)
})

test('dispatch errors resolve against execution metadata rather than static numeric indices', () => {
  const error = { attributes: { dispatch_error: { Module: { index: 7, error: '0x02000000' } } } }
  assert.equal(chainErrorFromDispatch(error).code, ErrorCode.UNKNOWN)
  const value = chainErrorFromDispatch(error, (pallet, index) => {
    assert.equal(pallet, 7); assert.equal(index, 2)
    return ['SubtensorModule.SlippageTooHigh', ['Historical runtime docs.']]
  })
  assert.equal(value.code, ErrorCode.INSUFFICIENT_LIQUIDITY)
  assert.equal(value.docs, 'Historical runtime docs.')
  assert.equal(chainErrorFromDispatch({ Err: { Token: { FundsUnavailable: null } } }).code, ErrorCode.INSUFFICIENT_BALANCE)
  const circular = {}; circular.Err = circular
  assert.equal(chainErrorFromDispatch(circular).code, ErrorCode.UNKNOWN)
})
