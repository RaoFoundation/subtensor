//! # Aggregate BLS signature library with extensive tuning options.
//!
//! In short, anyone using BLS signatures should normally choose both
//! an orientation as well as some aggregation and batching strategies
//! These two decissions impact performance dramaticaly, but making
//! the optimal choises requires some attentiom.  This crate employs
//! convenient abstraction boundaries between curver arithmatic,
//! verifier routines, and aggregated and/or batched BLS signatures.
//!
//! ### Pairings
//!
//! If we have two elliptic curve with a pairing `e`, then
//! a BLS signature `sigma = s*H(msg)` by a public key `S = s g1`
//! can be verified with the one equation `e(g1,sigma) = e(S,H(msg))`.
//! These simple BLS signatures are very slow to verify however
//! because the pairing map `e` is far slower than many cryptographic
//! primitives.
//!
//! Our pairing `e` maps from a small curve over `F(q)` and a larger
//! curve over `F(q^2)` into some multipliccative group if a field,
//! normally over `F(q^12)`.  In principle, this map `e` into `F(q^12)`
//! makes pairing based cryptography like BLS less secure than
//! other elliptic curve based cryptography, which further slows down
//! BLS signatures by requiring larger `q`.
//!
//! ### Arithmatic
//!
//! An almost universally applicable otimization is to seperate the
//! "Miller loop" that computes in `F(q)` and `F(q^2)` from the slow
//! final exponentiation that happens in `F(q^12)`.  So our actual
//! verification equation more resembles `e(-g1,sigma) e(S,H(msg)) = 1`.
//!
//! As one curve is smaller and hence faster, the user should choose
//! which orientation of curves they prefer, meaning to which curve
//! they hash, and which curves hold the signatues and public keys.
//! In other words, your desired aggregation techniques and usage
//! characteristics should determine if youp refer the verification
//! equation `e(g1,sigma) = e(S,H(msg))` or the fliped form
//! `e(sigma,g2) = e(H(msg),S)`.  See `UsualBLS` and `TinyBLS`.
//!
//! ### Aggregation
//!
//! We consder BLS signatures interesting because they support
//! dramatic optimizations when handling multiple signatures together.
//! In fact, BLS signatures support aggregation by a third party
//! that makes signatures smaller, not merely batch verification.  
//! All this stems from the bilinearity of `e`, meaning we reduce
//! the number of pairings, or size of the miller loop, by appling
//! rules like `e(x,z)e(y,z) = e(x+y,z)`, `e(x,y)e(x,z) = e(x,y+z)`,
//! etc.
//!
//! In essence, our aggregation tricks fall into two categories,
//! linear aggregation, in which only addition is used, and
//! delinearized optimiztions, in which we multiply curve points
//! by values unforseeable to the signers.
//! In general, linear techniques provide much better performance,
//! but require stronger invariants be maintained by the caller,
//! like messages being distinct, or limited signer sets with
//! proofs-of-possession.  Also, the delinearized techniques remain
//! secure without tricky assumptions, but require more computation.
//!
//! ### Verification
//!
//! We can often further reduce the pairings required in the
//! verification equation, beyond the naieve information tracked
//! by the aggregated signature itself.  Aggregated signature must
//! state all the individual messages and/or public keys, but
//! verifiers may collapse anything permitted.
//! We thus encounter aggregation-like decissions that impact
//! verifier performance.
//!
//! We therefore provide an abstract interface that permits
//! doing further aggregation and/or passing any aggregate signature
//! to any verification routine.
//!
//! As a rule, we also attempt to batch normalize different arithmatic
//! outputs, but concievably small signer set sizes might make this
//! a pessimization.
//!
//!
//!

//#![feature(test)] needed for cargo bench
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg_attr(feature = "std", doc = include_str!("../README.md"))]
#[cfg(doctest)]
pub struct ReadmeDoctests;

extern crate ark_serialize;
extern crate ark_serialize_derive;

