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

export interface StorageEntry {
  pallet: string
  name: string
  prefix: string
  modifier: string
  valueType: string
  valueTypeId: number
  paramTypes: string[]
  paramTypeIds: number[]
  paramHashers: string[]
  defaultBytes: Buffer
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
