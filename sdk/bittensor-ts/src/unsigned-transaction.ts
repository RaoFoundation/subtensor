import { bytesHex, hexBytes, unsignedInteger, type PortableCrypto } from './portable'
import type { ScaleValue } from './types'

export interface UnsignedParams {
  era: ScaleValue
  nonce: bigint | number | string
  tip?: bigint | number | string
  tipAssetId?: bigint | number | string | null
  genesisHash: Uint8Array
  eraBlockHash: Uint8Array
  metadataHash?: Uint8Array | null
}
export interface UnsignedRuntime {
  readonly specVersion: number
  readonly transactionVersion: number
  encodeEra(era: ScaleValue): Uint8Array
  signaturePayload(call: Uint8Array, params: UnsignedParams): Uint8Array
  signaturePayloadParts(params: UnsignedParams): { includedInExtrinsic: Uint8Array; includedInSignedData: Uint8Array }
  signerPayload(address: string, call: Uint8Array, params: UnsignedParams): unknown
}
export interface UnsignedTransactionState {
  address: string; publicKey: Uint8Array; cryptoType: number; callData: Uint8Array; payload: Uint8Array
  includedInExtrinsic: Uint8Array; includedInSignedData: Uint8Array; metadataProof?: Uint8Array
  runtime: UnsignedRuntime; txParams: UnsignedParams
}
/** JSON-native signed-context data. It carries no policy approval or private material. */
export interface SerializedUnsignedTransaction {
  version: 1
  address: string; publicKey: string; cryptoType: number; callData: string; payload: string
  includedInExtrinsic: string; includedInSignedData: string; metadataProof: string | null
  specVersion: number; transactionVersion: number; metadataFingerprint: string
  era: string; nonce: string; tip: string; tipAssetId: string | null
  genesisHash: string; eraBlockHash: string; metadataHash: string | null
}
export interface RestoredUnsignedTransaction extends UnsignedTransactionState {
  genesisHash: string
  nonce: number
  signerPayload: unknown
}
type Crypto = Pick<PortableCrypto, 'blake2_256' | 'publicKeyFromSs58'>
const MAX_BYTES = 16 * 1024 * 1024
function equal(a: Uint8Array, b: Uint8Array): boolean { return a.length === b.length && a.every((byte, index) => byte === b[index]) }
function bytes(value: unknown, name: string, size?: number): Uint8Array {
  if (typeof value !== 'string' || value.length > 2 + MAX_BYTES * 2 || !/^0x(?:[0-9a-f]{2})*$/i.test(value)) throw new TypeError(`${name} must be bounded hexadecimal bytes`)
  const decoded = hexBytes(value)
  if (size != null && decoded.length !== size) throw new TypeError(`${name} must contain ${size} bytes`)
  return decoded
}
function version(value: unknown, name: string): number { if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value > 0xffffffff) throw new TypeError(`invalid ${name}`); return value }
function decimal(value: unknown, bits: number, name: string): bigint { if (typeof value !== 'string' || !/^(?:0|[1-9][0-9]*)$/.test(value)) throw new TypeError(`${name} must be a canonical unsigned decimal string`); return unsignedInteger(value, bits, name) }

