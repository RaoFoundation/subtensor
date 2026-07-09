//! The one place `CoreError` becomes a Python exception.

use bittensor_core::CoreError;
use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;

pyo3::create_exception!(
    bittensor_core,
    KeyfileError,
    pyo3::exceptions::PyException,
    "Keyfile data is malformed, unreadable, or uses an unknown encryption envelope."
);

pyo3::create_exception!(
    bittensor_core,
    WrongPasswordError,
    KeyfileError,
    "Keyfile decryption failed because the password is wrong."
);

pub fn to_py_err(error: CoreError) -> PyErr {
    match error {
        CoreError::Keyfile(msg) => KeyfileError::new_err(msg),
        CoreError::WrongPassword(msg) => WrongPasswordError::new_err(msg),
        CoreError::NotInRuntime(what) => PyKeyError::new_err(what),
        CoreError::Codec(msg) | CoreError::Crypto(msg) => PyValueError::new_err(msg),
    }
}
