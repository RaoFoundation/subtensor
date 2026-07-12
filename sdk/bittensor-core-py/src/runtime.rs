//! Bindings for `bittensor_core::runtime` + `codec` — the `Runtime` class the
//! SDK's codec seam is built on. No logic lives here: value materialization,
//! method forwarding, and error mapping only.

use std::sync::Arc;

use bittensor_core::codec::extrinsic::{era_birth, multisig_account_id, TxParams};
use bittensor_core::codec::{decode::Cursor, storage::storage_prefix};
use bittensor_core::runtime::{Runtime, StorageInfo};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::errors::to_py_err;
use crate::values::{materialize_pairs, py_to_value, value_to_py, value_to_py_cached, StrCache};

fn h256_arg(name: &str, raw: &[u8]) -> PyResult<[u8; 32]> {
    raw.try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err(format!("{name} must be 32 bytes")))
}

// --- StorageEntry -----------------------------------------------------------

/// Everything the SDK needs to build keys for / decode values of one storage
/// item. Type references are ``scale_info::N`` strings for this runtime.
#[pyclass(name = "StorageEntry", frozen)]
pub struct PyStorageEntry {
    #[pyo3(get)]
    pallet: String,
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    prefix: String,
    #[pyo3(get)]
    modifier: String,
    #[pyo3(get)]
    value_type: String,
    #[pyo3(get)]
    param_types: Vec<String>,
    #[pyo3(get)]
    param_hashers: Vec<String>,
    default_bytes: Vec<u8>,
}

#[pymethods]
impl PyStorageEntry {
    #[getter]
    fn default_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.default_bytes)
    }
}

fn storage_entry_py(pallet: &str, info: &StorageInfo) -> PyStorageEntry {
    PyStorageEntry {
        pallet: pallet.to_owned(),
        name: info.name.clone(),
        prefix: info.prefix.clone(),
        modifier: info.modifier.clone(),
        value_type: format!("scale_info::{}", info.value_type),
        param_types: info
            .key_types
            .iter()
            .map(|id| format!("scale_info::{id}"))
            .collect(),
        param_hashers: info.hashers.clone(),
        default_bytes: info.default_bytes.clone(),
    }
}

// --- Runtime ----------------------------------------------------------------

/// One runtime's complete metadata view and SCALE codec, parsed once from the
/// raw ``MetadataVersioned`` bytes the transport downloads and caches.
#[pyclass(name = "Runtime", frozen)]
pub struct PyRuntime {
    inner: Arc<Runtime>,
}

impl PyRuntime {
    fn entry(&self, pallet: &str, name: &str) -> PyResult<&StorageInfo> {
        self.inner.storage_entry(pallet, name).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!(
                "storage function {pallet}.{name} not found"
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn tx_params(
        &self,
        era: &Bound<'_, PyAny>,
        nonce: u64,
        tip: u128,
        tip_asset_id: Option<u128>,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<&[u8]>,
    ) -> PyResult<TxParams> {
        Ok(TxParams {
            era: py_to_value(era)?,
            nonce,
            tip,
            tip_asset_id,
            genesis_hash: h256_arg("genesis_hash", genesis_hash)?,
            era_block_hash: h256_arg("era_block_hash", era_block_hash)?,
            metadata_hash: metadata_hash
                .map(|h| h256_arg("metadata_hash", h))
                .transpose()?,
        })
    }
}

#[pymethods]
impl PyRuntime {
    /// Parse a raw ``MetadataVersioned`` blob (magic ``meta`` + version byte +
    /// V14/V15 payload).
    #[new]
    #[pyo3(signature = (metadata_bytes, spec_version, transaction_version, ss58_format=42))]
    fn new(
        metadata_bytes: Vec<u8>,
        spec_version: u32,
        transaction_version: u32,
        ss58_format: u16,
    ) -> PyResult<Self> {
        let inner = Runtime::parse(
            &metadata_bytes,
            spec_version,
            transaction_version,
            ss58_format,
        )
        .map_err(to_py_err)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    #[getter]
    fn spec_version(&self) -> u32 {
        self.inner.spec_version
    }

    #[getter]
    fn transaction_version(&self) -> u32 {
        self.inner.transaction_version
    }

    #[getter]
    fn ss58_format(&self) -> u16 {
        self.inner.ss58_format
    }

    #[getter]
    fn is_v15(&self) -> bool {
        self.inner.is_v15
    }

    #[getter]
    fn extrinsic_version(&self) -> u8 {
        self.inner.extrinsic.version
    }

    // -- generic encode/decode ------------------------------------------------

    /// Decode SCALE ``data`` as ``type_string``, returning plain Python values.
    #[pyo3(signature = (type_string, data, strict=true))]
    fn decode(
        &self,
        py: Python<'_>,
        type_string: &str,
        data: &[u8],
        strict: bool,
    ) -> PyResult<PyObject> {
        let spec = self.inner.type_spec(type_string).map_err(to_py_err)?;
        let value = self
            .inner
            .decode_spec(&spec, data, strict)
            .map_err(to_py_err)?;
        value_to_py(py, &value)
    }

    /// Bulk decode — the read-heavy hot loop. Type specs resolve once per
    /// distinct string; the SCALE work runs off the GIL (in parallel for
    /// large batches) and only Python-object materialization holds it.
    fn batch_decode(
        &self,
        py: Python<'_>,
        type_strings: Vec<String>,
        datas: Vec<Vec<u8>>,
    ) -> PyResult<Vec<PyObject>> {
        let inner = &self.inner;
        let values = py
            .allow_threads(|| inner.decode_batch(&type_strings, &datas))
            .map_err(to_py_err)?;
        let mut cache = StrCache::default();
        values
            .iter()
            .map(|v| value_to_py_cached(py, v, &mut cache))
            .collect()
    }

    /// SCALE-encode ``value`` as ``type_string``.
    fn encode<'py>(
        &self,
        py: Python<'py>,
        type_string: &str,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let spec = self.inner.type_spec(type_string).map_err(to_py_err)?;
        let encoded = self
            .inner
            .encode_spec(&spec, &py_to_value(value)?)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &encoded))
    }

