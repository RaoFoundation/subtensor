//! The one place `CoreError` becomes a Python exception.

use bittensor_core::CoreError;
use pyo3::exceptions::{PyConnectionError, PyKeyError, PyPermissionError, PyValueError};
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

pyo3::create_exception!(
    bittensor_core,
    LedgerError,
    pyo3::exceptions::PyException,
    "A hardware signing device could not be reached, rejected the request, \
     or the user declined on-device."
);

pub fn to_py_err(error: CoreError) -> PyErr {
    match error {
        CoreError::Keyfile(msg) => KeyfileError::new_err(msg),
        CoreError::WrongPassword(msg) => WrongPasswordError::new_err(msg),
        CoreError::NotInRuntime(what) => PyKeyError::new_err(what),
        CoreError::Codec(msg) | CoreError::Crypto(msg) => PyValueError::new_err(msg),
        CoreError::Device(msg) => LedgerError::new_err(msg),
        CoreError::Rpc(msg) => PyConnectionError::new_err(msg),
        CoreError::Policy(msg) => PyPermissionError::new_err(msg),
    }
}
