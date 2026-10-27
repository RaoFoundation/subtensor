//! Immutable original Falcon padded profiles inside the existing signed-v4 envelope.
//!
//! This is an application protocol using original Falcon, not a claim of final
//! FN-DSA conformance. The backend, key-generation stream and encodings are pinned;
//! a future FN-DSA format requires different profile identifiers.

use alloc::{vec, vec::Vec};
use codec::{Decode, DecodeWithMemTracking, Encode, Input, MaxEncodedLen, Output};
use fn_dsa_vrfy::{DOMAIN_NONE, HASH_ID_ORIGINAL_FALCON, VerifyingKey, VerifyingKeyStandard};
use sha3::{
    Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};

pub use crate::mldsa::{
    COMMITMENT_LEN, Commitment, Descriptor, MAX_PAYLOAD_LEN, rejects_legacy_public_keys,
};
pub const VERSION: u8 = 2;
pub const ALGORITHM_512: u8 = 6;
pub const FIXED_ALGORITHM_512: u8 = 7;
pub const ALGORITHM_1024: u8 = 8;
pub const FIXED_ALGORITHM_1024: u8 = 9;
pub const PUBLIC_KEY_LEN_512: usize = 897;
pub const PUBLIC_KEY_LEN_1024: usize = 1793;
pub const SIGNATURE_LEN_512: usize = 666;
pub const SIGNATURE_LEN_1024: usize = 1280;
pub const PROOF_LEN_512: usize = 1669;
pub const PROOF_LEN_1024: usize = 3179;
pub const MAX_PUBLIC_KEY_LEN: usize = PUBLIC_KEY_LEN_1024;
pub const MAX_SIGNATURE_LEN: usize = SIGNATURE_LEN_1024;
pub const MAX_PROOF_LEN: usize = PROOF_LEN_1024;
pub const ACCOUNT_DOMAIN: &[u8] = b"bittensor/falcon/v1/account";
pub const KEY_DOMAIN: &[u8] = b"bittensor/falcon/v1/key";
pub const TRANSACTION_DOMAIN: &[u8] = b"bittensor/falcon/v1/transaction";
pub const TRANSACTION_CONTEXT: &[u8] = b"bittensor/falcon/v1/tx";
pub const CARRIER_SIGNATURE: [u8; 64] = crate::mldsa::CARRIER_SIGNATURE;
const MESSAGE_DOMAIN: &[u8] = b"bittensor/falcon/v1/message";

pub const fn is_account_profile(profile: u8) -> bool {
    matches!(
        profile,
        ALGORITHM_512 | FIXED_ALGORITHM_512 | ALGORITHM_1024 | FIXED_ALGORITHM_1024
    )
}
pub const fn is_supported_profile(profile: u8) -> bool {
    is_account_profile(profile)
}
pub const fn is_rotating_profile(profile: u8) -> bool {
    matches!(profile, ALGORITHM_512 | ALGORITHM_1024)
}
pub const fn is_fixed_profile(profile: u8) -> bool {
    matches!(profile, FIXED_ALGORITHM_512 | FIXED_ALGORITHM_1024)
}
pub const fn public_key_len(profile: u8) -> Option<usize> {
    match profile {
        ALGORITHM_512 | FIXED_ALGORITHM_512 => Some(PUBLIC_KEY_LEN_512),
        ALGORITHM_1024 | FIXED_ALGORITHM_1024 => Some(PUBLIC_KEY_LEN_1024),
        _ => None,
    }
}
pub const fn signature_len(profile: u8) -> Option<usize> {
    match profile {
        ALGORITHM_512 | FIXED_ALGORITHM_512 => Some(SIGNATURE_LEN_512),
        ALGORITHM_1024 | FIXED_ALGORITHM_1024 => Some(SIGNATURE_LEN_1024),
        _ => None,
    }
}
pub const fn proof_len(profile: u8) -> Option<usize> {
    match profile {
        ALGORITHM_512 | FIXED_ALGORITHM_512 => Some(PROOF_LEN_512),
        ALGORITHM_1024 | FIXED_ALGORITHM_1024 => Some(PROOF_LEN_1024),
        _ => None,
    }
}
pub const fn header_len(profile: u8) -> Option<usize> {
    match profile {
        ALGORITHM_512 | FIXED_ALGORITHM_512 => Some(1003),
        ALGORITHM_1024 | FIXED_ALGORITHM_1024 => Some(1899),
        _ => None,
    }
}

