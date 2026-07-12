export type ByteLike = Buffer | Uint8Array
export type IntegerLike = number | bigint

export type ScaleValue =
  | null
  | boolean
  | number
  | bigint
  | string
  | ByteLike
  | ScaleValue[]
  | { [key: string]: ScaleValue }
  | Map<ScaleValue, ScaleValue>

/** Exact, lossless representation of Rust's `bittensor_core::codec::Value`. */
export type CoreValueDescriptor =
  | { kind: 'null' }
  | { kind: 'bool'; value: boolean }
  | { kind: 'int'; value: string }
  | { kind: 'uint'; value: string }
  | { kind: 'u256'; littleEndianHex: `0x${string}` }
  | { kind: 'str'; value: string }
  | { kind: 'bytes'; hex: `0x${string}` }
  | { kind: 'list'; items: CoreValueDescriptor[] }
  | { kind: 'tuple'; items: CoreValueDescriptor[] }
  | { kind: 'dict'; entries: CoreValueEntry[] }

export interface CoreValueEntry {
  key: CoreValueDescriptor
  value: CoreValueDescriptor
}

export type PrimitiveName =
  | 'bool'
  | 'char'
  | 'str'
  | 'u8'
  | 'u16'
  | 'u32'
  | 'u64'
  | 'u128'
  | 'u256'
  | 'i8'
  | 'i16'
  | 'i32'
  | 'i64'
  | 'i128'
  | 'i256'

/** Exact public shape of Rust's `runtime::type_string::TypeSpec`. */
export type TypeSpec =
  | { kind: 'id'; id: number }
  | { kind: 'primitive'; name: PrimitiveName }
  | { kind: 'sequence'; inner: TypeSpec }
  | { kind: 'option'; inner: TypeSpec }
  | { kind: 'array'; inner: TypeSpec; length: number }
  | { kind: 'tuple'; items: TypeSpec[] }
  | { kind: 'compact'; inner: TypeSpec }
  | { kind: 'bytes' }
  | { kind: 'accountId' }
  | { kind: 'era' }
  | { kind: 'call' }
  | { kind: 'extrinsic' }

export interface PartialDecode<T = ScaleValue> {
  value: T
  offset: number
  remaining: number
}

export interface CompactDecode {
  value: bigint
  offset: number
  remaining: number
}

export interface StorageEntryLike {
  pallet?: string
  name: string
  prefix: string
  modifier?: string
  valueType?: string
  value_type?: string
  valueTypeId?: number
  value_type_id?: number
  paramTypes?: string[]
  param_types?: string[]
  paramTypeIds?: number[]
  param_type_ids?: number[]
  paramHashers?: string[]
  param_hashers?: string[]
  defaultBytes?: ByteLike
  default_bytes?: ByteLike
}

export interface StorageEntry extends StorageEntryLike {
  pallet: string
  modifier: string
  valueType: string
  value_type: string
  valueTypeId: number
  value_type_id: number
  paramTypes: string[]
  param_types: string[]
  paramTypeIds: number[]
  param_type_ids: number[]
  paramHashers: string[]
  param_hashers: string[]
  defaultBytes: Buffer
  default_bytes: Buffer
}

export interface StorageChange {
  key: string
  value?: string | null
}

export interface MapPair<K = ScaleValue, V = ScaleValue> {
  key: K
  value: V
}

export interface ModuleError {
  name: string
  docs: string[]
}

export interface RuntimeConstantInfo {
  name: string
  typeId: number
  type: string
  valueHex: string
  docs: string[]
}

export interface RuntimeStorageInfo {
  name: string
  prefix: string
  modifier: string
  hashers: string[]
  keyTypeIds: number[]
  keyTypes: string[]
  valueTypeId: number
  valueType: string
  defaultHex: string
}

export interface PalletInfo {
  name: string
  index: number
  callsType: number | null
  eventsType: number | null
  errorsType: number | null
  constants: RuntimeConstantInfo[]
  storage: RuntimeStorageInfo[]
}

