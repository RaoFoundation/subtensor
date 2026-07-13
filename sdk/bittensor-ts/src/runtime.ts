import native, {
  type NativeCursorHandle,
  type NativeExtrinsicParams,
  type NativeRuntimeHandle,
  type NativeSignerPayload,
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
import type { RustRuntimePublic } from './rust-bindings'

export type PayloadPartsTuple = [Buffer, Buffer]
export type SignedExtrinsicTuple = [Buffer, Buffer]

export interface RuntimeSignerPayload extends NativeSignerPayload {
  assetId?: string | null
  metadataHash?: string | null
  mode?: number | null
}

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
  const defaultBytes = Buffer.from(value.defaultBytes)
  return {
    ...value,
    value_type: value.valueType,
    value_type_id: value.valueTypeId,
    param_types: value.paramTypes,
    param_type_ids: value.paramTypeIds,
    param_hashers: value.paramHashers,
    defaultBytes,
    default_bytes: defaultBytes,
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

export class Runtime implements RustRuntimePublic<Buffer, ByteLike, IntegerLike> {
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

  get spec_version(): number {
    return this.specVersion
  }

  get transactionVersion(): number {
    return this.handle.transactionVersion
  }

  get transaction_version(): number {
    return this.transactionVersion
  }

  get ss58Format(): number {
    return this.handle.ss58Format
  }

  get ss58_format(): number {
    return this.ss58Format
  }

  get isV15(): boolean {
    return this.handle.isV15
  }

  get is_v15(): boolean {
    return this.isV15
  }

  get extrinsicVersion(): number {
    return this.handle.extrinsicVersion
  }

  get extrinsic_version(): number {
    return this.extrinsicVersion
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

  batch_decode<T extends ScaleValue = ScaleValue>(
    typeStrings: string[],
    data: ByteLike[],
  ): T[] {
    return this.decodeBatch(typeStrings, data)
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

  type_id_of(name: string): number | null {
    return this.typeIdOf(name)
  }

  typeNameOf(id: number): string | null {
    return this.handle.typeNameOf(id) ?? null
  }

  type_name_of(id: number): string | null {
    return this.typeNameOf(id)
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

  registry_json(): string {
    return this.registryJson()
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

  encodeRuntimeApiInput(api: string, method: string, params: ScaleValue[]): Buffer {
    return nativeCall(() => this.handle.encodeRuntimeApiInput(api, method, toWire(params)))
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

  compose_call(pallet: string, fn: string, params: ScaleValue): Buffer {
    return this.composeCall(pallet, fn, params)
  }

  decodeCall<T extends ScaleValue = ScaleValue>(data: ByteLike): T {
    return nativeCall(
      () => fromWire(this.handle.decodeCall(toBuffer(data, 'data'))) as T,
    )
  }

  decode_call<T extends ScaleValue = ScaleValue>(data: ByteLike): T {
    return this.decodeCall(data)
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

  storage_entry(pallet: string, storageFunction: string): StorageEntry {
    return this.storageEntry(pallet, storageFunction)
  }

  storagePrefix(pallet: string, storageFunction: string): Buffer {
    return nativeCall(() => this.handle.storagePrefix(pallet, storageFunction))
  }

  storage_prefix(pallet: string, storageFunction: string): Buffer {
    return this.storagePrefix(pallet, storageFunction)
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

  storage_key(
    pallet: string,
    storageFunction: string,
    params: ScaleValue[] = [],
  ): Buffer {
    return this.storageKey(pallet, storageFunction, params)
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

  storage_key_batch(
    pallet: string,
    storageFunction: string,
    paramsList: ScaleValue[][],
  ): Buffer[] {
    return this.storageKeyBatch(pallet, storageFunction, paramsList)
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

  decode_storage_key_params<T extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    key: ByteLike,
    fixed = 0,
  ): T[] {
    return this.decodeStorageKeyParams(pallet, storageFunction, key, fixed)
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

  decode_map_pairs<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    rawKeys: ByteLike[],
    rawValues: ByteLike[],
    fixed = 0,
  ): Array<[K, V]> {
    return this.decodeMapPairs<K, V>(
      pallet,
      storageFunction,
      rawKeys,
      rawValues,
      fixed,
    ).map((pair) => [pair.key, pair.value])
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

  decode_map_changes<K extends ScaleValue = ScaleValue, V extends ScaleValue = ScaleValue>(
    pallet: string,
    storageFunction: string,
    changes: StorageChange[],
    fixed = 0,
  ): Array<[K, V]> {
    return this.decodeMapChanges<K, V>(
      pallet,
      storageFunction,
      changes,
      fixed,
    ).map((pair) => [pair.key, pair.value])
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

  module_error(moduleIndex: number, errorIndex: number): [string, string[]] {
    const error = this.moduleError(moduleIndex, errorIndex)
    return [error.name, error.docs]
  }

  signedExtensionIdentifiers(): string[] {
    return this.handle.signedExtensionIdentifiers()
  }

  signed_extension_identifiers(): string[] {
    return this.signedExtensionIdentifiers()
  }

  encodeEra(era: ScaleValue): Buffer {
    return nativeCall(() => this.handle.encodeEra(toWire(era)))
  }

  encode_era(era: ScaleValue): Buffer {
    return this.encodeEra(era)
  }

  signaturePayloadParts(params: TransactionParams): PayloadParts {
    return nativeCall(() => this.handle.signaturePayloadParts(nativeTxParams(params)))
  }

  signature_payload_parts(
    era: ScaleValue,
    nonce: IntegerLike,
    tip: IntegerLike,
    tip_asset_id: IntegerLike | null,
    genesis_hash: ByteLike,
    era_block_hash: ByteLike,
    metadata_hash?: ByteLike | null,
  ): PayloadPartsTuple {
    const parts = this.signaturePayloadParts({
      era,
      nonce,
      tip,
      tipAssetId: tip_asset_id,
      genesisHash: genesis_hash,
      eraBlockHash: era_block_hash,
      metadataHash: metadata_hash,
    })
    return [parts.includedInExtrinsic, parts.includedInSignedData]
  }

  signaturePayload(callData: ByteLike, params: TransactionParams): Buffer {
    return nativeCall(() =>
      this.handle.signaturePayload(toBuffer(callData, 'callData'), nativeTxParams(params)),
    )
  }

  signerPayload(
    address: string,
    callData: ByteLike,
    params: TransactionParams,
  ): RuntimeSignerPayload {
    return nativeCall(() =>
      this.handle.signerPayload(address, toBuffer(callData, 'callData'), nativeTxParams(params)),
    )
  }

  signature_payload(
    call_data: ByteLike,
    era: ScaleValue,
    nonce: IntegerLike,
    tip: IntegerLike,
    tip_asset_id: IntegerLike | null,
    genesis_hash: ByteLike,
    era_block_hash: ByteLike,
    metadata_hash?: ByteLike | null,
  ): Buffer {
    return this.signaturePayload(call_data, {
      era,
      nonce,
      tip,
      tipAssetId: tip_asset_id,
      genesisHash: genesis_hash,
      eraBlockHash: era_block_hash,
      metadataHash: metadata_hash,
    })
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

  encode_signed_extrinsic(
    call_data: ByteLike,
    public_key: ByteLike,
    signature: ByteLike,
    signature_version: number,
    era: ScaleValue,
    nonce: IntegerLike,
    tip: IntegerLike,
    tip_asset_id: IntegerLike | null,
    metadata_hash_enabled = false,
  ): SignedExtrinsicTuple {
    const extrinsic = this.encodeSignedExtrinsic(
      call_data,
      public_key,
      signature,
      signature_version,
      {
        era,
        nonce,
        tip,
        tipAssetId: tip_asset_id,
        metadataHashEnabled: metadata_hash_enabled,
      },
    )
    return [extrinsic.bytes, extrinsic.hash]
  }

  decodeExtrinsic<T extends ScaleValue = ScaleValue>(
    data: ByteLike,
    strict = true,
  ): T {
    return nativeCall(
      () => fromWire(this.handle.decodeExtrinsic(toBuffer(data, 'data'), strict)) as T,
    )
  }

  decode_extrinsic<T extends ScaleValue = ScaleValue>(
    data: ByteLike,
    strict = true,
  ): T {
    return this.decodeExtrinsic(data, strict)
  }

  runtimeApiMap(): RuntimeApiMap {
    return this.handle.runtimeApiMap() as RuntimeApiMap
  }

  runtime_api_map(): RuntimeApiMap {
    return this.runtimeApiMap()
  }

  metadataIr(): MetadataIr {
    return nativeCall(() => this.handle.metadataIr() as MetadataIr)
  }

  metadata_ir(): MetadataIr {
    return this.metadataIr()
  }
}

export function eraBirth(period: IntegerLike, current: IntegerLike): bigint {
  return nativeCall(() => native.eraBirth(toBigInt(period, 'period'), toBigInt(current, 'current')))
}

export const era_birth = eraBirth

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

export function multisig_account_id(
  signatories: ByteLike[],
  threshold: number,
): [Buffer, Buffer[]] {
  const account = multisigAccountId(signatories, threshold)
  return [account.accountId, account.sortedSignatories]
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

export function decodeOptionalOpaqueMetadata(data: ByteLike): Buffer | null {
  return nativeCall(() => native.decodeOptionalOpaqueMetadata(toBuffer(data, 'data')) ?? null)
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
