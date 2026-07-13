//! Bindings for `bittensor_core::keys` — the portable subset of the Python
//! binding's `Keypair` surface. Host-only pieces (keyfiles, NaCl sealed-box
//! encrypt/decrypt, encrypted-JSON import) are deliberately absent.

use bittensor_core::keys::{self, DEFAULT_SS58_FORMAT};
use wasm_bindgen::prelude::*;

use crate::errors::{to_js_err, value_err};

/// Crypto type codes, matching the py-substrate-interface / btwallet
/// convention (`CRYPTO_ED25519 = 0`, `CRYPTO_SR25519 = 1`).
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum CryptoType {
    Ed25519 = 0,
    Sr25519 = 1,
}

fn crypto_code(crypto_type: Option<CryptoType>) -> u8 {
    match crypto_type.unwrap_or(CryptoType::Sr25519) {
        CryptoType::Ed25519 => keys::CRYPTO_ED25519,
        CryptoType::Sr25519 => keys::CRYPTO_SR25519,
    }
}

/// Accept the same lenient message inputs the Python binding takes: a
/// Uint8Array, a `0x`-hex string, or a plain UTF-8 string.
fn coerce_message_bytes(message: &JsValue) -> Result<Vec<u8>, JsValue> {
    if let Some(text) = message.as_string() {
        if let Some(hex_part) = text.strip_prefix("0x") {
            return hex::decode(hex_part)
                .map_err(|error| value_err(format!("invalid hex message: {error}")));
        }
        return Ok(text.into_bytes());
    }
    if let Some(bytes) = message.dyn_ref::<js_sys::Uint8Array>() {
        return Ok(bytes.to_vec());
    }
    Err(value_err("message must be a string or Uint8Array"))
}

/// An sr25519 or ed25519 keypair backed by the workspace's sp-core.
#[wasm_bindgen]
pub struct Keypair {
    inner: keys::Keypair,
}

