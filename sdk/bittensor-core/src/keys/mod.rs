//! Substrate `sp-core` key primitives (absorbed from `py-sp-core`).
//!
//! Keypair derivation, signing, verification, sealed-box encryption, and
//! SS58 codec — built against the same `sp-core` revision as the runtime, so
//! client crypto can never drift from the chain's.

// Client-side code: slicing and arithmetic on locally validated buffers is
// the norm here, and this crate never runs inside the runtime.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

#[cfg(feature = "host")]
use sodiumoxide::crypto::box_;
#[cfg(feature = "host")]
use sodiumoxide::crypto::sealedbox;
#[cfg(feature = "host")]
use sodiumoxide::crypto::sign::ed25519 as sign_ed25519;
use sp_core::crypto::{Pair as PairT, Ss58AddressFormat};
use sp_core::{ed25519, sr25519, ByteArray};
use zeroize::Zeroizing;

use crate::error::CoreError;

mod base58;
#[cfg(feature = "host")]
mod encrypted_json;

/// Crypto type codes, matching the py-substrate-interface / btwallet convention.
pub const CRYPTO_ED25519: u8 = 0;
pub const CRYPTO_SR25519: u8 = 1;

pub const DEFAULT_SS58_FORMAT: u16 = 42;
const MAX_SS58_ACCOUNT_ADDRESS_LEN: usize = 64;

fn crypto_err(msg: impl Into<String>) -> CoreError {
    CoreError::Crypto(msg.into())
}

#[cfg(feature = "host")]
pub(crate) fn ensure_sodium() -> Result<(), CoreError> {
    sodiumoxide::init().map_err(|_| crypto_err("failed to initialize libsodium"))?;
    Ok(())
}

// sp-core's CryptoBytes has several AsRef impls; pin down the byte view.
fn as_bytes<T: AsRef<[u8]>>(value: &T) -> Vec<u8> {
    value.as_ref().to_vec()
}

pub fn public_key_from_ss58(ss58_address: &str) -> Result<[u8; 32], CoreError> {
    if ss58_address.len() > MAX_SS58_ACCOUNT_ADDRESS_LEN {
        return Err(crypto_err("invalid ss58 address length"));
    }
    let decoded = base58::base58_decode(ss58_address)
        .ok_or_else(|| crypto_err("invalid ss58 address: invalid base58"))?;
    if decoded.len() != 35 && decoded.len() != 36 {
        return Err(crypto_err("invalid ss58 address length"));
    }
    let (prefix_len, ident) = match decoded[0] {
        0..=63 => (1, decoded[0] as u16),
        64..=127 => {
            let lower = (decoded[0] << 2) | (decoded[1] >> 6);
            let upper = decoded[1] & 0b0011_1111;
            (2, (lower as u16) | ((upper as u16) << 8))
        }
        _ => return Err(crypto_err("invalid ss58 address prefix")),
    };
    if Ss58AddressFormat::from(ident).is_reserved() {
        return Err(crypto_err("invalid ss58 address format"));
    }
    if decoded.len() != prefix_len + 32 + 2 {
        return Err(crypto_err("invalid ss58 account length"));
    }
    let body_end = prefix_len + 32;
    let mut checksum_input = Vec::with_capacity(7 + body_end);
    checksum_input.extend_from_slice(b"SS58PRE");
    checksum_input.extend_from_slice(&decoded[..body_end]);
    let checksum = sp_core::hashing::blake2_512(&checksum_input);
    if decoded[body_end] != checksum[0] || decoded[body_end + 1] != checksum[1] {
        return Err(crypto_err("invalid ss58 checksum"));
    }
    <[u8; 32]>::try_from(&decoded[prefix_len..body_end])
        .map_err(|_| crypto_err("invalid ss58 public key length"))
}

