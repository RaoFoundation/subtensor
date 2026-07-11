import * as crypto from './crypto'
import * as keys from './keys'
import { LedgerDevice } from './ledger'
import native from './native'
import * as runtime from './runtime'
import * as timelock from './timelock'
import * as value from './value'
import { toBuffer } from './wire'
import type { ByteLike } from './types'

/**
 * Rust-module-shaped namespace. It mirrors the public `bittensor_core` crate
 * while retaining the idiomatic top-level TypeScript exports.
 */
export const rustCore = Object.freeze({
  keys: Object.freeze({
    Keypair: keys.Keypair,
    CRYPTO_ED25519: keys.CRYPTO_ED25519,
    CRYPTO_SR25519: keys.CRYPTO_SR25519,
    DEFAULT_SS58_FORMAT: keys.DEFAULT_SS58_FORMAT,
    generateMnemonic: keys.generateMnemonic,
    verify: keys.verify,
    publicKeyFromSs58: keys.publicKeyFromSs58,
    ss58FromPublic: keys.ss58FromPublic,
    encryptFor: keys.encryptFor,
  }),
  keyfiles: Object.freeze({
    serializeKeypair: keys.serializeKeypair,
    serializedKeypairToKeyfileData: keys.serializedKeypairToKeyfileData,
    deserializeKeypair: keys.deserializeKeypair,
    deserializeKeypairFromKeyfileData: keys.deserializeKeypairFromKeyfileData,
    encryptKeyfileData: keys.encryptKeyfileData,
    decryptKeyfileData: keys.decryptKeyfileData,
    keyfileDataIsEncrypted: keys.keyfileDataIsEncrypted,
    keyfileDataIsEncryptedNacl: keys.keyfileDataIsEncryptedNacl,
    keyfileDataIsEncryptedAnsible: keys.keyfileDataIsEncryptedAnsible,
    keyfileDataIsEncryptedLegacy: keys.keyfileDataIsEncryptedLegacy,
    keyfileDataEncryptionMethod: keys.keyfileDataEncryptionMethod,
    getPasswordFromEnvironment: keys.getPasswordFromEnvironment,
    savePasswordToEnvironment: keys.savePasswordToEnvironment,
  }),
  codec: Object.freeze({
    value: Object.freeze({
      Value: value.coreValue,
      descriptor: value.coreValueDescriptor,
      normalize: value.normalizeCoreValue,
      toCorpusJson: value.coreValueDescriptorToCorpusJson,
      u256Decimal(raw: ByteLike): string {
        return native.u256LeToDecimal(toBuffer(raw, 'raw'))
      },
    }),
    decode: Object.freeze({
      Cursor: runtime.ScaleCursor,
      compactU128: runtime.decodeCompactU128,
      compactLength: runtime.decodeCompactLength,
      convertTypeString: runtime.convertTypeString,
    }),
    encode: Object.freeze({
      compact: runtime.encodeCompact,
    }),
    storage: Object.freeze({
      hashParam: runtime.hashStorageParam,
      concatHashLen: runtime.concatHashLength,
      storagePrefix: runtime.storagePrefixFor,
    }),
    extrinsic: Object.freeze({
      eraBirth: runtime.eraBirth,
      multisigAccountId: runtime.multisigAccountId,
      multisigSs58: runtime.multisigSs58,
    }),
    batch: Object.freeze({
      PARALLEL_THRESHOLD: runtime.PARALLEL_THRESHOLD,
    }),
  }),
  digest: Object.freeze({
    metadataDigest: crypto.metadataDigest,
    generateExtrinsicProof: crypto.generateExtrinsicProof,
  }),
  mlkem: Object.freeze({
    seal: crypto.mlkemSeal,
    twox128: crypto.mlkemTwox128,
    MLKEM_NONCE_LEN: crypto.MLKEM_NONCE_LEN,
    KDF_ID: crypto.KDF_ID,
  }),
  runtime: Object.freeze({
    Runtime: runtime.Runtime,
    TypeSpec: runtime.typeSpec,
    Primitive: runtime.primitiveFromName,
  }),
  timelock: Object.freeze({
    encryptAndCompress: timelock.encryptAndCompress,
    decryptAndDecompress: timelock.decryptAndDecompress,
    generateCommitV2: timelock.generateCommitV2,
    encryptCommitment: timelock.encryptCommitment,
    encryptNBlocks: timelock.encryptNBlocks,
    encryptAtRound: timelock.encryptAtRound,
    getRoundInfo: timelock.getRoundInfo,
    getRevealRoundSignature: timelock.getRevealRoundSignature,
    decrypt: timelock.decrypt,
    decryptWithSignature: timelock.decryptWithSignature,
    constants: Object.freeze({
      MAX_TEMPO: timelock.MAX_TEMPO,
      MAX_TEMPO_U64: timelock.MAX_TEMPO_U64,
      DRAND_PUBLIC_KEY: timelock.DRAND_PUBLIC_KEY,
      GENESIS_TIME: timelock.GENESIS_TIME,
      DRAND_PERIOD: timelock.DRAND_PERIOD,
      QUICKNET_CHAIN_HASH: timelock.QUICKNET_CHAIN_HASH,
      DRAND_ENDPOINTS: timelock.DRAND_ENDPOINTS,
      SECURITY_BLOCK_OFFSET: timelock.SECURITY_BLOCK_OFFSET,
      COMMIT_INCLUSION_BLOCK_OFFSET: timelock.COMMIT_INCLUSION_BLOCK_OFFSET,
      maxSimulationBlocks: timelock.maxSimulationBlocks,
    }),
    epoch_schedule: Object.freeze({
      EpochScheduleError: timelock.EpochScheduleError,
      shouldRunEpoch: timelock.shouldRunEpoch,
      currentEpochPreRunCoinbase: timelock.currentEpochPreRunCoinbase,
      simulateRunCoinbase: timelock.simulateRunCoinbase,
      advanceBlocks: timelock.advanceBlocks,
      predictFirstRevealBlock: timelock.predictFirstRevealBlock,
      predictFirstRevealBlockResult: timelock.predictFirstRevealBlockResult,
    }),
  }),
  signers: Object.freeze({
    ledger: Object.freeze({ LedgerDevice }),
  }),
})
