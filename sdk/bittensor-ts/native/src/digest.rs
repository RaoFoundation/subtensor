use bittensor_core::digest::{self, ChainInfo};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

use crate::errors::{CoreResultExt, NapiResult};

#[napi(object)]
pub struct NativeChainInfo {
    pub spec_version: u32,
    pub spec_name: String,
    pub base58_prefix: u16,
    pub decimals: u8,
    pub token_symbol: String,
}

impl From<NativeChainInfo> for ChainInfo {
    fn from(value: NativeChainInfo) -> Self {
        Self {
            spec_version: value.spec_version,
            spec_name: value.spec_name,
            base58_prefix: value.base58_prefix,
            decimals: value.decimals,
            token_symbol: value.token_symbol,
        }
    }
}

#[napi(js_name = "metadataDigest")]
pub fn metadata_digest(metadata_bytes: Buffer, info: NativeChainInfo) -> NapiResult<Buffer> {
    let info = ChainInfo::from(info);
    digest::metadata_digest(metadata_bytes.as_ref(), &info)
        .napi()
        .map(|value| value.to_vec().into())
}

#[napi(js_name = "generateExtrinsicProof")]
pub fn generate_extrinsic_proof(
    call_data: Buffer,
    included_in_extrinsic: Buffer,
    included_in_signed_data: Buffer,
    metadata_bytes: Buffer,
    info: NativeChainInfo,
) -> NapiResult<Buffer> {
    let info = ChainInfo::from(info);
    digest::generate_extrinsic_proof(
        call_data.as_ref(),
        included_in_extrinsic.as_ref(),
        included_in_signed_data.as_ref(),
        metadata_bytes.as_ref(),
        &info,
    )
    .napi()
    .map(Into::into)
}
