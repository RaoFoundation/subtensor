#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::too_many_arguments
)]

use std::sync::Arc;

use bittensor_core::codec::batch::PARALLEL_THRESHOLD;
use bittensor_core::codec::decode::{compact_len, compact_u128, convert_type_string, Cursor};
use bittensor_core::codec::encode::compact;
use bittensor_core::codec::extrinsic::{era_birth, multisig_account_id, multisig_ss58, TxParams};
use bittensor_core::codec::storage::{concat_hash_len, hash_param, storage_prefix};
use bittensor_core::codec::Value;
use bittensor_core::runtime::type_string::{Primitive, TypeSpec};
use bittensor_core::runtime::{PalletInfo, Runtime, StorageInfo};
use bittensor_core::CoreError;
use napi::bindgen_prelude::{BigInt, Buffer};
use napi_derive::napi;
use scale_info::TypeDef;
use serde_json::{json, Map, Value as JsonValue};

use crate::errors::{into_napi, invalid_arg, CoreResultExt, NapiResult};
use crate::values::{
    from_descriptor, from_wire, to_descriptor, to_wire, values_from_wire, values_to_wire,
};

#[napi(object)]
pub struct NativeStorageEntry {
    pub pallet: String,
    pub name: String,
    pub prefix: String,
    pub modifier: String,
    pub value_type: String,
    pub value_type_id: u32,
    pub param_types: Vec<String>,
    pub param_type_ids: Vec<u32>,
    pub param_hashers: Vec<String>,
    pub default_bytes: Buffer,
}

#[napi(object)]
pub struct NativeStorageChange {
    pub key: String,
    pub value: Option<String>,
}

#[napi(object)]
pub struct NativeMapPair {
    pub key: JsonValue,
    pub value: JsonValue,
}

#[napi(object)]
pub struct NativeOptionalValue {
    pub found: bool,
    pub value: JsonValue,
}

#[napi(object)]
pub struct NativeModuleError {
    pub name: String,
    pub docs: Vec<String>,
}

#[napi(object)]
pub struct NativeTxParams {
    pub era: JsonValue,
    pub nonce: BigInt,
    pub tip: BigInt,
    pub tip_asset_id: Option<BigInt>,
    pub genesis_hash: Buffer,
    pub era_block_hash: Buffer,
    pub metadata_hash: Option<Buffer>,
}

#[napi(object)]
pub struct NativeExtrinsicParams {
    pub era: JsonValue,
    pub nonce: BigInt,
    pub tip: BigInt,
    pub tip_asset_id: Option<BigInt>,
    pub metadata_hash_enabled: bool,
}

#[napi(object)]
pub struct NativePayloadParts {
    pub included_in_extrinsic: Buffer,
    pub included_in_signed_data: Buffer,
}

#[napi(object)]
pub struct NativeSignedExtrinsic {
    pub bytes: Buffer,
    pub hash: Buffer,
}

#[napi(object)]
pub struct NativeMultisigAccount {
    pub account_id: Buffer,
    pub sorted_signatories: Vec<Buffer>,
}

#[napi(object)]
pub struct NativePartialDecode {
    pub value: JsonValue,
    pub offset: u32,
    pub remaining: u32,
}

#[napi(object)]
pub struct NativeCompactDecode {
    pub value: BigInt,
    pub offset: u32,
    pub remaining: u32,
}

#[napi]
pub struct NativeCursor {
    data: Vec<u8>,
    offset: usize,
    strict: bool,
}

impl NativeCursor {
    fn consume<T>(
        &mut self,
        operation: impl FnOnce(&mut Cursor<'_>) -> Result<T, CoreError>,
    ) -> NapiResult<T> {
        let tail = self
            .data
            .get(self.offset..)
            .ok_or_else(|| invalid_arg("cursor offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = self.strict;
        let result = operation(&mut cursor).napi()?;
        self.offset = self.offset.saturating_add(cursor.offset);
        Ok(result)
    }
}

#[napi]
impl NativeCursor {
    #[napi(factory, js_name = "fromBytes")]
    pub fn from_bytes(data: Buffer, strict: bool, offset: u32) -> napi::Result<Self> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("cursor offset does not fit usize"))?;
        if offset > data.len() {
            return Err(invalid_arg("cursor offset is beyond the input buffer"));
        }
        Ok(Self {
            data: data.as_ref().to_vec(),
            offset,
            strict,
        })
    }

    #[napi(getter)]
    pub fn data(&self) -> Buffer {
        self.data.clone().into()
    }

    #[napi(getter)]
    pub fn offset(&self) -> NapiResult<u32> {
        u32::try_from(self.offset).map_err(|_| invalid_arg("cursor offset exceeds u32"))
    }

    #[napi(getter)]
    pub fn remaining(&self) -> NapiResult<u32> {
        u32::try_from(self.data.len().saturating_sub(self.offset))
            .map_err(|_| invalid_arg("cursor remaining byte count exceeds u32"))
    }

    #[napi(getter)]
    pub fn strict(&self) -> bool {
        self.strict
    }

    #[napi(js_name = "setStrict")]
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    #[napi]
    pub fn seek(&mut self, offset: u32) -> NapiResult<()> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("cursor offset does not fit usize"))?;
        if offset > self.data.len() {
            return Err(invalid_arg("cursor offset is beyond the input buffer"));
        }
        self.offset = offset;
        Ok(())
    }

    #[napi]
    pub fn reset(&mut self, data: Buffer, strict: bool, offset: u32) -> NapiResult<()> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("cursor offset does not fit usize"))?;
        if offset > data.len() {
            return Err(invalid_arg("cursor offset is beyond the input buffer"));
        }
        self.data = data.as_ref().to_vec();
        self.offset = offset;
        self.strict = strict;
        Ok(())
    }

    #[napi]
    pub fn take(&mut self, length: u32) -> NapiResult<Buffer> {
        let length =
            usize::try_from(length).map_err(|_| invalid_arg("take length does not fit usize"))?;
        self.consume(|cursor| cursor.take(length).map(ToOwned::to_owned))
            .map(Into::into)
    }

    #[napi]
    pub fn byte(&mut self) -> NapiResult<u8> {
        self.consume(|cursor| cursor.byte())
    }

    #[napi(js_name = "decodeCompactU128")]
    pub fn decode_compact_u128(&mut self) -> NapiResult<BigInt> {
        self.consume(compact_u128).map(BigInt::from)
    }

    #[napi(js_name = "decodeCompactLength")]
    pub fn decode_compact_length(&mut self) -> NapiResult<BigInt> {
        self.consume(compact_len)
            .map(|value| BigInt::from(value as u128))
    }
}

