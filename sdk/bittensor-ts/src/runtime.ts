import native, {
  type NativeCursorHandle,
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
  CoreValueDescriptor,
  ExtrinsicInfo,
  IntegerLike,
  MapPair,
  MetadataIr,
  ModuleError,
  MultisigAccount,
  PalletInfo,
  PartialDecode,
  PayloadParts,
  PrimitiveName,
  RuntimeApiInfo,
  RuntimeApiMap,
  RuntimeConstantInfo,
  RuntimeSnapshot,
  ScaleValue,
  SignedExtrinsic,
  SignedExtrinsicParams,
  StorageChange,
  StorageEntry,
  StorageEntryLike,
  TransactionParams,
  TypeSpec,
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

/** cyscale-compatible metadata type-name normalization from Rust. */
export function convertTypeString(name: string): string {
  return nativeCall(() => native.convertTypeString(name))
}

export function primitiveFromName(name: string): PrimitiveName | null {
  return (
    (nativeCall(() => native.primitiveFromName(name)) as
      | PrimitiveName
      | null
      | undefined) ?? null
  )
}

export function normalizeTypeSpec(spec: TypeSpec): TypeSpec {
  return nativeCall(() => native.normalizeTypeSpec(spec) as TypeSpec)
}

export const typeSpec = Object.freeze({
  id(id: number): TypeSpec {
    return normalizeTypeSpec({ kind: 'id', id })
  },
  primitive(name: PrimitiveName): TypeSpec {
    return normalizeTypeSpec({ kind: 'primitive', name })
  },
  sequence(inner: TypeSpec): TypeSpec {
    return normalizeTypeSpec({ kind: 'sequence', inner })
  },
  option(inner: TypeSpec): TypeSpec {
    return normalizeTypeSpec({ kind: 'option', inner })
  },
  array(inner: TypeSpec, length: number): TypeSpec {
    return normalizeTypeSpec({ kind: 'array', inner, length })
  },
  tuple(items: TypeSpec[]): TypeSpec {
    return normalizeTypeSpec({ kind: 'tuple', items })
  },
  compact(inner: TypeSpec): TypeSpec {
    return normalizeTypeSpec({ kind: 'compact', inner })
  },
  bytes(): TypeSpec {
    return normalizeTypeSpec({ kind: 'bytes' })
  },
  accountId(): TypeSpec {
    return normalizeTypeSpec({ kind: 'accountId' })
  },
  era(): TypeSpec {
    return normalizeTypeSpec({ kind: 'era' })
  },
  call(): TypeSpec {
    return normalizeTypeSpec({ kind: 'call' })
  },
  extrinsic(): TypeSpec {
    return normalizeTypeSpec({ kind: 'extrinsic' })
  },
})

/** Owned Node-API wrapper over Rust's public SCALE `Cursor`. */
export class ScaleCursor {
  private readonly handle: NativeCursorHandle

  constructor(data: ByteLike, strict = false, offset = 0) {
    this.handle = nativeCall(() =>
      native.NativeCursor.fromBytes(toBuffer(data, 'data'), strict, offset),
    )
  }

  get data(): Buffer {
    return Buffer.from(this.handle.data)
  }

  get offset(): number {
    return this.handle.offset
  }

  get remaining(): number {
    return this.handle.remaining
  }

  get strict(): boolean {
    return this.handle.strict
  }

  set strict(value: boolean) {
    nativeCall(() => this.handle.setStrict(value))
  }

  seek(offset: number): void {
    nativeCall(() => this.handle.seek(offset))
  }

  reset(data: ByteLike, strict = false, offset = 0): void {
    nativeCall(() => this.handle.reset(toBuffer(data, 'data'), strict, offset))
  }

  take(length: number): Buffer {
    return nativeCall(() => Buffer.from(this.handle.take(length)))
  }

  byte(): number {
    return nativeCall(() => this.handle.byte())
  }

  decodeCompactU128(): bigint {
    return nativeCall(() => this.handle.decodeCompactU128())
  }

  decodeCompactLength(): bigint {
    return nativeCall(() => this.handle.decodeCompactLength())
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

  typeSpec(typeString: string): TypeSpec {
    return nativeCall(() => this.handle.typeSpec(typeString) as TypeSpec)
  }

  decodeSpec<T extends ScaleValue = ScaleValue>(
    spec: TypeSpec,
    data: ByteLike,
    strict = true,
  ): T {
    return nativeCall(
      () =>
        fromWire(
          this.handle.decodeSpec(spec, toBuffer(data, 'data'), strict),
        ) as T,
    )
  }

  decodeSpecDescriptor(
    spec: TypeSpec,
    data: ByteLike,
    strict = true,
  ): CoreValueDescriptor {
    return nativeCall(
      () =>
        this.handle.decodeSpecDescriptor(
          spec,
          toBuffer(data, 'data'),
          strict,
        ) as CoreValueDescriptor,
    )
  }

  decodeValue<T extends ScaleValue = ScaleValue>(
    spec: TypeSpec,
    data: ByteLike,
    offset = 0,
    strict = false,
  ): PartialDecode<T> {
    return nativeCall(() => {
      const decoded = this.handle.decodeValue(
        spec,
        toBuffer(data, 'data'),
        offset,
        strict,
      )
      return { ...decoded, value: fromWire(decoded.value) as T }
    })
  }

  decodeValueDescriptor(
    spec: TypeSpec,
    data: ByteLike,
    offset = 0,
    strict = false,
  ): PartialDecode<CoreValueDescriptor> {
    return nativeCall(
      () =>
        this.handle.decodeValueDescriptor(
          spec,
          toBuffer(data, 'data'),
          offset,
          strict,
        ) as PartialDecode<CoreValueDescriptor>,
    )
  }

  decodeTypeIdDescriptor(
    typeId: number,
    data: ByteLike,
    strict = true,
  ): CoreValueDescriptor {
    return nativeCall(
      () =>
        this.handle.decodeTypeIdDescriptor(
          typeId,
          toBuffer(data, 'data'),
          strict,
        ) as CoreValueDescriptor,
    )
  }

  decodeTypeIdDescriptorPartial(
    typeId: number,
    data: ByteLike,
    offset = 0,
    strict = false,
  ): PartialDecode<CoreValueDescriptor> {
    return nativeCall(
      () =>
        this.handle.decodeTypeIdDescriptorPartial(
          typeId,
          toBuffer(data, 'data'),
          offset,
          strict,
        ) as PartialDecode<CoreValueDescriptor>,
    )
  }

  encodeSpec(spec: TypeSpec, value: ScaleValue): Buffer {
    return nativeCall(() => this.handle.encodeSpec(spec, toWire(value)))
  }

  encodeSpecDescriptor(spec: TypeSpec, value: CoreValueDescriptor): Buffer {
    return nativeCall(() => this.handle.encodeSpecDescriptor(spec, value))
  }

  encodeValue(
    spec: TypeSpec,
    value: ScaleValue,
    prefix?: ByteLike | null,
  ): Buffer {
    return nativeCall(() =>
      this.handle.encodeValue(
        spec,
        toWire(value),
        prefix == null ? undefined : toBuffer(prefix, 'prefix'),
      ),
    )
  }

  encodeValueDescriptor(
    spec: TypeSpec,
    value: CoreValueDescriptor,
    prefix?: ByteLike | null,
  ): Buffer {
    return nativeCall(() =>
      this.handle.encodeValueDescriptor(
        spec,
        value,
        prefix == null ? undefined : toBuffer(prefix, 'prefix'),
      ),
    )
  }

  encodeId(
    typeId: number,
    value: ScaleValue,
    prefix?: ByteLike | null,
  ): Buffer {
    return nativeCall(() =>
      this.handle.encodeId(
        typeId,
        toWire(value),
        prefix == null ? undefined : toBuffer(prefix, 'prefix'),
      ),
    )
  }

  encodeIdDescriptor(
    typeId: number,
    value: CoreValueDescriptor,
    prefix?: ByteLike | null,
  ): Buffer {
    return nativeCall(() =>
      this.handle.encodeIdDescriptor(
        typeId,
        value,
        prefix == null ? undefined : toBuffer(prefix, 'prefix'),
      ),
    )
  }

  coerceAccountId(value: ScaleValue): Buffer {
    return nativeCall(() => Buffer.from(this.handle.coerceAccountId(toWire(value))))
  }

  coerceAccountIdDescriptor(value: CoreValueDescriptor): Buffer {
    return nativeCall(() =>
      Buffer.from(this.handle.coerceAccountIdDescriptor(value)),
    )
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

  pallet(name: string): PalletInfo | null {
    return (this.handle.pallet(name) as PalletInfo | null | undefined) ?? null
  }

  palletAt(index: number): PalletInfo | null {
    return (this.handle.palletAt(index) as PalletInfo | null | undefined) ?? null
  }

  pallets(): PalletInfo[] {
    return this.handle.pallets() as PalletInfo[]
  }

  extrinsicInfo(): ExtrinsicInfo {
    return this.handle.extrinsicInfo() as ExtrinsicInfo
  }

  runtimeApis(): RuntimeApiMap {
    return this.handle.runtimeApis() as RuntimeApiMap
  }

  runtimeApiInfos(): RuntimeApiInfo[] {
    return this.handle.runtimeApiInfos() as RuntimeApiInfo[]
  }

  runtimeSnapshot(): RuntimeSnapshot {
    return this.handle.runtimeSnapshot() as RuntimeSnapshot
  }

  composeCall(pallet: string, fn: string, params: ScaleValue): Buffer {
    return nativeCall(() => this.handle.composeCall(pallet, fn, toWire(params)))
  }

  decodeCall<T extends ScaleValue = ScaleValue>(data: ByteLike): T {
    return nativeCall(
      () => fromWire(this.handle.decodeCall(toBuffer(data, 'data'))) as T,
    )
  }

  decodeCallValue<T extends ScaleValue = ScaleValue>(
    data: ByteLike,
    offset = 0,
    strict = false,
  ): PartialDecode<T> {
    return nativeCall(() => {
      const decoded = this.handle.decodeCallValue(
        toBuffer(data, 'data'),
        offset,
        strict,
      )
      return { ...decoded, value: fromWire(decoded.value) as T }
    })
  }

  decodeCallValueDescriptor(
    data: ByteLike,
    offset = 0,
    strict = false,
  ): PartialDecode<CoreValueDescriptor> {
    return nativeCall(
      () =>
        this.handle.decodeCallValueDescriptor(
          toBuffer(data, 'data'),
          offset,
          strict,
        ) as PartialDecode<CoreValueDescriptor>,
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

  constantInfo(pallet: string, name: string): RuntimeConstantInfo | null {
    return (
      (this.handle.constantInfo(pallet, name) as
        | RuntimeConstantInfo
        | null
        | undefined) ?? null
    )
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

  runtimeApiMap(): RuntimeApiMap {
    return this.handle.runtimeApiMap() as RuntimeApiMap
  }

  metadataIr(): MetadataIr {
    return nativeCall(() => this.handle.metadataIr() as MetadataIr)
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

export function storagePrefixFor(entry: StorageEntryLike): Buffer
export function storagePrefixFor(prefix: string, name: string): Buffer
export function storagePrefixFor(
  entryOrPrefix: StorageEntryLike | string,
  name?: string,
): Buffer {
  const prefix =
    typeof entryOrPrefix === 'string' ? entryOrPrefix : entryOrPrefix.prefix
  const storageName =
    typeof entryOrPrefix === 'string' ? name : entryOrPrefix.name
  if (storageName == null) {
    throw new TypeError('storage name is required')
  }
  return nativeCall(() => native.storagePrefixFor(prefix, storageName))
}

export function concatHashLength(hasher: string): number {
  return nativeCall(() => native.concatHashLength(hasher))
}

export const PARALLEL_DECODE_THRESHOLD = native.parallelDecodeThreshold()
/** Rust-name alias. */
export const PARALLEL_THRESHOLD = PARALLEL_DECODE_THRESHOLD
