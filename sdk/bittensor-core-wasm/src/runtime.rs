//! Bindings for `bittensor_core::runtime` + `codec` — the `Runtime` class
//! the TS shell's codec seam is built on, mirroring the Python binding
//! method-for-method (camelCased). No logic lives here: value
//! materialization, method forwarding, and error mapping only.

use bittensor_core::codec::extrinsic::{era_birth, multisig_account_id, TxParams};
use bittensor_core::codec::{decode::Cursor, storage::storage_prefix};
use bittensor_core::runtime::{Runtime as CoreRuntime, StorageInfo};
use js_sys::{Array, Object, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::errors::{to_js_err, value_err};
use crate::values::{
    js_to_value, materialize_pairs, optional_u128_arg, u128_arg, u64_arg, value_to_js,
};

fn h256_arg(name: &str, raw: &[u8]) -> Result<[u8; 32], JsValue> {
    raw.try_into()
        .map_err(|_| value_err(format!("{name} must be 32 bytes")))
}

// Keys here include metadata-derived names (pallets, APIs, methods), which
// come from whatever RPC node supplied the metadata — same `__proto__`
// hazard as decoded dict keys, same guard.
fn set(target: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    crate::values::set_own(target, key, value)
}

fn string_array(items: impl IntoIterator<Item = impl AsRef<str>>) -> Array {
    let out = Array::new();
    for item in items {
        out.push(&JsValue::from_str(item.as_ref()));
    }
    out
}

fn bytes_array(uint8_arrays: &Array, name: &str) -> Result<Vec<Vec<u8>>, JsValue> {
    let mut out = Vec::with_capacity(uint8_arrays.length() as usize);
    for item in uint8_arrays.iter() {
        let bytes: Uint8Array = item
            .dyn_into()
            .map_err(|_| value_err(format!("{name} must contain Uint8Array items")))?;
        out.push(bytes.to_vec());
    }
    Ok(out)
}

/// Everything the shell needs to build keys for / decode values of one
/// storage item, as a plain object. Type references are `scale_info::N`
/// strings for this runtime.
fn storage_entry_js(pallet: &str, info: &StorageInfo) -> Result<JsValue, JsValue> {
    let entry = Object::new();
    set(&entry, "pallet", &JsValue::from_str(pallet))?;
    set(&entry, "name", &JsValue::from_str(&info.name))?;
    set(&entry, "prefix", &JsValue::from_str(&info.prefix))?;
    set(&entry, "modifier", &JsValue::from_str(&info.modifier))?;
    set(
        &entry,
        "valueType",
        &JsValue::from_str(&format!("scale_info::{}", info.value_type)),
    )?;
    set(
        &entry,
        "paramTypes",
        &string_array(info.key_types.iter().map(|id| format!("scale_info::{id}"))).into(),
    )?;
    set(&entry, "paramHashers", &string_array(&info.hashers).into())?;
    set(
        &entry,
        "defaultBytes",
        &Uint8Array::from(info.default_bytes.as_slice()).into(),
    )?;
    Ok(entry.into())
}

/// One runtime's complete metadata view and SCALE codec, parsed once from
/// the raw `MetadataVersioned` bytes the transport downloads and caches.
#[wasm_bindgen]
pub struct Runtime {
    inner: CoreRuntime,
}

impl Runtime {
    fn entry(&self, pallet: &str, name: &str) -> Result<&StorageInfo, JsValue> {
        self.inner.storage_entry(pallet, name).ok_or_else(|| {
            let error = js_sys::Error::new(&format!("storage function {pallet}.{name} not found"));
            error.set_name("NotInRuntimeError");
            error.into()
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn tx_params(
        &self,
        era: &JsValue,
        nonce: &JsValue,
        tip: &JsValue,
        tip_asset_id: &JsValue,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<Vec<u8>>,
    ) -> Result<TxParams, JsValue> {
        Ok(TxParams {
            era: js_to_value(era)?,
            nonce: u64_arg(nonce, "nonce")?,
            tip: u128_arg(tip, "tip")?,
            tip_asset_id: optional_u128_arg(tip_asset_id, "tipAssetId")?,
            genesis_hash: h256_arg("genesisHash", genesis_hash)?,
            era_block_hash: h256_arg("eraBlockHash", era_block_hash)?,
            metadata_hash: metadata_hash
                .as_deref()
                .map(|h| h256_arg("metadataHash", h))
                .transpose()?,
        })
    }
}

#[wasm_bindgen]
impl Runtime {
    /// Parse a raw `MetadataVersioned` blob (magic `meta` + version byte +
    /// V14/V15 payload).
    #[wasm_bindgen(constructor)]
    pub fn new(
        metadata_bytes: &[u8],
        spec_version: u32,
        transaction_version: u32,
        ss58_format: Option<u16>,
    ) -> Result<Runtime, JsValue> {
        let inner = CoreRuntime::parse(
            metadata_bytes,
            spec_version,
            transaction_version,
            ss58_format.unwrap_or(42),
        )
        .map_err(to_js_err)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(getter, js_name = specVersion)]
    pub fn spec_version(&self) -> u32 {
        self.inner.spec_version
    }

    #[wasm_bindgen(getter, js_name = transactionVersion)]
    pub fn transaction_version(&self) -> u32 {
        self.inner.transaction_version
    }

    #[wasm_bindgen(getter, js_name = ss58Format)]
    pub fn ss58_format(&self) -> u16 {
        self.inner.ss58_format
    }

    #[wasm_bindgen(getter, js_name = isV15)]
    pub fn is_v15(&self) -> bool {
        self.inner.is_v15
    }

    #[wasm_bindgen(getter, js_name = extrinsicVersion)]
    pub fn extrinsic_version(&self) -> u8 {
        self.inner.extrinsic.version
    }

    // -- generic encode/decode ------------------------------------------------

    /// Decode SCALE `data` as `typeString`, returning plain JS values.
    pub fn decode(
        &self,
        type_string: &str,
        data: &[u8],
        strict: Option<bool>,
    ) -> Result<JsValue, JsValue> {
        let spec = self.inner.type_spec(type_string).map_err(to_js_err)?;
        let value = self
            .inner
            .decode_spec(&spec, data, strict.unwrap_or(true))
            .map_err(to_js_err)?;
        value_to_js(&value)
    }

    /// Bulk decode. Type specs resolve once per distinct string; one FFI
    /// crossing per page instead of one per entry.
    #[wasm_bindgen(js_name = batchDecode)]
    pub fn batch_decode(&self, type_strings: Vec<String>, datas: &Array) -> Result<Array, JsValue> {
        let datas = bytes_array(datas, "datas")?;
        let values = self
            .inner
            .decode_batch(&type_strings, &datas)
            .map_err(to_js_err)?;
        let out = Array::new();
        for value in &values {
            out.push(&value_to_js(value)?);
        }
        Ok(out)
    }

    /// SCALE-encode `value` as `typeString`.
    pub fn encode(&self, type_string: &str, value: &JsValue) -> Result<Vec<u8>, JsValue> {
        let spec = self.inner.type_spec(type_string).map_err(to_js_err)?;
        self.inner
            .encode_spec(&spec, &js_to_value(value)?)
            .map_err(to_js_err)
    }

    /// Portable-registry type id for a named type, or undefined.
    #[wasm_bindgen(js_name = typeIdOf)]
    pub fn type_id_of(&self, name: &str) -> Option<u32> {
        self.inner.type_id_of(name)
    }

    #[wasm_bindgen(js_name = typeNameOf)]
    pub fn type_name_of(&self, id: u32) -> Option<String> {
        self.inner.type_name_of(id).map(ToOwned::to_owned)
    }

    /// The portable registry as a JSON string (for registry-walking tooling).
    #[wasm_bindgen(js_name = registryJson)]
    pub fn registry_json(&self) -> Result<String, JsValue> {
        self.inner.registry_json().map_err(to_js_err)
    }

    // -- calls ------------------------------------------------------------------

    /// Compose a call to raw SCALE bytes. Params may embed pre-composed
    /// calls as `Uint8Array` (Sudo, batches, proxies).
    #[wasm_bindgen(js_name = composeCall)]
    pub fn compose_call(
        &self,
        module: &str,
        function: &str,
        params: &JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        self.inner
            .compose_call(module, function, &js_to_value(params)?)
            .map_err(to_js_err)
    }

    /// Decode raw call bytes into the plain call object
    /// (`call_module`/`call_function`/`call_args`/`call_hash`).
    #[wasm_bindgen(js_name = decodeCall)]
    pub fn decode_call(&self, data: &[u8]) -> Result<JsValue, JsValue> {
        let mut cursor = Cursor::new(data);
        let value = self
            .inner
            .decode_call_value(&mut cursor)
            .map_err(to_js_err)?;
        if cursor.remaining() != 0 {
            return Err(value_err(format!(
                "{} undecoded bytes remain after the call",
                cursor.remaining()
            )));
        }
        value_to_js(&value)
    }

    // -- storage ------------------------------------------------------------------

    /// Storage-item metadata, or throws NotInRuntimeError.
    #[wasm_bindgen(js_name = storageEntry)]
    pub fn storage_entry(&self, pallet: &str, storage_function: &str) -> Result<JsValue, JsValue> {
        storage_entry_js(pallet, self.entry(pallet, storage_function)?)
    }

    /// The 32-byte item prefix (`twox128(prefix) ++ twox128(name)`).
    #[wasm_bindgen(js_name = storagePrefix)]
    pub fn storage_prefix(&self, pallet: &str, storage_function: &str) -> Result<Vec<u8>, JsValue> {
        Ok(storage_prefix(self.entry(pallet, storage_function)?).to_vec())
    }

    /// The full storage key for one item (params may be a partial prefix).
    #[wasm_bindgen(js_name = storageKey)]
    pub fn storage_key(
        &self,
        pallet: &str,
        storage_function: &str,
        params: &Array,
    ) -> Result<Vec<u8>, JsValue> {
        let entry = self.entry(pallet, storage_function)?;
        let values = params
            .iter()
            .map(|p| js_to_value(&p))
            .collect::<Result<Vec<_>, _>>()?;
        self.inner.storage_key(entry, &values).map_err(to_js_err)
    }

    /// Keys for many parameter sets of one item; takes an array of
    /// parameter arrays, returns an array of `Uint8Array`s.
    #[wasm_bindgen(js_name = storageKeyBatch)]
    pub fn storage_key_batch(
        &self,
        pallet: &str,
        storage_function: &str,
        params_list: &Array,
    ) -> Result<Array, JsValue> {
        let entry = self.entry(pallet, storage_function)?;
        let out = Array::new();
        for params in params_list.iter() {
            let params: Array = params
                .dyn_into()
                .map_err(|_| value_err("paramsList must contain arrays"))?;
            let values = params
                .iter()
                .map(|p| js_to_value(&p))
                .collect::<Result<Vec<_>, _>>()?;
            let key = self.inner.storage_key(entry, &values).map_err(to_js_err)?;
            out.push(&Uint8Array::from(key.as_slice()).into());
        }
        Ok(out)
    }

    /// Recover the free map-key components from one full storage key
    /// (`fixed` leading params were part of the queried prefix).
    #[wasm_bindgen(js_name = decodeStorageKeyParams)]
    pub fn decode_storage_key_params(
        &self,
        pallet: &str,
        storage_function: &str,
        key: &[u8],
        fixed: Option<u32>,
    ) -> Result<Array, JsValue> {
        let entry = self.entry(pallet, storage_function)?;
        let values = self
            .inner
            .decode_storage_key_params(entry, key, fixed.unwrap_or(0) as usize)
            .map_err(to_js_err)?;
        let out = Array::new();
        for value in &values {
            out.push(&value_to_js(value)?);
        }
        Ok(out)
    }

    /// Decode one page of a storage map in a single crossing: recover the
    /// free key components from each full storage key and decode each value.
    /// Returns `[key, value]` pairs; a single free key yields a scalar key,
    /// multiple yield an array.
    #[wasm_bindgen(js_name = decodeMapPairs)]
    pub fn decode_map_pairs(
        &self,
        pallet: &str,
        storage_function: &str,
        raw_keys: &Array,
        raw_values: &Array,
        fixed: Option<u32>,
    ) -> Result<Array, JsValue> {
        let entry = self.entry(pallet, storage_function)?;
        let raw_keys = bytes_array(raw_keys, "rawKeys")?;
        let raw_values = bytes_array(raw_values, "rawValues")?;
        let decoded = self
            .inner
            .decode_map_page(entry, &raw_keys, &raw_values, fixed.unwrap_or(0) as usize)
            .map_err(to_js_err)?;
        materialize_pairs(&decoded)
    }

    /// Like `decodeMapPairs`, but takes the raw `state_queryStorageAt`
    /// change tuples as `[keyHex, valueHex | null]` pairs (`null` values —
    /// keys deleted between the key listing and the value fetch — are
    /// skipped).
    #[wasm_bindgen(js_name = decodeMapChanges)]
    pub fn decode_map_changes(
        &self,
        pallet: &str,
        storage_function: &str,
        changes: &Array,
        fixed: Option<u32>,
    ) -> Result<Array, JsValue> {
        let entry = self.entry(pallet, storage_function)?;
        let mut pairs = Vec::with_capacity(changes.length() as usize);
        for change in changes.iter() {
            let change: Array = change
                .dyn_into()
                .map_err(|_| value_err("changes must contain [keyHex, valueHex] pairs"))?;
            let key = change
                .get(0)
                .as_string()
                .ok_or_else(|| value_err("change keys must be hex strings"))?;
            let value = change.get(1);
            let value = if value.is_null() || value.is_undefined() {
                None
            } else {
                Some(
                    value
                        .as_string()
                        .ok_or_else(|| value_err("change values must be hex strings or null"))?,
                )
            };
            pairs.push((key, value));
        }
        let decoded = self
            .inner
            .decode_map_changes(entry, &pairs, fixed.unwrap_or(0) as usize)
            .map_err(to_js_err)?;
        materialize_pairs(&decoded)
    }

    // -- constants / errors -----------------------------------------------------

    /// Decoded value of a pallet constant, or undefined when it does not
    /// exist.
    pub fn constant(&self, module: &str, name: &str) -> Result<JsValue, JsValue> {
        let Some(constant) = self.inner.constant(module, name) else {
            return Ok(JsValue::UNDEFINED);
        };
        let mut cursor = Cursor::new(&constant.value);
        let value = self
            .inner
            .decode_id(constant.ty, &mut cursor)
            .map_err(to_js_err)?;
        value_to_js(&value)
    }

    /// `[name, docs]` for a dispatch module error.
    #[wasm_bindgen(js_name = moduleError)]
    pub fn module_error(&self, module_index: u8, error_index: u8) -> Result<Array, JsValue> {
        let (name, docs) = self
            .inner
            .module_error(module_index, error_index)
            .map_err(to_js_err)?;
        let out = Array::new();
        out.push(&JsValue::from_str(&name));
        out.push(&string_array(&docs).into());
        Ok(out)
    }

    // -- extrinsics ---------------------------------------------------------------

    /// Ordered identifiers of the runtime's signed extensions.
    #[wasm_bindgen(js_name = signedExtensionIdentifiers)]
    pub fn signed_extension_identifiers(&self) -> Vec<String> {
        self.inner
            .extrinsic
            .signed_extensions
            .iter()
            .map(|e| e.identifier.clone())
            .collect()
    }

    /// Era bytes for `"00"` or `{period: N, phase/current: M}`.
    #[wasm_bindgen(js_name = encodeEra)]
    pub fn encode_era(&self, era: &JsValue) -> Result<Vec<u8>, JsValue> {
        let mut out = Vec::new();
        self.inner
            .encode_era_value(&js_to_value(era)?, &mut out)
            .map_err(to_js_err)?;
        Ok(out)
    }

    /// The signature payload split at its wire seams:
    /// `[includedInExtrinsic, includedInSignedData]`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = signaturePayloadParts)]
    pub fn signature_payload_parts(
        &self,
        era: &JsValue,
        nonce: &JsValue,
        tip: &JsValue,
        tip_asset_id: &JsValue,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<Vec<u8>>,
    ) -> Result<Array, JsValue> {
        let params = self.tx_params(
            era,
            nonce,
            tip,
            tip_asset_id,
            genesis_hash,
            era_block_hash,
            metadata_hash,
        )?;
        let (extra, additional) = self
            .inner
            .signature_payload_parts(&params)
            .map_err(to_js_err)?;
        let out = Array::new();
        out.push(&Uint8Array::from(extra.as_slice()).into());
        out.push(&Uint8Array::from(additional.as_slice()).into());
        Ok(out)
    }

    /// The exact bytes a signer signs for the given raw call (blake2b-hashed
    /// when longer than 256 bytes, per the Substrate convention).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = signaturePayload)]
    pub fn signature_payload(
        &self,
        call_data: &[u8],
        era: &JsValue,
        nonce: &JsValue,
        tip: &JsValue,
        tip_asset_id: &JsValue,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, JsValue> {
        let params = self.tx_params(
            era,
            nonce,
            tip,
            tip_asset_id,
            genesis_hash,
            era_block_hash,
            metadata_hash,
        )?;
        self.inner
            .signature_payload(call_data, &params)
            .map_err(to_js_err)
    }

    /// Assemble the full signed extrinsic; returns `[bytes, hash]`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = encodeSignedExtrinsic)]
    pub fn encode_signed_extrinsic(
        &self,
        call_data: &[u8],
        public_key: &[u8],
        signature: &[u8],
        signature_version: u8,
        era: &JsValue,
        nonce: &JsValue,
        tip: &JsValue,
        tip_asset_id: &JsValue,
        metadata_hash_enabled: Option<bool>,
    ) -> Result<Array, JsValue> {
        let params = TxParams {
            era: js_to_value(era)?,
            nonce: u64_arg(nonce, "nonce")?,
            tip: u128_arg(tip, "tip")?,
            tip_asset_id: optional_u128_arg(tip_asset_id, "tipAssetId")?,
            // Only the "extra" section is encoded here; implied data (hashes)
            // never travels in the extrinsic.
            genesis_hash: [0; 32],
            era_block_hash: [0; 32],
            metadata_hash: metadata_hash_enabled.unwrap_or(false).then_some([0; 32]),
        };
        let (data, hash) = self
            .inner
            .encode_signed_extrinsic(
                call_data,
                h256_arg("publicKey", public_key)?,
                signature,
                signature_version,
                &params,
            )
            .map_err(to_js_err)?;
        let out = Array::new();
        out.push(&Uint8Array::from(data.as_slice()).into());
        out.push(&Uint8Array::from(hash.as_slice()).into());
        Ok(out)
    }

    /// Decode one raw extrinsic into its plain value object.
    #[wasm_bindgen(js_name = decodeExtrinsic)]
    pub fn decode_extrinsic(&self, data: &[u8], strict: Option<bool>) -> Result<JsValue, JsValue> {
        let value = self
            .inner
            .decode_extrinsic(data, strict.unwrap_or(true))
            .map_err(to_js_err)?;
        value_to_js(&value)
    }

    // -- runtime APIs / metadata IR ----------------------------------------------

    /// `{api: {method: {name, inputs: [[name, typeString]], output,
    /// docs}}}` from V15 metadata (empty for V14).
    #[wasm_bindgen(js_name = runtimeApiMap)]
    pub fn runtime_api_map(&self) -> Result<JsValue, JsValue> {
        let apis = Object::new();
        for api in &self.inner.apis {
            let methods = Object::new();
            for method in &api.methods {
                let entry = Object::new();
                set(&entry, "name", &JsValue::from_str(&method.name))?;
                let inputs = Array::new();
                for param in &method.inputs {
                    let pair = Array::new();
                    pair.push(&JsValue::from_str(&param.name));
                    pair.push(&JsValue::from_str(&format!("scale_info::{}", param.ty)));
                    inputs.push(&pair);
                }
                set(&entry, "inputs", &inputs.into())?;
                set(
                    &entry,
                    "output",
                    &JsValue::from_str(&format!("scale_info::{}", method.output)),
                )?;
                set(&entry, "docs", &string_array(&method.docs).into())?;
                set(&methods, &method.name, &entry.into())?;
            }
            set(&apis, &api.name, &methods.into())?;
        }
        Ok(apis.into())
    }

    /// The codegen IR: `{specVersion, pallets: [...], runtimeApis: [...]}`
    /// with call args/docs, indexed errors, storage and constant names.
    #[wasm_bindgen(js_name = metadataIr)]
    pub fn metadata_ir(&self) -> Result<JsValue, JsValue> {
        let join_docs = |docs: &[String]| -> String {
            docs.iter()
                .map(|d| d.trim())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        };
        let ir = Object::new();
        set(&ir, "specVersion", &JsValue::from(self.inner.spec_version))?;
        let pallets = Array::new();
        for pallet in &self.inner.pallets {
            let entry = Object::new();
            set(&entry, "name", &JsValue::from_str(&pallet.name))?;
            set(&entry, "index", &JsValue::from(pallet.index))?;
            let calls = Array::new();
            if let Some(calls_type) = pallet.calls_type {
                let ty = self.inner.resolve(calls_type).map_err(to_js_err)?;
                if let scale_info::TypeDef::Variant(variant) = &ty.type_def {
                    for call in &variant.variants {
                        let call_entry = Object::new();
                        set(&call_entry, "name", &JsValue::from_str(&call.name))?;
                        let args = string_array(
                            call.fields
                                .iter()
                                .map(|f| f.name.clone().unwrap_or_default()),
                        );
                        set(&call_entry, "args", &args.into())?;
                        set(
                            &call_entry,
                            "docs",
                            &JsValue::from_str(&join_docs(&call.docs)),
                        )?;
                        calls.push(&call_entry.into());
                    }
                }
            }
            set(&entry, "calls", &calls.into())?;
            let errors = Array::new();
            if let Some(errors_type) = pallet.errors_type {
                let ty = self.inner.resolve(errors_type).map_err(to_js_err)?;
                if let scale_info::TypeDef::Variant(variant) = &ty.type_def {
                    for error in &variant.variants {
                        let error_entry = Object::new();
                        set(&error_entry, "index", &JsValue::from(error.index))?;
                        set(&error_entry, "name", &JsValue::from_str(&error.name))?;
                        set(
                            &error_entry,
                            "docs",
                            &JsValue::from_str(&join_docs(&error.docs)),
                        )?;
                        errors.push(&error_entry.into());
                    }
                }
            }
            set(&entry, "errors", &errors.into())?;
            // Skip pseudo-entries like `:__STORAGE_VERSION__:`.
            let storage = string_array(
                pallet
                    .storage
                    .iter()
                    .filter(|s| !s.name.contains(':'))
                    .map(|s| s.name.as_str()),
            );
            set(&entry, "storage", &storage.into())?;
            let constants = string_array(pallet.constants.iter().map(|c| c.name.as_str()));
            set(&entry, "constants", &constants.into())?;
            pallets.push(&entry.into());
        }
        set(&ir, "pallets", &pallets.into())?;
        let apis = Array::new();
        for api in &self.inner.apis {
            let entry = Object::new();
            set(&entry, "name", &JsValue::from_str(&api.name))?;
            let methods = string_array(api.methods.iter().map(|m| m.name.as_str()));
            set(&entry, "methods", &methods.into())?;
            apis.push(&entry.into());
        }
        set(&ir, "runtimeApis", &apis.into())?;
        Ok(ir.into())
    }
}

// --- free functions -----------------------------------------------------------

/// The block at which a mortal era starts (its `birth`).
#[wasm_bindgen(js_name = eraBirth)]
pub fn era_birth_js(period: &JsValue, current: &JsValue) -> Result<f64, JsValue> {
    Ok(era_birth(u64_arg(period, "period")?, u64_arg(current, "current")?) as f64)
}

/// Derive the deterministic M-of-N multisig account for a signer set.
///
/// Takes raw 32-byte public keys; returns `[accountId, sortedPublicKeys]`.
#[wasm_bindgen(js_name = multisigAccountId)]
pub fn multisig_account_id_js(signatories: &Array, threshold: u16) -> Result<Array, JsValue> {
    let mut keys = Vec::with_capacity(signatories.length() as usize);
    for (i, raw) in signatories.iter().enumerate() {
        let bytes: Uint8Array = raw.dyn_into().map_err(|_| {
            value_err(format!(
                "signatory #{} must be a 32-byte Uint8Array",
                i.saturating_add(1)
            ))
        })?;
        let key: [u8; 32] = bytes.to_vec().try_into().map_err(|_| {
            value_err(format!(
                "signatory #{} must be a 32-byte public key",
                i.saturating_add(1)
            ))
        })?;
        keys.push(key);
    }
    let (account, sorted) = multisig_account_id(&keys, threshold).map_err(to_js_err)?;
    let sorted_js = Array::new();
    for key in &sorted {
        sorted_js.push(&Uint8Array::from(key.as_slice()).into());
    }
    let out = Array::new();
    out.push(&Uint8Array::from(account.as_slice()).into());
    out.push(&sorted_js);
    Ok(out)
}
