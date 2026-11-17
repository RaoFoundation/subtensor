//! Bounded mining of public registration challenges. No signing keys enter a miner.

use std::sync::{Arc, Mutex};

use bittensor_core::pow::{self, GpuDevice, GpuMiner};
use napi::bindgen_prelude::{AsyncTask, BigInt, Buffer};
use napi::{Env, Task};
use napi_derive::napi;

use crate::errors::{invalid_arg, NapiResult};

#[napi(object)]
pub struct NativePowSolution {
    pub nonce: BigInt,
    pub work: Buffer,
}

#[napi(object)]
pub struct NativePowGpuDevice {
    pub id: u32,
    pub name: String,
    pub vendor: String,
}

fn devices_to_native(devices: Vec<GpuDevice>) -> NapiResult<Vec<NativePowGpuDevice>> {
    devices
        .into_iter()
        .map(|device| {
            Ok(NativePowGpuDevice {
                id: u32::try_from(device.id)
                    .map_err(|_| invalid_arg("GPU device id exceeds u32"))?,
                name: device.name,
                vendor: device.vendor,
            })
        })
        .collect()
}

fn solution_to_native(solution: Option<pow::Solution>) -> Option<NativePowSolution> {
    solution.map(|(nonce, work)| NativePowSolution {
        nonce: nonce.into(),
        work: work.to_vec().into(),
    })
}

fn as_u64(value: &BigInt, name: &str) -> NapiResult<u64> {
    let (negative, value, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(invalid_arg(format!("{name} must be a non-negative u64")));
    }
    Ok(value)
}

#[napi(js_name = "powSeal")]
pub fn pow_seal(prefix: Buffer, nonce: BigInt) -> NapiResult<Buffer> {
    pow::seal(&prefix, as_u64(&nonce, "nonce")?)
        .map(|work| work.to_vec().into())
        .map_err(invalid_arg)
}

#[napi(js_name = "powPrefixLength")]
pub fn pow_prefix_length() -> u32 {
    pow::PREFIX_LEN as u32
}

#[napi(js_name = "powMaxBatch")]
pub fn pow_max_batch() -> u32 {
    pow::MAX_BATCH
}

pub struct PowMineTask {
    prefix: Vec<u8>,
    difficulty: u64,
    start: u64,
    attempts: u32,
    gpu: Option<Arc<Mutex<GpuMiner>>>,
}

impl Task for PowMineTask {
    type Output = Option<pow::Solution>;
    type JsValue = Option<NativePowSolution>;

    fn compute(&mut self) -> NapiResult<Self::Output> {
        match &self.gpu {
            Some(gpu) => gpu
                .lock()
                .map_err(|_| napi::Error::from_reason("GPU miner lock poisoned"))?
                .mine(&self.prefix, self.difficulty, self.start, self.attempts)
                .map_err(napi::Error::from_reason),
            None => pow::mine_cpu(&self.prefix, self.difficulty, self.start, self.attempts)
                .map_err(invalid_arg),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(solution_to_native(output))
    }
}

#[napi(js_name = "powMineCpu")]
pub fn pow_mine_cpu(
    prefix: Buffer,
    difficulty: BigInt,
    start: BigInt,
    attempts: u32,
) -> NapiResult<AsyncTask<PowMineTask>> {
    Ok(AsyncTask::new(PowMineTask {
        prefix: prefix.to_vec(),
        difficulty: as_u64(&difficulty, "difficulty")?,
        start: as_u64(&start, "start")?,
        attempts,
        gpu: None,
    }))
}

pub struct PowGpuDevicesTask;

impl Task for PowGpuDevicesTask {
    type Output = Vec<GpuDevice>;
    type JsValue = Vec<NativePowGpuDevice>;

    fn compute(&mut self) -> NapiResult<Self::Output> {
        pow::gpu_devices().map_err(napi::Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> NapiResult<Self::JsValue> {
        devices_to_native(output)
    }
}

#[napi(js_name = "powGpuDevices")]
pub fn pow_gpu_devices() -> AsyncTask<PowGpuDevicesTask> {
    AsyncTask::new(PowGpuDevicesTask)
}

#[napi]
pub struct NativePowGpuMiner {
    inner: Arc<Mutex<GpuMiner>>,
    devices: Vec<GpuDevice>,
}

pub struct PowGpuCreateTask {
    device_ids: Option<Vec<usize>>,
}

impl Task for PowGpuCreateTask {
    type Output = GpuMiner;
    type JsValue = NativePowGpuMiner;

    fn compute(&mut self) -> NapiResult<Self::Output> {
        GpuMiner::new(self.device_ids.as_deref()).map_err(napi::Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, miner: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(NativePowGpuMiner {
            devices: miner.devices(),
            inner: Arc::new(Mutex::new(miner)),
        })
    }
}

#[napi]
impl NativePowGpuMiner {
    #[napi]
    pub fn create(device_ids: Option<Vec<u32>>) -> AsyncTask<PowGpuCreateTask> {
        AsyncTask::new(PowGpuCreateTask {
            device_ids: device_ids.map(|ids| ids.into_iter().map(|id| id as usize).collect()),
        })
    }

    #[napi]
    pub fn devices(&self) -> NapiResult<Vec<NativePowGpuDevice>> {
        devices_to_native(self.devices.clone())
    }

    #[napi]
    pub fn mine(
        &self,
        prefix: Buffer,
        difficulty: BigInt,
        start: BigInt,
        attempts_per_device: u32,
    ) -> NapiResult<AsyncTask<PowMineTask>> {
        Ok(AsyncTask::new(PowMineTask {
            prefix: prefix.to_vec(),
            difficulty: as_u64(&difficulty, "difficulty")?,
            start: as_u64(&start, "start")?,
            attempts: attempts_per_device,
            gpu: Some(Arc::clone(&self.inner)),
        }))
    }
}