extern crate ark_ec;
extern crate ark_ff;
extern crate digest;
extern crate rand;
extern crate rand_chacha;
extern crate rand_core;
extern crate sha3;

extern crate alloc;

use core::borrow::Borrow;
use digest::DynDigest;

pub mod chaum_pedersen_signature;
pub mod double;
pub mod double_pop;
pub mod engine;
pub mod schnorr_pop;
pub mod serialize;
pub mod single;
pub mod verifiers;

#[cfg(feature = "std")]
pub mod multi_pop_aggregator;
#[cfg(feature = "std")]
pub mod single_pop_aggregator;

#[cfg(feature = "experimental")]
pub mod bit;
#[cfg(feature = "experimental")]
pub mod delinear;
#[cfg(feature = "experimental")]
pub mod distinct;

pub use engine::*;

pub use double::{
    DoublePublicKey, DoublePublicKeyScheme, DoubleSignature, PublicKeyInSignatureGroup,
};
pub use double_pop::{NuggetBLSPoP, NuggetBLSnCPPoP};
pub use schnorr_pop::SchnorrProof;
pub use serialize::SerializableToBytes;
pub use single::{Keypair, KeypairVT, PublicKey, SecretKey, SecretKeyVT, Signature, SignedMessage};

use alloc::vec::Vec;

/// Internal message hash size.  
///
/// We choose 256 bits here so that birthday bound attacks cannot
/// find messages with the same hash.
const MESSAGE_SIZE: usize = 32;

/// Ciphersuite standards from BLS signature draft IETF proposal
const PROOF_OF_POSSESSION_ID: &'static [u8] = b"BLS_POP_";
const NORMAL_MESSAGE_SIGNATURE_ID: &'static [u8] = b"BLS_SIG_";

const NORMAL_MESSAGE_SIGNATURE_ASSUMING_POP: &'static [u8] = b"POP_";
const NORMAL_MESSAGE_SIGNATURE_BASIC: &'static [u8] = b"NUL_";
const POP_MESSAGE: &'static [u8] = b"POP_";

type MessageDigest = [u8; MESSAGE_SIZE];
/// Internal message hash type.  Short for frequent rehashing
/// by `HashMap`, etc.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Message(pub MessageDigest, pub alloc::vec::Vec<u8>, MessageType);

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
enum MessageType {
    ProofOfPossession,
    NormalAssumingPoP,
    NormalBasic,
}

impl Message {
    pub fn new(context: &[u8], message: &[u8]) -> Message {
        let msg_hash = Self::compute_internal_hash(context, message);
        Message(
            msg_hash,
            [context, message].concat(),
            MessageType::NormalBasic,
        )
    }

    pub fn new_assuming_pop(context: &[u8], message: &[u8]) -> Message {
        let msg_hash = Self::compute_internal_hash(context, message);
        Message(
            msg_hash,
            [context, message].concat(),
            MessageType::NormalAssumingPoP,
        )
    }

    pub fn new_pop_message(context: &[u8], message: &[u8]) -> Message {
        let msg_hash = Self::compute_internal_hash(context, message);
        Message(
            msg_hash,
            [context, message].concat(),
            MessageType::ProofOfPossession,
        )
    }

    fn compute_internal_hash(context: &[u8], message: &[u8]) -> [u8; MESSAGE_SIZE] {
        use sha3::{
            digest::{ExtendableOutput, Update, XofReader},
            Shake128,
        };
        let mut h = Shake128::default();
        h.update(context);
        let l = message.len() as u64;
        h.update(&l.to_le_bytes());
        h.update(message);

        let mut msg_hash = [0u8; MESSAGE_SIZE];
        h.finalize_xof().read(&mut msg_hash[..]);

        msg_hash
    }

