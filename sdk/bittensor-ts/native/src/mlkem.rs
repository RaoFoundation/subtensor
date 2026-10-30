use bittensor_core::mlkem;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

use crate::errors::{CoreResultExt, NapiResult};

#[napi(js_name = "mlkemSeal")]
pub fn seal(public_key: Buffer, plaintext: Buffer, include_key_hash: bool) -> NapiResult<Buffer> {
    mlkem::seal(public_key.as_ref(), plaintext.as_ref(), include_key_hash)
        .napi()
        .map(Into::into)
}

#[napi(js_name = "mlkemTwox128")]
pub fn twox_128(data: Buffer) -> Buffer {
    mlkem::twox_128(data.as_ref()).to_vec().into()
}

#[napi(js_name = "mlkemNonceLength")]
pub fn nonce_length() -> u32 {
    u32::try_from(mlkem::MLKEM_NONCE_LEN).unwrap_or(u32::MAX)
}

#[napi(js_name = "mlkemKdfId")]
pub fn kdf_id() -> Buffer {
    mlkem::KDF_ID.to_vec().into()
}
