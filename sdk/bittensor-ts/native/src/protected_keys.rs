use crate::errors::{invalid_arg, CoreResultExt, NapiResult};
use crate::keys::NativeKeypair;

use bittensor_core::keys;
use napi::bindgen_prelude::{BigInt, Buffer};
use napi_derive::napi;

fn u64_value(value: BigInt) -> NapiResult<u64> {
    let (negative, value, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(invalid_arg("value must fit u64"));
    }
    Ok(value)
}
fn array<const N: usize>(value: &[u8]) -> NapiResult<[u8; N]> {
    value
        .try_into()
        .map_err(|_| invalid_arg(format!("expected {N} bytes")))
}

#[napi]
impl NativeKeypair {
    #[napi]
    pub fn dummy_authorization(&self) -> NapiResult<Buffer> {
        keys::dummy_authorization(self.inner.crypto_type(), self.inner.public_key_bytes())
            .napi()
            .map(Into::into)
    }
    #[napi(getter)]
    pub fn account_type(&self) -> String {
        self.inner.account_type().into()
    }
    #[napi(getter)]
    pub fn signing_scheme(&self) -> String {
        self.inner.signing_scheme().into()
    }
    #[napi]
    pub fn public_only(&self) -> NapiResult<NativeKeypair> {
        Ok(NativeKeypair {
            inner: self.inner.public_only().napi()?,
        })
    }
    #[napi]
    pub fn at_generation(&self, generation: BigInt) -> NapiResult<NativeKeypair> {
        Ok(NativeKeypair {
            inner: self.inner.at_generation(u64_value(generation)?).napi()?,
        })
    }
    #[napi]
    pub fn authorization_generation(&self) -> NapiResult<BigInt> {
        self.inner
            .authorization_generation()
            .napi()
            .map(BigInt::from)
    }
    #[napi]
    pub fn authorization_descriptor(&self) -> NapiResult<Buffer> {
        self.inner.authorization_descriptor().napi().map(Into::into)
    }
    #[napi]
    pub fn authorization_public_key(&self) -> NapiResult<Buffer> {
        self.inner.authorization_public_key().napi().map(Into::into)
    }
    #[napi]
    pub fn authorization_commitment(&self, generation: BigInt) -> NapiResult<Buffer> {
        self.inner
            .authorization_commitment(u64_value(generation)?)
            .napi()
            .map(Into::into)
    }
    #[napi]
    pub fn authorization_current_commitment(&self) -> NapiResult<Buffer> {
        self.inner
            .authorization_current_commitment()
            .napi()
            .map(Into::into)
    }
    #[napi]
    pub fn authorization_next_commitment(&self) -> NapiResult<Buffer> {
        self.inner
            .authorization_next_commitment()
            .napi()
            .map(Into::into)
    }
    #[napi]
    pub fn receiving_address(&self) -> NapiResult<String> {
        self.inner.hashed_receiving_address().napi()
    }
    #[napi]
    pub fn sign_authorization(&self, payload: Buffer) -> NapiResult<Buffer> {
        self.inner
            .sign_authorization(&payload)
            .napi()
            .map(Into::into)
    }
    #[napi]
    pub fn pq_profile(&self) -> NapiResult<u8> {
        self.inner.mldsa_profile().napi()
    }
}
#[napi]
pub fn keypair_from_descriptor(descriptor: Buffer, ss58_format: u16) -> NapiResult<NativeKeypair> {
    Ok(NativeKeypair {
        inner: keys::Keypair::from_hashed_descriptor(&descriptor, ss58_format).napi()?,
    })
}
#[napi]
pub fn decode_receiving_address(address: String) -> NapiResult<Buffer> {
    keys::decode_hashed_receiving_address(&address)
        .napi()
        .map(Into::into)
}
#[napi]
pub fn encode_receiving_address(descriptor: Buffer) -> NapiResult<String> {
    keys::encode_hashed_receiving_address(&descriptor).napi()
}
#[napi]
pub fn verify_authorization(
    proof: Buffer,
    payload: Buffer,
    descriptor: Buffer,
    generation: BigInt,
    commitment: Buffer,
) -> NapiResult<bool> {
    let generation = u64_value(generation)?;
    if descriptor.first() == Some(&2) {
        keys::verify_mldsa_authorization(
            &proof,
            &payload,
            &descriptor,
            generation,
            &array(&commitment)?,
        )
        .napi()
    } else {
        keys::verify_hashed_authorization(
            &proof,
            &payload,
            &descriptor,
            generation,
            &array(&commitment)?,
        )
        .napi()
    }
}
