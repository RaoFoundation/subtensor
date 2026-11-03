use bittensor_core::signers::ledger::LedgerDevice;
use napi::bindgen_prelude::{AsyncTask, Buffer};
use napi::{Env, Task};
use napi_derive::napi;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::errors::CoreResultExt;

#[napi(object)]
pub struct NativeLedgerVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[napi(object)]
pub struct NativeLedgerAddress {
    pub public_key: Buffer,
    pub ss58_address: String,
}

type SharedLedgerDevice = Arc<Mutex<LedgerDevice>>;

#[napi]
pub struct NativeLedgerDevice {
    inner: SharedLedgerDevice,
}

pub struct OpenLedgerTask;

impl Task for OpenLedgerTask {
    type Output = LedgerDevice;
    type JsValue = NativeLedgerDevice;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        LedgerDevice::open().napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeLedgerDevice {
            inner: Arc::new(Mutex::new(output)),
        })
    }
}

pub struct LedgerAppVersionTask {
    inner: SharedLedgerDevice,
}

impl Task for LedgerAppVersionTask {
    type Output = (u16, u16, u16);
    type JsValue = NativeLedgerVersion;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        lock_ledger(&self.inner)?.app_version().napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let (major, minor, patch) = output;
        Ok(NativeLedgerVersion {
            major,
            minor,
            patch,
        })
    }
}

pub struct LedgerAddressOutput {
    public_key: Vec<u8>,
    ss58_address: String,
}

pub struct LedgerAddressTask {
    inner: SharedLedgerDevice,
    account: u32,
    index: u32,
    ss58_prefix: u16,
    confirm: bool,
}

impl Task for LedgerAddressTask {
    type Output = LedgerAddressOutput;
    type JsValue = NativeLedgerAddress;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let address = lock_ledger(&self.inner)?
            .address(self.account, self.index, self.ss58_prefix, self.confirm)
            .napi()?;
        Ok(LedgerAddressOutput {
            public_key: address.public_key.to_vec(),
            ss58_address: address.ss58_address,
        })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeLedgerAddress {
            public_key: output.public_key.into(),
            ss58_address: output.ss58_address,
        })
    }
}

pub struct LedgerSignTask {
    inner: SharedLedgerDevice,
    account: u32,
    index: u32,
    payload: Vec<u8>,
    proof: Vec<u8>,
}

impl Task for LedgerSignTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        lock_ledger(&self.inner)?
            .sign(self.account, self.index, &self.payload, &self.proof)
            .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

fn lock_ledger(inner: &SharedLedgerDevice) -> napi::Result<MutexGuard<'_, LedgerDevice>> {
    inner
        .lock()
        .map_err(|_| napi::Error::from_reason("Ledger device lock was poisoned"))
}

#[napi]
impl NativeLedgerDevice {
    #[napi]
    pub fn open() -> AsyncTask<OpenLedgerTask> {
        AsyncTask::new(OpenLedgerTask)
    }

    #[napi]
    pub fn app_version(&self) -> AsyncTask<LedgerAppVersionTask> {
        AsyncTask::new(LedgerAppVersionTask {
            inner: Arc::clone(&self.inner),
        })
    }

    #[napi]
    pub fn address(
        &self,
        account: u32,
        index: u32,
        ss58_prefix: u16,
        confirm: bool,
    ) -> AsyncTask<LedgerAddressTask> {
        AsyncTask::new(LedgerAddressTask {
            inner: Arc::clone(&self.inner),
            account,
            index,
            ss58_prefix,
            confirm,
        })
    }

    #[napi]
    pub fn sign(
        &self,
        account: u32,
        index: u32,
        payload: Buffer,
        proof: Buffer,
    ) -> AsyncTask<LedgerSignTask> {
        AsyncTask::new(LedgerSignTask {
            inner: Arc::clone(&self.inner),
            account,
            index,
            payload: payload.to_vec(),
            proof: proof.to_vec(),
        })
    }
}