/// Commitments bind the exact profile as well as the canonical public key.
pub fn commitment(profile: u8, public_key: &[u8]) -> Result<Commitment, &'static str> {
    if public_key_len(profile) != Some(public_key.len()) {
        return Err("Invalid Falcon public-key profile or length");
    }
    Ok(crate::mldsa::shake256_64(&[
        KEY_DOMAIN,
        &[VERSION, profile],
        public_key,
    ]))
}

pub fn account_id(descriptor: &Descriptor) -> [u8; 32] {
    crate::mldsa::account_id(descriptor)
}

/// Raw fixed-profile bytes: vectors are internal storage, not SCALE length prefixes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub account: [u8; 32],
    pub version: u8,
    pub algorithm: u8,
    pub generation: u64,
    pub public_key: Vec<u8>,
    pub next_commitment: Commitment,
}

impl Header {
    pub fn is_supported(&self) -> bool {
        self.version == VERSION && public_key_len(self.algorithm) == Some(self.public_key.len())
    }

    pub fn is_valid_stateless(&self) -> bool {
        if !self.is_supported() || !is_fixed_profile(self.algorithm) || self.generation != 0 {
            return false;
        }
        let Ok(current) = commitment(self.algorithm, &self.public_key) else {
            return false;
        };
        let descriptor = Descriptor {
            version: VERSION,
            algorithm: self.algorithm,
            initial_commitment: current,
        };
        current != [0; COMMITMENT_LEN]
            && self.next_commitment == current
            && self.account == account_id(&descriptor)
            && rejects_legacy_public_keys(&self.account)
    }

    fn representative(&self, payload: &impl Encode) -> Option<[u8; 64]> {
        let mut output = message_hash(self.algorithm, &self.public_key, TRANSACTION_CONTEXT)?;
        output.hash.update(TRANSACTION_DOMAIN);
        // A malformed programmatically constructed header must not expand work.
        output.remaining = header_len(self.algorithm)?;
        self.encode_to(&mut output);
        if output.exhausted || output.remaining != 0 {
            return None;
        }
        output.remaining = MAX_PAYLOAD_LEN;
        payload.encode_to(&mut output);
        output.finish()
    }

    #[cfg(feature = "signing")]
    pub fn sign(
        &self,
        seed: &[u8; 32],
        payload: &[u8],
        randomness: &[u8; 32],
    ) -> Result<SignerAddress, &'static str> {
        if !self.is_supported() || (is_fixed_profile(self.algorithm) && !self.is_valid_stateless())
        {
            return Err("Invalid Falcon signing header");
        }
        let digest = self
            .representative(&RawPayload(payload))
            .ok_or("Falcon payload exceeds protocol limit")?;
        let (public_key, signature) =
            signing::sign_digest(self.algorithm, seed, &digest, randomness)?;
        if public_key != self.public_key {
            return Err("Falcon signing seed does not match public key");
        }
        Ok(SignerAddress {
            account: self.account,
            version: self.version,
            algorithm: self.algorithm,
            generation: self.generation,
            public_key,
            next_commitment: self.next_commitment,
            signature,
        })
    }
}

impl Encode for Header {
    fn size_hint(&self) -> usize {
        header_len(self.algorithm).unwrap_or(0)
    }
    fn encode_to<T: Output + ?Sized>(&self, output: &mut T) {
        self.account.encode_to(output);
        self.version.encode_to(output);
        self.algorithm.encode_to(output);
        self.generation.encode_to(output);
        output.write(&self.public_key);
        output.write(&self.next_commitment);
    }
}

