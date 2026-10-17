//! ML-DSA-65 authorization inside the unchanged signed-v4 `MultiAddress::Raw`.
//!
//! V1 encodings and domains are independent. The ordinary `MultiSignature`
//! field contains only [`CARRIER_SIGNATURE`]; it is never an authorization.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen, Output};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer, Verifier},
};
use scale_info::TypeInfo;
use sha3::{
    Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};

pub const VERSION: u8 = 2;
/// Original rotating ML-DSA-65 authorization profile; its encoding is unchanged.
pub const ALGORITHM: u8 = 4;
/// Fixed-key ML-DSA-65 account profile. The descriptor and signed header bind it.
/// Capability proofs still use ALGORITHM: their independently revocable keys
/// are scoped to their owner, purpose and epoch, not the account rotation mode.
pub const FIXED_ALGORITHM: u8 = 5;

pub fn is_account_profile(profile: u8) -> bool {
    matches!(profile, ALGORITHM | FIXED_ALGORITHM)
}
pub const PUBLIC_KEY_LEN: usize = 1952;
pub const SIGNATURE_LEN: usize = 3309;
pub const COMMITMENT_LEN: usize = 64;
pub const ACCOUNT_DOMAIN: &[u8] = b"bittensor/ms/v2/account";
pub const KEY_DOMAIN: &[u8] = b"bittensor/ms/v2/key";
pub const TRANSACTION_DOMAIN: &[u8] = b"bittensor/ms/v2/transaction";
pub const TRANSACTION_CONTEXT: &[u8] = b"bittensor/ms/v2/tx";
const _: () = assert!(TRANSACTION_CONTEXT.len() == 18);
pub const CARRIER_SIGNATURE: [u8; 64] = [0x4d; 64];
/// Protocol ceiling, in addition to the runtime's stricter admission limits.
pub const MAX_PAYLOAD_LEN: usize = 16 * 1024 * 1024;

pub type PublicKeyBytes = [u8; PUBLIC_KEY_LEN];
pub type SignatureBytes = [u8; SIGNATURE_LEN];
pub type Commitment = [u8; COMMITMENT_LEN];

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct Descriptor {
    pub version: u8,
    pub algorithm: u8,
    pub initial_commitment: Commitment,
}

impl Descriptor {
    pub const ENCODED_LEN: usize = 66;

    pub fn is_supported(&self) -> bool {
        self.version == VERSION
            && is_account_profile(self.algorithm)
            && self.initial_commitment != [0; 64]
    }

    pub fn is_rotating(&self) -> bool {
        self.algorithm == ALGORITHM
    }

    /// Rotation policy is immutable account identity, never supplied by a signer.
    pub fn accepts_next_commitment(&self, current: &Commitment, next: &Commitment) -> bool {
        self.is_supported()
            && *current != [0; 64]
            && *next != [0; 64]
            && if self.is_rotating() {
                next != current
            } else {
                current == &self.initial_commitment && next == current
            }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err("Invalid ML-DSA descriptor length".into());
        }
        let descriptor = Self::decode(&mut &bytes[..])?;
        if !descriptor.is_supported() {
            return Err("Unsupported ML-DSA descriptor".into());
        }
        Ok(descriptor)
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct Header {
    pub account: [u8; 32],
    pub version: u8,
    pub algorithm: u8,
    pub generation: u64,
    pub public_key: PublicKeyBytes,
    pub next_commitment: Commitment,
}

impl Header {
    pub const ENCODED_LEN: usize = 2058;

    pub fn is_supported(&self) -> bool {
        self.version == VERSION && is_account_profile(self.algorithm)
    }

    /// SDK signing. `payload` is the full encoded `(call, extension, implicit)`
    /// tuple, without Substrate's 256-byte shortening rule.
    /// `randomness` is fresh CSPRNG output, or zero for deterministic signing.
    pub fn sign(
        &self,
        seed: &[u8; 32],
        payload: &[u8],
        randomness: &[u8; 32],
    ) -> Result<SignerAddress, &'static str> {
        if !self.is_supported() || payload.len() > MAX_PAYLOAD_LEN {
            return Err("Unsupported ML-DSA signing header or payload size");
        }
        if public_key_from_seed(seed)? != self.public_key {
            return Err("ML-DSA signing seed does not match header public key");
        }
        // Only the SDK signing path allocates a complete transcript. Runtime
        // verification streams Encode directly through the pure-mode mu hash.
        let mut transcript = TRANSACTION_DOMAIN.to_vec();
        self.encode_to(&mut transcript);
        transcript.extend_from_slice(payload);
        let signature = sign_message_unbounded(seed, &transcript, TRANSACTION_CONTEXT, randomness)?;
        Ok(SignerAddress {
            account: self.account,
            version: self.version,
            algorithm: self.algorithm,
            generation: self.generation,
            public_key: self.public_key,
            next_commitment: self.next_commitment,
            signature,
        })
    }