export function serializeUnsignedTransaction(state: UnsignedTransactionState, crypto: Crypto, metadata: Uint8Array): SerializedUnsignedTransaction {
  if (!equal(crypto.publicKeyFromSs58(state.address), state.publicKey)) throw new TypeError('unsigned address and public key differ')
  const payload = state.runtime.signaturePayload(state.callData, state.txParams), parts = state.runtime.signaturePayloadParts(state.txParams)
  if (!equal(payload, state.payload) || !equal(parts.includedInExtrinsic, state.includedInExtrinsic) || !equal(parts.includedInSignedData, state.includedInSignedData)) throw new TypeError('unsigned signing context was modified')
  const result: SerializedUnsignedTransaction = {
    version: 1, address: state.address, publicKey: bytesHex(state.publicKey), cryptoType: state.cryptoType,
    callData: bytesHex(state.callData), payload: bytesHex(state.payload), includedInExtrinsic: bytesHex(state.includedInExtrinsic), includedInSignedData: bytesHex(state.includedInSignedData), metadataProof: state.metadataProof == null ? null : bytesHex(state.metadataProof),
    specVersion: state.runtime.specVersion, transactionVersion: state.runtime.transactionVersion, metadataFingerprint: bytesHex(crypto.blake2_256(metadata)),
    era: bytesHex(state.runtime.encodeEra(state.txParams.era)), nonce: unsignedInteger(state.txParams.nonce, 64, 'nonce').toString(), tip: unsignedInteger(state.txParams.tip ?? 0, 128, 'tip').toString(), tipAssetId: state.txParams.tipAssetId == null ? null : unsignedInteger(state.txParams.tipAssetId, 128, 'tipAssetId').toString(),
    genesisHash: bytesHex(state.txParams.genesisHash), eraBlockHash: bytesHex(state.txParams.eraBlockHash), metadataHash: state.txParams.metadataHash == null ? null : bytesHex(state.txParams.metadataHash),
  }
  // Share strict validation with imports, including size bounds and supported signature schemes.
  restoreUnsignedTransaction(result, state.runtime, crypto, metadata, result.genesisHash)
  return Object.freeze(result)
}
/** Reconstruct and verify bytes against this chain's current metadata; this does not authorize submission. */
export function restoreUnsignedTransaction(value: unknown, runtime: UnsignedRuntime, crypto: Crypto, metadata: Uint8Array, genesisHash: string): RestoredUnsignedTransaction {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError('unsigned transaction must be an object')
  const input = value as Record<string, unknown>
  const allowed = new Set(['version', 'address', 'publicKey', 'cryptoType', 'callData', 'payload', 'includedInExtrinsic', 'includedInSignedData', 'metadataProof', 'specVersion', 'transactionVersion', 'metadataFingerprint', 'era', 'nonce', 'tip', 'tipAssetId', 'genesisHash', 'eraBlockHash', 'metadataHash'])
  if (Object.keys(input).some(key => !allowed.has(key))) throw new TypeError('unsigned transaction contains an unsupported field')
  if (input.version !== 1 || typeof input.address !== 'string' || ![0, 1].includes(Number(input.cryptoType)) || typeof input.cryptoType !== 'number') throw new TypeError('unsupported unsigned transaction version or signer')
  if (version(input.specVersion, 'specVersion') !== runtime.specVersion || version(input.transactionVersion, 'transactionVersion') !== runtime.transactionVersion) throw new Error('runtime upgraded since transaction preparation; prepare and sign again')
  if (!equal(bytes(input.metadataFingerprint, 'metadataFingerprint', 32), crypto.blake2_256(metadata))) throw new Error('runtime metadata differs from transaction preparation')
  if (!equal(bytes(input.genesisHash, 'genesisHash', 32), bytes(genesisHash, 'connected genesis', 32))) throw new Error('unsigned transaction belongs to another chain')
  const publicKey = bytes(input.publicKey, 'publicKey', 32)
  if (!equal(publicKey, crypto.publicKeyFromSs58(input.address))) throw new TypeError('unsigned address and public key differ')
  const era = bytes(input.era, 'era')
  if (!(era.length === 1 && era[0] === 0) && era.length !== 2) throw new TypeError('invalid transaction era encoding')
  const encodedEra = era[0] | ((era[1] ?? 0) << 8)
  const period = 2 << (encodedEra & 15), phase = (encodedEra >> 4) * Math.max(period >> 12, 1)
  if (era.length === 2 && (period < 4 || phase >= period)) throw new TypeError('invalid mortal transaction era')
  const decodedEra: ScaleValue = era.length === 1 ? '00' : { period, phase }
  const nonce = decimal(input.nonce, 64, 'nonce')
  if (nonce > BigInt(Number.MAX_SAFE_INTEGER)) throw new RangeError('nonce exceeds JavaScript safe integer range')
  const params: UnsignedParams = { era: decodedEra, nonce, tip: decimal(input.tip, 128, 'tip'), tipAssetId: input.tipAssetId == null ? null : decimal(input.tipAssetId, 128, 'tipAssetId'), genesisHash: bytes(input.genesisHash, 'genesisHash', 32), eraBlockHash: bytes(input.eraBlockHash, 'eraBlockHash', 32), metadataHash: input.metadataHash == null ? null : bytes(input.metadataHash, 'metadataHash', 32) }
  if (!equal(runtime.encodeEra(params.era), era)) throw new TypeError('noncanonical transaction era')
  const callData = bytes(input.callData, 'callData'), payload = bytes(input.payload, 'payload'), parts = runtime.signaturePayloadParts(params)
  if (!callData.length || !equal(runtime.signaturePayload(callData, params), payload) || !equal(parts.includedInExtrinsic, bytes(input.includedInExtrinsic, 'includedInExtrinsic')) || !equal(parts.includedInSignedData, bytes(input.includedInSignedData, 'includedInSignedData'))) throw new TypeError('unsigned transaction payload does not match its call and signing context')
  return { address: input.address, publicKey, cryptoType: input.cryptoType, callData, payload, ...parts, txParams: params, runtime, genesisHash: bytesHex(params.genesisHash), nonce: Number(nonce), signerPayload: runtime.signerPayload(input.address, callData, params), metadataProof: input.metadataProof == null ? undefined : bytes(input.metadataProof, 'metadataProof') }
}