/// ss58 rendering, byte-identical to sp-core's `to_ss58check_with_version`
/// but ~6x faster (see `base58.rs`); every decoded account id goes through
/// here, thousands per storage-map page / metagraph payload.
pub fn ss58_from_public(public_key: [u8; 32], ss58_format: u16) -> String {
    // "SS58PRE" ++ prefix (1-2 bytes) ++ key (32) ++ checksum (2)
    let mut buf = [0u8; 7 + 2 + 32 + 2];
    buf[..7].copy_from_slice(b"SS58PRE");
    let ident = ss58_format & 0b0011_1111_1111_1111;
    let prefix_len = if ident < 64 {
        buf[7] = ident as u8;
        1
    } else {
        // Two-byte prefix encoding per the ss58 registry spec.
        buf[7] = (((ident & 0b0000_0000_1111_1100) >> 2) as u8) | 0b0100_0000;
        buf[8] = ((ident >> 8) as u8) | (((ident & 0b11) as u8) << 6);
        2
    };
    buf[7 + prefix_len..7 + prefix_len + 32].copy_from_slice(&public_key);
    let body_end = 7 + prefix_len + 32;
    let checksum = sp_core::hashing::blake2_512(&buf[..body_end]);
    buf[body_end] = checksum[0];
    buf[body_end + 1] = checksum[1];
    base58::base58_encode(&buf[7..body_end + 2])
}

fn verify_with_crypto(
    crypto_type: u8,
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, CoreError> {
    match crypto_type {
        CRYPTO_SR25519 => {
            let public = sr25519::Public::from_raw(*public_key);
            let sig = sr25519::Signature::from_slice(signature)
                .map_err(|_| crypto_err("invalid sr25519 signature length"))?;
            Ok(sr25519::Pair::verify(&sig, message, &public))
        }
        CRYPTO_ED25519 => {
            let public = ed25519::Public::from_raw(*public_key);
            let sig = ed25519::Signature::from_slice(signature)
                .map_err(|_| crypto_err("invalid ed25519 signature length"))?;
            Ok(ed25519::Pair::verify(&sig, message, &public))
        }
        other => Err(crypto_err(format!("unknown crypto type {other}"))),
    }
}

#[cfg(feature = "host")]
fn ed25519_to_x25519_pk(public_key: &[u8; 32]) -> Result<box_::PublicKey, CoreError> {
    ensure_sodium()?;
    let ed25519_pk = sign_ed25519::PublicKey::from_slice(public_key)
        .ok_or_else(|| crypto_err("invalid ed25519 public key"))?;
    sign_ed25519::to_curve25519_pk(&ed25519_pk)
        .map_err(|_| crypto_err("failed to convert ed25519 public key to x25519"))
}

#[cfg(feature = "host")]
fn ed25519_x25519_from_pair(
    pair: &ed25519::Pair,
) -> Result<(box_::PublicKey, box_::SecretKey), CoreError> {
    ensure_sodium()?;
    let seed_bytes = pair.to_raw_vec();
    let seed = sign_ed25519::Seed::from_slice(&seed_bytes)
        .or_else(|| sign_ed25519::Seed::from_slice(&seed_bytes[..32.min(seed_bytes.len())]))
        .ok_or_else(|| crypto_err("failed to derive x25519 keypair for decryption"))?;
    let (pk, sk) = sign_ed25519::keypair_from_seed(&seed);
    let x25519_pk = sign_ed25519::to_curve25519_pk(&pk)
        .map_err(|_| crypto_err("failed to derive x25519 keypair for decryption"))?;
    let x25519_sk = sign_ed25519::to_curve25519_sk(&sk)
        .map_err(|_| crypto_err("failed to derive x25519 keypair for decryption"))?;
    Ok((x25519_pk, x25519_sk))
}

// Each Keypair holds exactly one variant; the size skew between ed25519 and
// public-only is irrelevant here.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum KeypairInner {
    Ed25519(ed25519::Pair),
    Sr25519(sr25519::Pair),
    PublicOnly {
        public_key: [u8; 32],
        crypto_type: u8,
    },
}

impl KeypairInner {
    fn has_private_key(&self) -> bool {
        matches!(self, KeypairInner::Ed25519(_) | KeypairInner::Sr25519(_))
    }

    fn private_key_bytes(&self) -> Option<Vec<u8>> {
        match self {
            KeypairInner::Ed25519(pair) => Some(pair.to_raw_vec()),
            KeypairInner::Sr25519(pair) => Some(pair.to_raw_vec()),
            KeypairInner::PublicOnly { .. } => None,
        }
    }
}

#[derive(Clone)]
struct DerivationSource {
    base_uri: Zeroizing<String>,
    password: Option<Zeroizing<String>>,
}

impl DerivationSource {
    fn from_mnemonic(mnemonic: &str, password: Option<&str>) -> Self {
        Self {
            base_uri: Zeroizing::new(mnemonic.to_owned()),
            password: password.map(|value| Zeroizing::new(value.to_owned())),
        }
    }

