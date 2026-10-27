import { Interface, FunctionFragment, ParamType, HDNodeWallet, Wallet as EthereumWallet, getBytes, hexlify, keccak256, type InterfaceAbi, type TransactionRequest } from 'ethers'
import { Balance } from './balance'
import { PRECOMPILE_CATALOG } from './evm-catalog'
import { bytesHex, concatBytes, integerLE, unsignedInteger, type PortableCrypto } from './portable'
import { buildIntent, type IntentDescriptor } from './intents'

export const WEI_PER_TAO = 10n ** 18n
export const WEI_PER_RAO = 10n ** 9n
export const ETH_DERIVATION_PATH = "m/44'/60'/0'/0/0"
export { EthereumWallet, HDNodeWallet }
export type EvmAccount = EthereumWallet | HDNodeWallet

export function isH160(value: string): boolean { return /^0x[0-9a-fA-F]{40}$/.test(value) }
export function normalizeH160(value: string): string {
  const text = value.trim().replace(/^(?!0x)/, '0x')
  if (!isH160(text)) throw new TypeError('expected a 20-byte EVM address')
  return text.toLowerCase()
}
export function weiToBalance(wei: bigint): Balance { return Balance.fromRao(wei / WEI_PER_RAO) }
export function balanceToWei(balance: Balance): bigint {
  if (balance.netuid !== 0) throw new TypeError('EVM values must be TAO, not alpha')
  return balance.rao * WEI_PER_RAO
}

