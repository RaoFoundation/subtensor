//! Delinearized batching and aggregation for BLS signatures
//!
//! We handle delinearized flavors of aggregate BLS signatures here,
//! meaning we multiply signatures by an exponent that seems random
//! relative to the signers public key.  In this, we support both
//! batching with explicit randomness, and delinearization in which
//! we [treat the hash of all included public keys as a random oracle](https://crypto.stanford.edu/~dabo/pubs/papers/BLSmultisig.html)
//!
//! We caution that delinerized aggregation leans towards slightly
//! different abstractions than linear aggregation.  In this module,
//! we select an approach that complements well our linear strategies,
//! but if you need delinearized aggregation then you should consider
//! adding a more finely tuned scheme.

use ark_ec::CurveGroup;
use ark_ff::BigInteger;
use ark_ff::{PrimeField, Zero};
use ark_serialize::CanonicalSerialize;
use arrayref::array_refs;
#[cfg(feature = "std")]
use rand::thread_rng;
use rand::Rng;
use sha3::{
    digest::{ExtendableOutput, Update, XofReader},
    Shake128,
};

use std::collections::HashMap;

use super::single::SignedMessage;
use super::verifiers::verify_with_distinct_messages;
use super::*;

/// Delinearized batched and aggregated BLS signatures.
///
/// We caution that this type only represents one of several
/// optimizations possible.  We believe it fits well when messages
/// are often repeated but signers are rarely repeated.
///
/// We should create another type for when repeated signers are
/// expected, likely by keying the hash map on the pubkic key.
/// In practice though, if signers are often repeated then you should
/// should consider a proof-of-possession scheme, which requiees all
/// signers register in advance.
pub struct Delinearized<E: EngineBLS> {
    key: Shake128,
    messages_n_publickeys: HashMap<Message, PublicKey<E>>,
    signature: Signature<E>,
}

impl<E: EngineBLS> Clone for Delinearized<E> {
    fn clone(&self) -> Delinearized<E> {
        Delinearized {
            key: self.key.clone(),
            messages_n_publickeys: self.messages_n_publickeys.clone(),
            signature: self.signature.clone(),
        }
    }
}

impl<'a, E: EngineBLS> Signed for &'a Delinearized<E> {
    type E = E;

    type M = &'a Message;
    type PKG = &'a PublicKey<Self::E>;
    type PKnM = ::std::collections::hash_map::Iter<'a, Message, PublicKey<E>>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        self.messages_n_publickeys.iter()
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        verify_with_distinct_messages(self, true)
    }
}

impl<E: EngineBLS> Delinearized<E> {
    pub fn new(key: Shake128) -> Delinearized<E> {
        Delinearized {
            key,
            messages_n_publickeys: HashMap::new(),
            signature: Signature(E::SignatureGroup::zero()),
        }
    }
    pub fn new_keyed(key: &[u8]) -> Delinearized<E> {
        let mut t = Shake128::default();
        t.update(b"Delinearised BLS with key:");
        let l = key.len() as u64;
        t.update(&l.to_le_bytes());
        t.update(key);
        Delinearized::new(t)
    }
    pub fn new_batched_rng<R: Rng>(mut rng: R) -> Delinearized<E> {
        let r = rng.gen::<[u8; 32]>();
        Delinearized::new_keyed(&r[..])
    }

    #[cfg(feature = "std")]
    pub fn new_batched() -> Delinearized<E> {
        Delinearized::new_batched_rng(thread_rng())
    }

    /// Return the mask used for a particular public key.
    ///
    /// TODO: We only want 128 bits here, not a full scalar.  We thus
    /// need `mul_bits` exposed by the pairing crate, at which point
    /// our return type here changes.
    pub fn mask(&self, publickey: &PublicKey<E>) -> E::Scalar {
        let mut t = self.key.clone();
        let pk_affine = publickey.0.into_affine();
        let mut pk_uncompressed = vec![0; pk_affine.uncompressed_size()];
        pk_affine
            .serialize_uncompressed(&mut pk_uncompressed[..])
            .unwrap();
        t.update(&pk_uncompressed);
        let mut b = [0u8; 16];
        t.finalize_xof().read(&mut b[..]);
        let (x, y) = array_refs!(&b, 8, 8);
        let mut x: <E::Scalar as PrimeField>::BigInt = u64::from_le_bytes(*x).into();
        let y: <E::Scalar as PrimeField>::BigInt = u64::from_le_bytes(*y).into();
        x.muln(64);
        x.add_with_carry(&y);
        <E::Scalar as PrimeField>::from_bigint(x).unwrap()
    }