#[napi]
pub struct NativeRuntime {
    inner: Arc<Runtime>,
}

impl NativeRuntime {
    fn entry(&self, pallet: &str, name: &str) -> NapiResult<&StorageInfo> {
        self.inner.storage_entry(pallet, name).ok_or_else(|| {
            into_napi(CoreError::NotInRuntime(format!(
                "storage function {pallet}.{name}"
            )))
        })
    }

    fn tx_params(&self, params: NativeTxParams) -> NapiResult<TxParams> {
        Ok(TxParams {
            era: from_wire(params.era)?,
            nonce: bigint_u64("nonce", &params.nonce)?,
            tip: bigint_u128("tip", &params.tip)?,
            tip_asset_id: params
                .tip_asset_id
                .as_ref()
                .map(|value| bigint_u128("tipAssetId", value))
                .transpose()?,
            genesis_hash: h256("genesisHash", params.genesis_hash.as_ref())?,
            era_block_hash: h256("eraBlockHash", params.era_block_hash.as_ref())?,
            metadata_hash: params
                .metadata_hash
                .as_ref()
                .map(|value| h256("metadataHash", value.as_ref()))
                .transpose()?,
        })
    }

    fn partial_decode(
        &self,
        value: &Value,
        base_offset: usize,
        cursor: &Cursor<'_>,
        descriptor: bool,
    ) -> NapiResult<NativePartialDecode> {
        let absolute = base_offset.saturating_add(cursor.offset);
        Ok(NativePartialDecode {
            value: if descriptor {
                to_descriptor(value)?
            } else {
                to_wire(value)?
            },
            offset: u32::try_from(absolute)
                .map_err(|_| invalid_arg("decoded offset exceeds u32"))?,
            remaining: u32::try_from(cursor.remaining())
                .map_err(|_| invalid_arg("remaining byte count exceeds u32"))?,
        })
    }

    fn decode_value_inner(
        &self,
        spec: &TypeSpec,
        data: Buffer,
        offset: u32,
        strict: bool,
        descriptor: bool,
    ) -> NapiResult<NativePartialDecode> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("offset does not fit usize"))?;
        let tail = data
            .as_ref()
            .get(offset..)
            .ok_or_else(|| invalid_arg("offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = strict;
        let value = self.inner.decode_value(spec, &mut cursor).napi()?;
        self.partial_decode(&value, offset, &cursor, descriptor)
    }

    fn decode_id_inner(
        &self,
        type_id: u32,
        data: Buffer,
        offset: u32,
        strict: bool,
        descriptor: bool,
    ) -> NapiResult<NativePartialDecode> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("offset does not fit usize"))?;
        let tail = data
            .as_ref()
            .get(offset..)
            .ok_or_else(|| invalid_arg("offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = strict;
        let value = self.inner.decode_id(type_id, &mut cursor).napi()?;
        self.partial_decode(&value, offset, &cursor, descriptor)
    }

    fn decode_call_value_inner(
        &self,
        data: Buffer,
        offset: u32,
        strict: bool,
        descriptor: bool,
    ) -> NapiResult<NativePartialDecode> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("offset does not fit usize"))?;
        let tail = data
            .as_ref()
            .get(offset..)
            .ok_or_else(|| invalid_arg("offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = strict;
        let value = self.inner.decode_call_value(&mut cursor).napi()?;
        self.partial_decode(&value, offset, &cursor, descriptor)
    }
}

