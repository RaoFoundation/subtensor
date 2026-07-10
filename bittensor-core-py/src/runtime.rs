//! Bindings for `bittensor_core::runtime` + `codec` — the `Runtime` class the
//! SDK's codec seam is built on. No logic lives here: value materialization,
//! method forwarding, and error mapping only.

use std::sync::Arc;

use bittensor_core::codec::extrinsic::{era_birth, multisig_account_id, TxParams};
use bittensor_core::codec::value::{u256_decimal, Value};
use bittensor_core::codec::{decode::Cursor, storage::storage_prefix};
use bittensor_core::runtime::type_string::TypeSpec;
use bittensor_core::runtime::{Runtime, StorageInfo};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt, PyList, PyString, PyTuple};
use rayon::prelude::*;

use crate::errors::to_py_err;

/// Below this many items the rayon fan-out costs more than it saves.
const PARALLEL_THRESHOLD: usize = 64;

// --- Value <-> Python -----------------------------------------------------

/// Per-call cache of repeated Python objects.
///
/// Decoded trees repeat the same field names thousands of times (one dict per
/// map entry / per collection element); reusing one `PyString` per distinct
/// key skips both the allocation and the re-hashing on dict insert. Big
/// integers beyond the CPython i64 fast path repeat too (`flags` is the
/// constant 2^127 on virtually every account). The cache lives for a single
/// binding call, so unique strings (ss58 BTreeMap keys) cannot accumulate
/// across calls.
///
/// Linear-scan `Vec`s, not `HashMap`s: a decoded shape has ~a dozen distinct
/// field names, and this sits on the hot materialization path where SipHash
/// per lookup costs more than a short scan. `get` caps the cache so a
/// pathological dict (thousands of distinct string keys) degrades to plain
/// allocation instead of quadratic scanning.
#[derive(Default)]
struct StrCache {
    strings: Vec<(String, Py<PyString>)>,
    big_uints: Vec<(u128, PyObject)>,
}

const STR_CACHE_CAP: usize = 64;

impl StrCache {
    fn get(&mut self, py: Python<'_>, s: &str) -> PyObject {
        if let Some((_, cached)) = self.strings.iter().find(|(k, _)| k == s) {
            return cached.clone_ref(py).into_any();
        }
        let obj = PyString::new(py, s).unbind();
        if self.strings.len() < STR_CACHE_CAP {
            self.strings.push((s.to_owned(), obj.clone_ref(py)));
        }
        obj.into_any()
    }

    fn get_big_uint(&mut self, py: Python<'_>, u: u128) -> PyResult<PyObject> {
        if let Some((_, cached)) = self.big_uints.iter().find(|(k, _)| *k == u) {
            return Ok(cached.clone_ref(py));
        }
        let obj: PyObject = u.into_pyobject(py)?.into_any().unbind();
        if self.big_uints.len() < STR_CACHE_CAP {
            self.big_uints.push((u, obj.clone_ref(py)));
        }
        Ok(obj)
    }
}

/// Materialize a decoded value as the exact Python objects cyscale produced.
fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    value_to_py_cached(py, value, &mut StrCache::default())
}