    fn from_uri(uri: &str) -> Self {
        let Some((base_uri, password)) = uri.split_once("///") else {
            return Self {
                base_uri: Zeroizing::new(uri.to_owned()),
                password: None,
            };
        };
        Self {
            base_uri: Zeroizing::new(base_uri.to_owned()),
            password: Some(Zeroizing::new(password.to_owned())),
        }
    }

    fn child(&self, path: &str) -> Result<Self, CoreError> {
        if path.is_empty() || !path.starts_with('/') || path.contains("///") {
            return Err(crypto_err(
                "derivation path must start with '/' and must not contain a password",
            ));
        }
        let mut base_uri = String::with_capacity(self.base_uri.len() + path.len());
        base_uri.push_str(self.base_uri.as_str());
        base_uri.push_str(path);
        Ok(Self {
            base_uri: Zeroizing::new(base_uri),
            password: self
                .password
                .as_ref()
                .map(|value| Zeroizing::new(value.as_str().to_owned())),
        })
    }

    fn secret_uri(&self) -> Zeroizing<String> {
        let password_len = self.password.as_ref().map_or(0, |value| value.len() + 3);
        let mut uri = String::with_capacity(self.base_uri.len() + password_len);
        uri.push_str(self.base_uri.as_str());
        if let Some(password) = &self.password {
            uri.push_str("///");
            uri.push_str(password.as_str());
        }
        Zeroizing::new(uri)
    }
}

/// An sr25519 or ed25519 keypair backed by the workspace's sp-core.
#[derive(Clone)]
pub struct Keypair {
    inner: KeypairInner,
    ss58_format: u16,
    derivation_source: Option<DerivationSource>,
}

impl Keypair {
    /// Public-only or full keypair from SS58 address and/or raw public key bytes.
    pub fn new(
        ss58_address: Option<&str>,
        public_key: Option<&[u8]>,
        crypto_type: u8,
        ss58_format: u16,
    ) -> Result<Self, CoreError> {
        match crypto_type {
            CRYPTO_SR25519 | CRYPTO_ED25519 => {}
            other => return Err(crypto_err(format!("unknown crypto type {other}"))),
        }

        let public_key = match (ss58_address, public_key) {
            (Some(addr), None) => public_key_from_ss58(addr)?,
            (None, Some(pk)) => <[u8; 32]>::try_from(pk)
                .map_err(|_| crypto_err("public key must be exactly 32 bytes"))?,
            (Some(addr), Some(pk)) => {
                let from_addr = public_key_from_ss58(addr)?;
                let from_pk = <[u8; 32]>::try_from(pk)
                    .map_err(|_| crypto_err("public key must be exactly 32 bytes"))?;
                if from_addr != from_pk {
                    return Err(crypto_err(
                        "ss58 address and public key refer to different accounts",
                    ));
                }
                from_pk
            }
            (None, None) => {
                return Err(crypto_err(
                    "no ss58 formatted address or public key provided",
                ));
            }
        };

        Ok(Self {
            inner: KeypairInner::PublicOnly {
                public_key,
                crypto_type,
            },
            ss58_format,
            derivation_source: None,
        })
    }

