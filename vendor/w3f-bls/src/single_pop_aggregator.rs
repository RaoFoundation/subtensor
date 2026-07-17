//! ## Aggregation of BLS signatures using proofs-of-possession
//!
//! In this module, we provide the linear flavor of aggregate
//! BLS signature in which the verifiers has previously checked
//! proofs-of-possession for all public keys.  In other words,
//! we simply add up the signatures because the previously checked
//! proofs-of-possession for all signers prevent rogue key attacks.
//! See the security arguments in The Power of Proofs-of-Possession:
//! Securing Multiparty Signatures against Rogue-Key Attacks
//! by Thomas Ristenpart and Scott Yilek at https://eprint.iacr.org/2007/264.pdf
//!
//! These proof-of-possession are simply self-signed certificates,
//! so a BLS signature by each secret key on its own public key.
//! Importantly, the message for this self-signed certificates
//! must uniquely distinguish the public key for which the signature
//! establishes a proof-of-possession.
//! It follows that each proof-of-possession has a unique message,
//! so distinct message aggregation is optimal for verifying them.
//!
//! In this vein, we note that aggregation under proofs-of-possession
//! cannot improve performance when signers sign distinct messages,
//! so proofs-of-possession help with aggregating votes in a concensus
//! protocol, but should never be used for accounts on a block chain.
//!
//! We assume here that users provide their own data structure for
//! proofs-of-poossession.  We provide more structure for users who
//! one bit per vote in a concensus protocol:  
//! You first verify the proofs-of-possession when building a data
//! structure that holds the voters' keys.  You implement the
//! `ProofsOfPossession` trait for this data strtcuture as well,
//! so that the `BitPoPSignedMessage` type provides a signature
//! data type with reasonable sanity checks.

// Aside about proof-of-possession in the DLOG setting
// https://twitter.com/btcVeg/status/1085490561082183681

use ark_ff::Zero;

use super::verifiers::{
    verify_using_aggregated_auxiliary_public_keys, verify_with_distinct_messages,
};
use super::*;

use digest::DynDigest;

/// Batch or aggregate BLS signatures with attached messages and
/// signers, for whom we previously checked proofs-of-possession.
///
/// In this type, we provide a high-risk low-level batching and
/// aggregation mechanism that merely adds up signatures under the
/// assumption that all required proofs-of-possession were previously
/// checked.
///
/// We say a signing key has provided a proof-of-possession if the
/// verifier remembers having checked some self-signed certificate
/// by that key.  It's insecure to use this aggregation strategy
/// without first cehcking proofs-of-possession.  In particular
/// it is insecure to use this aggregation strategy when checking
/// proofs-of-possession, and could not improve performance anyways.  
/// Distinct message aggregation is always optimal for checking
/// proofs-of-possession.  Please see the module level doumentation
/// for additional discussion and notes on security.
///
/// We foresee this type primarily being used to batch several
/// `BitPoPSignedMessage`s into one verification.  We do not track
/// aggreggated public keys here, instead merging multiples signers
/// public keys anytime they sign the same message, so this type
/// essentially provides only fast batch verificartion.  
/// In principle, our `add_*` methods suffice for building an actual
/// aggregate signature type.  Yet, normally direct approaches like
/// `BitPoPSignedMessage` work better for aggregation because
/// the `ProofsOfPossession` trait tooling permits both enforce the
/// proofs-of-possession and provide a compact serialization.
/// We see no reason to support serialization for this type as present.
/// message assumptions, or other aggre
///
/// In principle, one might combine proof-of-possession with distinct
/// message assumptions, or other aggregation strategies, when
/// verifiers have only observed a subset of the proofs-of-possession,
/// but this sounds complex or worse fragile.
///
/// TODO: Implement gaussian elimination verification scheme.
use core::iter::once;

use double::PublicKeyInSignatureGroup;
use single::PublicKey;

#[derive(Clone)]
pub struct SignatureAggregatorAssumingPoP<E: EngineBLS> {
    message: Message,
    aggregated_publickey: PublicKey<E>,
    signature: Signature<E>,
    aggregated_auxiliary_public_key: PublicKeyInSignatureGroup<E>,
}