#[napi]
impl NativeRuntime {
    #[napi(factory, js_name = "fromMetadata")]
    pub fn from_metadata(
        metadata_bytes: Buffer,
        spec_version: u32,
        transaction_version: u32,
        ss58_format: u16,
    ) -> napi::Result<Self> {
        let inner = Runtime::parse(
            metadata_bytes.as_ref(),
            spec_version,
            transaction_version,
            ss58_format,
        )
        .napi()?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    #[napi(getter)]
    pub fn spec_version(&self) -> u32 {
        self.inner.spec_version
    }

    #[napi(getter)]
    pub fn transaction_version(&self) -> u32 {
        self.inner.transaction_version
    }

    #[napi(getter)]
    pub fn ss58_format(&self) -> u16 {
        self.inner.ss58_format
    }

    #[napi(getter)]
    pub fn is_v15(&self) -> bool {
        self.inner.is_v15
    }

    #[napi(getter)]
    pub fn extrinsic_version(&self) -> u8 {
        self.inner.extrinsic.version
    }

    #[napi(getter)]
    pub fn outer_event_type(&self) -> Option<u32> {
        self.inner.outer_event_type
    }

    #[napi(getter)]
    pub fn metadata_bytes(&self) -> Buffer {
        self.inner.metadata_bytes.clone().into()
    }

    #[napi]
    pub fn decode(&self, type_string: String, data: Buffer, strict: bool) -> NapiResult<JsonValue> {
        let spec = self.inner.type_spec(&type_string).napi()?;
        let value = self
            .inner
            .decode_spec(&spec, data.as_ref(), strict)
            .napi()?;
        to_wire(&value)
    }

    #[napi]
    pub fn decode_partial(
        &self,
        type_string: String,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        let spec = self.inner.type_spec(&type_string).napi()?;
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("offset does not fit usize"))?;
        let tail = data
            .as_ref()
            .get(offset..)
            .ok_or_else(|| invalid_arg("offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = strict;
        let value = self.inner.decode_value(&spec, &mut cursor).napi()?;
        let absolute = offset.saturating_add(cursor.offset);
        Ok(NativePartialDecode {
            value: to_wire(&value)?,
            offset: u32::try_from(absolute)
                .map_err(|_| invalid_arg("decoded offset exceeds u32"))?,
            remaining: u32::try_from(cursor.remaining())
                .map_err(|_| invalid_arg("remaining byte count exceeds u32"))?,
        })
    }

    #[napi]
    pub fn decode_type_id(
        &self,
        type_id: u32,
        data: Buffer,
        strict: bool,
    ) -> NapiResult<JsonValue> {
        let value = self
            .inner
            .decode_spec(&TypeSpec::Id(type_id), data.as_ref(), strict)
            .napi()?;
        to_wire(&value)
    }

    #[napi]
    pub fn decode_type_id_partial(
        &self,
        type_id: u32,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        let offset =
            usize::try_from(offset).map_err(|_| invalid_arg("offset does not fit usize"))?;
        let tail = data
            .as_ref()
            .get(offset..)
            .ok_or_else(|| invalid_arg("offset is beyond the input buffer"))?;
        let mut cursor = Cursor::new(tail);
        cursor.strict = strict;
        let value = self.inner.decode_id(type_id, &mut cursor).napi()?;
        let absolute = offset.saturating_add(cursor.offset);
        Ok(NativePartialDecode {
            value: to_wire(&value)?,
            offset: u32::try_from(absolute)
                .map_err(|_| invalid_arg("decoded offset exceeds u32"))?,
            remaining: u32::try_from(cursor.remaining())
                .map_err(|_| invalid_arg("remaining byte count exceeds u32"))?,
        })
    }

    #[napi]
    pub fn decode_batch(
        &self,
        type_strings: Vec<String>,
        datas: Vec<Buffer>,
    ) -> NapiResult<Vec<JsonValue>> {
        let datas: Vec<Vec<u8>> = datas
            .into_iter()
            .map(|value| value.as_ref().to_vec())
            .collect();
        let values = self.inner.decode_batch(&type_strings, &datas).napi()?;
        values.iter().map(to_wire).collect()
    }

    #[napi]
    pub fn encode(&self, type_string: String, value: JsonValue) -> NapiResult<Buffer> {
        let spec = self.inner.type_spec(&type_string).napi()?;
        self.inner
            .encode_spec(&spec, &from_wire(value)?)
            .napi()
            .map(Into::into)
    }

    #[napi]
    pub fn encode_type_id(&self, type_id: u32, value: JsonValue) -> NapiResult<Buffer> {
        self.inner
            .encode_spec(&TypeSpec::Id(type_id), &from_wire(value)?)
            .napi()
            .map(Into::into)
    }

    #[napi]
    pub fn type_id_of(&self, name: String) -> Option<u32> {
        self.inner.type_id_of(&name)
    }

    #[napi]
    pub fn type_name_of(&self, id: u32) -> Option<String> {
        self.inner.type_name_of(id).map(ToOwned::to_owned)
    }

    #[napi]
    pub fn type_spec(&self, type_string: String) -> NapiResult<JsonValue> {
        let spec = self.inner.type_spec(&type_string).napi()?;
        Ok(type_spec_json(&spec))
    }

    #[napi(js_name = "decodeSpec")]
    pub fn decode_spec_native(
        &self,
        spec: JsonValue,
        data: Buffer,
        strict: bool,
    ) -> NapiResult<JsonValue> {
        let spec = type_spec_from_json(spec)?;
        let value = self
            .inner
            .decode_spec(&spec, data.as_ref(), strict)
            .napi()?;
        to_wire(&value)
    }

    #[napi(js_name = "decodeSpecDescriptor")]
    pub fn decode_spec_descriptor(
        &self,
        spec: JsonValue,
        data: Buffer,
        strict: bool,
    ) -> NapiResult<JsonValue> {
        let spec = type_spec_from_json(spec)?;
        let value = self
            .inner
            .decode_spec(&spec, data.as_ref(), strict)
            .napi()?;
        to_descriptor(&value)
    }

    #[napi(js_name = "decodeValue")]
    pub fn decode_value_native(
        &self,
        spec: JsonValue,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        let spec = type_spec_from_json(spec)?;
        self.decode_value_inner(&spec, data, offset, strict, false)
    }

    #[napi(js_name = "decodeValueDescriptor")]
    pub fn decode_value_descriptor(
        &self,
        spec: JsonValue,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        let spec = type_spec_from_json(spec)?;
        self.decode_value_inner(&spec, data, offset, strict, true)
    }

    #[napi(js_name = "decodeTypeIdDescriptor")]
    pub fn decode_type_id_descriptor(
        &self,
        type_id: u32,
        data: Buffer,
        strict: bool,
    ) -> NapiResult<JsonValue> {
        let value = self
            .inner
            .decode_spec(&TypeSpec::Id(type_id), data.as_ref(), strict)
            .napi()?;
        to_descriptor(&value)
    }

    #[napi(js_name = "decodeTypeIdDescriptorPartial")]
    pub fn decode_type_id_descriptor_partial(
        &self,
        type_id: u32,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        self.decode_id_inner(type_id, data, offset, strict, true)
    }

    #[napi(js_name = "encodeSpec")]
    pub fn encode_spec_native(&self, spec: JsonValue, value: JsonValue) -> NapiResult<Buffer> {
        let spec = type_spec_from_json(spec)?;
        self.inner
            .encode_spec(&spec, &from_wire(value)?)
            .napi()
            .map(Into::into)
    }

    #[napi(js_name = "encodeSpecDescriptor")]
    pub fn encode_spec_descriptor(&self, spec: JsonValue, value: JsonValue) -> NapiResult<Buffer> {
        let spec = type_spec_from_json(spec)?;
        self.inner
            .encode_spec(&spec, &from_descriptor(value)?)
            .napi()
            .map(Into::into)
    }

    #[napi(js_name = "encodeValue")]
    pub fn encode_value_native(
        &self,
        spec: JsonValue,
        value: JsonValue,
        prefix: Option<Buffer>,
    ) -> NapiResult<Buffer> {
        let spec = type_spec_from_json(spec)?;
        let mut output = prefix
            .as_ref()
            .map(|value| value.as_ref().to_vec())
            .unwrap_or_default();
        self.inner
            .encode_value(&spec, &from_wire(value)?, &mut output)
            .napi()?;
        Ok(output.into())
    }

    #[napi(js_name = "encodeValueDescriptor")]
    pub fn encode_value_descriptor(
        &self,
        spec: JsonValue,
        value: JsonValue,
        prefix: Option<Buffer>,
    ) -> NapiResult<Buffer> {
        let spec = type_spec_from_json(spec)?;
        let mut output = prefix
            .as_ref()
            .map(|value| value.as_ref().to_vec())
            .unwrap_or_default();
        self.inner
            .encode_value(&spec, &from_descriptor(value)?, &mut output)
            .napi()?;
        Ok(output.into())
    }

    #[napi(js_name = "encodeId")]
    pub fn encode_id_native(
        &self,
        type_id: u32,
        value: JsonValue,
        prefix: Option<Buffer>,
    ) -> NapiResult<Buffer> {
        let mut output = prefix
            .as_ref()
            .map(|value| value.as_ref().to_vec())
            .unwrap_or_default();
        self.inner
            .encode_id(type_id, &from_wire(value)?, &mut output)
            .napi()?;
        Ok(output.into())
    }

    #[napi(js_name = "encodeIdDescriptor")]
    pub fn encode_id_descriptor(
        &self,
        type_id: u32,
        value: JsonValue,
        prefix: Option<Buffer>,
    ) -> NapiResult<Buffer> {
        let mut output = prefix
            .as_ref()
            .map(|value| value.as_ref().to_vec())
            .unwrap_or_default();
        self.inner
            .encode_id(type_id, &from_descriptor(value)?, &mut output)
            .napi()?;
        Ok(output.into())
    }

    #[napi(js_name = "coerceAccountId")]
    pub fn coerce_account_id_native(&self, value: JsonValue) -> NapiResult<Buffer> {
        self.inner
            .coerce_account_id(&from_wire(value)?)
            .napi()
            .map(|value| value.to_vec().into())
    }

    #[napi(js_name = "coerceAccountIdDescriptor")]
    pub fn coerce_account_id_descriptor(&self, value: JsonValue) -> NapiResult<Buffer> {
        self.inner
            .coerce_account_id(&from_descriptor(value)?)
            .napi()
            .map(|value| value.to_vec().into())
    }

    #[napi]
    pub fn resolve_type(&self, id: u32) -> NapiResult<JsonValue> {
        let ty = self.inner.resolve(id).napi()?;
        serde_json::to_value(ty)
            .map_err(|error| invalid_arg(format!("type serialization failed: {error}")))
    }

    #[napi]
    pub fn registry_json(&self) -> NapiResult<String> {
        self.inner.registry_json().napi()
    }

    #[napi]
    pub fn registry(&self) -> NapiResult<JsonValue> {
        let json = self.inner.registry_json().napi()?;
        serde_json::from_str(&json)
            .map_err(|error| invalid_arg(format!("registry JSON parse failed: {error}")))
    }

    #[napi]
    pub fn pallet(&self, name: String) -> Option<JsonValue> {
        self.inner.pallet(&name).map(pallet_json)
    }

    #[napi]
    pub fn pallet_at(&self, index: u8) -> Option<JsonValue> {
        self.inner.pallet_at(index).map(pallet_json)
    }

    #[napi]
    pub fn pallets(&self) -> Vec<JsonValue> {
        self.inner.pallets.iter().map(pallet_json).collect()
    }

    #[napi]
    pub fn extrinsic_info(&self) -> JsonValue {
        extrinsic_json(&self.inner)
    }

    #[napi]
    pub fn runtime_apis(&self) -> JsonValue {
        runtime_api_map_json(&self.inner)
    }

    #[napi]
    pub fn runtime_api_infos(&self) -> JsonValue {
        runtime_api_infos_json(&self.inner)
    }

    #[napi]
    pub fn runtime_snapshot(&self) -> JsonValue {
        json!({
            "specVersion": self.inner.spec_version,
            "transactionVersion": self.inner.transaction_version,
            "ss58Format": self.inner.ss58_format,
            "isV15": self.inner.is_v15,
            "outerEventType": self.inner.outer_event_type,
            "pallets": self.inner.pallets.iter().map(pallet_json).collect::<Vec<_>>(),
            "extrinsic": extrinsic_json(&self.inner),
            "runtimeApis": runtime_api_map_json(&self.inner),
            "runtimeApiInfos": runtime_api_infos_json(&self.inner),
        })
    }

    #[napi]
    pub fn compose_call(
        &self,
        pallet: String,
        function: String,
        params: JsonValue,
    ) -> NapiResult<Buffer> {
        self.inner
            .compose_call(&pallet, &function, &from_wire(params)?)
            .napi()
            .map(Into::into)
    }

    #[napi]
    pub fn decode_call(&self, data: Buffer) -> NapiResult<JsonValue> {
        let mut cursor = Cursor::new(data.as_ref());
        let value = self.inner.decode_call_value(&mut cursor).napi()?;
        if cursor.remaining() != 0 {
            return Err(invalid_arg(format!(
                "{} undecoded bytes remain after the call",
                cursor.remaining()
            )));
        }
        to_wire(&value)
    }

    #[napi(js_name = "decodeCallValue")]
    pub fn decode_call_value_native(
        &self,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        self.decode_call_value_inner(data, offset, strict, false)
    }

    #[napi(js_name = "decodeCallValueDescriptor")]
    pub fn decode_call_value_descriptor(
        &self,
        data: Buffer,
        offset: u32,
        strict: bool,
    ) -> NapiResult<NativePartialDecode> {
        self.decode_call_value_inner(data, offset, strict, true)
    }

    #[napi]
    pub fn storage_entry(
        &self,
        pallet: String,
        storage_function: String,
    ) -> NapiResult<NativeStorageEntry> {
        Ok(storage_entry_native(
            &pallet,
            self.entry(&pallet, &storage_function)?,
        ))
    }

    #[napi]
    pub fn storage_prefix(&self, pallet: String, storage_function: String) -> NapiResult<Buffer> {
        Ok(storage_prefix(self.entry(&pallet, &storage_function)?).into())
    }

    #[napi]
    pub fn storage_key(
        &self,
        pallet: String,
        storage_function: String,
        params: JsonValue,
    ) -> NapiResult<Buffer> {
        let entry = self.entry(&pallet, &storage_function)?;
        let values = values_from_wire(params)?;
        self.inner
            .storage_key(entry, &values)
            .napi()
            .map(Into::into)
    }

    #[napi]
    pub fn storage_key_batch(
        &self,
        pallet: String,
        storage_function: String,
        params_list: JsonValue,
    ) -> NapiResult<Vec<Buffer>> {
        let JsonValue::Array(rows) = params_list else {
            return Err(invalid_arg("paramsList must be an array of arrays"));
        };
        let entry = self.entry(&pallet, &storage_function)?;
        rows.into_iter()
            .map(|row| {
                let values = values_from_wire(row)?;
                self.inner
                    .storage_key(entry, &values)
                    .napi()
                    .map(Into::into)
            })
            .collect()
    }

    #[napi]
    pub fn decode_storage_key_params(
        &self,
        pallet: String,
        storage_function: String,
        key: Buffer,
        fixed: u32,
    ) -> NapiResult<JsonValue> {
        let fixed = usize::try_from(fixed).map_err(|_| invalid_arg("fixed does not fit usize"))?;
        let values = self
            .inner
            .decode_storage_key_params(self.entry(&pallet, &storage_function)?, key.as_ref(), fixed)
            .napi()?;
        values_to_wire(&values)
    }

    #[napi]
    pub fn decode_map_pairs(
        &self,
        pallet: String,
        storage_function: String,
        raw_keys: Vec<Buffer>,
        raw_values: Vec<Buffer>,
        fixed: u32,
    ) -> NapiResult<Vec<NativeMapPair>> {
        let fixed = usize::try_from(fixed).map_err(|_| invalid_arg("fixed does not fit usize"))?;
        let raw_keys: Vec<Vec<u8>> = raw_keys
            .into_iter()
            .map(|value| value.as_ref().to_vec())
            .collect();
        let raw_values: Vec<Vec<u8>> = raw_values
            .into_iter()
            .map(|value| value.as_ref().to_vec())
            .collect();
        let decoded = self
            .inner
            .decode_map_page(
                self.entry(&pallet, &storage_function)?,
                &raw_keys,
                &raw_values,
                fixed,
            )
            .napi()?;
        decoded.into_iter().map(map_pair_native).collect()
    }

    #[napi]
    pub fn decode_map_changes(
        &self,
        pallet: String,
        storage_function: String,
        changes: Vec<NativeStorageChange>,
        fixed: u32,
    ) -> NapiResult<Vec<NativeMapPair>> {
        let fixed = usize::try_from(fixed).map_err(|_| invalid_arg("fixed does not fit usize"))?;
        let changes: Vec<(String, Option<String>)> = changes
            .into_iter()
            .map(|change| (change.key, change.value))
            .collect();
        let decoded = self
            .inner
            .decode_map_changes(self.entry(&pallet, &storage_function)?, &changes, fixed)
            .napi()?;
        decoded.into_iter().map(map_pair_native).collect()
    }

    #[napi]
    pub fn constant(&self, pallet: String, name: String) -> NapiResult<NativeOptionalValue> {
        let Some(constant) = self.inner.constant(&pallet, &name) else {
            return Ok(NativeOptionalValue {
                found: false,
                value: JsonValue::Null,
            });
        };
        let mut cursor = Cursor::new(&constant.value);
        let value = self.inner.decode_id(constant.ty, &mut cursor).napi()?;
        Ok(NativeOptionalValue {
            found: true,
            value: to_wire(&value)?,
        })
    }

    #[napi]
    pub fn constant_info(&self, pallet: String, name: String) -> Option<JsonValue> {
        self.inner.constant(&pallet, &name).map(|constant| {
            json!({
                "name": constant.name,
                "typeId": constant.ty,
                "type": format!("scale_info::{}", constant.ty),
                "valueHex": format!("0x{}", hex::encode(&constant.value)),
                "docs": constant.docs,
            })
        })
    }

    #[napi]
    pub fn module_error(&self, module_index: u8, error_index: u8) -> NapiResult<NativeModuleError> {
        let (name, docs) = self.inner.module_error(module_index, error_index).napi()?;
        Ok(NativeModuleError { name, docs })
    }

    #[napi]
    pub fn signed_extension_identifiers(&self) -> Vec<String> {
        self.inner
            .extrinsic
            .signed_extensions
            .iter()
            .map(|extension| extension.identifier.clone())
            .collect()
    }

    #[napi]
    pub fn encode_era(&self, era: JsonValue) -> NapiResult<Buffer> {
        let mut output = Vec::new();
        self.inner
            .encode_era_value(&from_wire(era)?, &mut output)
            .napi()?;
        Ok(output.into())
    }

    #[napi]
    pub fn signature_payload_parts(
        &self,
        params: NativeTxParams,
    ) -> NapiResult<NativePayloadParts> {
        let params = self.tx_params(params)?;
        let (extra, additional) = self.inner.signature_payload_parts(&params).napi()?;
        Ok(NativePayloadParts {
            included_in_extrinsic: extra.into(),
            included_in_signed_data: additional.into(),
        })
    }

    #[napi]
    pub fn signature_payload(
        &self,
        call_data: Buffer,
        params: NativeTxParams,
    ) -> NapiResult<Buffer> {
        let params = self.tx_params(params)?;
        self.inner
            .signature_payload(call_data.as_ref(), &params)
            .napi()
            .map(Into::into)
    }

    #[napi]
    pub fn encode_signed_extrinsic(
        &self,
        call_data: Buffer,
        public_key: Buffer,
        signature: Buffer,
        signature_version: u8,
        params: NativeExtrinsicParams,
    ) -> NapiResult<NativeSignedExtrinsic> {
        let tx_params = TxParams {
            era: from_wire(params.era)?,
            nonce: bigint_u64("nonce", &params.nonce)?,
            tip: bigint_u128("tip", &params.tip)?,
            tip_asset_id: params
                .tip_asset_id
                .as_ref()
                .map(|value| bigint_u128("tipAssetId", value))
                .transpose()?,
            genesis_hash: [0; 32],
            era_block_hash: [0; 32],
            metadata_hash: params.metadata_hash_enabled.then_some([0; 32]),
        };
        let public_key = h256("publicKey", public_key.as_ref())?;
        let (bytes, hash) = self
            .inner
            .encode_signed_extrinsic(
                call_data.as_ref(),
                public_key,
                signature.as_ref(),
                signature_version,
                &tx_params,
            )
            .napi()?;
        Ok(NativeSignedExtrinsic {
            bytes: bytes.into(),
            hash: hash.to_vec().into(),
        })
    }

    #[napi]
    pub fn decode_extrinsic(&self, data: Buffer, strict: bool) -> NapiResult<JsonValue> {
        let value = self.inner.decode_extrinsic(data.as_ref(), strict).napi()?;
        to_wire(&value)
    }

    #[napi]
    pub fn runtime_api_map(&self) -> JsonValue {
        runtime_api_map_json(&self.inner)
    }

    #[napi]
    pub fn metadata_ir(&self) -> NapiResult<JsonValue> {
        metadata_ir_json(&self.inner)
    }
}

fn h256(name: &str, raw: &[u8]) -> NapiResult<[u8; 32]> {
    raw.try_into()
        .map_err(|_| invalid_arg(format!("{name} must be exactly 32 bytes")))
}

fn bigint_u64(name: &str, value: &BigInt) -> NapiResult<u64> {
    let (negative, value, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(invalid_arg(format!(
            "{name} must be an unsigned 64-bit bigint"
        )));
    }
    Ok(value)
}

fn bigint_u128(name: &str, value: &BigInt) -> NapiResult<u128> {
    let (negative, value, lossless) = value.get_u128();
    if negative || !lossless {
        return Err(invalid_arg(format!(
            "{name} must be an unsigned 128-bit bigint"
        )));
    }
    Ok(value)
}

fn storage_entry_native(pallet: &str, info: &StorageInfo) -> NativeStorageEntry {
    NativeStorageEntry {
        pallet: pallet.to_owned(),
        name: info.name.clone(),
        prefix: info.prefix.clone(),
        modifier: info.modifier.clone(),
        value_type: format!("scale_info::{}", info.value_type),
        value_type_id: info.value_type,
        param_types: info
            .key_types
            .iter()
            .map(|id| format!("scale_info::{id}"))
            .collect(),
        param_type_ids: info.key_types.clone(),
        param_hashers: info.hashers.clone(),
        default_bytes: info.default_bytes.clone().into(),
    }
}

fn map_pair_native(pair: (Vec<Value>, Value)) -> NapiResult<NativeMapPair> {
    let (keys, value) = pair;
    let key = match keys.as_slice() {
        [single] => single.clone(),
        _ => Value::Tuple(keys),
    };
    Ok(NativeMapPair {
        key: to_wire(&key)?,
        value: to_wire(&value)?,
    })
}

fn type_spec_json(spec: &TypeSpec) -> JsonValue {
    match spec {
        TypeSpec::Id(id) => json!({"kind": "id", "id": id}),
        TypeSpec::Primitive(primitive) => {
            json!({"kind": "primitive", "name": primitive_name(*primitive)})
        }
        TypeSpec::Sequence(inner) => {
            json!({"kind": "sequence", "inner": type_spec_json(inner)})
        }
        TypeSpec::Option(inner) => json!({"kind": "option", "inner": type_spec_json(inner)}),
        TypeSpec::Array(inner, len) => {
            json!({"kind": "array", "inner": type_spec_json(inner), "length": len})
        }
        TypeSpec::Tuple(items) => json!({
            "kind": "tuple",
            "items": items.iter().map(type_spec_json).collect::<Vec<_>>()
        }),
        TypeSpec::Compact(inner) => {
            json!({"kind": "compact", "inner": type_spec_json(inner)})
        }
        TypeSpec::Bytes => json!({"kind": "bytes"}),
        TypeSpec::AccountId => json!({"kind": "accountId"}),
        TypeSpec::Era => json!({"kind": "era"}),
        TypeSpec::Call => json!({"kind": "call"}),
        TypeSpec::Extrinsic => json!({"kind": "extrinsic"}),
    }
}

fn type_spec_from_json(value: JsonValue) -> NapiResult<TypeSpec> {
    type_spec_from_json_at(value, 0)
}

fn type_spec_from_json_at(value: JsonValue, depth: usize) -> NapiResult<TypeSpec> {
    if depth > 256 {
        return Err(invalid_arg("type spec nesting exceeds 256 levels"));
    }
    let JsonValue::Object(mut map) = value else {
        return Err(invalid_arg("type spec must be an object"));
    };
    let kind = map
        .remove("kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| invalid_arg("type spec is missing string `kind`"))?;
    let inner = |map: &mut Map<String, JsonValue>| -> NapiResult<TypeSpec> {
        let value = map
            .remove("inner")
            .ok_or_else(|| invalid_arg(format!("{kind} type spec is missing `inner`")))?;
        type_spec_from_json_at(value, depth + 1)
    };
    match kind.as_str() {
        "id" => map
            .remove("id")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .map(TypeSpec::Id)
            .ok_or_else(|| invalid_arg("id type spec needs a u32 `id`")),
        "primitive" => {
            let name = map
                .remove("name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| invalid_arg("primitive type spec needs string `name`"))?;
            Primitive::from_name(&name)
                .map(TypeSpec::Primitive)
                .ok_or_else(|| invalid_arg(format!("unknown primitive type {name:?}")))
        }
        "sequence" => inner(&mut map).map(|value| TypeSpec::Sequence(Box::new(value))),
        "option" => inner(&mut map).map(|value| TypeSpec::Option(Box::new(value))),
        "array" => {
            let inner = inner(&mut map)?;
            let length = map
                .remove("length")
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| invalid_arg("array type spec needs a u32 `length`"))?;
            Ok(TypeSpec::Array(Box::new(inner), length))
        }
        "tuple" => {
            let items = map
                .remove("items")
                .and_then(|value| value.as_array().cloned())
                .ok_or_else(|| invalid_arg("tuple type spec needs array `items`"))?;
            items
                .into_iter()
                .map(|item| type_spec_from_json_at(item, depth + 1))
                .collect::<NapiResult<Vec<_>>>()
                .map(TypeSpec::Tuple)
        }
        "compact" => inner(&mut map).map(|value| TypeSpec::Compact(Box::new(value))),
        "bytes" => Ok(TypeSpec::Bytes),
        "accountId" => Ok(TypeSpec::AccountId),
        "era" => Ok(TypeSpec::Era),
        "call" => Ok(TypeSpec::Call),
        "extrinsic" => Ok(TypeSpec::Extrinsic),
        _ => Err(invalid_arg(format!("unknown type spec kind {kind:?}"))),
    }
}

fn primitive_name(primitive: Primitive) -> &'static str {
    match primitive {
        Primitive::Bool => "bool",
        Primitive::Char => "char",
        Primitive::Str => "str",
        Primitive::U8 => "u8",
        Primitive::U16 => "u16",
        Primitive::U32 => "u32",
        Primitive::U64 => "u64",
        Primitive::U128 => "u128",
        Primitive::U256 => "u256",
        Primitive::I8 => "i8",
        Primitive::I16 => "i16",
        Primitive::I32 => "i32",
        Primitive::I64 => "i64",
        Primitive::I128 => "i128",
        Primitive::I256 => "i256",
    }
}

fn pallet_json(pallet: &PalletInfo) -> JsonValue {
    json!({
        "name": pallet.name,
        "index": pallet.index,
        "callsType": pallet.calls_type,
        "eventsType": pallet.events_type,
        "errorsType": pallet.errors_type,
        "constants": pallet.constants.iter().map(|constant| json!({
            "name": constant.name,
            "typeId": constant.ty,
            "type": format!("scale_info::{}", constant.ty),
            "valueHex": format!("0x{}", hex::encode(&constant.value)),
            "docs": constant.docs,
        })).collect::<Vec<_>>(),
        "storage": pallet.storage.iter().map(|storage| json!({
            "name": storage.name,
            "prefix": storage.prefix,
            "modifier": storage.modifier,
            "hashers": storage.hashers,
            "keyTypeIds": storage.key_types,
            "keyTypes": storage.key_types.iter().map(|id| format!("scale_info::{id}")).collect::<Vec<_>>(),
            "valueTypeId": storage.value_type,
            "valueType": format!("scale_info::{}", storage.value_type),
            "defaultHex": format!("0x{}", hex::encode(&storage.default_bytes)),
        })).collect::<Vec<_>>(),
    })
}

fn extrinsic_json(runtime: &Runtime) -> JsonValue {
    json!({
        "version": runtime.extrinsic.version,
        "addressType": runtime.extrinsic.address_type,
        "callType": runtime.extrinsic.call_type,
        "signatureType": runtime.extrinsic.signature_type,
        "signedExtensions": runtime.extrinsic.signed_extensions.iter().map(|extension| json!({
            "identifier": extension.identifier,
            "typeId": extension.ty,
            "type": format!("scale_info::{}", extension.ty),
            "additionalSignedTypeId": extension.additional_signed,
            "additionalSignedType": format!("scale_info::{}", extension.additional_signed),
        })).collect::<Vec<_>>(),
    })
}

fn runtime_api_map_json(runtime: &Runtime) -> JsonValue {
    let mut apis = Map::new();
    for api in &runtime.apis {
        let mut methods = Map::new();
        for method in &api.methods {
            methods.insert(
                method.name.clone(),
                json!({
                    "name": method.name,
                    "inputs": method.inputs.iter().map(|param| json!([
                        param.name,
                        format!("scale_info::{}", param.ty),
                    ])).collect::<Vec<_>>(),
                    "output": format!("scale_info::{}", method.output),
                    "outputTypeId": method.output,
                    "docs": method.docs,
                }),
            );
        }
        apis.insert(api.name.clone(), JsonValue::Object(methods));
    }
    JsonValue::Object(apis)
}

fn runtime_api_infos_json(runtime: &Runtime) -> JsonValue {
    JsonValue::Array(
        runtime
            .apis
            .iter()
            .map(|api| {
                json!({
                    "name": api.name,
                    "methods": api.methods.iter().map(|method| {
                        json!({
                            "name": method.name,
                            "inputs": method.inputs.iter().map(|param| {
                                json!({
                                    "name": param.name,
                                    "typeId": param.ty,
                                    "type": format!("scale_info::{}", param.ty),
                                })
                            }).collect::<Vec<_>>(),
                            "output": method.output,
                            "outputType": format!("scale_info::{}", method.output),
                            "docs": method.docs,
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn metadata_ir_json(runtime: &Runtime) -> NapiResult<JsonValue> {
    let join_docs = |docs: &[String]| -> String {
        docs.iter()
            .map(|doc| doc.trim())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned()
    };

    let mut pallets = Vec::with_capacity(runtime.pallets.len());
    for pallet in &runtime.pallets {
        let mut calls = Vec::new();
        if let Some(calls_type) = pallet.calls_type {
            let ty = runtime.resolve(calls_type).napi()?;
            if let TypeDef::Variant(variant) = &ty.type_def {
                for call in &variant.variants {
                    calls.push(json!({
                        "name": call.name,
                        "index": call.index,
                        "args": call.fields.iter().map(|field| field.name.clone().unwrap_or_default()).collect::<Vec<_>>(),
                        "argTypes": call.fields.iter().map(|field| format!("scale_info::{}", field.ty.id)).collect::<Vec<_>>(),
                        "docs": join_docs(&call.docs),
                    }));
                }
            }
        }

        let mut errors = Vec::new();
        if let Some(errors_type) = pallet.errors_type {
            let ty = runtime.resolve(errors_type).napi()?;
            if let TypeDef::Variant(variant) = &ty.type_def {
                for error in &variant.variants {
                    errors.push(json!({
                        "index": error.index,
                        "name": error.name,
                        "docs": join_docs(&error.docs),
                    }));
                }
            }
        }

        pallets.push(json!({
            "name": pallet.name,
            "index": pallet.index,
            "calls": calls,
            "errors": errors,
            "storage": pallet.storage.iter().filter(|storage| !storage.name.contains(':')).map(|storage| storage.name.clone()).collect::<Vec<_>>(),
            "constants": pallet.constants.iter().map(|constant| constant.name.clone()).collect::<Vec<_>>(),
        }));
    }

    Ok(json!({
        "specVersion": runtime.spec_version,
        "pallets": pallets,
        "runtimeApis": runtime.apis.iter().map(|api| json!({
            "name": api.name,
            "methods": api.methods.iter().map(|method| method.name.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    }))
}

#[napi(js_name = "convertTypeString")]
pub fn convert_type_string_native(name: String) -> String {
    convert_type_string(&name)
}

#[napi(js_name = "primitiveFromName")]
pub fn primitive_from_name_native(name: String) -> Option<String> {
    Primitive::from_name(&name).map(|primitive| primitive_name(primitive).to_owned())
}

#[napi(js_name = "normalizeTypeSpec")]
pub fn normalize_type_spec(spec: JsonValue) -> NapiResult<JsonValue> {
    Ok(type_spec_json(&type_spec_from_json(spec)?))
}

#[napi(js_name = "eraBirth")]
pub fn era_birth_native(period: BigInt, current: BigInt) -> NapiResult<BigInt> {
    Ok(BigInt::from(era_birth(
        bigint_u64("period", &period)?,
        bigint_u64("current", &current)?,
    )))
}

#[napi(js_name = "multisigAccountId")]
pub fn multisig_account_id_native(
    signatories: Vec<Buffer>,
    threshold: u16,
) -> NapiResult<NativeMultisigAccount> {
    let keys = signatories
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            raw.as_ref().try_into().map_err(|_| {
                invalid_arg(format!(
                    "signatory #{} must be exactly 32 bytes",
                    index.saturating_add(1)
                ))
            })
        })
        .collect::<NapiResult<Vec<[u8; 32]>>>()?;
    let (account_id, sorted_signatories) = multisig_account_id(&keys, threshold).napi()?;
    Ok(NativeMultisigAccount {
        account_id: account_id.to_vec().into(),
        sorted_signatories: sorted_signatories
            .into_iter()
            .map(|key| key.to_vec().into())
            .collect(),
    })
}

#[napi(js_name = "multisigSs58")]
pub fn multisig_ss58_native(account_id: Buffer, ss58_format: u16) -> NapiResult<String> {
    Ok(multisig_ss58(
        h256("accountId", account_id.as_ref())?,
        ss58_format,
    ))
}

#[napi(js_name = "encodeCompact")]
pub fn encode_compact(value: BigInt) -> NapiResult<Buffer> {
    let value = bigint_u128("value", &value)?;
    let mut output = Vec::new();
    compact(value, &mut output).napi()?;
    Ok(output.into())
}

#[napi(js_name = "decodeCompactU128")]
pub fn decode_compact_u128(data: Buffer, strict: bool) -> NapiResult<NativeCompactDecode> {
    let mut cursor = Cursor::new(data.as_ref());
    cursor.strict = strict;
    let value = compact_u128(&mut cursor).napi()?;
    Ok(NativeCompactDecode {
        value: BigInt::from(value),
        offset: u32::try_from(cursor.offset)
            .map_err(|_| invalid_arg("compact offset exceeds u32"))?,
        remaining: u32::try_from(cursor.remaining())
            .map_err(|_| invalid_arg("compact remaining byte count exceeds u32"))?,
    })
}

#[napi(js_name = "decodeCompactLength")]
pub fn decode_compact_length(data: Buffer, strict: bool) -> NapiResult<NativeCompactDecode> {
    let mut cursor = Cursor::new(data.as_ref());
    cursor.strict = strict;
    let value = compact_len(&mut cursor).napi()?;
    Ok(NativeCompactDecode {
        value: BigInt::from(value as u128),
        offset: u32::try_from(cursor.offset)
            .map_err(|_| invalid_arg("compact offset exceeds u32"))?,
        remaining: u32::try_from(cursor.remaining())
            .map_err(|_| invalid_arg("compact remaining byte count exceeds u32"))?,
    })
}

#[napi(js_name = "hashStorageParam")]
pub fn hash_storage_param(hasher: String, data: Buffer) -> NapiResult<Buffer> {
    hash_param(&hasher, data.as_ref()).napi().map(Into::into)
}

#[napi(js_name = "storagePrefixFor")]
pub fn storage_prefix_for(prefix: String, name: String) -> Buffer {
    storage_prefix(&StorageInfo {
        name,
        prefix,
        modifier: "Optional".to_owned(),
        hashers: Vec::new(),
        key_types: Vec::new(),
        value_type: 0,
        default_bytes: Vec::new(),
    })
    .into()
}

#[napi(js_name = "concatHashLength")]
pub fn concat_hash_length(hasher: String) -> NapiResult<u32> {
    let length = concat_hash_len(&hasher).napi()?;
    u32::try_from(length).map_err(|_| invalid_arg("hash length exceeds u32"))
}

#[napi(js_name = "parallelDecodeThreshold")]
pub fn parallel_decode_threshold() -> u32 {
    u32::try_from(PARALLEL_THRESHOLD).unwrap_or(u32::MAX)
}
