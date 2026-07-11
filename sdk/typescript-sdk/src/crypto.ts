import native from './native'
import { nativeCall } from './errors'
import { toBuffer } from './wire'
import type { ByteLike, ChainInfo } from './types'

function nativeChainInfo(info: ChainInfo): Record<string, unknown> {
  return {
    specVersion: info.specVersion,
    specName: info.specName,
    base58Prefix: info.base58Prefix,
    decimals: info.decimals,
    tokenSymbol: info.tokenSymbol,
  }
}

export function metadataDigest(metadataBytes: ByteLike, info: ChainInfo): Buffer {
  return nativeCall(() =>
    native.metadataDigest(toBuffer(metadataBytes, 'metadataBytes'), nativeChainInfo(info)),
  )
}

export function generateExtrinsicProof(
  callData: ByteLike,
  includedInExtrinsic: ByteLike,
  includedInSignedData: ByteLike,
  metadataBytes: ByteLike,
  info: ChainInfo,
): Buffer {
  return nativeCall(() =>
    native.generateExtrinsicProof(
      toBuffer(callData, 'callData'),
      toBuffer(includedInExtrinsic, 'includedInExtrinsic'),
      toBuffer(includedInSignedData, 'includedInSignedData'),
      toBuffer(metadataBytes, 'metadataBytes'),
      nativeChainInfo(info),
    ),
  )
}

export const MLKEM_NONCE_LENGTH = native.mlkemNonceLength()
export const MLKEM_KDF_ID = Buffer.from(native.mlkemKdfId())
/** Rust-name aliases. */
export const MLKEM_NONCE_LEN = MLKEM_NONCE_LENGTH
export const KDF_ID = MLKEM_KDF_ID

export function mlkemSeal(
  publicKey: ByteLike,
  plaintext: ByteLike,
  includeKeyHash = false,
): Buffer {
  return nativeCall(() =>
    native.mlkemSeal(
      toBuffer(publicKey, 'publicKey'),
      toBuffer(plaintext, 'plaintext'),
      includeKeyHash,
    ),
  )
}

export function mlkemTwox128(data: ByteLike): Buffer {
  return nativeCall(() => native.mlkemTwox128(toBuffer(data, 'data')))
}


/** Substrate-compatible twox_128, implemented by bittensor-core Rust. */
export function twox_128(data: ByteLike): Buffer {
  return mlkemTwox128(data)
}

/** Substrate-compatible BLAKE2b-256, implemented by bittensor-core Rust. */
export function blake2_256(data: ByteLike): Buffer {
  return nativeCall(() => native.hashStorageParam('Blake2_256', toBuffer(data, 'data')))
}

export function bytesToHex(data: ByteLike, prefix = true): string {
  const hex = toBuffer(data, 'data').toString('hex')
  return prefix ? `0x${hex}` : hex
}

export function hexToBytes(value: string, name = 'hex'): Buffer {
  const raw = value.startsWith('0x') ? value.slice(2) : value
  if (raw.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(raw)) {
    throw new TypeError(`${name} must be an even-length hexadecimal string`)
  }
  return Buffer.from(raw, 'hex')
}

/**
 * Build the exact MEV-shield ciphertext envelope expected by pallet-shield.
 * The key hash, ML-KEM encapsulation, nonce generation, and XChaCha20-Poly1305
 * encryption all execute in bittensor-core Rust.
 */
export function sealMevShieldTransaction(
  publicKey: ByteLike,
  plaintext: ByteLike,
): Buffer {
  return mlkemSeal(publicKey, plaintext, true)
}