impl<E: EngineBLS> SignatureAggregatorAssumingPoP<E> {
    pub fn new(message: Message) -> SignatureAggregatorAssumingPoP<E> {
        SignatureAggregatorAssumingPoP {
            message: message,
            aggregated_publickey: PublicKey(E::PublicKeyGroup::zero()),
            signature: Signature(E::SignatureGroup::zero()),
            aggregated_auxiliary_public_key: PublicKeyInSignatureGroup(E::SignatureGroup::zero()),
        }
    }

    /// Add only a `Signature<E>` to our internal signature.
    ///
    /// Useful for constructing an aggregate signature, but we
    pub fn add_signature(&mut self, signature: &Signature<E>) {
        self.signature.0 += &signature.0;
    }

    /// Add only a `PublicKey<E>` to our internal data.
    ///
    /// Useful for constructing an aggregate signature, but we
    /// recommend instead using a custom types like `BitPoPSignedMessage`.
    pub fn add_publickey(&mut self, publickey: &PublicKey<E>) {
        self.aggregated_publickey.0 += publickey.0;
    }

    /// Aggregate the auxiliary public keys in the signature group to be used verification using aux key
    pub fn add_auxiliary_public_key(
        &mut self,
        publickey_in_signature_group: &PublicKeyInSignatureGroup<E>,
    ) {
        self.aggregated_auxiliary_public_key.0 += publickey_in_signature_group.0;
    }

    /// Returns the aggergated public key.
    ///
    pub fn aggregated_publickey(&self) -> PublicKey<E> {
        self.aggregated_publickey
    }

    // /// Aggregage BLS signatures assuming they have proofs-of-possession
    // /// TODO this function should return Result refusing to aggregate messages
    // /// different than the message the aggregator is initiated at
    // pub fn aggregate<'a,S>(&mut self, signed: &'a S)
    // where
    //     &'a S: Signed<E=E>,
    //     <&'a S as Signed>::PKG: Borrow<PublicKey<E>>,
    // {
    //     let signature = signed.signature();
    //     for (message,pubickey) in signed.messages_and_publickeys() {
    //         self.add_message_n_publickey(message.borrow(),pubickey.borrow());
    //     }
    //     self.add_signature(&signature);
    // }

    pub fn verify_using_aggregated_auxiliary_public_keys<
        RandomOracle: DynDigest + Default + Clone,
    >(
        &self,
    ) -> bool {
        verify_using_aggregated_auxiliary_public_keys::<E, RandomOracle>(
            self,
            true,
            self.aggregated_auxiliary_public_key.0,
        )
    }
}

impl<'a, E: EngineBLS> Signed for &'a SignatureAggregatorAssumingPoP<E> {
    type E = E;