export interface SignedExtensionInfo {
  identifier: string
  typeId: number
  type: string
  additionalSignedTypeId: number
  additionalSignedType: string
}

export interface ExtrinsicInfo {
  version: number
  addressType: number | null
  callType: number | null
  signatureType: number | null
  signedExtensions: SignedExtensionInfo[]
}

export interface RuntimeApiParamInfo {
  name: string
  typeId: number
  type: string
}

export interface RuntimeApiMethodInfo {
  name: string
  inputs: RuntimeApiParamInfo[]
  output: number
  outputType: string
  docs: string[]
}

export interface RuntimeApiInfo {
  name: string
  methods: RuntimeApiMethodInfo[]
}

export type RuntimeApiMap = Record<
  string,
  Record<
    string,
    {
      name: string
      inputs: Array<[string, string]>
      inputDetails?: RuntimeApiParamInfo[]
      output: string
      outputTypeId: number
      docs: string[]
    }
  >
>

export interface RuntimeSnapshot {
  specVersion: number
  transactionVersion: number
  ss58Format: number
  isV15: boolean
  outerEventType: number | null
  pallets: PalletInfo[]
  extrinsic: ExtrinsicInfo
  runtimeApis: RuntimeApiMap
  runtimeApiInfos: RuntimeApiInfo[]
}

export interface MetadataIrCall {
  name: string
  index: number
  args: string[]
  argTypes: string[]
  argTypeIds: number[]
  docs: string
}

export interface MetadataIrError {
  index: number
  name: string
  docs: string
}

export interface MetadataIrPallet {
  name: string
  index: number
  calls: MetadataIrCall[]
  errors: MetadataIrError[]
  storage: string[]
  constants: string[]
}

export interface MetadataIr {
  specVersion: number
  pallets: MetadataIrPallet[]
  runtimeApis: Array<{ name: string; methods: string[] }>
}

export interface TransactionParams {
  era: ScaleValue
  nonce: IntegerLike
  tip?: IntegerLike
  tipAssetId?: IntegerLike | null
  genesisHash: ByteLike
  eraBlockHash: ByteLike
  metadataHash?: ByteLike | null
}

export interface SignedExtrinsicParams {
  era: ScaleValue
  nonce: IntegerLike
  tip?: IntegerLike
  tipAssetId?: IntegerLike | null
  metadataHashEnabled?: boolean
}

export interface PayloadParts {
  includedInExtrinsic: Buffer
  includedInSignedData: Buffer
}

export interface SignedExtrinsic {
  bytes: Buffer
  hash: Buffer
}

export interface MultisigAccount {
  accountId: Buffer
  sortedSignatories: Buffer[]
}

export interface ChainInfo {
  specVersion: number
  specName: string
  base58Prefix: number
  decimals: number
  tokenSymbol: string
}

export interface CiphertextRound {
  ciphertext: Buffer
  revealRound: bigint
}

export interface DrandResponse {
  round: bigint
  signature: string
}

export interface EpochScheduleState {
  lastEpochBlock: IntegerLike
  pendingEpochAt: IntegerLike
  subnetEpochIndex: IntegerLike
  tempo: number
  blocksSinceLastStep: IntegerLike
  currentBlock: IntegerLike
}

export type EpochScheduleErrorName = 'BoundExceeded' | 'TempoIsZero'

export interface EpochScheduleResult {
  ok: boolean
  block: bigint | null
  error: EpochScheduleErrorName | null
}

export interface WeightsTlockPayload {
  hotkey: ByteLike
  uids: number[]
  values: number[]
  versionKey: IntegerLike
}

export interface TimelockUserData {
  encryptedData: ByteLike
  revealRound: IntegerLike
}

export interface LedgerVersion {
  major: number
  minor: number
  patch: number
}

export interface LedgerAddress {
  publicKey: Buffer
  ss58Address: string
}
