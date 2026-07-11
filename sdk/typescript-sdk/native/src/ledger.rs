use bittensor_core::signers::ledger::LedgerDevice;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

use crate::errors::{CoreResultExt, NapiResult};

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

#[napi]
pub struct NativeLedgerDevice {
    inner: LedgerDevice,
}

#[napi]
impl NativeLedgerDevice {
    #[napi(factory)]
    pub fn open() -> napi::Result<Self> {
        LedgerDevice::open().napi().map(|inner| Self { inner })
    }

    #[napi]
    pub fn app_version(&self) -> NapiResult<NativeLedgerVersion> {
        let (major, minor, patch) = self.inner.app_version().napi()?;
        Ok(NativeLedgerVersion {
            major,
            minor,
            patch,
        })
    }

    #[napi]
    pub fn address(
        &self,
        account: u32,
        index: u32,
        ss58_prefix: u16,
        confirm: bool,
    ) -> NapiResult<NativeLedgerAddress> {
        let address = self
            .inner
            .address(account, index, ss58_prefix, confirm)
            .napi()?;
        Ok(NativeLedgerAddress {
            public_key: address.public_key.to_vec().into(),
            ss58_address: address.ss58_address,
        })
    }

    #[napi]
    pub fn sign(
        &self,
        account: u32,
        index: u32,
        payload: Buffer,
        proof: Buffer,
    ) -> NapiResult<Buffer> {
        self.inner
            .sign(account, index, payload.as_ref(), proof.as_ref())
            .napi()
            .map(Into::into)
    }
}