fn value_to_py_cached(py: Python<'_>, value: &Value, cache: &mut StrCache) -> PyResult<PyObject> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
        // 64-bit fast paths: pyo3 converts 128-bit ints through a byte-array
        // round-trip, which dominates materialization for the common small
        // values (balances, counters).
        Value::Int(i) => match i64::try_from(*i) {
            Ok(small) => small.into_pyobject(py)?.into_any().unbind(),
            Err(_) => i.into_pyobject(py)?.into_any().unbind(),
        },
        Value::Uint(u) => match u64::try_from(*u) {
            Ok(small) => small.into_pyobject(py)?.into_any().unbind(),
            Err(_) => cache.get_big_uint(py, *u)?,
        },
        Value::U256(le) => py
            .get_type::<PyInt>()
            .call1((u256_decimal(le),))?
            .unbind(),
        Value::Str(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Bytes(b) => PyBytes::new(py, b).into_any().unbind(),
        Value::List(items) => {
            let converted = items
                .iter()
                .map(|item| value_to_py_cached(py, item, cache))
                .collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, converted)?.into_any().unbind()
        }
        Value::Tuple(items) => {
            let converted = items
                .iter()
                .map(|item| value_to_py_cached(py, item, cache))
                .collect::<PyResult<Vec<_>>>()?;
            PyTuple::new(py, converted)?.into_any().unbind()
        }
        Value::Dict(entries) => {
            let dict = PyDict::new(py);
            for (k, v) in entries {
                let key = match k {
                    Value::Str(s) => cache.get(py, s),
                    other => value_to_py_cached(py, other, cache)?,
                };
                dict.set_item(key, value_to_py_cached(py, v, cache)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

/// Accept the lenient Python inputs the codec seam always took.
fn py_to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    // bool before int: Python bools are ints.
    if let Ok(b) = obj.downcast::<pyo3::types::PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.downcast::<PyInt>().is_ok() {
        if let Ok(i) = obj.extract::<i128>() {
            return Ok(Value::Int(i));
        }
        if let Ok(u) = obj.extract::<u128>() {
            return Ok(Value::Uint(u));
        }
        // Bigger than u128: U256 little-endian.
        let raw: Vec<u8> = obj
            .call_method1("to_bytes", (32usize, "little"))?
            .extract()?;
        let le: [u8; 32] = raw
            .try_into()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("integer out of u256 range"))?;
        return Ok(Value::U256(le));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(Value::Str(s));
    }
    if let Ok(b) = obj.extract::<Vec<u8>>() {
        // bytes/bytearray only — a list of ints also extracts to Vec<u8>, so
        // check the concrete type first.
        if obj.downcast::<PyBytes>().is_ok()
            || obj.downcast::<pyo3::types::PyByteArray>().is_ok()
        {
            return Ok(Value::Bytes(b));
        }
    }
    if let Ok(list) = obj.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value(&item)?);
        }
        return Ok(Value::List(items));
    }
    if let Ok(tuple) = obj.downcast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple.iter() {
            items.push(py_to_value(&item)?);
        }
        return Ok(Value::Tuple(items));
    }
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (k, v) in dict.iter() {
            entries.push((py_to_value(&k)?, py_to_value(&v)?));
        }
        return Ok(Value::Dict(entries));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(format!(
        "cannot encode Python object of type {:?} as SCALE",
        obj.get_type().name()?
    )))
}

/// Materialize decoded map pages: single free key yields a scalar key,
/// multiple yield a tuple.
fn materialize_pairs(
    py: Python<'_>,
    decoded: &[(Vec<Value>, Value)],
) -> PyResult<Vec<(PyObject, PyObject)>> {
    let mut cache = StrCache::default();
    let mut out = Vec::with_capacity(decoded.len());
    for (params, value) in decoded {
        let key_obj = if let [single] = params.as_slice() {
            value_to_py_cached(py, single, &mut cache)?
        } else {
            let parts = params
                .iter()
                .map(|p| value_to_py_cached(py, p, &mut cache))
                .collect::<PyResult<Vec<_>>>()?;
            PyTuple::new(py, parts)?.into_any().unbind()
        };
        out.push((key_obj, value_to_py_cached(py, value, &mut cache)?));
    }
    Ok(out)
}