    /// Portable-registry type id for a named type, or None.
    fn type_id_of(&self, name: &str) -> Option<u32> {
        self.inner.type_id_of(name)
    }

    /// The portable registry as a JSON string (for registry-walking tooling
    /// like the shape-corpus recorder).
    fn registry_json(&self) -> PyResult<String> {
        self.inner.registry_json().map_err(to_py_err)
    }

    fn type_name_of(&self, id: u32) -> Option<String> {
        self.inner.type_name_of(id).map(ToOwned::to_owned)
    }

    // -- calls ------------------------------------------------------------------

    /// Compose a call to raw SCALE bytes (the SDK's ``CallBytes``). Params may
    /// embed pre-composed calls as ``bytes`` (Sudo, batches, proxies).
    fn compose_call<'py>(
        &self,
        py: Python<'py>,
        module: &str,
        function: &str,
        params: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let data = self
            .inner
            .compose_call(module, function, &py_to_value(params)?)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &data))
    }

    /// Decode raw call bytes into the plain call dict
    /// (``call_module``/``call_function``/``call_args``/``call_hash``).
    fn decode_call(&self, py: Python<'_>, data: &[u8]) -> PyResult<PyObject> {
        let mut cursor = Cursor::new(data);
        let value = self
            .inner
            .decode_call_value(&mut cursor)
            .map_err(to_py_err)?;
        if cursor.remaining() != 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "{} undecoded bytes remain after the call",
                cursor.remaining()
            )));
        }
        value_to_py(py, &value)
    }

    // -- storage ------------------------------------------------------------------

    /// Storage-item metadata, or raises KeyError.
    fn storage_entry(&self, pallet: &str, storage_function: &str) -> PyResult<PyStorageEntry> {
        Ok(storage_entry_py(
            pallet,
            self.entry(pallet, storage_function)?,
        ))
    }

    /// The 32-byte item prefix (``twox128(prefix) ++ twox128(name)``).
    fn storage_prefix<'py>(
        &self,
        py: Python<'py>,
        pallet: &str,
        storage_function: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &storage_prefix(self.entry(pallet, storage_function)?),
        ))
    }

    /// The full storage key for one item (params may be a partial prefix).
    fn storage_key<'py>(
        &self,
        py: Python<'py>,
        pallet: &str,
        storage_function: &str,
        params: Vec<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let entry = self.entry(pallet, storage_function)?;
        let values = params
            .iter()
            .map(py_to_value)
            .collect::<PyResult<Vec<_>>>()?;
        let key = self.inner.storage_key(entry, &values).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &key))
    }

    /// Keys for many parameter sets of one item.
    fn storage_key_batch<'py>(
        &self,
        py: Python<'py>,
        pallet: &str,
        storage_function: &str,
        params_list: Vec<Vec<Bound<'py, PyAny>>>,
    ) -> PyResult<Vec<Bound<'py, PyBytes>>> {
        let entry = self.entry(pallet, storage_function)?;
        let mut out = Vec::with_capacity(params_list.len());
        for params in &params_list {
            let values = params
                .iter()
                .map(py_to_value)
                .collect::<PyResult<Vec<_>>>()?;
            let key = self.inner.storage_key(entry, &values).map_err(to_py_err)?;
            out.push(PyBytes::new(py, &key));
        }
        Ok(out)
    }

    /// Recover the free map-key components from one full storage key
    /// (``fixed`` leading params were part of the queried prefix).
    #[pyo3(signature = (pallet, storage_function, key, fixed=0))]
    fn decode_storage_key_params(
        &self,
        py: Python<'_>,
        pallet: &str,
        storage_function: &str,
        key: &[u8],
        fixed: usize,
    ) -> PyResult<Vec<PyObject>> {
        let entry = self.entry(pallet, storage_function)?;
        let values = self
            .inner
            .decode_storage_key_params(entry, key, fixed)
            .map_err(to_py_err)?;
        values.iter().map(|v| value_to_py(py, v)).collect()
    }

    /// Decode one page of a storage map in a single crossing: recover the
    /// free key components from each full storage key and decode each value.
    /// Single free key yields a scalar key, multiple yield a tuple.
    ///
    /// The SCALE + ss58 work runs off the GIL, in parallel for large pages;
    /// only Python-object materialization holds it.
    #[pyo3(signature = (pallet, storage_function, raw_keys, raw_values, fixed=0))]
    fn decode_map_pairs<'py>(
        &self,
        py: Python<'py>,
        pallet: &str,
        storage_function: &str,
        raw_keys: Vec<Vec<u8>>,
        raw_values: Vec<Vec<u8>>,
        fixed: usize,
    ) -> PyResult<Vec<(PyObject, PyObject)>> {
        let entry = self.entry(pallet, storage_function)?;
        let inner = &self.inner;
        let decoded = py
            .allow_threads(|| inner.decode_map_page(entry, &raw_keys, &raw_values, fixed))
            .map_err(to_py_err)?;
        materialize_pairs(py, &decoded)
    }

    /// Like `decode_map_pairs`, but takes the raw ``state_queryStorageAt``
    /// change tuples (``0x``-hex key/value strings; ``None`` values — keys
    /// deleted between the key listing and the value fetch — are skipped), so
    /// hex parsing also runs off the GIL and in parallel.
    #[pyo3(signature = (pallet, storage_function, changes, fixed=0))]
    fn decode_map_changes<'py>(
        &self,
        py: Python<'py>,
        pallet: &str,
        storage_function: &str,
        changes: Vec<(String, Option<String>)>,
        fixed: usize,
    ) -> PyResult<Vec<(PyObject, PyObject)>> {
        let entry = self.entry(pallet, storage_function)?;
        let inner = &self.inner;
        let decoded = py
            .allow_threads(|| inner.decode_map_changes(entry, &changes, fixed))
            .map_err(to_py_err)?;
        materialize_pairs(py, &decoded)
    }

    // -- constants / errors -----------------------------------------------------

    /// Decoded value of a pallet constant, or None when it does not exist.
    fn constant(&self, py: Python<'_>, module: &str, name: &str) -> PyResult<PyObject> {
        let Some(constant) = self.inner.constant(module, name) else {
            return Ok(py.None());
        };
        let mut cursor = Cursor::new(&constant.value);
        let value = self
            .inner
            .decode_id(constant.ty, &mut cursor)
            .map_err(to_py_err)?;
        value_to_py(py, &value)
    }

    /// ``(name, docs)`` for a dispatch module error.
    fn module_error(&self, module_index: u8, error_index: u8) -> PyResult<(String, Vec<String>)> {
        self.inner
            .module_error(module_index, error_index)
            .map_err(to_py_err)
    }

    // -- extrinsics ---------------------------------------------------------------

    /// Ordered identifiers of the runtime's signed extensions.
    fn signed_extension_identifiers(&self) -> Vec<String> {
        self.inner
            .extrinsic
            .signed_extensions
            .iter()
            .map(|e| e.identifier.clone())
            .collect()
    }

    /// Era bytes for ``"00"`` or ``{"period": N, "phase"/"current": M}``.
    fn encode_era<'py>(
        &self,
        py: Python<'py>,
        era: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut out = Vec::new();
        self.inner
            .encode_era_value(&py_to_value(era)?, &mut out)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &out))
    }

    /// The signature payload split at its wire seams:
    /// ``(included_in_extrinsic, included_in_signed_data)``.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, era, nonce, tip, tip_asset_id, genesis_hash, era_block_hash, metadata_hash=None))]
    fn signature_payload_parts<'py>(
        &self,
        py: Python<'py>,
        era: &Bound<'py, PyAny>,
        nonce: u64,
        tip: u128,
        tip_asset_id: Option<u128>,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<&[u8]>,
    ) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyBytes>)> {
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
            .map_err(to_py_err)?;
        Ok((PyBytes::new(py, &extra), PyBytes::new(py, &additional)))
    }

    /// The exact bytes a signer signs for the given raw call (blake2b-hashed
    /// when longer than 256 bytes, per the Substrate convention).
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (call_data, *, era, nonce, tip, tip_asset_id, genesis_hash, era_block_hash, metadata_hash=None))]
    fn signature_payload<'py>(
        &self,
        py: Python<'py>,
        call_data: &[u8],
        era: &Bound<'py, PyAny>,
        nonce: u64,
        tip: u128,
        tip_asset_id: Option<u128>,
        genesis_hash: &[u8],
        era_block_hash: &[u8],
        metadata_hash: Option<&[u8]>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let params = self.tx_params(
            era,
            nonce,
            tip,
            tip_asset_id,
            genesis_hash,
            era_block_hash,
            metadata_hash,
        )?;
        let payload = self
            .inner
            .signature_payload(call_data, &params)
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &payload))
    }

    /// Assemble the full signed extrinsic; returns ``(bytes, hash)``.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (call_data, *, public_key, signature, signature_version, era, nonce, tip, tip_asset_id, metadata_hash_enabled=false))]
    fn encode_signed_extrinsic<'py>(
        &self,
        py: Python<'py>,
        call_data: &[u8],
        public_key: &[u8],
        signature: &[u8],
        signature_version: u8,
        era: &Bound<'py, PyAny>,
        nonce: u64,
        tip: u128,
        tip_asset_id: Option<u128>,
        metadata_hash_enabled: bool,
    ) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyBytes>)> {
        let params = TxParams {
            era: py_to_value(era)?,
            nonce,
            tip,
            tip_asset_id,
            // Only the "extra" section is encoded here; implied data (hashes)
            // never travels in the extrinsic.
            genesis_hash: [0; 32],
            era_block_hash: [0; 32],
            metadata_hash: metadata_hash_enabled.then_some([0; 32]),
        };
        let (data, hash) = self
            .inner
            .encode_signed_extrinsic(
                call_data,
                h256_arg("public_key", public_key)?,
                signature,
                signature_version,
                &params,
            )
            .map_err(to_py_err)?;
        Ok((PyBytes::new(py, &data), PyBytes::new(py, &hash)))
    }

    /// Decode one raw extrinsic into its plain value dict.
    #[pyo3(signature = (data, strict=true))]
    fn decode_extrinsic(&self, py: Python<'_>, data: &[u8], strict: bool) -> PyResult<PyObject> {
        let value = self
            .inner
            .decode_extrinsic(data, strict)
            .map_err(to_py_err)?;
        value_to_py(py, &value)
    }

    // -- runtime APIs / metadata IR ----------------------------------------------

    /// ``{api: {method: {"inputs": [(name, type_string)], "output":
    /// type_string, "docs": [...]}}}`` from V15 metadata (empty for V14).
    fn runtime_api_map(&self, py: Python<'_>) -> PyResult<PyObject> {
        let apis = PyDict::new(py);
        for api in &self.inner.apis {
            let methods = PyDict::new(py);
            for method in &api.methods {
                let entry = PyDict::new(py);
                entry.set_item("name", &method.name)?;
                let inputs: Vec<(String, String)> = method
                    .inputs
                    .iter()
                    .map(|p| (p.name.clone(), format!("scale_info::{}", p.ty)))
                    .collect();
                entry.set_item("inputs", inputs)?;
                entry.set_item("output", format!("scale_info::{}", method.output))?;
                entry.set_item("docs", &method.docs)?;
                methods.set_item(&method.name, entry)?;
            }
            apis.set_item(&api.name, methods)?;
        }
        Ok(apis.into_any().unbind())
    }

    /// The codegen IR: ``{spec_version, pallets: [...], runtime_apis: [...]}``
    /// with call args/docs, indexed errors, storage entries (name + value
    /// type identity), and constant names.
    fn metadata_ir(&self, py: Python<'_>) -> PyResult<PyObject> {
        let join_docs = |docs: &[String]| -> String {
            docs.iter()
                .map(|d| d.trim())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        };
        let ir = PyDict::new(py);
        ir.set_item("spec_version", self.inner.spec_version)?;
        let pallets = PyList::empty(py);
        for pallet in &self.inner.pallets {
            let entry = PyDict::new(py);
            entry.set_item("name", &pallet.name)?;
            entry.set_item("index", pallet.index)?;
            let calls = PyList::empty(py);
            if let Some(calls_type) = pallet.calls_type {
                let ty = self.inner.resolve(calls_type).map_err(to_py_err)?;
                if let scale_info::TypeDef::Variant(variant) = &ty.type_def {
                    for call in &variant.variants {
                        let call_entry = PyDict::new(py);
                        call_entry.set_item("name", &call.name)?;
                        let args = PyList::empty(py);
                        for field in &call.fields {
                            let arg = PyDict::new(py);
                            arg.set_item("name", field.name.clone().unwrap_or_default())?;
                            arg.set_item("type_ident", self.inner.type_ident(field.ty.id))?;
                            args.append(arg)?;
                        }
                        call_entry.set_item("args", args)?;
                        call_entry.set_item("docs", join_docs(&call.docs))?;
                        calls.append(call_entry)?;
                    }
                }
            }
            entry.set_item("calls", calls)?;
            let errors = PyList::empty(py);
            if let Some(errors_type) = pallet.errors_type {
                let ty = self.inner.resolve(errors_type).map_err(to_py_err)?;
                if let scale_info::TypeDef::Variant(variant) = &ty.type_def {
                    for error in &variant.variants {
                        let error_entry = PyDict::new(py);
                        error_entry.set_item("index", error.index)?;
                        error_entry.set_item("name", &error.name)?;
                        error_entry.set_item("docs", join_docs(&error.docs))?;
                        errors.append(error_entry)?;
                    }
                }
            }
            entry.set_item("errors", errors)?;
            // Skip pseudo-entries like `:__STORAGE_VERSION__:`.
            let storage = PyList::empty(py);
            for item in pallet.storage.iter().filter(|s| !s.name.contains(':')) {
                let storage_entry = PyDict::new(py);
                storage_entry.set_item("name", &item.name)?;
                storage_entry
                    .set_item("value_type_ident", self.inner.type_ident(item.value_type))?;
                storage.append(storage_entry)?;
            }
            entry.set_item("storage", storage)?;
            let constants: Vec<&str> = pallet.constants.iter().map(|c| c.name.as_str()).collect();
            entry.set_item("constants", constants)?;
            pallets.append(entry)?;
        }
        ir.set_item("pallets", pallets)?;
        let apis = PyList::empty(py);
        for api in &self.inner.apis {
            let entry = PyDict::new(py);
            entry.set_item("name", &api.name)?;
            let methods: Vec<&str> = api.methods.iter().map(|m| m.name.as_str()).collect();
            entry.set_item("methods", methods)?;
            apis.append(entry)?;
        }
        ir.set_item("runtime_apis", apis)?;
        Ok(ir.into_any().unbind())
    }
}

