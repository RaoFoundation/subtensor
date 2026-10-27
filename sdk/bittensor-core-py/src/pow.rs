//! Thin public-challenge mining bindings; no wallet or signing operations.

use bittensor_core::pow::{self, GpuDevice, GpuMiner};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};
use std::sync::Mutex;

fn device_dict(py: Python<'_>, device: GpuDevice) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    result.set_item("id", device.id)?;
    result.set_item("name", device.name)?;
    result.set_item("vendor", device.vendor)?;
    Ok(result.unbind())
}

fn solution(py: Python<'_>, result: Option<pow::Solution>) -> Option<(u64, Py<PyBytes>)> {
    result.map(|(nonce, work)| (nonce, PyBytes::new(py, &work).unbind()))
}

/// Enumerate OpenCL GPU devices, returning an empty list when none are present.
#[pyfunction]
fn pow_gpu_devices(py: Python<'_>) -> PyResult<Vec<Py<PyDict>>> {
    let devices = py
        .allow_threads(pow::gpu_devices)
        .map_err(PyRuntimeError::new_err)?;
    devices
        .into_iter()
        .map(|device| device_dict(py, device))
        .collect()
}

/// Search one bounded nonce interval on native CPU, without holding the GIL.
#[pyfunction]
fn pow_mine_cpu(
    py: Python<'_>,
    prefix: Vec<u8>,
    difficulty: u64,
    start: u64,
    attempts: u32,
) -> PyResult<Option<(u64, Py<PyBytes>)>> {
    let result = py
        .allow_threads(|| pow::mine_cpu(&prefix, difficulty, start, attempts))
        .map_err(PyValueError::new_err)?;
    Ok(solution(py, result))
}

/// Persistent OpenCL contexts for all discovered GPUs, or selected device ids.
#[pyclass]
struct PowGpuMiner {
    inner: Mutex<GpuMiner>,
}

#[pymethods]
impl PowGpuMiner {
    #[new]
    #[pyo3(signature = (device_ids=None))]
    fn new(py: Python<'_>, device_ids: Option<Vec<usize>>) -> PyResult<Self> {
        let miner = py
            .allow_threads(|| GpuMiner::new(device_ids.as_deref()))
            .map_err(PyRuntimeError::new_err)?;
        Ok(Self {
            inner: Mutex::new(miner),
        })
    }

    fn devices(&self, py: Python<'_>) -> PyResult<Vec<Py<PyDict>>> {
        let devices = self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("GPU miner lock poisoned"))?
            .devices();
        devices
            .into_iter()
            .map(|device| device_dict(py, device))
            .collect()
    }

    /// Search disjoint bounded batches across devices. Rust verifies any proof.
    fn mine(
        &self,
        py: Python<'_>,
        prefix: Vec<u8>,
        difficulty: u64,
        start: u64,
        attempts_per_device: u32,
    ) -> PyResult<Option<(u64, Py<PyBytes>)>> {
        let result = py
            .allow_threads(|| {
                self.inner
                    .lock()
                    .map_err(|_| "GPU miner lock poisoned".to_owned())?
                    .mine(&prefix, difficulty, start, attempts_per_device)
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok(solution(py, result))
    }
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(pow_gpu_devices, module)?)?;
    module.add_function(wrap_pyfunction!(pow_mine_cpu, module)?)?;
    module.add_class::<PowGpuMiner>()?;
    Ok(())
}
