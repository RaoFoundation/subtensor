'use strict'
const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs/promises')
const os = require('node:os')
const path = require('node:path')
const { Interface, verifyMessage, getBytes, keccak256 } = require('ethers')
const { EthereumWallet, createEvmWorkflows, createEvmAddressUtilities, parseContractArtifact, encodeDeploy, coerceEvmArgument } = require('../dist/evm.js')
const { EvmKeystore } = require('../dist/evm-keystore.js')
const { Wallet } = require('../dist/wallet.js')
const { Keypair, publicKeyFromSs58, ss58FromPublic, verifySignature } = require('../dist/keys.js')
const { blake2_256 } = require('../dist/crypto.js')
const { Balance } = require('../dist/balance.js')
const crypto = { publicKeyFromSs58, ss58FromPublic, blake2_256, verify: verifySignature }
const alice = Keypair.fromUri('//Alice'), bob = Keypair.fromUri('//Bob')

test('EVM artifacts support Hardhat, Foundry, bare ABI and raw bytecode', () => {
  const abi = ['constructor(uint256 count)', 'function total() view returns (uint256)']
  for (const input of [{ abi, bytecode: '6000' }, { abi, bytecode: { object: '0x6000' } }, JSON.stringify({ abi, evm: { bytecode: { object: '6000' } } })]) {
    const artifact = parseContractArtifact(input)
    assert.equal(encodeDeploy(artifact, ['42']), '0x6000' + new Interface(abi).encodeDeploy([42]).slice(2))
  }
  assert.equal(parseContractArtifact(JSON.stringify(abi)).bytecode, null)
  assert.equal(encodeDeploy(parseContractArtifact('60\n00')), '0x6000')
  assert.throws(() => encodeDeploy(parseContractArtifact(abi)), /no deployment bytecode/)
  assert.deepEqual(coerceEvmArgument('bytes32[]', [alice.ss58Address, bob.ss58Address], crypto), [alice.publicKey, bob.publicKey])
  assert.throws(() => coerceEvmArgument('bool', 'maybe'), /boolean/)
})

test('EVM association signs exact hotkey and SCALE-u64 block proof', async () => {
  const account = new EthereumWallet('0x' + '11'.repeat(32)), addresses = createEvmAddressUtilities(crypto)
  const result = await addresses.associationProof(account, bob.ss58Address, 42n)
  const block = Buffer.alloc(8); block.writeBigUInt64LE(42n)
  const payload = Buffer.concat([bob.publicKey, getBytes(keccak256(block))])
  assert.equal(verifyMessage(payload, result.signature), account.address)
  const intent = await createEvmWorkflows(crypto).associateEvmKey(account, bob.ss58Address, 1, 42n)
  assert.equal(intent.op, 'associate_evm_key')
  assert.equal(intent.args.block_number, 42n)
})

test('EVM stake and native transfer helpers retain rao versus wei denominations', async () => {
  const captured = [], rpc = {
    async estimateGas(tx) { captured.push(tx); return 100000n }, async gasPrice() { return 1n }, async getNonce() { return 0 }, async chainId() { return 945n },
  }
  const flow = createEvmWorkflows(crypto), sender = '0x' + '11'.repeat(20)
  const preview = await flow.prepareAddStake(rpc, sender, bob.ss58Address, 1, '1.5')
  const call = flow.precompile('staking-v2').interface.decodeFunctionData('addStake', preview.data)
  assert.equal(call[1], 1500000000n)
  assert.equal(preview.valueWei, 0n)
  const transfer = await flow.prepareSendToSs58(rpc, sender, alice.ss58Address, Balance.fromTao('2'))
  assert.equal(transfer.valueWei, 2000000000000000000n)
  await assert.rejects(async () => flow.prepareRemoveStake(rpc, sender, bob.ss58Address, 1, Balance.fromTao('2')), /denominated/)
})

test('EVM V3 keystores are encrypted, owner-only, recoverable and refuse path escapes', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bt-evm-keystore-'))
  t.after(() => fs.rm(root, { recursive: true, force: true }))
  const store = new EvmKeystore(new Wallet({ path: root, name: 'keys' }))
  const expected = new EthereumWallet('0x' + '11'.repeat(32))
  const info = await store.import({ privateKey: expected.privateKey }, 'evm', { password: 'test-only-keystore-password' })
  assert.equal(info.address, expected.address.toLowerCase())
  assert.equal((await fs.stat(info.path)).mode & 0o777, 0o600)
  const exported = await store.export('evm')
  assert.equal(exported.version, 3)
  assert.equal(JSON.stringify(exported).includes(expected.privateKey.slice(2)), false)
  assert.equal((await store.unlock('evm', { password: 'test-only-keystore-password' })).address, expected.address)
  assert.equal((await store.list())[0].ss58Mirror, info.ss58Mirror)
  assert.throws(() => store.keyfilePath('../escape'), /single path/)
  await fs.symlink(info.path, store.keyfilePath('link'))
  await assert.rejects(store.export('link'), /ELOOP|symbolic link/i)
})