fn h256_arg(name: &str, raw: &[u8]) -> PyResult<[u8; 32]> {
    raw.try_into().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(format!("{name} must be 32 bytes"))
    })
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
        let value = self.inner.decode_spec(&spec, data, strict).map_err(to_py_err)?;
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
        if type_strings.len() != datas.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "type_strings and datas must have the same length",
            ));
        }
        let mut specs: Vec<TypeSpec> = Vec::with_capacity(type_strings.len());
        let mut last: Option<(&str, TypeSpec)> = None;
        for type_string in &type_strings {
            let spec = match &last {
                Some((s, spec)) if *s == type_string.as_str() => spec.clone(),
                _ => {
                    let spec = self.inner.type_spec(type_string).map_err(to_py_err)?;
                    last = Some((type_string.as_str(), spec.clone()));
                    spec
                }
            };
            specs.push(spec);
        }
        let inner = &self.inner;
        let values = py
            .allow_threads(|| {
                if datas.len() >= PARALLEL_THRESHOLD {
                    specs
                        .par_iter()
                        .zip(datas.par_iter())
                        .map(|(spec, data)| inner.decode_spec(spec, data, true))
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    specs
                        .iter()
                        .zip(&datas)
                        .map(|(spec, data)| inner.decode_spec(spec, data, true))
                        .collect()
                }
            })
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
        let value = self.inner.decode_call_value(&mut cursor).map_err(to_py_err)?;
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
        Ok(storage_entry_py(pallet, self.entry(pallet, storage_function)?))
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
        if raw_keys.len() != raw_values.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "raw_keys and raw_values must have the same length",
            ));
        }
        let entry = self.entry(pallet, storage_function)?;
        let value_spec = TypeSpec::Id(entry.value_type);
        let inner = &self.inner;
        let decode_one = |raw_key: &Vec<u8>, raw_value: &Vec<u8>| {
            let params = inner.decode_storage_key_params(entry, raw_key, fixed)?;
            let value = inner.decode_spec(&value_spec, raw_value, true)?;
            Ok((params, value))
        };
        let decoded = py
            .allow_threads(|| {
                if raw_keys.len() >= PARALLEL_THRESHOLD {
                    raw_keys
                        .par_iter()
                        .zip(raw_values.par_iter())
                        .map(|(k, v)| decode_one(k, v))
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    raw_keys
                        .iter()
                        .zip(&raw_values)
                        .map(|(k, v)| decode_one(k, v))
                        .collect()
                }
            })
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
        let value_spec = TypeSpec::Id(entry.value_type);
        let inner = &self.inner;
        let unhex = |s: &str| {
            hex::decode(s.trim_start_matches("0x"))
                .map_err(|e| bittensor_core::CoreError::Codec(format!("bad hex in changes: {e}")))
        };
        let decode_one = |key_hex: &str, value_hex: &str| {
            let raw_key = unhex(key_hex)?;
            let raw_value = unhex(value_hex)?;
            let params = inner.decode_storage_key_params(entry, &raw_key, fixed)?;
            let value = inner.decode_spec(&value_spec, &raw_value, true)?;
            Ok((params, value))
        };
        let present: Vec<(&str, &str)> = changes
            .iter()
            .filter_map(|(k, v)| v.as_deref().map(|v| (k.as_str(), v)))
            .collect();
        let decoded = py
            .allow_threads(|| {
                if present.len() >= PARALLEL_THRESHOLD {
                    present
                        .par_iter()
                        .map(|(k, v)| decode_one(k, v))
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    present.iter().map(|(k, v)| decode_one(k, v)).collect()
                }
            })
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
        let value = self.inner.decode_extrinsic(data, strict).map_err(to_py_err)?;
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
    /// with call args/docs, indexed errors, storage and constant names.
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
                        let args: Vec<String> = call
                            .fields
                            .iter()
                            .map(|f| f.name.clone().unwrap_or_default())
                            .collect();
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
            let storage: Vec<&str> = pallet
                .storage
                .iter()
                .filter(|s| !s.name.contains(':'))
                .map(|s| s.name.as_str())
                .collect();
            entry.set_item("storage", storage)?;
            let constants: Vec<&str> =
                pallet.constants.iter().map(|c| c.name.as_str()).collect();
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
