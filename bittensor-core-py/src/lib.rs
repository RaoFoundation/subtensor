//! PyO3 bindings for `bittensor-core`. No logic lives here: constructors,
//! method forwarding, error mapping, and Python-object materialization only.

use pyo3::prelude::*;

mod errors;

/// The `bittensor_core` extension module (PyPI package `bittensor-core`).
#[pymodule]
fn bittensor_core(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__core_version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