export interface EvmNetwork { name: string; chainId: number | null; rpcUrl: string; currencySymbol: string; evmDecimals: number }
export const EVM_NETWORKS: Readonly<Record<string, EvmNetwork>> = Object.freeze({
  finney: { name: 'finney', chainId: 964, rpcUrl: 'https://lite.chain.opentensor.ai', currencySymbol: 'TAO', evmDecimals: 18 },
  test: { name: 'test', chainId: 945, rpcUrl: 'https://test.chain.opentensor.ai', currencySymbol: 'TAO', evmDecimals: 18 },
  local: { name: 'local', chainId: null, rpcUrl: 'http://127.0.0.1:9944', currencySymbol: 'TAO', evmDecimals: 18 },
})
export function evmNetwork(network: string): EvmNetwork {
  const aliases: Record<string, string> = { 'wss://entrypoint-finney.opentensor.ai:443': 'finney', 'wss://lite.chain.opentensor.ai:443': 'finney', 'wss://test.finney.opentensor.ai:443': 'test', 'ws://127.0.0.1:9944': 'local' }
  const known = EVM_NETWORKS[aliases[network] ?? network]
  if (known) return known
  if (/^(https?|wss?):\/\//.test(network)) return { name: network, chainId: null, rpcUrl: network.replace(/^ws/, 'http'), currencySymbol: 'TAO', evmDecimals: 18 }
  throw new TypeError(`unknown EVM network ${network}`)
}

export class EvmRpcError extends Error {
  constructor(readonly code: number, message: string, readonly data?: unknown) { super(message); this.name = 'EvmRpcError' }
}
export class EvmRpc {
  private id = 0
  constructor(readonly url: string, readonly timeoutMs = 30_000, private readonly fetcher: typeof fetch = globalThis.fetch) {}
  async call<T = unknown>(method: string, params: unknown[] = [], signal?: AbortSignal): Promise<T> {
    const id = ++this.id
    const timeout = AbortSignal.timeout(this.timeoutMs)
    const response = await this.fetcher(this.url, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id, method, params }), signal: signal ? AbortSignal.any([signal, timeout]) : timeout })
    if (!response.ok) throw new EvmRpcError(response.status, `EVM RPC HTTP ${response.status}`)
    const payload = await response.json() as { jsonrpc: string; id: number; result?: T; error?: { code: number; message: string; data?: unknown } }
    if (payload.jsonrpc !== '2.0' || payload.id !== id || (('result' in payload) === ('error' in payload))) throw new EvmRpcError(-1, 'invalid EVM RPC response')
    if (payload.error) throw new EvmRpcError(payload.error.code, payload.error.message, payload.error.data)
    return payload.result as T
  }
  async chainId(): Promise<bigint> { return BigInt(await this.call<string>('eth_chainId')) }
  async blockNumber(): Promise<number> { return Number(BigInt(await this.call<string>('eth_blockNumber'))) }
  async getBalanceWei(address: string, block = 'latest'): Promise<bigint> { return BigInt(await this.call<string>('eth_getBalance', [normalizeH160(address), block])) }
  async getNonce(address: string, block = 'pending'): Promise<number> { return Number(BigInt(await this.call<string>('eth_getTransactionCount', [normalizeH160(address), block]))) }
  async gasPrice(): Promise<bigint> { return BigInt(await this.call<string>('eth_gasPrice')) }
  async estimateGas(tx: Record<string, unknown>): Promise<bigint> { return BigInt(await this.call<string>('eth_estimateGas', [tx])) }
  ethCall(tx: Record<string, unknown>, block = 'latest'): Promise<string> { return this.call('eth_call', [tx, block]) }
  sendRawTransaction(raw: string | Uint8Array): Promise<string> { return this.call('eth_sendRawTransaction', [typeof raw === 'string' ? raw : bytesHex(raw)]) }
  async waitForReceipt(hash: string, options: { timeoutMs?: number; pollIntervalMs?: number; signal?: AbortSignal } = {}): Promise<EvmReceipt> {
    const deadline = Date.now() + (options.timeoutMs ?? 120_000)
    for (;;) {
      options.signal?.throwIfAborted()
      const receipt = await this.call<EvmReceipt | null>('eth_getTransactionReceipt', [hash], options.signal)
      if (receipt) return receipt
      if (Date.now() >= deadline) throw new Error(`receipt timeout for ${hash}`)
      await new Promise(resolve => setTimeout(resolve, Math.min(options.pollIntervalMs ?? 3000, deadline - Date.now())))
    }
  }
}
export interface EvmReceipt { status: string; blockNumber: string; gasUsed: string; contractAddress?: string | null; [key: string]: unknown }
export interface EvmTxPreview { sender: string; to: string | null; valueWei: bigint; data: string; gas: bigint; gasPriceWei: bigint; nonce: number; chainId: bigint }
export async function prepareEvmTransaction(rpc: EvmRpc, sender: string, to: string | null, options: { valueWei?: bigint; data?: string } = {}): Promise<EvmTxPreview> {
  sender = normalizeH160(sender)
  if (to != null) to = normalizeH160(to)
  const valueWei = unsignedInteger(options.valueWei ?? 0n, 256, 'valueWei'), data = options.data ?? '0x'
  getBytes(data)
  const [gas, gasPriceWei, nonce, chainId] = await Promise.all([rpc.estimateGas({ from: sender, ...(to ? { to } : {}), value: '0x' + valueWei.toString(16), data }), rpc.gasPrice(), rpc.getNonce(sender), rpc.chainId()])
  return { sender, to, valueWei, data, gas, gasPriceWei, nonce, chainId }
}
export async function sendEvmTransaction(rpc: EvmRpc, account: Pick<EvmAccount, 'address' | 'signTransaction'>, preview: EvmTxPreview, options: { wait?: boolean; signal?: AbortSignal } = {}) {
  if (normalizeH160(account.address) !== preview.sender) throw new Error('transaction signer differs from preview sender')
  if (await rpc.chainId() !== preview.chainId) throw new Error('EVM chain changed since transaction preview')
  const tx: TransactionRequest = { type: 0, to: preview.to, value: preview.valueWei, data: preview.data, gasLimit: preview.gas, gasPrice: preview.gasPriceWei, nonce: preview.nonce, chainId: preview.chainId }
  const txHash = await rpc.sendRawTransaction(await account.signTransaction(tx))
  if (options.wait === false) return { txHash }
  const receipt = await rpc.waitForReceipt(txHash, options)
  return { txHash, receipt, success: BigInt(receipt.status) === 1n, blockNumber: Number(BigInt(receipt.blockNumber)), gasUsed: BigInt(receipt.gasUsed), contractAddress: receipt.contractAddress }
}

