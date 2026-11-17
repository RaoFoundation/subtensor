use crate::errors::{to_js_err, value_err};
use crate::keys::Keypair;
use bittensor_core::keys;
use wasm_bindgen::prelude::*;

fn array<const N: usize>(value: &[u8]) -> Result<[u8; N], JsValue> {
    value
        .try_into()
        .map_err(|_| value_err(format!("expected {N} bytes")))
}
#[wasm_bindgen]
impl Keypair {
    #[wasm_bindgen(js_name = dummyAuthorization)]
    pub fn dummy_authorization(&self) -> Result<Vec<u8>, JsValue> {
        keys::dummy_authorization(self.inner.crypto_type(), self.inner.public_key_bytes())
            .map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = publicOnly)]
    pub fn public_only(&self) -> Result<Keypair, JsValue> {
        Ok(Keypair {
            inner: self.inner.public_only().map_err(to_js_err)?,
        })
    }
    #[wasm_bindgen(js_name = authorizationGeneration)]
    pub fn authorization_generation(&self) -> Result<u64, JsValue> {
        self.inner.authorization_generation().map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = authorizationDescriptor)]
    pub fn authorization_descriptor(&self) -> Result<Vec<u8>, JsValue> {
        self.inner.authorization_descriptor().map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = authorizationPublicKey)]
    pub fn authorization_public_key(&self) -> Result<Vec<u8>, JsValue> {
        self.inner.authorization_public_key().map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = authorizationCommitment)]
    pub fn authorization_commitment(&self, generation: u64) -> Result<Vec<u8>, JsValue> {
        self.inner
            .authorization_commitment(generation)
            .map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = authorizationCurrentCommitment)]
    pub fn authorization_current_commitment(&self) -> Result<Vec<u8>, JsValue> {
        self.inner
            .authorization_current_commitment()
            .map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = authorizationNextCommitment)]
    pub fn authorization_next_commitment(&self) -> Result<Vec<u8>, JsValue> {
        self.inner
            .authorization_next_commitment()
            .map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = receivingAddress)]
    pub fn receiving_address(&self) -> Result<String, JsValue> {
        self.inner.hashed_receiving_address().map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = signAuthorization)]
    pub fn sign_authorization(&self, payload: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.sign_authorization(payload).map_err(to_js_err)
    }
    #[wasm_bindgen(js_name = pqProfile)]
    pub fn pq_profile(&self) -> Result<u8, JsValue> {
        self.inner.mldsa_profile().map_err(to_js_err)
    }
}
#[wasm_bindgen(js_name = decodeReceivingAddress)]
pub fn decode_receiving_address(address: &str) -> Result<Vec<u8>, JsValue> {
    keys::decode_hashed_receiving_address(address).map_err(to_js_err)
}
#[wasm_bindgen(js_name = encodeReceivingAddress)]
pub fn encode_receiving_address(descriptor: &[u8]) -> Result<String, JsValue> {
    keys::encode_hashed_receiving_address(descriptor).map_err(to_js_err)
}
#[wasm_bindgen(js_name = verifyAuthorization)]
pub fn verify_authorization(
    proof: &[u8],
    payload: &[u8],
    descriptor: &[u8],
    generation: u64,
    commitment: &[u8],
) -> Result<bool, JsValue> {
    if descriptor.first() == Some(&2) {
        keys::verify_mldsa_authorization(
            proof,
            payload,
            descriptor,
            generation,
            &array(commitment)?,
        )
        .map_err(to_js_err)
    } else {
        keys::verify_hashed_authorization(
            proof,
            payload,
            descriptor,
            generation,
            &array(commitment)?,
        )
        .map_err(to_js_err)
    }
}
