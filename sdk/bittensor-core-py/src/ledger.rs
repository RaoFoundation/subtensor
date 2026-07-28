//! Bindings for `bittensor_core::signers::ledger` (feature `ledger`).

use bittensor_core::signers::ledger;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::errors::to_py_err;

/// One connected Ledger running the Polkadot generic app (USB HID).
///
/// All methods block on device I/O (and on-device user approval); call them
/// off the event loop (e.g. ``asyncio.to_thread``) from async code.
#[pyclass(name = "LedgerDevice", module = "bittensor_core")]
pub struct PyLedgerDevice {
    inner: ledger::LedgerDevice,
}

#[pymethods]
impl PyLedgerDevice {
    /// Connect to the first Ledger reachable over HID.
    ///
    /// Raises:
    ///     LedgerError: When no device is connected/unlocked or HID access
    ///         is denied.
    #[new]
    fn new() -> PyResult<Self> {
        let inner = ledger::LedgerDevice::open().map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// The generic app's version as ``(major, minor, patch)``.
    ///
    /// Also the cheapest "is the Polkadot app open?" probe.
    fn app_version(&self) -> PyResult<(u16, u16, u16)> {
        self.inner.app_version().map_err(to_py_err)
    }

    /// Derive ``m/44'/354'/account'/0'/index'`` on-device; returns
    /// ``(public_key, ss58_address)``.
    ///
    /// With ``confirm=True`` the device displays the address and waits for
    /// user approval before returning.
    #[pyo3(signature = (account=0, index=0, ss58_prefix=42, confirm=false))]
    fn address(
        &self,
        py: Python,
        account: u32,
        index: u32,
        ss58_prefix: u16,
        confirm: bool,
    ) -> PyResult<(Py<PyBytes>, String)> {
        let address = self
            .inner
            .address(account, index, ss58_prefix, confirm)
            .map_err(to_py_err)?;
        Ok((
            PyBytes::new(py, &address.public_key).into(),
            address.ss58_address,
        ))
    }

    /// Clear-sign ``payload`` (exact unhashed signature payload bytes) with
    /// the RFC-0078 ``proof`` for on-device decoding.
    ///
    /// Blocks until approved or rejected on the device. Returns the 65-byte
    /// MultiSignature (version prefix + ed25519 signature).
    #[pyo3(signature = (payload, proof, account=0, index=0))]
    fn sign(
        &self,
        py: Python,
        payload: Vec<u8>,
        proof: Vec<u8>,
        account: u32,
        index: u32,
    ) -> PyResult<Py<PyBytes>> {
        let signature = py
            .allow_threads(|| self.inner.sign(account, index, &payload, &proof))
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &signature).into())
    }
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyLedgerDevice>()?;
    Ok(())
}