    type M = Message;
    type PKG = PublicKey<Self::E>;
    type PKnM = ::core::iter::Once<(Message, PublicKey<E>)>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        once((self.message.clone(), self.aggregated_publickey)) // TODO:  Avoid clone
    }

    fn signature(&self) -> Signature<E> {
        self.signature
    }

    fn verify(self) -> bool {
        // We have already aggregated distinct messages, so our distinct
        // message verification code provides reasonable optimizations,
        // except the public keys might not be normalized here.
        // We foresee verification via gaussian elimination being faster,
        // but requires affine keys or normalization.
        verify_with_distinct_messages(self, true)
        // TODO: verify_with_gaussian_elimination(self)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {

    use crate::EngineBLS;
    use crate::Keypair;
    use crate::Message;
    use crate::TinyBLS;
    use crate::UsualBLS;
    use rand::thread_rng;
    use sha2::Sha256;

    use ark_bls12_377::Bls12_377;
    use ark_bls12_381::Bls12_381;

    use super::*;

    #[test]
    fn verify_aggregate_single_message_single_signer() {
        let good = Message::new(b"ctx", b"test message");

        let mut keypair =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig0 = keypair.sign(&good);
        assert!(good_sig0.verify(&good, &keypair.public));
    }

    #[test]
    fn verify_aggregate_single_message_multi_signers() {
        let good = Message::new(b"ctx", b"test message");

        let mut keypair0 =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig0 = keypair0.sign(&good);

        let mut keypair1 =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig1 = keypair1.sign(&good);

        let mut aggregated_sigs =
            SignatureAggregatorAssumingPoP::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::new(good);
        aggregated_sigs.add_signature(&good_sig0);
        aggregated_sigs.add_signature(&good_sig1);

        aggregated_sigs.add_publickey(&keypair0.public);
        aggregated_sigs.add_publickey(&keypair1.public);

        assert!(
            aggregated_sigs.verify() == true,
            "good aggregated signature of a single message with multiple key does not verify"
        );
    }

    #[test]
    fn verify_aggregate_single_message_repetative_signers() {
        let good = Message::new(b"ctx", b"test message");

        let mut keypair =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig = keypair.sign(&good);

        let mut aggregated_sigs =
            SignatureAggregatorAssumingPoP::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::new(good);
        aggregated_sigs.add_signature(&good_sig);
        aggregated_sigs.add_signature(&good_sig);

        aggregated_sigs.add_publickey(&keypair.public);
        aggregated_sigs.add_publickey(&keypair.public);

        assert!(
            aggregated_sigs.verify() == true,
            "good aggregate of a repetitive signature does not verify"
        );
    }

    #[test]
    fn aggregate_of_signature_of_a_wrong_message_should_not_verify() {
        let good0 = Message::new(b"ctx", b"Space over Tab");
        let bad1 = Message::new(b"ctx", b"Tab over Space");

        let mut keypair0 =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let good_sig0 = keypair0.sign(&good0);

        let mut keypair1 =
            Keypair::<UsualBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let bad_sig1 = keypair1.sign(&bad1);

        let mut aggregated_sigs = SignatureAggregatorAssumingPoP::<
            UsualBLS<Bls12_381, ark_bls12_381::Config>,
        >::new(good0);
        aggregated_sigs.add_signature(&good_sig0);
        aggregated_sigs.add_signature(&bad_sig1);

        aggregated_sigs.add_publickey(&keypair0.public);
        aggregated_sigs.add_publickey(&keypair1.public);

        assert!(
            aggregated_sigs.verify() == false,
            "aggregated signature of a wrong message should not verify"
        );
    }

    #[test]
    fn test_aggregate_tiny_sigs_and_verify_in_g1() {
        let message = Message::new(b"ctx", b"test message");
        let mut keypairs: Vec<_> = (0..3)
            .into_iter()
            .map(|_| Keypair::<TinyBLS<Bls12_377, ark_bls12_377::Config>>::generate(thread_rng()))
            .collect();
        let pub_keys_in_sig_grp: Vec<PublicKeyInSignatureGroup<TinyBLS377>> = keypairs
            .iter()
            .map(|k| k.into_public_key_in_signature_group())
            .collect();

        let mut aggregator = SignatureAggregatorAssumingPoP::<TinyBLS377>::new(message.clone());
        let mut aggregated_public_key =
            PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());

        for k in &mut keypairs {
            aggregator.add_signature(&k.sign(&message));
            aggregated_public_key.0 += k.public.0;
        }

        let mut verifier_aggregator = SignatureAggregatorAssumingPoP::<TinyBLS377>::new(message);

        verifier_aggregator.add_signature(&aggregator.signature);
        verifier_aggregator.add_publickey(&aggregated_public_key);

        for k in &pub_keys_in_sig_grp {
            verifier_aggregator.add_auxiliary_public_key(k);
        }

        assert!(
            verifier_aggregator.verify_using_aggregated_auxiliary_public_keys::<Sha256>(),
            "verifying with honest auxilary public key should pass"
        );

        //false aggregation in signature group should fails verification.
        verifier_aggregator
            .add_auxiliary_public_key(&keypairs[0].into_public_key_in_signature_group());
        assert!(
            !verifier_aggregator.verify_using_aggregated_auxiliary_public_keys::<Sha256>(),
            "verification using non-matching auxilary public key should fail"
        );
    }
}
