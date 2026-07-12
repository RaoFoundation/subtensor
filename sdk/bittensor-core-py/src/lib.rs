//! PyO3 bindings for `bittensor-core`. No logic lives here: constructors,
//! method forwarding, error mapping, and Python-object materialization only.
//!
//! The module exposes the union of the retired `py_sp_core` and
//! `bittensor_drand` surfaces under one name, `bittensor_core`.

use pyo3::prelude::*;

mod digest;
mod errors;
mod keys;
#[cfg(feature = "ledger")]
mod ledger;
mod runtime;
mod timelock;
mod values;

/// The `bittensor_core` extension module (PyPI package `bittensor-core`).
#[pymodule]
fn bittensor_core(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__core_version__", env!("CARGO_PKG_VERSION"))?;
    module.add("KeyfileError", py.get_type::<errors::KeyfileError>())?;
    module.add(
        "WrongPasswordError",
        py.get_type::<errors::WrongPasswordError>(),
    )?;
    module.add("LedgerError", py.get_type::<errors::LedgerError>())?;
    digest::register(module)?;
    keys::register(module)?;
    runtime::register(module)?;
    #[cfg(feature = "ledger")]
    ledger::register(module)?;
    timelock::register(module)?;
    Ok(())
}