    /// Add only a `Signature<E>` to our internal signature,
    /// assumes the signature was previously delinearized elsewhere.
    ///
    /// Useful for constructing an aggregate signature.
    pub fn add_delinearized_signature(&mut self, signature: &Signature<E>) {
        self.signature.0 += signature.0;
    }

    /// Add only a `Message` and `PublicKey<E>` to our internal data,
    /// doing delinearization ourselves.
    ///
    /// Useful for constructing an aggregate signature, but we
    /// recommend instead using a custom types like `BitPoPSignedMessage`.
    pub fn add_message_n_publickey(
        &mut self,
        message: &Message,
        mut publickey: PublicKey<E>,
    ) -> E::Scalar {
        let mask = self.mask(&publickey);
        // We must use projective corrdinates here, dispite converting to
        // affine just above, because only `CurveGroup::mul_assign`
        // skips doubling until a set bit is found.
        // In fact, there is no method to do this without abusing variable
        // time arithmatic, which might change in future, so we should add
        // some `CurveGroup` method `fn mul_128(&self, blinding: u128)`.
        // Or even expose the `AffineRepr::mul_bits` method.
        // TODO: Is using affine here actually faster?
        publickey.0 *= mask;
        self.messages_n_publickeys
            .entry(message.clone())
            .and_modify(|pk0| pk0.0 += publickey.0)
            .or_insert(publickey);
        mask
    }

    /// Aggregage BLS signatures from singletons using delinearization
    pub fn add(&mut self, signed: &SignedMessage<E>) {
        let mut signature = signed.signature;
        let mask = self.add_message_n_publickey(&signed.message, signed.publickey);
        signature.0 *= mask;
        self.add_delinearized_signature(&signature);
    }

    /// Test that two `Delinearized` use the same key.
    ///
    /// You should call this before calling `merge`, although
    /// we do enforce this because several untestable related
    /// conditions suffice too.
    // TODO: See https://github.com/dalek-cryptography/merlin/pull/37
    pub fn agreement(&self, other: &Delinearized<E>) -> bool {
        let mut c = [[0u8; 16]; 2];
        self.key.clone().finalize_xof().read(&mut c[0]);
        other.key.clone().finalize_xof().read(&mut c[1]);
        c[0] == c[1]
    }

    /// Merge another `Delinearized` for simultanious verification.
    ///
    /// You should only call this if `self.agreement(other)` or some
    /// related condition holds, or if you have message disjointness.
    // TODO: Feed into disjoint message aggregation.
    pub fn merge(&mut self, other: &Delinearized<E>) {
        // if ! self.agreement(other) { return Err(()); }
        for (message, publickey) in other.messages_n_publickeys.iter() {
            self.messages_n_publickeys
                .entry(message.clone())
                .and_modify(|pk0| pk0.0 += publickey.0)
                .or_insert(*publickey);
        }
        self.signature.0 += other.signature.0;
        // Ok(())
    }
}

/*
type PublicKeyUncompressed<E> = <<<E as EngineBLS>::$group as ProjectiveCurve>::Affine as AffineRepr>::Compressed;

#[derive(Clone)]
pub struct DelinearizedRepeatedSigners<E: EngineBLS> {
    key: Shake128,
    messages_n_publickeys: HashMap<PublicKeyUncompressed<E>,(Message,PublicKey<E>)>,
    signature: Signature<E>,
}
*/

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn delinearized() {
        let msg1 = Message::new(b"ctx", b"some message");

        let k = |_| Keypair::<ZBLS>::generate(thread_rng());
        let mut keypairs = (0..4).into_iter().map(k).collect::<Vec<_>>();
        let dup = keypairs[3].clone();
        keypairs.push(dup);
        let sigs1 = keypairs
            .iter_mut()
            .map(|k| k.signed_message(&msg1))
            .collect::<Vec<_>>();

        let mut dl = Delinearized::<ZBLS>::new_batched();
        for sig in sigs1.iter() {
            dl.add(sig);
            assert!(dl.verify()); // verifiers::verify_with_distinct_messages(&dms,true)
        }
        assert!(verifiers::verify_unoptimized(&dl));
        assert!(verifiers::verify_simple(&dl));
        assert!(verifiers::verify_with_distinct_messages(&dl, false));
        // assert!( verifiers::verify_with_gaussian_elimination(&dl) );

        assert!(dl.agreement(&dl));
        let dl_too = dl.clone();
        dl.merge(&dl_too);
        assert!(dl.verify());
        // TODO: more more
    }
}