impl Decode for Header {
    fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
        let account = <[u8; 32]>::decode(input)?;
        let version = u8::decode(input)?;
        let algorithm = u8::decode(input)?;
        if version != VERSION {
            return Err("Unsupported Falcon version".into());
        }
        let len = public_key_len(algorithm).ok_or("Unsupported Falcon profile")?;
        let generation = u64::decode(input)?;
        input.on_before_alloc_mem(len)?;
        let mut public_key = vec![0; len];
        input.read(&mut public_key)?;
        let next_commitment = <[u8; 64]>::decode(input)?;
        Ok(Self {
            account,
            version,
            algorithm,
            generation,
            public_key,
            next_commitment,
        })
    }
}
impl DecodeWithMemTracking for Header {}
impl MaxEncodedLen for Header {
    fn max_encoded_len() -> usize {
        1899
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignerAddress {
    pub account: [u8; 32],
    pub version: u8,
    pub algorithm: u8,
    pub generation: u64,
    pub public_key: Vec<u8>,
    pub next_commitment: Commitment,
    pub signature: Vec<u8>,
}
impl SignerAddress {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        // Inspect fixed prefix and exact length before allocation or crypto.
        let profile = *bytes.get(33).ok_or("Truncated Falcon proof")?;
        if proof_len(profile) != Some(bytes.len()) {
            return Err("Invalid Falcon proof length".into());
        }
        Self::decode(&mut &bytes[..])
    }
    pub fn header(&self) -> Header {
        Header {
            account: self.account,
            version: self.version,
            algorithm: self.algorithm,
            generation: self.generation,
            public_key: self.public_key.clone(),
            next_commitment: self.next_commitment,
        }
    }
    pub fn verify(&self, payload: &[u8]) -> bool {
        self.verify_encoded(&RawPayload(payload))
    }
    pub fn verify_encoded(&self, payload: &impl Encode) -> bool {
        if public_key_len(self.algorithm) != Some(self.public_key.len())
            || signature_len(self.algorithm) != Some(self.signature.len())
        {
            return false;
        }
        let header = self.header();
        if !header.is_supported()
            || (is_fixed_profile(self.algorithm) && !header.is_valid_stateless())
        {
            return false;
        }
        let Some(digest) = header.representative(payload) else {
            return false;
        };
        verify_digest(self.algorithm, &self.public_key, &digest, &self.signature)
    }
    pub fn encoded_len(&self) -> usize {
        proof_len(self.algorithm).unwrap_or(0)
    }
}
impl Encode for SignerAddress {
    fn size_hint(&self) -> usize {
        self.encoded_len()
    }
    fn encode_to<T: Output + ?Sized>(&self, output: &mut T) {
        // Do not clone the key just to serialize it.
        self.account.encode_to(output);
        self.version.encode_to(output);
        self.algorithm.encode_to(output);
        self.generation.encode_to(output);
        output.write(&self.public_key);
        output.write(&self.next_commitment);
        output.write(&self.signature);
    }
}
impl Decode for SignerAddress {
    fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
        let header = Header::decode(input)?;
        let len = signature_len(header.algorithm).ok_or("Unsupported Falcon signature profile")?;
        input.on_before_alloc_mem(len)?;
        let mut signature = vec![0; len];
        input.read(&mut signature)?;
        Ok(Self {
            account: header.account,
            version: header.version,
            algorithm: header.algorithm,
            generation: header.generation,
            public_key: header.public_key,
            next_commitment: header.next_commitment,
            signature,
        })
    }
}
impl DecodeWithMemTracking for SignerAddress {}
impl MaxEncodedLen for SignerAddress {
    fn max_encoded_len() -> usize {
        MAX_PROOF_LEN
    }
}

fn message_hash(profile: u8, public_key: &[u8], context: &[u8]) -> Option<HashOutput> {
    if context.len() > 255 || public_key_len(profile) != Some(public_key.len()) {
        return None;
    }
    let mut hash = Shake256::default();
    hash.update(MESSAGE_DOMAIN);
    hash.update(&[VERSION, profile]);
    // SCALE context length removes any ambiguity between context and message.
    let mut output = HashOutput {
        hash,
        remaining: 4096,
        exhausted: false,
    };
    context.encode_to(&mut output);
    output.hash.update(public_key);
    output.remaining = MAX_PAYLOAD_LEN;
    Some(output)
}
fn verify_digest(profile: u8, public_key: &[u8], digest: &[u8; 64], signature: &[u8]) -> bool {
    if public_key_len(profile) != Some(public_key.len())
        || signature_len(profile) != Some(signature.len())
    {
        return false;
    }
    let expected_header = match profile {
        ALGORITHM_512 | FIXED_ALGORITHM_512 => 9,
        ALGORITHM_1024 | FIXED_ALGORITHM_1024 => 10,
        _ => return false,
    };
    if public_key.first() != Some(&expected_header) {
        return false;
    }
    let Some(key) = VerifyingKeyStandard::decode(public_key) else {
        return false;
    };
    key.verify(signature, &DOMAIN_NONE, &HASH_ID_ORIGINAL_FALCON, digest)
}
pub fn verify_message(
    profile: u8,
    public_key: &[u8],
    message: &[u8],
    context: &[u8],
    signature: &[u8],
) -> bool {
    let Some(mut output) = message_hash(profile, public_key, context) else {
        return false;
    };
    output.write(message);
    let Some(digest) = output.finish() else {
        return false;
    };
    verify_digest(profile, public_key, &digest, signature)
}