#[wasm_bindgen]
impl Keypair {
    /// Public-only keypair from an SS58 address and/or raw public key bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(
        ss58_address: Option<String>,
        public_key: Option<Vec<u8>>,
        crypto_type: Option<CryptoType>,
        ss58_format: Option<u16>,
    ) -> Result<Keypair, JsValue> {
        let inner = keys::Keypair::new(
            ss58_address.as_deref(),
            public_key.as_deref(),
            crypto_code(crypto_type),
            ss58_format.unwrap_or(DEFAULT_SS58_FORMAT),
        )
        .map_err(to_js_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a BIP39 mnemonic (with optional password).
    #[wasm_bindgen(js_name = fromMnemonic)]
    pub fn from_mnemonic(
        mnemonic: &str,
        crypto_type: Option<CryptoType>,
        password: Option<String>,
    ) -> Result<Keypair, JsValue> {
        let inner =
            keys::Keypair::from_mnemonic(mnemonic, crypto_code(crypto_type), password.as_deref())
                .map_err(to_js_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a 32-byte seed.
    #[wasm_bindgen(js_name = fromSeed)]
    pub fn from_seed(seed: &[u8], crypto_type: Option<CryptoType>) -> Result<Keypair, JsValue> {
        let inner = keys::Keypair::from_seed(seed, crypto_code(crypto_type)).map_err(to_js_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a secret URI (e.g. `//Alice` or
    /// `<mnemonic>//hard/soft`).
    #[wasm_bindgen(js_name = fromUri)]
    pub fn from_uri(uri: &str, crypto_type: Option<CryptoType>) -> Result<Keypair, JsValue> {
        let inner = keys::Keypair::from_uri(uri, crypto_code(crypto_type)).map_err(to_js_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a hex-encoded private key or seed bytes.
    #[wasm_bindgen(js_name = fromPrivateKey)]
    pub fn from_private_key(
        private_key: &str,
        crypto_type: Option<CryptoType>,
    ) -> Result<Keypair, JsValue> {
        let inner = keys::Keypair::from_private_key(private_key, crypto_code(crypto_type))
            .map_err(to_js_err)?;
        Ok(Self { inner })
    }

    /// Derive a child without exposing or reconstructing its secret URI
    /// outside Rust/WASM.
    pub fn derive(&self, path: &str) -> Result<Keypair, JsValue> {
        self.inner
            .derive(path)
            .map(|inner| Self { inner })
            .map_err(to_js_err)
    }

    /// Generate a fresh mnemonic with `nWords` (12/15/18/21/24; default 12).
    #[wasm_bindgen(js_name = generateMnemonic)]
    pub fn generate_mnemonic(n_words: Option<u32>) -> Result<String, JsValue> {
        keys::Keypair::generate_mnemonic(n_words.unwrap_or(12) as usize).map_err(to_js_err)
    }

    #[wasm_bindgen(getter, js_name = cryptoType)]
    pub fn crypto_type(&self) -> u8 {
        self.inner.crypto_type()
    }

    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        if !self.inner.has_private_key() {
            return "PublicOnly".to_owned();
        }
        match self.inner.crypto_type() {
            keys::CRYPTO_ED25519 => "Ed25519".to_owned(),
            keys::CRYPTO_SR25519 => "Sr25519".to_owned(),
            _ => "PublicOnly".to_owned(),
        }
    }

    #[wasm_bindgen(getter, js_name = publicKey)]
    pub fn public_key(&self) -> Vec<u8> {
        self.inner.public_key_bytes().to_vec()
    }

    #[wasm_bindgen(getter, js_name = ss58Address)]
    pub fn ss58_address(&self) -> String {
        self.inner.ss58_address()
    }

    #[wasm_bindgen(getter, js_name = ss58Format)]
    pub fn ss58_format(&self) -> u16 {
        self.inner.ss58_format()
    }

    /// Sign a message (Uint8Array, `0x`-hex string, or UTF-8 string);
    /// returns the raw 64-byte signature.
    pub fn sign(&self, message: &JsValue) -> Result<Vec<u8>, JsValue> {
        let message = coerce_message_bytes(message)?;
        self.inner.sign(&message).map_err(to_js_err)
    }

    /// Verify a signature. Matches btwallet's `<Bytes>` wrapping fallback.
    pub fn verify(&self, message: &JsValue, signature: &[u8]) -> Result<bool, JsValue> {
        let message = coerce_message_bytes(message)?;
        self.inner.verify(&message, signature).map_err(to_js_err)
    }
}

/// Verify a signature against an SS58 address without holding the secret key.
#[wasm_bindgen(js_name = verifySignature)]
pub fn verify_signature(
    message: &JsValue,
    signature: &[u8],
    ss58_address: &str,
    crypto_type: Option<CryptoType>,
) -> Result<bool, JsValue> {
    let message = coerce_message_bytes(message)?;
    keys::verify(&message, signature, ss58_address, crypto_code(crypto_type)).map_err(to_js_err)
}

/// Decode an SS58 address to its raw 32-byte public key.
#[wasm_bindgen(js_name = ss58Decode)]
pub fn ss58_decode(ss58_address: &str) -> Result<Vec<u8>, JsValue> {
    keys::public_key_from_ss58(ss58_address)
        .map(|pk| pk.to_vec())
        .map_err(to_js_err)
}

/// Encode a raw 32-byte public key as an SS58 address.
#[wasm_bindgen(js_name = ss58Encode)]
pub fn ss58_encode(public_key: &[u8], ss58_format: Option<u16>) -> Result<String, JsValue> {
    let public_key = <[u8; 32]>::try_from(public_key)
        .map_err(|_| value_err("public key must be exactly 32 bytes"))?;
    Ok(keys::ss58_from_public(
        public_key,
        ss58_format.unwrap_or(DEFAULT_SS58_FORMAT),
    ))
}