    /// Derive a keypair from a BIP39 mnemonic (with optional password).
    pub fn from_mnemonic(
        mnemonic: &str,
        crypto_type: u8,
        password: Option<&str>,
    ) -> Result<Self, CoreError> {
        let inner = match crypto_type {
            CRYPTO_SR25519 => {
                let (pair, _seed) = sr25519::Pair::from_phrase(mnemonic, password)
                    .map_err(|e| crypto_err(format!("invalid mnemonic: {e:?}")))?;
                KeypairInner::Sr25519(pair)
            }
            CRYPTO_ED25519 => {
                let (pair, _seed) = ed25519::Pair::from_phrase(mnemonic, password)
                    .map_err(|e| crypto_err(format!("invalid mnemonic: {e:?}")))?;
                KeypairInner::Ed25519(pair)
            }
            other => return Err(crypto_err(format!("unknown crypto type {other}"))),
        };
        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
            derivation_source: Some(DerivationSource::from_mnemonic(mnemonic, password)),
        })
    }

    /// Derive a keypair from a 32-byte seed.
    pub fn from_seed(seed: &[u8], crypto_type: u8) -> Result<Self, CoreError> {
        let inner = match crypto_type {
            CRYPTO_SR25519 => KeypairInner::Sr25519(
                sr25519::Pair::from_seed_slice(seed)
                    .map_err(|e| crypto_err(format!("invalid seed: {e:?}")))?,
            ),
            CRYPTO_ED25519 => KeypairInner::Ed25519(
                ed25519::Pair::from_seed_slice(seed)
                    .map_err(|e| crypto_err(format!("invalid seed: {e:?}")))?,
            ),
            other => return Err(crypto_err(format!("unknown crypto type {other}"))),
        };
        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
            derivation_source: None,
        })
    }

    fn inner_from_uri(uri: &str, crypto_type: u8) -> Result<KeypairInner, CoreError> {
        match crypto_type {
            CRYPTO_SR25519 => sr25519::Pair::from_string(uri, None)
                .map(KeypairInner::Sr25519)
                .map_err(|e| crypto_err(format!("invalid secret uri: {e:?}"))),
            CRYPTO_ED25519 => ed25519::Pair::from_string(uri, None)
                .map(KeypairInner::Ed25519)
                .map_err(|e| crypto_err(format!("invalid secret uri: {e:?}"))),
            other => Err(crypto_err(format!("unknown crypto type {other}"))),
        }
    }

    fn from_derivation_source(
        source: DerivationSource,
        crypto_type: u8,
        ss58_format: u16,
    ) -> Result<Self, CoreError> {
        let secret_uri = source.secret_uri();
        let inner = Self::inner_from_uri(secret_uri.as_str(), crypto_type)?;
        Ok(Self {
            inner,
            ss58_format,
            derivation_source: Some(source),
        })
    }

    /// Derive a keypair from a secret URI (e.g. "//Alice" or "<mnemonic>//hard/soft").
    pub fn from_uri(uri: &str, crypto_type: u8) -> Result<Self, CoreError> {
        Self::from_derivation_source(
            DerivationSource::from_uri(uri),
            crypto_type,
            DEFAULT_SS58_FORMAT,
        )
    }

    /// Derive a child without exposing or reconstructing its secret URI outside Rust.
    pub fn derive(&self, path: &str) -> Result<Self, CoreError> {
        let source = self
            .derivation_source
            .as_ref()
            .ok_or_else(|| crypto_err("derivation requires a mnemonic or secret URI keypair"))?;
        Self::from_derivation_source(source.child(path)?, self.crypto_type(), self.ss58_format)
    }

    /// Derive a keypair from a hex-encoded private key or seed bytes.
    pub fn from_private_key(private_key: &str, crypto_type: u8) -> Result<Self, CoreError> {
        // Don't interpolate the hex error: it echoes offending secret characters.
        let private_key_vec = Zeroizing::new(
            hex::decode(private_key.trim_start_matches("0x"))
                .map_err(|_| crypto_err("invalid private_key hex string"))?,
        );

        let inner = match crypto_type {
            CRYPTO_SR25519 => {
                KeypairInner::Sr25519(sr25519::Pair::from_seed_slice(&private_key_vec).map_err(
                    |error| crypto_err(format!("invalid sr25519 private key: {error:?}")),
                )?)
            }
            CRYPTO_ED25519 => {
                let seed = if private_key_vec.len() >= 32 {
                    &private_key_vec[..32]
                } else {
                    return Err(crypto_err("ed25519 private key must be at least 32 bytes"));
                };
                KeypairInner::Ed25519(ed25519::Pair::from_seed_slice(seed).map_err(|error| {
                    crypto_err(format!("invalid ed25519 private key: {error:?}"))
                })?)
            }
            other => return Err(crypto_err(format!("unknown crypto type {other}"))),
        };

        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
            derivation_source: None,
        })
    }

    /// Generate a fresh mnemonic with ``n_words`` (12/15/18/21/24).
    pub fn generate_mnemonic(n_words: usize) -> Result<String, CoreError> {
        use bip39::{Language, Mnemonic};

        if !matches!(n_words, 12 | 15 | 18 | 21 | 24) {
            return Err(crypto_err(format!(
                "unsupported mnemonic length {n_words}; expected 12, 15, 18, 21, or 24"
            )));
        }

        Mnemonic::generate_in(Language::English, n_words)
            .map(|mnemonic| mnemonic.to_string())
            .map_err(|error| crypto_err(format!("failed to generate mnemonic: {error}")))
    }

    /// Import a keypair from a PolkadotJS encrypted JSON keystore (v3).
    #[cfg(feature = "host")]
    pub fn from_encrypted_json(json_data: &str, passphrase: &str) -> Result<Self, CoreError> {
        encrypted_json::create_from_encrypted_json(json_data, passphrase)
    }

    pub fn crypto_type(&self) -> u8 {
        match &self.inner {
            KeypairInner::Ed25519(_) => CRYPTO_ED25519,
            KeypairInner::Sr25519(_) => CRYPTO_SR25519,
            KeypairInner::PublicOnly { crypto_type, .. } => *crypto_type,
        }
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        match &self.inner {
            KeypairInner::Ed25519(pair) => pair.public().0,
            KeypairInner::Sr25519(pair) => pair.public().0,
            KeypairInner::PublicOnly { public_key, .. } => *public_key,
        }
    }

    pub fn ss58_address(&self) -> String {
        ss58_from_public(self.public_key_bytes(), self.ss58_format)
    }

    pub fn ss58_format(&self) -> u16 {
        self.ss58_format
    }

    pub fn has_private_key(&self) -> bool {
        self.inner.has_private_key()
    }

    pub fn private_key_bytes(&self) -> Option<Vec<u8>> {
        self.inner.private_key_bytes()
    }

    #[cfg(feature = "host")]
    pub(crate) fn from_inner(inner: KeypairInner, ss58_format: u16) -> Self {
        Self {
            inner,
            ss58_format,
            derivation_source: None,
        }
    }

    /// Sign a message; returns the raw 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, CoreError> {
        match &self.inner {
            KeypairInner::Ed25519(pair) => Ok(as_bytes(&pair.sign(message))),
            KeypairInner::Sr25519(pair) => Ok(as_bytes(&pair.sign(message))),
            KeypairInner::PublicOnly { .. } => {
                Err(crypto_err("no private key set to create signatures"))
            }
        }
    }

    /// Verify a signature. Matches btwallet's ``<Bytes>`` wrapping fallback.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<bool, CoreError> {
        let public_key = self.public_key_bytes();
        let crypto_type = self.crypto_type();
        if verify_with_crypto(crypto_type, &public_key, message, signature)? {
            return Ok(true);
        }
        let wrapped = [b"<Bytes>", message, b"</Bytes>"].concat();
        verify_with_crypto(crypto_type, &public_key, &wrapped, signature)
    }

    /// Encrypt a message to this keypair's ed25519 public key (sealed box).
    #[cfg(feature = "host")]
    pub fn encrypt(&self, message: &[u8]) -> Result<Vec<u8>, CoreError> {
        if self.crypto_type() != CRYPTO_ED25519 {
            return Err(crypto_err(
                "encrypt/decrypt is only supported for ed25519 keypairs",
            ));
        }
        ensure_sodium()?;
        let x25519_pk = ed25519_to_x25519_pk(&self.public_key_bytes())?;
        Ok(sealedbox::seal(message, &x25519_pk))
    }

    /// Decrypt a sealed-box ciphertext with this ed25519 keypair's private key.
    #[cfg(feature = "host")]
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CoreError> {
        if self.crypto_type() != CRYPTO_ED25519 {
            return Err(crypto_err(
                "encrypt/decrypt is only supported for ed25519 keypairs",
            ));
        }
        let KeypairInner::Ed25519(pair) = &self.inner else {
            return Err(crypto_err(
                "decryption requires a keypair with a private key",
            ));
        };
        ensure_sodium()?;
        let (x25519_pk, x25519_sk) = ed25519_x25519_from_pair(pair)?;
        sealedbox::open(ciphertext, &x25519_pk, &x25519_sk)
            .map_err(|_| crypto_err("decryption failed: invalid ciphertext or wrong key"))
    }

    /// Encrypt a message for a recipient SS58 address (ed25519 sealed box).
    #[cfg(feature = "host")]
    pub fn encrypt_for(
        ss58_address: &str,
        message: &[u8],
        crypto_type: u8,
    ) -> Result<Vec<u8>, CoreError> {
        let recipient = Self::new(Some(ss58_address), None, crypto_type, DEFAULT_SS58_FORMAT)?;
        recipient.encrypt(message)
    }
}