struct RawPayload<'a>(&'a [u8]);
impl Encode for RawPayload<'_> {
    fn encode_to<T: Output + ?Sized>(&self, output: &mut T) {
        output.write(self.0);
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
impl HashOutput {
    fn finish(self) -> Option<[u8; 64]> {
        if self.exhausted {
            return None;
        }
        let mut digest = [0; 64];
        self.hash.finalize_xof().read(&mut digest);
        Some(digest)
    }
}

#[cfg(feature = "signing")]
pub use signing::{public_key_from_seed, sign_message};

#[cfg(feature = "signing")]
mod signing {
    use super::*;
    use fn_dsa_comm::{CryptoRng, RngCore, RngError, sign_key_size};
    use fn_dsa_kgen::{KeyPairGenerator, KeyPairGeneratorStandard};
    use fn_dsa_sign::{SigningKey, SigningKeyStandard};
    use zeroize::Zeroizing;

    /// Domain-separated SHAKE stream. Its exact construction is a recovery contract.
    /// The signing stream additionally contains fresh caller-supplied CSPRNG entropy.
    struct Stream(sha3::Shake256Reader);
    impl Stream {
        fn new(domain: &[u8], profile: u8, seed: &[u8; 32], binding: &[u8]) -> Self {
            let mut hash = Shake256::default();
            hash.update(domain);
            hash.update(&[VERSION, profile]);
            hash.update(seed);
            hash.update(binding);
            Self(hash.finalize_xof())
        }
    }
    impl CryptoRng for Stream {}
    impl RngCore for Stream {
        fn next_u32(&mut self) -> u32 {
            let mut b = [0; 4];
            self.fill_bytes(&mut b);
            u32::from_le_bytes(b)
        }
        fn next_u64(&mut self) -> u64 {
            let mut b = [0; 8];
            self.fill_bytes(&mut b);
            u64::from_le_bytes(b)
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            self.0.read(dest);
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    fn keypair(
        profile: u8,
        seed: &[u8; 32],
    ) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>), &'static str> {
        let logn = match profile {
            ALGORITHM_512 | FIXED_ALGORITHM_512 => 9,
            ALGORITHM_1024 | FIXED_ALGORITHM_1024 => 10,
            _ => return Err("Unsupported Falcon key profile"),
        };
        let len = public_key_len(profile).ok_or("Unsupported Falcon profile")?;
        let mut public = vec![0; len];
        let mut private = Zeroizing::new(vec![0; sign_key_size(logn)]);
        let mut rng = Stream::new(b"bittensor/falcon/v1/keygen", profile, seed, &[]);
        KeyPairGeneratorStandard::default().keygen(logn, &mut rng, &mut private, &mut public);
        Ok((public, private))
    }
    pub fn public_key_from_seed(profile: u8, seed: &[u8; 32]) -> Result<Vec<u8>, &'static str> {
        Ok(keypair(profile, seed)?.0)
    }
    pub(super) fn sign_digest(
        profile: u8,
        seed: &[u8; 32],
        digest: &[u8; 64],
        randomness: &[u8; 32],
    ) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
        let (public, private) = keypair(profile, seed)?;
        let mut key =
            SigningKeyStandard::decode(&private).ok_or("Invalid derived Falcon private key")?;
        // Include the message and private seed as defense against accidental
        // reuse of caller entropy. Production SDK callers fail closed on RNG failure.
        let binding = crate::mldsa::shake256_64(&[seed, digest, &public]);
        let mut rng = Stream::new(
            b"bittensor/falcon/v1/signing",
            profile,
            randomness,
            &binding,
        );
        let mut signature = vec![0; signature_len(profile).ok_or("Unsupported Falcon profile")?];
        key.sign(
            &mut rng,
            &DOMAIN_NONE,
            &HASH_ID_ORIGINAL_FALCON,
            digest,
            &mut signature,
        );
        // Detect signing faults before a client submits an unusable authorization.
        if !verify_digest(profile, &public, digest, &signature) {
            return Err("Falcon signature self-check failed");
        }
        Ok((public, signature))
    }
    pub fn sign_message(
        profile: u8,
        seed: &[u8; 32],
        message: &[u8],
        context: &[u8],
        randomness: &[u8; 32],
    ) -> Result<Vec<u8>, &'static str> {
        if message.len() > MAX_PAYLOAD_LEN || context.len() > 255 {
            return Err("Falcon message or context exceeds protocol limit");
        }
        let public = public_key_from_seed(profile, seed)?;
        let mut output =
            message_hash(profile, &public, context).ok_or("Unsupported Falcon profile")?;
        output.write(message);
        let digest = output
            .finish()
            .ok_or("Falcon message exceeds protocol limit")?;
        Ok(sign_digest(profile, seed, &digest, randomness)?.1)
    }
}

#[cfg(test)]
#[path = "falcon_tests.rs"]
mod tests;