export class Precompile {
  readonly interface: Interface
  constructor(readonly name: keyof typeof PRECOMPILE_CATALOG, private readonly crypto?: PortableCrypto) { this.interface = new Interface(PRECOMPILE_CATALOG[name].abi as InterfaceAbi) }
  get address(): string { return PRECOMPILE_CATALOG[this.name].address }
  get abi() { return PRECOMPILE_CATALOG[this.name].abi }
  get description(): string { return PRECOMPILE_CATALOG[this.name].description }
  get deprecated(): boolean { return PRECOMPILE_CATALOG[this.name].deprecated }
  functions(): FunctionFragment[] { return this.interface.fragments.filter((item): item is FunctionFragment => item.type === 'function') }
  function(name: string): FunctionFragment { const fn = this.interface.getFunction(name); if (!fn) throw new Error(`unknown or ambiguous function ${name}`); return fn }
  encodeCall(name: string, args: readonly unknown[]): string {
    const fn = this.function(name)
    if (fn.inputs.length !== args.length) throw new TypeError(`${name} expects ${fn.inputs.length} arguments`)
    return this.interface.encodeFunctionData(fn, args.map((value, i) => coerceEvmArgument(fn.inputs[i], value, this.crypto)))
  }
  decodeResult(name: string, data: string | Uint8Array): unknown[] { return Array.from(this.interface.decodeFunctionResult(this.function(name), data)) }
  async call(rpc: EvmRpc, name: string, args: readonly unknown[] = [], options: { from?: string; block?: string } = {}) { return this.decodeResult(name, await rpc.ethCall({ to: this.address, data: this.encodeCall(name, args), ...(options.from ? { from: normalizeH160(options.from) } : {}) }, options.block)) }
  prepare(rpc: EvmRpc, sender: string, name: string, args: readonly unknown[] = [], valueWei = 0n) { return prepareEvmTransaction(rpc, sender, this.address, { data: this.encodeCall(name, args), valueWei }) }
}
export const PRECOMPILES = Object.freeze(Object.fromEntries(Object.keys(PRECOMPILE_CATALOG).map(name => [name, new Precompile(name as keyof typeof PRECOMPILE_CATALOG)])))
export function getPrecompile(name: string): Precompile { const p = PRECOMPILES[name]; if (!p) throw new Error(`unknown precompile ${name}`); return p }
export function precompileCallerRole(name: keyof typeof PRECOMPILE_CATALOG, fn: string): string | undefined { return (PRECOMPILE_CATALOG[name].callerRoles as Record<string, string>)[fn] }
export function precompileFunctionDeprecation(name: keyof typeof PRECOMPILE_CATALOG, fn: string): string | undefined { return (PRECOMPILE_CATALOG[name].deprecatedFunctions as Record<string, string>)[fn] }
export interface ContractArtifact { abi: InterfaceAbi | null; bytecode: string | null; contractName?: string }
export function parseContractArtifact(input: unknown): ContractArtifact {
  if (typeof input === 'string' && !/^[\s]*[\[{]/.test(input)) {
    const value = input.replace(/\s/g, ''), bytecode = value.startsWith('0x') ? value : '0x' + value
    getBytes(bytecode)
    if (bytecode === '0x') throw new TypeError('empty contract bytecode')
    return { abi: null, bytecode }
  }
  const parsed = typeof input === 'string' ? JSON.parse(input) : input
  if (Array.isArray(parsed)) { new Interface(parsed); return { abi: parsed, bytecode: null } }
  if (parsed == null || typeof parsed !== 'object') throw new TypeError('expected contract artifact, ABI array, or bytecode')
  const value = parsed as { abi?: InterfaceAbi; bytecode?: string | { object?: string }; evm?: { bytecode?: { object?: string } }; contractName?: string }
  const code = typeof value.bytecode === 'string' ? value.bytecode : value.bytecode?.object ?? value.evm?.bytecode?.object
  if (!value.abi && !code) throw new TypeError('artifact needs ABI or bytecode')
  const bytecode = code && code !== '0x' ? code.startsWith('0x') ? code : '0x' + code : null
  if (bytecode) getBytes(bytecode)
  if (value.abi) new Interface(value.abi)
  return { abi: value.abi ?? null, bytecode, contractName: value.contractName }
}
export function encodeDeploy(artifact: ContractArtifact, args: readonly unknown[] = [], crypto?: PortableCrypto): string {
  if (!artifact.bytecode) throw new TypeError('artifact has no deployment bytecode')
  const iface = new Interface(artifact.abi ?? [])
  if (iface.deploy.inputs.length !== args.length) throw new TypeError(`constructor expects ${iface.deploy.inputs.length} arguments`)
  return artifact.bytecode + iface.encodeDeploy(args.map((arg, i) => coerceEvmArgument(iface.deploy.inputs[i], arg, crypto))).slice(2)
}

export function coerceEvmArgument(type: ParamType | string, value: unknown, crypto?: PortableCrypto): unknown {
  const param = typeof type === 'string' ? ParamType.from(type) : type
  if (param.baseType === 'array') {
    const values = typeof value === 'string' ? value.trim().startsWith('[') ? JSON.parse(value) : value.split(',') : value
    if (!Array.isArray(values)) throw new TypeError(`${param.type} requires an array`)
    if (param.arrayLength !== -1 && values.length !== param.arrayLength) throw new RangeError(`expected ${param.arrayLength} array values`)
    return values.map(item => coerceEvmArgument(param.arrayChildren!, item, crypto))
  }
  if (param.baseType === 'tuple') {
    if (value == null || typeof value !== 'object') throw new TypeError('tuple requires an array or object')
    return param.components!.map((component, index) => coerceEvmArgument(component, Array.isArray(value) ? value[index] : (value as Record<string, unknown>)[component.name], crypto))
  }
  if (/^u?int/.test(param.type)) {
    if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new RangeError('ABI integers above the safe range require bigint or strings')
    return BigInt(value as string | number | bigint)
  }
  if (param.type === 'bool') {
    if (typeof value === 'boolean') return value
    if (['1', 'true', 'yes', 'y'].includes(String(value).trim().toLowerCase())) return true
    if (['0', 'false', 'no', 'n'].includes(String(value).trim().toLowerCase())) return false
    throw new TypeError('invalid ABI boolean')
  }
  if (param.type === 'address') return normalizeH160(String(value))
  if (param.type.startsWith('bytes')) {
    if (value instanceof Uint8Array) return value
    if (typeof value !== 'string') throw new TypeError('ABI bytes require a hex string or bytes')
    const text = value.trim()
    if (/^(?:0x)?(?:[0-9a-f]{2})*$/i.test(text)) return getBytes(text.startsWith('0x') ? text : '0x' + text)
    if (param.type === 'bytes32' && crypto) return crypto.publicKeyFromSs58(text)
    throw new TypeError('bytes32 SS58 arguments require an address codec')
  }
  return value
}

export class EvmContract {
  readonly interface: Interface
  readonly address: string
  constructor(address: string, readonly abi: InterfaceAbi, private readonly crypto?: PortableCrypto) { this.address = normalizeH160(address); this.interface = new Interface(abi) }
  encodeCall(name: string, args: readonly unknown[] = []): string {
    const fn = this.interface.getFunction(name)
    if (!fn || fn.inputs.length !== args.length) throw new TypeError('unknown function or incorrect argument count')
    return this.interface.encodeFunctionData(fn, args.map((arg, i) => coerceEvmArgument(fn.inputs[i], arg, this.crypto)))
  }
  async call(rpc: EvmRpc, name: string, args: readonly unknown[] = [], options: { from?: string; block?: string } = {}): Promise<unknown[]> {
    const data = await rpc.ethCall({ to: this.address, data: this.encodeCall(name, args), ...(options.from ? { from: normalizeH160(options.from) } : {}) }, options.block)
    return Array.from(this.interface.decodeFunctionResult(name, data))
  }
  prepare(rpc: EvmRpc, sender: string, name: string, args: readonly unknown[] = [], valueWei = 0n): Promise<EvmTxPreview> {
    return prepareEvmTransaction(rpc, sender, this.address, { data: this.encodeCall(name, args), valueWei })
  }
}

export async function evmSetupLocalnetPlan(rpc: EvmRpc, network: string, chainId = 945): Promise<IntentDescriptor[]> {
  if (evmNetwork(network).name === 'finney' || evmNetwork(network).name === 'test') throw new Error('EVM setup is restricted to local or explicitly configured custom networks')
  const id = unsignedInteger(chainId, 64, 'chainId'), calls: IntentDescriptor[] = []
  let current: bigint | null
  try { current = await rpc.chainId() } catch (error) { if (!(error instanceof EvmRpcError)) throw error; current = null }
  if (current !== id) calls.push({ pallet: 'AdminUtils', call: 'sudo_set_evm_chain_id', params: { chain_id: id } })
  calls.push({ pallet: 'EVM', call: 'disable_whitelist', params: { disabled: true } })
  return calls.map(call => ({ pallet: 'Sudo', call: 'sudo', params: { call } }))
}

export function createEvmWorkflows(crypto: PortableCrypto) {
  const addresses = createEvmAddressUtilities(crypto), staking = new Precompile('staking-v2', crypto)
  const amount = (value: Balance | string | number, netuid: number): Balance => {
    const balance = value instanceof Balance ? value : netuid === 0 ? Balance.fromTao(value) : Balance.fromAlpha(value, netuid)
    if (balance.netuid !== netuid || balance.rao <= 0n) throw new RangeError('amount must be positive and denominated in the expected subnet asset')
    return balance
  }
  return {
    ...addresses,
    precompile: (name: keyof typeof PRECOMPILE_CATALOG) => new Precompile(name, crypto),
    contract: (address: string, abi: InterfaceAbi) => new EvmContract(address, abi, crypto),
    prepareDeploy: (rpc: EvmRpc, sender: string, artifact: ContractArtifact, args: readonly unknown[] = [], valueWei = 0n) => prepareEvmTransaction(rpc, sender, null, { data: encodeDeploy(artifact, args, crypto), valueWei }),
    prepareSend: (rpc: EvmRpc, sender: string, to: string, value: Balance | string | number) => prepareEvmTransaction(rpc, sender, to, { valueWei: balanceToWei(amount(value, 0)) }),
    prepareSendToSs58: (rpc: EvmRpc, sender: string, to: string, value: Balance | string | number) => new Precompile('balance-transfer', crypto).prepare(rpc, sender, 'transfer', [to], balanceToWei(amount(value, 0))),
    prepareAddStake: (rpc: EvmRpc, sender: string, hotkey: string, netuid: number, value: Balance | string | number) => staking.prepare(rpc, sender, 'addStake', [hotkey, amount(value, 0).rao, netuid]),
    prepareRemoveStake: (rpc: EvmRpc, sender: string, hotkey: string, netuid: number, value: Balance | string | number) => staking.prepare(rpc, sender, 'removeStake', [hotkey, amount(value, netuid).rao, netuid]),
    async stake(rpc: EvmRpc, address: string, hotkey: string, netuid: number): Promise<Balance> {
      const [rao] = await staking.call(rpc, 'getStake', [hotkey, addresses.h160ToSs58(address), netuid])
      return Balance.fromRao(BigInt(rao as bigint), netuid)
    },
    async associateEvmKey(account: Pick<EvmAccount, 'address' | 'signMessage'>, hotkey: string, netuid: number, block: bigint | number) {
      const proof = await addresses.associationProof(account, hotkey, block)
      return buildIntent('associate_evm_key', { netuid, evm_key: normalizeH160(account.address), block_number: proof.blockNumber, signature: proof.signature })
    },
    fundEvmKey: (address: string, value: Balance | string | number) => buildIntent('fund_evm_key', { evm_address: normalizeH160(address), amount_tao: value }),
  }
}
export function createEvmAddressUtilities(crypto: PortableCrypto) {
  return {
    h160ToSs58(address: string, format = 42) { return crypto.ss58FromPublic(crypto.blake2_256(concatBytes(new TextEncoder().encode('evm:'), getBytes(normalizeH160(address)))), format) },
    ss58ToPubkey(address: string) { return bytesHex(crypto.publicKeyFromSs58(address)) },
    pubkeyToSs58(key: string | Uint8Array, format = 42) { const bytes = typeof key === 'string' ? getBytes(key) : key; if (bytes.length !== 32) throw new Error('public key must be 32 bytes'); return crypto.ss58FromPublic(bytes, format) },
    ss58ToH160Truncated(address: string) { return bytesHex(crypto.publicKeyFromSs58(address).slice(0, 20)) },
    async associationProof(account: Pick<EvmAccount, 'signMessage'>, hotkey: string, block: bigint | number): Promise<{ signature: string; blockNumber: bigint }> {
      const blockNumber = unsignedInteger(block, 64, 'blockNumber')
      return { signature: await account.signMessage(concatBytes(crypto.publicKeyFromSs58(hotkey), getBytes(keccak256(integerLE(blockNumber, 8))))), blockNumber }
    },
  }
}
