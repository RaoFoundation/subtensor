import * as crypto from './crypto'
import * as errors from './errors'
import * as keys from './keys'
import * as client from './client'
import { LedgerDevice, LedgerSigner } from './ledger'
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
  CoreError: errors.CoreError,
  error: Object.freeze({
    CoreError: errors.CoreError,
    BittensorCoreError: errors.BittensorCoreError,
    KeyfileError: errors.KeyfileError,
    WrongPasswordError: errors.WrongPasswordError,
    NotInRuntimeError: errors.NotInRuntimeError,
    CodecError: errors.CodecError,
    CryptoError: errors.CryptoError,
    DeviceError: errors.DeviceError,
  }),
  keys: Object.freeze({
    Keypair: keys.Keypair,
    CRYPTO_ED25519: keys.CRYPTO_ED25519,
    CRYPTO_SR25519: keys.CRYPTO_SR25519,
    DEFAULT_SS58_FORMAT: keys.DEFAULT_SS58_FORMAT,
    generateMnemonic: keys.generateMnemonic,
    generate_mnemonic: keys.generate_mnemonic,
    verify: keys.verify,
    verify_signature: keys.verify_signature,
    publicKeyFromSs58: keys.publicKeyFromSs58,
    ss58_decode: keys.ss58_decode,
    decode_ss58: keys.decode_ss58,
    ss58FromPublic: keys.ss58FromPublic,
    ss58_encode: keys.ss58_encode,
    encode_ss58: keys.encode_ss58,
    encryptFor: keys.encryptFor,
    encrypt_for: keys.encrypt_for,
  }),
  keyfiles: Object.freeze({
    serializeKeypair: keys.serializeKeypair,
    serializedKeypairToKeyfileData: keys.serializedKeypairToKeyfileData,
    serialized_keypair_to_keyfile_data: keys.serialized_keypair_to_keyfile_data,
    keypairToKeyfileData: keys.keypairToKeyfileData,
    keypair_to_keyfile_data: keys.keypair_to_keyfile_data,
    deserializeKeypair: keys.deserializeKeypair,
    deserializeKeypairFromKeyfileData: keys.deserializeKeypairFromKeyfileData,
    deserialize_keypair_from_keyfile_data: keys.deserialize_keypair_from_keyfile_data,
    deserializeKeypairFromKeyfile: keys.deserializeKeypairFromKeyfile,
    deserialize_keypair_from_keyfile: keys.deserialize_keypair_from_keyfile,
    readKeypairKeyfile: keys.readKeypairKeyfile,
    read_keypair_keyfile: keys.read_keypair_keyfile,
    writeKeypairPairKeyfile: keys.writeKeypairPairKeyfile,
    write_keypair_pair_keyfile: keys.write_keypair_pair_keyfile,
    encryptKeyfileData: keys.encryptKeyfileData,
    encrypt_keyfile_data: keys.encrypt_keyfile_data,
    keyfileDataIsEncrypted: keys.keyfileDataIsEncrypted,
    keyfile_data_is_encrypted: keys.keyfile_data_is_encrypted,
    keyfileDataIsEncryptedNacl: keys.keyfileDataIsEncryptedNacl,
    keyfile_data_is_encrypted_nacl: keys.keyfile_data_is_encrypted_nacl,
    keyfileDataIsEncryptedAnsible: keys.keyfileDataIsEncryptedAnsible,
    keyfile_data_is_encrypted_ansible: keys.keyfile_data_is_encrypted_ansible,
    keyfileDataIsEncryptedLegacy: keys.keyfileDataIsEncryptedLegacy,
    keyfile_data_is_encrypted_legacy: keys.keyfile_data_is_encrypted_legacy,
    keyfileDataEncryptionMethod: keys.keyfileDataEncryptionMethod,
    keyfile_data_encryption_method: keys.keyfile_data_encryption_method,
    dangerous: keys.dangerousKeyfiles,
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
      era_birth: runtime.era_birth,
      multisigAccountId: runtime.multisigAccountId,
      multisig_account_id: runtime.multisig_account_id,
      multisigSs58: runtime.multisigSs58,
    }),
    batch: Object.freeze({
      PARALLEL_THRESHOLD: runtime.PARALLEL_THRESHOLD,
    }),
  }),
  digest: Object.freeze({
    metadataDigest: crypto.metadataDigest,
    metadata_digest: crypto.metadata_digest,
    generateExtrinsicProof: crypto.generateExtrinsicProof,
    generate_extrinsic_proof: crypto.generate_extrinsic_proof,
  }),
  mlkem: Object.freeze({
    seal: crypto.mlkemSeal,
    encryptMlkem768: crypto.encryptMlkem768,
    encrypt_mlkem768: crypto.encrypt_mlkem768,
    twox128: crypto.mlkemTwox128,
    MLKEM_NONCE_LEN: crypto.MLKEM_NONCE_LEN,
    KDF_ID: crypto.KDF_ID,
    mlkemKdfId: crypto.mlkemKdfId,
    mlkem_kdf_id: crypto.mlkem_kdf_id,
  }),
  runtime: Object.freeze({
    Runtime: runtime.Runtime,
    TypeSpec: runtime.typeSpec,
    Primitive: runtime.primitiveFromName,
  }),
  client: Object.freeze({
    Client: client.Client,
    Subtensor: client.Subtensor,
    storage: client.storage,
    constants: client.constants,
    runtimeApi: client.runtimeApi,
    calls: client.calls,
  }),
  timelock: Object.freeze({
    encryptAndCompress: timelock.encryptAndCompress,
    decryptAndDecompress: timelock.decryptAndDecompress,
    generateCommitV2: timelock.generateCommitV2,
    get_encrypted_commit_v2: timelock.get_encrypted_commit_v2,
    encryptCommitment: timelock.encryptCommitment,
    get_encrypted_commitment: timelock.get_encrypted_commitment,
    encryptNBlocks: timelock.encryptNBlocks,
    encrypt: timelock.encrypt,
    encryptAtRound: timelock.encryptAtRound,
    encrypt_at_round: timelock.encrypt_at_round,
    getRoundInfo: timelock.getRoundInfo,
    get_latest_round: timelock.get_latest_round,
    getRevealRoundSignature: timelock.getRevealRoundSignature,
    get_signature_for_round: timelock.get_signature_for_round,
    decrypt: timelock.decrypt,
    decryptWithSignature: timelock.decryptWithSignature,
    decrypt_with_signature: timelock.decrypt_with_signature,
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
    ledger: Object.freeze({ LedgerDevice, LedgerSigner }),
  }),
})