// --- free functions -----------------------------------------------------------

/// The block at which a mortal era starts (its ``birth``).
#[pyfunction]
#[pyo3(name = "era_birth")]
fn era_birth_py(period: u64, current: u64) -> u64 {
    era_birth(period, current)
}

/// Derive the deterministic M-of-N multisig account for a signer set.
///
/// Takes raw 32-byte public keys; returns ``(account_id, sorted_public_keys)``.
#[pyfunction]
#[pyo3(name = "multisig_account_id")]
fn multisig_account_id_py<'py>(
    py: Python<'py>,
    signatories: Vec<Vec<u8>>,
    threshold: u16,
) -> PyResult<(Bound<'py, PyBytes>, Vec<Bound<'py, PyBytes>>)> {
    let keys = signatories
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            raw.as_slice().try_into().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "signatory #{} must be a 32-byte public key",
                    i.saturating_add(1)
                ))
            })
        })
        .collect::<PyResult<Vec<[u8; 32]>>>()?;
    let (account, sorted) = multisig_account_id(&keys, threshold).map_err(to_py_err)?;
    Ok((
        PyBytes::new(py, &account),
        sorted.iter().map(|k| PyBytes::new(py, k)).collect(),
    ))
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyRuntime>()?;
    module.add_class::<PyStorageEntry>()?;
    module.add_function(wrap_pyfunction!(era_birth_py, module)?)?;
    module.add_function(wrap_pyfunction!(multisig_account_id_py, module)?)?;
    Ok(())
}
