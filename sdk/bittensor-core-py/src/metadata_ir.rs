//! Python object materialization for the core's shared metadata IR.
//!
//! Metadata traversal, validation, filtering and identity rendering live in
//! bittensor-core. This module only converts the extracted records to dicts
//! and lists, retaining the Python code generator's existing field names.

use bittensor_core::ir::*;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

trait Materialize {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject>;
}

fn list<T: Materialize>(py: Python<'_>, items: &[T]) -> PyResult<PyObject> {
    let objects = items
        .iter()
        .map(|item| item.materialize(py))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyList::new(py, objects)?.into_any().unbind())
}

macro_rules! record {
    ($py:ident, $value:ident; $($scalar:ident),*; $($nested:ident),* $(; $($custom:ident = $object:expr),*)?) => {{
        let dict = PyDict::new($py);
        $(dict.set_item(stringify!($scalar), &$value.$scalar)?;)*
        $(dict.set_item(stringify!($nested), list($py, &$value.$nested)?)?;)*
        $($(dict.set_item(stringify!($custom), $object)?;)*)?
        Ok(dict.into_any().unbind())
    }};
}

impl Materialize for PalletIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, index, constants; calls, errors, storage, constant_details, events)
    }
}

impl Materialize for CallIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, index, docs; args)
    }
}

impl Materialize for FieldIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, type_ident, type_id;)
    }
}

impl Materialize for ErrorIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; index, name, docs;)
    }
}

impl Materialize for EventIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; index, name, docs; fields)
    }
}

impl Materialize for StorageIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, value_type_ident, prefix, modifier, hashers,
            key_type_idents, key_types, value_type; ; default_bytes = PyList::new(py, &self.default_bytes)?)
    }
}

impl Materialize for ConstantIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, type_ident, type_id, docs; ; value = PyList::new(py, &self.value)?)
    }
}

impl Materialize for RuntimeApiIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, methods, version; method_signatures)
    }
}

impl Materialize for RuntimeApiMethodIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; name, output_type_ident, output, docs; inputs)
    }
}

impl Materialize for ExtrinsicIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; version; signed_extensions)
    }
}

impl Materialize for SignedExtensionIr {
    fn materialize(&self, py: Python<'_>) -> PyResult<PyObject> {
        record!(py, self; identifier, type_ident, type_id,
            additional_signed_type_ident, additional_signed;)
    }
}

pub(crate) fn to_py(py: Python<'_>, ir: &MetadataIr) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("spec_version", ir.spec_version)?;
    dict.set_item("pallets", list(py, &ir.pallets)?)?;
    dict.set_item("runtime_apis", list(py, &ir.runtime_apis)?)?;
    dict.set_item("extrinsic", ir.extrinsic.materialize(py)?)?;
    Ok(dict.into_any().unbind())
}