    fn representative(&self, payload: &impl Encode) -> Option<[u8; 64]> {
        let tr = shake256_64(&[&self.public_key]);
        let mut hash = Shake256::default();
        hash.update(&tr);
        hash.update(&[0, 18]); // Pure mode and exact ASCII context length.
        hash.update(TRANSACTION_CONTEXT);
        hash.update(TRANSACTION_DOMAIN);
        // A header is small and fixed, but encode it directly as well.
        let mut output = HashOutput {
            hash,
            remaining: Header::ENCODED_LEN,
            exhausted: false,
        };
        self.encode_to(&mut output);
        if output.exhausted || output.remaining != 0 {
            return None;
        }
        output.remaining = MAX_PAYLOAD_LEN;
        payload.encode_to(&mut output);
        if output.exhausted {
            return None;
        }
        let mut mu = [0u8; 64];
        output.hash.finalize_xof().read(&mut mu);
        Some(mu)
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct SignerAddress {
    pub account: [u8; 32],
    pub version: u8,
    pub algorithm: u8,
    pub generation: u64,
    pub public_key: PublicKeyBytes,
    pub next_commitment: Commitment,
    pub signature: SignatureBytes,
}

impl SignerAddress {
    pub const ENCODED_LEN: usize = 5367;

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err("Invalid ML-DSA signer address length".into());
        }
        let address = Self::decode(&mut &bytes[..])?;
        if address.version != VERSION || !is_account_profile(address.algorithm) {
            return Err("Unsupported ML-DSA signer address".into());
        }
        Ok(address)
    }

    pub fn header(&self) -> Header {
        Header {
            account: self.account,
            version: self.version,
            algorithm: self.algorithm,
            generation: self.generation,
            public_key: self.public_key,
            next_commitment: self.next_commitment,
        }
    }

    /// Verify already encoded payload bytes; no Vec/slice SCALE length prefix.
    pub fn verify(&self, payload: &[u8]) -> bool {
        self.verify_encoded(&RawPayload(payload))
    }

    /// Stream the full canonical tuple through pure ML-DSA verification.
    /// The caller must perform state/fee/resource admission before invoking it.
    pub fn verify_encoded(&self, payload: &impl Encode) -> bool {
        let header = self.header();
        if !header.is_supported() {
            return false;
        }
        let Some(mu) = header.representative(payload) else {
            return false;
        };
        let Ok(public_key) = ml_dsa_65::PublicKey::try_from_bytes(self.public_key) else {
            return false;
        };
        public_key.verify_mu(&mu, &self.signature)
    }
}

/// Internal SDK authorization result. Its encoding is not a new extrinsic format.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct Proof {
    pub generation: u64,
    pub public_key: PublicKeyBytes,
    pub next_commitment: Commitment,
    pub signature: SignatureBytes,
}

pub fn commitment(public_key: &PublicKeyBytes) -> Commitment {
    shake256_64(&[KEY_DOMAIN, &[VERSION, ALGORITHM], public_key])
}

pub fn key_commitment(public_key: &PublicKeyBytes) -> Commitment {
    commitment(public_key)
}

pub fn account_id(descriptor: &Descriptor) -> [u8; 32] {
    let mut encoded = ACCOUNT_DOMAIN.to_vec();
    descriptor.encode_to(&mut encoded);
    sp_core::hashing::blake2_256(&encoded)
}

/// Derive the exact FIPS 204 public key from an independently derived key seed.
pub fn public_key_from_seed(seed: &[u8; 32]) -> Result<PublicKeyBytes, &'static str> {
    let (public_key, _secret_key) = ml_dsa_65::KG::keygen_from_seed(seed);
    Ok(public_key.into_bytes())
}

pub fn sign_message(
    seed: &[u8; 32],
    message: &[u8],
    context: &[u8],
    randomness: &[u8; 32],
) -> Result<SignatureBytes, &'static str> {
    if message.len() > MAX_PAYLOAD_LEN {
        return Err("ML-DSA message exceeds protocol size limit");
    }
    sign_message_unbounded(seed, message, context, randomness)
}

fn sign_message_unbounded(
    seed: &[u8; 32],
    message: &[u8],
    context: &[u8],
    randomness: &[u8; 32],
) -> Result<SignatureBytes, &'static str> {
    let (_public_key, secret_key) = ml_dsa_65::KG::keygen_from_seed(seed);
    secret_key.try_sign_with_seed(randomness, message, context)
}

pub fn verify_message(
    public_key: &PublicKeyBytes,
    message: &[u8],
    context: &[u8],
    signature: &SignatureBytes,
) -> bool {
    if message.len() > MAX_PAYLOAD_LEN || context.len() > 255 {
        return false;
    }
    let Ok(key) = ml_dsa_65::PublicKey::try_from_bytes(*public_key) else {
        return false;
    };
    key.verify(message, signature, context)
}

/// SHAKE256 with an exact 64-byte output; callers supply protocol domain separation.
pub fn shake256_64(parts: &[&[u8]]) -> [u8; 64] {
    let mut hash = Shake256::default();
    for part in parts {
        hash.update(part);
    }
    let mut digest = [0u8; 64];
    hash.finalize_xof().read(&mut digest);
    digest
}

struct RawPayload<'a>(&'a [u8]);

impl Encode for RawPayload<'_> {
    fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
        dest.write(self.0);
    }
}

struct HashOutput {
    hash: Shake256,
    remaining: usize,
    exhausted: bool,
}

impl Output for HashOutput {
    fn write(&mut self, bytes: &[u8]) {
        if self.exhausted {
            return;
        }
        if let Some(remaining) = self.remaining.checked_sub(bytes.len()) {
            self.remaining = remaining;
            self.hash.update(bytes);
        } else {
            self.exhausted = true;
        }
    }
}

#[cfg(test)]
#[path = "mldsa_tests.rs"]
mod tests;
