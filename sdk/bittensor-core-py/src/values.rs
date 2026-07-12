//! Value <-> Python materialization — the only part of the decode path that
//! must hold the GIL. Everything upstream (SCALE, ss58, hex) runs in the
//! core off-GIL.

use bittensor_core::codec::value::{u256_decimal, Value};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt, PyList, PyString, PyTuple};

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
pub(crate) struct StrCache {
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
pub(crate) fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    value_to_py_cached(py, value, &mut StrCache::default())
}

pub(crate) fn value_to_py_cached(
    py: Python<'_>,
    value: &Value,
    cache: &mut StrCache,
) -> PyResult<PyObject> {
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
        Value::U256(le) => py.get_type::<PyInt>().call1((u256_decimal(le),))?.unbind(),
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

/// Matches the core codec's recursion ceiling: nested Python containers can
/// be built iteratively far beyond stack limits, and a stack overflow aborts
/// the process rather than raising.
const MAX_PY_VALUE_DEPTH: usize = 256;

/// Accept the lenient Python inputs the codec seam always took.
pub(crate) fn py_to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    py_to_value_at(obj, 0)
}

fn py_to_value_at(obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Value> {
    if depth > MAX_PY_VALUE_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "value nesting exceeds {MAX_PY_VALUE_DEPTH} levels"
        )));
    }
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
        if obj.downcast::<PyBytes>().is_ok() || obj.downcast::<pyo3::types::PyByteArray>().is_ok() {
            return Ok(Value::Bytes(b));
        }
    }
    let deeper = depth.saturating_add(1);
    if let Ok(list) = obj.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value_at(&item, deeper)?);
        }
        return Ok(Value::List(items));
    }
    if let Ok(tuple) = obj.downcast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple.iter() {
            items.push(py_to_value_at(&item, deeper)?);
        }
        return Ok(Value::Tuple(items));
    }
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (k, v) in dict.iter() {
            entries.push((py_to_value_at(&k, deeper)?, py_to_value_at(&v, deeper)?));
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
pub(crate) fn materialize_pairs(
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