    /// generate ciphersuite string added to the context according to
    /// BLS Signature draft proposal to IETF
    fn cipher_suite<E: EngineBLS>(&self) -> Vec<u8> {
        let id = match self.2 {
            MessageType::ProofOfPossession => PROOF_OF_POSSESSION_ID,
            _ => NORMAL_MESSAGE_SIGNATURE_ID,
        };

        let h2c_suite_id = [
            E::CURVE_NAME,
            E::SIG_GROUP_NAME,
            E::CIPHER_SUIT_DOMAIN_SEPARATION,
        ]
        .concat();

        let sc_tag = match self.2 {
            MessageType::ProofOfPossession => POP_MESSAGE,
            MessageType::NormalAssumingPoP => NORMAL_MESSAGE_SIGNATURE_ASSUMING_POP,
            _ => NORMAL_MESSAGE_SIGNATURE_BASIC,
        };

        [id, &h2c_suite_id[..], sc_tag].concat()
    }

    pub fn hash_to_signature_curve<E: EngineBLS>(&self) -> E::SignatureGroup {
        E::hash_to_signature_curve(&[&self.cipher_suite::<E>()[..], &self.1[..]].concat()[..])
    }
}

impl<'a> From<&'a [u8]> for Message {
    fn from(x: &[u8]) -> Message {
        Message::new(b"", x)
    }
}

/// Representation of an aggregated BLS signature.
///
/// We implement this trait only for borrows of appropriate structs
/// because otherwise we'd need extensive lifetime plumbing here,
/// due to the absence of assocaited type constructers (ATCs).
/// We shall make `messages_and_publickeys` take `&sefl` and
/// remove these limitations in the future once ATCs stabalize,
/// thus removing `PKG`.  See [Rust RFC 1598](https://github.com/rust-lang/rfcs/blob/master/text/1598-generic_associated_types.md)
/// We shall eventually remove MnPK entirely whenever `-> impl Trait`
/// in traits gets stabalized.  See [Rust RFCs 1522, 1951, and 2071](https://github.com/rust-lang/rust/issues/34511
pub trait Signed: Sized {
    type E: EngineBLS;

    /// Return the aggregated signature
    fn signature(&self) -> Signature<Self::E>;

    type M: Borrow<Message>; // = Message;
    type PKG: Borrow<PublicKey<Self::E>>; // = PublicKey<Self::E>;

    /// Iterator over, messages and public key reference pairs.
    type PKnM: Iterator<Item = (Self::M, Self::PKG)> + ExactSizeIterator;
    // type PKnM<'a>: Iterator<Item = (
    //    &'a <<Self as Signed<'a>>::E as EngineBLS>::PublicKeyGroup,
    //    &'a Self::M,
    // )> + DoubleEndedIterator + ExactSizeIterator + 'a;

    /// Returns an iterator over messages and public key reference for
    /// pairings, often only partially aggregated.
    fn messages_and_publickeys(self) -> Self::PKnM;
    // fn messages_and_publickeys<'a>(&'s self) -> PKnM<'a>
    // -> impl Iterator<Item = (&'a Self::M, &'a Self::E::PublicKeyGroup)> + 'a;

    /// Appropriate BLS signature verification for the `Self` type.
    ///
    /// We use `verify_simple` as a default implementation because
    /// it supports unstable `self.messages_and_publickeys()` securely
    /// by calling it only once, and does not expect pulic key points
    /// to be normalized, but this should usually be replaced by more
    /// optimized variants.
    fn verify(self) -> bool {
        verifiers::verify_simple(self)
    }
}

pub trait ProofOfPossession<E, H, PV>
where
    E: EngineBLS,
    H: DynDigest + Default + Clone,
{
    fn verify(&self, public_key_of_prover: &PV) -> bool;
}

/// ProofOfPossion trait which should be implemented by secret
pub trait ProofOfPossessionGenerator<
    E: EngineBLS,
    H: DynDigest + Default + Clone,
    PV,
    P: ProofOfPossession<E, H, PV>,
>
{
    /// The proof of possession generator is supposed to
    /// to produce a schnorr signature or a bls signature using
    /// the secret key which it claims to possess. This proves that
    /// that the secret key is known.
    fn generate_pok(&mut self) -> P;
}
