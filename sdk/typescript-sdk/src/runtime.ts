import native, {
  type NativeExtrinsicParams,
  type NativeRuntimeHandle,
  type NativeStorageChange,
  type NativeTxParams,
} from './native'
import { nativeCall } from './errors'
import { fromWire, toBigInt, toBuffer, toWire } from './wire'
import type {
  ByteLike,
  CompactDecode,
  IntegerLike,
  MapPair,
  ModuleError,
  MultisigAccount,
  PartialDecode,
  PayloadParts,
  ScaleValue,
  SignedExtrinsic,
  SignedExtrinsicParams,
  StorageChange,
  StorageEntry,
  TransactionParams,
} from './types'

function nativeTxParams(params: TransactionParams): NativeTxParams {
  return {
    era: toWire(params.era),
    nonce: toBigInt(params.nonce, 'nonce'),
    tip: toBigInt(params.tip ?? 0, 'tip'),
    tipAssetId:
      params.tipAssetId == null ? undefined : toBigInt(params.tipAssetId, 'tipAssetId'),
    genesisHash: toBuffer(params.genesisHash, 'genesisHash'),
    eraBlockHash: toBuffer(params.eraBlockHash, 'eraBlockHash'),
    metadataHash:
      params.metadataHash == null ? undefined : toBuffer(params.metadataHash, 'metadataHash'),
  }
}

function nativeExtrinsicParams(params: SignedExtrinsicParams): NativeExtrinsicParams {
  return {
    era: toWire(params.era),
    nonce: toBigInt(params.nonce, 'nonce'),
    tip: toBigInt(params.tip ?? 0, 'tip'),
    tipAssetId:
      params.tipAssetId == null ? undefined : toBigInt(params.tipAssetId, 'tipAssetId'),
    metadataHashEnabled: params.metadataHashEnabled ?? false,
  }
}

function storageEntry(value: import('./native').NativeStorageEntry): StorageEntry {
  return {
    ...value,
    defaultBytes: Buffer.from(value.defaultBytes),
  }
}

export class Runtime {
  private readonly handle: NativeRuntimeHandle

  constructor(
    metadataBytes: ByteLike,
    specVersion: number,
    transactionVersion: number,
    ss58Format = 42,
  ) {
    this.handle = nativeCall(
      () =>
        native.NativeRuntime.fromMetadata(
          toBuffer(metadataBytes, 'metadataBytes'),
          specVersion,
          transactionVersion,
          ss58Format,
        ),
    )
  }

  get specVersion(): number {
    return this.handle.specVersion
  }

  get transactionVersion(): number {
    return this.handle.transactionVersion
  }

  get ss58Format(): number {
    return this.handle.ss58Format
  }

  get isV15(): boolean {
    return this.handle.isV15
  }

  get extrinsicVersion(): number {
    return this.handle.extrinsicVersion
  }

  get outerEventType(): number | null {
    return this.handle.outerEventType ?? null
  }

  get metadataBytes(): Buffer {
    return Buffer.from(this.handle.metadataBytes)
  }

