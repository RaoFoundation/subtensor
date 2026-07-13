import type {
  IntegerLike,
  MetadataIr,
  ModuleError,
  RuntimeApiMap,
  ScaleValue,
} from './types'

export type RustSubstrateKeyType = 'sr25519' | 'ed25519'
export type RustKeypairKind = 'Ed25519' | 'Sr25519' | 'PublicOnly'

export interface RustKeypairMetadata {
  address?: string
  name?: string
  type?: RustSubstrateKeyType
  [key: string]: unknown
}

export interface RustKeypairSignOptions {
  /** Prefix the raw signature with its Substrate MultiSignature variant byte. */
  withType?: boolean
}

export interface RustKeypairPublic<
  Bytes extends Uint8Array,
  ByteInput,
  Message = string | ByteInput,
> {
  readonly cryptoType: number
  readonly kind: RustKeypairKind
  readonly publicKey: Bytes
  readonly addressRaw: Bytes
  readonly ss58Address: string
  readonly address: string
  readonly type: RustSubstrateKeyType
  readonly scheme: 'Ed25519' | 'Sr25519'
  readonly ss58Format: number
  readonly meta: RustKeypairMetadata
  readonly isLocked: boolean
  sign(message: Message, options?: RustKeypairSignOptions): Bytes
  verify(message: Message, signature: ByteInput, signerPublic?: string | ByteInput): boolean
  derive(suri: string, meta?: RustKeypairMetadata): RustKeypairPublic<Bytes, ByteInput, Message>
  setMeta(meta: RustKeypairMetadata): void
}

export interface RustStorageEntry<Bytes> {
  pallet: string
  name: string
  prefix: string
  modifier: string
  valueType: string
  valueTypeId: number
  paramTypes: string[]
  paramTypeIds: number[]
  paramHashers: string[]
  defaultBytes: Bytes
}

export interface RustStorageChange {
  key: string
  value?: string | null
}

export interface RustMapPair<K = ScaleValue, V = ScaleValue> {
  key: K
  value: V
}

export interface RustPayloadParts<Bytes> {
  includedInExtrinsic: Bytes
  includedInSignedData: Bytes
}

export interface RustTransactionParams<Bytes, Integer = IntegerLike> {
  era: ScaleValue
  nonce: Integer
  tip?: Integer
  tipAssetId?: Integer | null
  genesisHash: Bytes
  eraBlockHash: Bytes
  metadataHash?: Bytes | null
}

export interface RustSignedExtrinsicParams<Integer = IntegerLike> {
  era: ScaleValue
  nonce: Integer
  tip?: Integer
  tipAssetId?: Integer | null
  metadataHashEnabled?: boolean
}

export interface RustSignedExtrinsic<Bytes> {
  bytes: Bytes
  hash: Bytes
}

export interface RustMultisigAccount<Bytes> {
  accountId: Bytes
  sortedSignatories: Bytes[]
}

export interface RustEpochScheduleState<Integer = IntegerLike> {
  lastEpochBlock: Integer
  pendingEpochAt: Integer
  subnetEpochIndex: Integer
  tempo: number
  blocksSinceLastStep: Integer
  currentBlock: Integer
}

export type RustRuntimeApiMap = RuntimeApiMap
export type RustMetadataIr = MetadataIr

export interface RustRuntimePublic<
  Bytes extends Uint8Array,
  ByteInput,
  Integer = IntegerLike,
> {
  readonly specVersion: number
  readonly transactionVersion: number
  readonly ss58Format: number
  readonly isV15: boolean
  readonly extrinsicVersion: number
  decode<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteInput,
    strict?: boolean,
  ): T
  decodeBatch<T extends ScaleValue = ScaleValue>(
    typeStrings: string[],
    data: ByteInput[],
  ): T[]
  encode(typeString: string, value: ScaleValue): Bytes
  typeIdOf(name: string): number | null
  typeNameOf(id: number): string | null
  registryJson(): string
  composeCall(pallet: string, fn: string, params: ScaleValue): Bytes
  decodeCall<T extends ScaleValue = ScaleValue>(data: ByteInput): T
  storageEntry(pallet: string, storageFunction: string): RustStorageEntry<Bytes>
  storagePrefix(pallet: string, storageFunction: string): Bytes
  storageKey(pallet: string, storageFunction: string, params?: ScaleValue[]): Bytes
  storageKeyBatch(
    pallet: string,
    storageFunction: string,
    paramsList: ScaleValue[][],
  ): Bytes[]
  decodeStorageKeyParams<T extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    key: ByteInput,
    fixed?: number,
  ): T[]
  decodeMapPairs<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    rawKeys: ByteInput[],
    rawValues: ByteInput[],
    fixed?: number,
  ): Array<RustMapPair<K, V>>
  decodeMapChanges<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    changes: RustStorageChange[],
    fixed?: number,
  ): Array<RustMapPair<K, V>>
  constant<T extends ScaleValue = ScaleValue>(pallet: string, name: string): T | undefined
  moduleError(moduleIndex: number, errorIndex: number): ModuleError
  signedExtensionIdentifiers(): string[]
  encodeEra(era: ScaleValue): Bytes
  signaturePayloadParts(
    params: RustTransactionParams<ByteInput, Integer>,
  ): RustPayloadParts<Bytes>
  signaturePayload(
    callData: ByteInput,
    params: RustTransactionParams<ByteInput, Integer>,
  ): Bytes
  encodeSignedExtrinsic(
    callData: ByteInput,
    publicKey: ByteInput,
    signature: ByteInput,
    signatureVersion: number,
    params: RustSignedExtrinsicParams<Integer>,
  ): RustSignedExtrinsic<Bytes>
  decodeExtrinsic<T extends ScaleValue = ScaleValue>(
    data: ByteInput,
    strict?: boolean,
  ): T
  runtimeApiMap(): RustRuntimeApiMap
  runtimeApis(): RustRuntimeApiMap
  metadataIr(): RustMetadataIr
}