/// Verify a signature against an SS58 address without holding the secret key.
pub fn verify(
    message: &[u8],
    signature: &[u8],
    ss58_address: &str,
    crypto_type: u8,
) -> Result<bool, CoreError> {
    let public_key = public_key_from_ss58(ss58_address)?;
    if verify_with_crypto(crypto_type, &public_key, message, signature)? {
        return Ok(true);
    }
    let wrapped = [b"<Bytes>", message, b"</Bytes>"].concat();
    verify_with_crypto(crypto_type, &public_key, &wrapped, signature)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use sp_core::crypto::{AccountId32, Ss58AddressFormat, Ss58Codec};

    use super::*;

    #[test]
    fn ss58_rendering_matches_sp_core() {
        let mut state = 0x243f_6a88_85a3_08d3u64;
        for format in [0u16, 2, 42, 63, 64, 255, 4096, 16383] {
            for i in 0..50u32 {
                let mut key = [0u8; 32];
                key[..4].copy_from_slice(&i.to_le_bytes());
                for b in &mut key[4..] {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    *b = (state >> 33) as u8;
                }
                assert_eq!(
                    ss58_from_public(key, format),
                    AccountId32::from(key)
                        .to_ss58check_with_version(Ss58AddressFormat::custom(format)),
                    "diverged for format {format}"
                );
            }
        }
    }

    #[test]
    fn ss58_decoding_matches_sp_core_allowed_and_reserved_formats() {
        let mut key = [0u8; 32];
        key[..4].copy_from_slice(&42u32.to_le_bytes());
        for format in [0u16, 2, 42, 45, 46, 47, 48, 63, 64, 255, 4096, 16383] {
            let address = ss58_from_public(key, format);
            let ours = public_key_from_ss58(&address);
            let sp_core = AccountId32::from_ss58check_with_version(&address).map(|(account, _)| {
                let bytes: &[u8] = account.as_ref();
                <[u8; 32]>::try_from(bytes).unwrap()
            });
            assert_eq!(
                ours.is_ok(),
                sp_core.is_ok(),
                "decode acceptance diverged for format {format}",
            );
            if let Ok(expected) = sp_core {
                assert_eq!(
                    ours.unwrap(),
                    expected,
                    "decoded key diverged for format {format}"
                );
            }
        }
    }

    #[test]
    fn alice_uri_matches_known_address() {
        let kp = Keypair::from_uri("//Alice", CRYPTO_SR25519).unwrap();
        assert_eq!(
            kp.ss58_address(),
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
        );
    }

    #[test]
    fn password_protected_mnemonic_derives_inside_keypair() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let password = "protected-derivation-password";
        let parent = Keypair::from_mnemonic(mnemonic, CRYPTO_SR25519, Some(password)).unwrap();
        let child = parent.derive("//child").unwrap();
        let expected =
            Keypair::from_uri(&format!("{mnemonic}//child///{password}"), CRYPTO_SR25519).unwrap();
        assert_eq!(child.public_key_bytes(), expected.public_key_bytes());

        let grandchild = child.derive("//grandchild").unwrap();
        let expected = Keypair::from_uri(
            &format!("{mnemonic}//child//grandchild///{password}"),
            CRYPTO_SR25519,
        )
        .unwrap();
        assert_eq!(grandchild.public_key_bytes(), expected.public_key_bytes());
        assert!(parent.derive("///replacement-password").is_err());
    }

    #[test]
    fn public_only_from_ss58_matches_full_key() {
        let full = Keypair::from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let public = Keypair::new(Some(&full.ss58_address()), None, CRYPTO_SR25519, 42).unwrap();
        assert_eq!(public.ss58_address(), full.ss58_address());
        assert_eq!(public.public_key_bytes(), full.public_key_bytes());
    }

    #[test]
    fn public_key_from_ss58_rejects_pathological_length_before_decode() {
        let long_address = "1".repeat(MAX_SS58_ACCOUNT_ADDRESS_LEN + 1);
        let error = public_key_from_ss58(&long_address).expect_err("long ss58 must reject");
        assert!(
            error.to_string().contains("invalid ss58 address length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn sr25519_verify_roundtrip() {
        let kp = Keypair::from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let message = b"hello";
        let sig = kp.sign(message).unwrap();
        assert!(kp.verify(message, &sig).unwrap());
    }

    #[test]
    fn sr25519_verify_bytes_wrapping() {
        let kp = Keypair::from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let sig = kp.sign(b"<Bytes>hello</Bytes>").unwrap();
        assert!(kp.verify(b"hello", &sig).unwrap());
    }
}