  decode<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteLike,
    strict = true,
  ): T {
    return nativeCall(
      () => fromWire(this.handle.decode(typeString, toBuffer(data, 'data'), strict)) as T,
    )
  }

  decodePartial<T extends ScaleValue = ScaleValue>(
    typeString: string,
    data: ByteLike,
    offset = 0,
    strict = true,
  ): PartialDecode<T> {
    return nativeCall(() => {
      const decoded = this.handle.decodePartial(
        typeString,
        toBuffer(data, 'data'),
        offset,
        strict,
      )
      return { ...decoded, value: fromWire(decoded.value) as T }
    })
  }

  decodeTypeId<T extends ScaleValue = ScaleValue>(
    typeId: number,
    data: ByteLike,
    strict = true,
  ): T {
    return nativeCall(
      () => fromWire(this.handle.decodeTypeId(typeId, toBuffer(data, 'data'), strict)) as T,
    )
  }

  decodeTypeIdPartial<T extends ScaleValue = ScaleValue>(
    typeId: number,
    data: ByteLike,
    offset = 0,
    strict = true,
  ): PartialDecode<T> {
    return nativeCall(() => {
      const decoded = this.handle.decodeTypeIdPartial(
        typeId,
        toBuffer(data, 'data'),
        offset,
        strict,
      )
      return { ...decoded, value: fromWire(decoded.value) as T }
    })
  }

  decodeBatch<T extends ScaleValue = ScaleValue>(
    typeStrings: string[],
    data: ByteLike[],
  ): T[] {
    return nativeCall(() =>
      this.handle
        .decodeBatch(typeStrings, data.map((value) => toBuffer(value, 'data')))
        .map((value) => fromWire(value) as T),
    )
  }

  encode(typeString: string, value: ScaleValue): Buffer {
    return nativeCall(() => this.handle.encode(typeString, toWire(value)))
  }

  encodeTypeId(typeId: number, value: ScaleValue): Buffer {
    return nativeCall(() => this.handle.encodeTypeId(typeId, toWire(value)))
  }

  typeIdOf(name: string): number | null {
    return this.handle.typeIdOf(name) ?? null
  }

  typeNameOf(id: number): string | null {
    return this.handle.typeNameOf(id) ?? null
  }

  typeSpec(typeString: string): unknown {
    return nativeCall(() => this.handle.typeSpec(typeString))
  }

  resolveType(id: number): unknown {
    return nativeCall(() => this.handle.resolveType(id))
  }

  registryJson(): string {
    return nativeCall(() => this.handle.registryJson())
  }

  registry(): unknown {
    return nativeCall(() => this.handle.registry())
  }

  pallet(name: string): unknown | null {
    return this.handle.pallet(name) ?? null
  }

  palletAt(index: number): unknown | null {
    return this.handle.palletAt(index) ?? null
  }

  pallets(): unknown[] {
    return this.handle.pallets()
  }

  extrinsicInfo(): unknown {
    return this.handle.extrinsicInfo()
  }

  runtimeApis(): unknown {
    return this.handle.runtimeApis()
  }

  runtimeSnapshot(): unknown {
    return this.handle.runtimeSnapshot()
  }

  composeCall(pallet: string, fn: string, params: ScaleValue): Buffer {
    return nativeCall(() => this.handle.composeCall(pallet, fn, toWire(params)))
  }

  decodeCall<T extends ScaleValue = ScaleValue>(data: ByteLike): T {
    return nativeCall(
      () => fromWire(this.handle.decodeCall(toBuffer(data, 'data'))) as T,
    )
  }

  storageEntry(pallet: string, storageFunction: string): StorageEntry {
    return nativeCall(() => storageEntry(this.handle.storageEntry(pallet, storageFunction)))
  }

  storagePrefix(pallet: string, storageFunction: string): Buffer {
    return nativeCall(() => this.handle.storagePrefix(pallet, storageFunction))
  }

  storageKey(
    pallet: string,
    storageFunction: string,
    params: ScaleValue[] = [],
  ): Buffer {
    return nativeCall(() =>
      this.handle.storageKey(pallet, storageFunction, toWire(params)),
    )
  }

  storageKeyBatch(
    pallet: string,
    storageFunction: string,
    paramsList: ScaleValue[][],
  ): Buffer[] {
    return nativeCall(() =>
      this.handle.storageKeyBatch(pallet, storageFunction, toWire(paramsList)),
    )
  }

  decodeStorageKeyParams<T extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    key: ByteLike,
    fixed = 0,
  ): T[] {
    return nativeCall(
      () =>
        fromWire(
          this.handle.decodeStorageKeyParams(
            pallet,
            storageFunction,
            toBuffer(key, 'key'),
            fixed,
          ),
        ) as T[],
    )
  }

  decodeMapPairs<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    rawKeys: ByteLike[],
    rawValues: ByteLike[],
    fixed = 0,
  ): MapPair<K, V>[] {
    return nativeCall(() =>
      this.handle
        .decodeMapPairs(
          pallet,
          storageFunction,
          rawKeys.map((value) => toBuffer(value, 'rawKey')),
          rawValues.map((value) => toBuffer(value, 'rawValue')),
          fixed,
        )
        .map((pair) => ({ key: fromWire(pair.key) as K, value: fromWire(pair.value) as V })),
    )
  }

  decodeMapChanges<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    changes: StorageChange[],
    fixed = 0,
  ): MapPair<K, V>[] {
    const nativeChanges: NativeStorageChange[] = changes.map((change) => ({
      key: change.key,
      value: change.value ?? undefined,
    }))
    return nativeCall(() =>
      this.handle
        .decodeMapChanges(pallet, storageFunction, nativeChanges, fixed)
        .map((pair) => ({ key: fromWire(pair.key) as K, value: fromWire(pair.value) as V })),
    )
  }

  constant<T extends ScaleValue = ScaleValue>(pallet: string, name: string): T | undefined {
    return nativeCall(() => {
      const value = this.handle.constant(pallet, name)
      return value.found ? (fromWire(value.value) as T) : undefined
    })
  }

  constantInfo(pallet: string, name: string): unknown | null {
    return this.handle.constantInfo(pallet, name) ?? null
  }

  moduleError(moduleIndex: number, errorIndex: number): ModuleError {
    return nativeCall(() => this.handle.moduleError(moduleIndex, errorIndex))
  }

  signedExtensionIdentifiers(): string[] {
    return this.handle.signedExtensionIdentifiers()
  }

  encodeEra(era: ScaleValue): Buffer {
    return nativeCall(() => this.handle.encodeEra(toWire(era)))
  }

  signaturePayloadParts(params: TransactionParams): PayloadParts {
    return nativeCall(() => this.handle.signaturePayloadParts(nativeTxParams(params)))
  }

  signaturePayload(callData: ByteLike, params: TransactionParams): Buffer {
    return nativeCall(() =>
      this.handle.signaturePayload(toBuffer(callData, 'callData'), nativeTxParams(params)),
    )
  }

  encodeSignedExtrinsic(
    callData: ByteLike,
    publicKey: ByteLike,
    signature: ByteLike,
    signatureVersion: number,
    params: SignedExtrinsicParams,
  ): SignedExtrinsic {
    return nativeCall(() =>
      this.handle.encodeSignedExtrinsic(
        toBuffer(callData, 'callData'),
        toBuffer(publicKey, 'publicKey'),
        toBuffer(signature, 'signature'),
        signatureVersion,
        nativeExtrinsicParams(params),
      ),
    )
  }

  decodeExtrinsic<T extends ScaleValue = ScaleValue>(
    data: ByteLike,
    strict = true,
  ): T {
    return nativeCall(
      () => fromWire(this.handle.decodeExtrinsic(toBuffer(data, 'data'), strict)) as T,
    )
  }

  runtimeApiMap(): unknown {
    return this.handle.runtimeApiMap()
  }

  metadataIr(): unknown {
    return nativeCall(() => this.handle.metadataIr())
  }
}

export function eraBirth(period: IntegerLike, current: IntegerLike): bigint {
  return nativeCall(() => native.eraBirth(toBigInt(period, 'period'), toBigInt(current, 'current')))
}

export function multisigAccountId(
  signatories: ByteLike[],
  threshold: number,
): MultisigAccount {
  return nativeCall(() =>
    native.multisigAccountId(
      signatories.map((value) => toBuffer(value, 'signatory')),
      threshold,
    ),
  )
}

export function multisigSs58(
  accountId: ByteLike,
  ss58Format = 42,
): string {
  return nativeCall(() => native.multisigSs58(toBuffer(accountId, 'accountId'), ss58Format))
}

export function encodeCompact(value: IntegerLike): Buffer {
  return nativeCall(() => native.encodeCompact(toBigInt(value)))
}

export function decodeCompactU128(data: ByteLike, strict = true): CompactDecode {
  return nativeCall(() => native.decodeCompactU128(toBuffer(data, 'data'), strict))
}

export function decodeCompactLength(data: ByteLike, strict = true): CompactDecode {
  return nativeCall(() => native.decodeCompactLength(toBuffer(data, 'data'), strict))
}

export function hashStorageParam(hasher: string, data: ByteLike): Buffer {
  return nativeCall(() => native.hashStorageParam(hasher, toBuffer(data, 'data')))
}

export function concatHashLength(hasher: string): number {
  return nativeCall(() => native.concatHashLength(hasher))
}

export const PARALLEL_DECODE_THRESHOLD = native.parallelDecodeThreshold()
