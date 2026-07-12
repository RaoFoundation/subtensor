//! ## BLS key pair with public key in both G1 and G2
//! ## Unaggreagated BLS signature along side with their DLEQ proof
//!
//! Implements schemes suggested the
//! [paper](https://eprint.iacr.org/2022/1611)
//!
//! The scheme proposes for the public key be represented by doube points,
//! both in G1 and G2 and aggregate keys in G1.
//!
//! It also proposes that each individual BLS signature accompany a DLEQ proof
//! for faster verification

use alloc::vec::Vec;
use core::iter::once;

use ark_ec::{AffineRepr, CurveGroup};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

use sha2::Sha256;

use crate::broken_derives;
use crate::chaum_pedersen_signature::{ChaumPedersenSigner, ChaumPedersenVerifier};
use crate::schnorr_pop::SchnorrProof;
use crate::serialize::SerializableToBytes;
use crate::single::{Keypair, KeypairVT, PublicKey, SecretKeyVT, Signature};
use crate::{EngineBLS, Message, Signed};

/// Wrapper for a point in the signature group which is supposed to
/// the same logarithm as the public key in the public key group
#[derive(Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct PublicKeyInSignatureGroup<E: EngineBLS>(pub E::SignatureGroup);
broken_derives!(PublicKeyInSignatureGroup); // Actually the derive works for this one, not sure why.

/// BLS Public Key with sub keys in both groups.
#[derive(Debug, Clone, CanonicalSerialize, CanonicalDeserialize)]
pub struct DoublePublicKey<E: EngineBLS>(pub E::SignatureGroup, pub E::PublicKeyGroup);

impl<E: EngineBLS> DoublePublicKey<E> {
    pub fn verify(&self, message: &Message, signature: &DoubleSignature<E>) -> bool {
        signature.verify(message, self)
    }
}

/// Serialization for DoublePublickey
impl<E: EngineBLS> SerializableToBytes for DoublePublicKey<E> {
    const SERIALIZED_BYTES_SIZE: usize =
        E::SIGNATURE_SERIALIZED_SIZE + E::PUBLICKEY_SERIALIZED_SIZE;
}

pub trait DoublePublicKeyScheme<E: EngineBLS> {
    fn into_public_key_in_signature_group(&self) -> PublicKeyInSignatureGroup<E>;

    /// Return a double public object containing public keys both in G1 and G2
    fn into_double_public_key(&self) -> DoublePublicKey<E>;
    fn sign(&mut self, message: &Message) -> DoubleSignature<E>;
}

impl<E: EngineBLS> DoublePublicKeyScheme<E> for SecretKeyVT<E> {
    fn into_public_key_in_signature_group(&self) -> PublicKeyInSignatureGroup<E> {
        PublicKeyInSignatureGroup(
            <E::SignatureGroup as CurveGroup>::Affine::generator().into_group() * self.0,
        )
    }

    fn into_double_public_key(&self) -> DoublePublicKey<E> {
        DoublePublicKey(
            self.into_public_key_in_signature_group().0,
            self.into_public().0,
        )
    }

    /// Sign a message using a Seedabale RNG created from a seed derived from the message and key
    fn sign(&mut self, message: &Message) -> DoubleSignature<E> {
        let chaum_pedersen_signature =
            ChaumPedersenSigner::<E, Sha256>::generate_cp_signature(self, &message);
        DoubleSignature(chaum_pedersen_signature.0 .0, chaum_pedersen_signature.1)
    }
}

impl<E: EngineBLS> DoublePublicKeyScheme<E> for KeypairVT<E> {
    fn into_public_key_in_signature_group(&self) -> PublicKeyInSignatureGroup<E> {
        self.secret.into_public_key_in_signature_group()
    }

    fn into_double_public_key(&self) -> DoublePublicKey<E> {
        self.secret.into_double_public_key()
    }

    /// Sign a message using a Seedabale RNG created from a seed derived from the message and key
    fn sign(&mut self, message: &Message) -> DoubleSignature<E> {
        DoublePublicKeyScheme::sign(&mut self.secret, message)
    }
}

impl<E: EngineBLS> DoublePublicKeyScheme<E> for Keypair<E> {
    fn into_public_key_in_signature_group(&self) -> PublicKeyInSignatureGroup<E> {
        self.into_vartime().into_public_key_in_signature_group()
    }

    fn into_double_public_key(&self) -> DoublePublicKey<E> {
        self.into_vartime().into_double_public_key()
    }

    /// Sign a message using a Seedabale RNG created from a seed derived from the message and key
    fn sign(&mut self, message: &Message) -> DoubleSignature<E> {
        DoublePublicKeyScheme::sign(&mut self.into_vartime(), message)
    }
}

/// Detached BLS Signature containing DLEQ
#[derive(Clone, Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct DoubleSignature<E: EngineBLS>(pub E::SignatureGroup, SchnorrProof<E>);

impl<E: EngineBLS> DoubleSignature<E> {
    //const DESCRIPTION : &'static str = "A BLS signature";

    /// Verify a single BLS signature using DLEQ proof
    pub fn verify(&self, message: &Message, publickey: &DoublePublicKey<E>) -> bool {
        <PublicKeyInSignatureGroup<E> as ChaumPedersenVerifier<E, Sha256>>::verify_cp_signature(
            &PublicKeyInSignatureGroup(publickey.0),
            &message,
            (Signature(self.0), self.1),
        )
    }
}

/// Message with attached BLS signature
///
///
#[derive(Debug, Clone)]
pub struct DoubleSignedMessage<E: EngineBLS> {
    pub message: Message,
    pub publickey: DoublePublicKey<E>,
    pub signature: DoubleSignature<E>,
}

impl<E: EngineBLS> PartialEq<Self> for DoubleSignedMessage<E> {
    fn eq(&self, other: &Self) -> bool {
        self.message.eq(&other.message)
            && self.publickey.0.eq(&other.publickey.0)
            && self.publickey.1.eq(&other.publickey.1)
            && self.signature.0.eq(&other.signature.0)
    }
}

impl<'a, E: EngineBLS> Signed for &'a DoubleSignedMessage<E> {
    type E = E;

    type M = Message;
    type PKG = PublicKey<E>;

    type PKnM = ::core::iter::Once<(Message, PublicKey<E>)>;

    fn messages_and_publickeys(self) -> Self::PKnM {
        once((self.message.clone(), PublicKey(self.publickey.1))) // TODO:  Avoid clone
    }

    fn signature(&self) -> Signature<E> {
        Signature(self.signature.0)
    }

    fn verify(self) -> bool {
        //we chaum pederesen verification which is faster
        ChaumPedersenVerifier::<E, Sha256>::verify_cp_signature(
            &PublicKeyInSignatureGroup::<E>(self.publickey.0),
            &self.message,
            (Signature(self.signature.0), self.signature.1),
        )
    }
}

/// Serialization for DoublePublickey
impl<E: EngineBLS> SerializableToBytes for DoubleSignature<E> {
    const SERIALIZED_BYTES_SIZE: usize = E::SIGNATURE_SERIALIZED_SIZE + 2 * E::SECRET_KEY_SIZE;
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use rand::thread_rng;

    use super::*;

    use ark_bls12_377::Bls12_377;
    use ark_bls12_381::Bls12_381;
    use ark_ec::bls12::Bls12Config;
    use ark_ec::hashing::curve_maps::wb::{WBConfig, WBMap};
    use ark_ec::hashing::map_to_curve_hasher::MapToCurve;
    use ark_ec::pairing::Pairing as PairingEngine;

    use crate::{EngineBLS, Message, TinyBLS};

    fn double_public_serialization_test<
        EB: EngineBLS<Engine = E>,
        E: PairingEngine,
        P: Bls12Config,
    >(
        x: DoubleSignedMessage<EB>,
    ) -> DoubleSignedMessage<EB>
    where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        let DoubleSignedMessage {
            message,
            publickey,
            signature,
        } = x;

        let publickey = DoublePublicKey::<EB>::from_bytes(&publickey.to_bytes()).unwrap();
        let signature = DoubleSignature::<EB>::from_bytes(&signature.to_bytes()).unwrap();

        DoubleSignedMessage {
            message,
            publickey,
            signature,
        }
    }

    fn test_single_bls_message_double_signature_scheme<
        EB: EngineBLS<Engine = E>,
        E: PairingEngine,
        P: Bls12Config,
    >()
    where
        <P as Bls12Config>::G2Config: WBConfig,
        WBMap<<P as Bls12Config>::G2Config>: MapToCurve<<E as PairingEngine>::G2>,
    {
        let good = Message::new(b"ctx", b"test message");

        let mut keypair = Keypair::<EB>::generate(thread_rng());
        let public_key = DoublePublicKeyScheme::into_double_public_key(&mut keypair);
        let good_sig = DoublePublicKeyScheme::sign(&mut keypair, &good);

        assert!(
            public_key.verify(&good, &good_sig),
            "Verification of a valid signature failed!"
        );

        let bad = Message::new(b"ctx", b"wrong message");
        let bad_sig = DoublePublicKeyScheme::sign(&mut keypair, &bad);

        assert!(bad_sig.verify(
            &bad,
            &DoublePublicKeyScheme::into_double_public_key(&keypair)
        ));

        assert!(good != bad, "good == bad");
        assert!(good_sig.0 != bad_sig.0, "good sig == bad sig");

        assert!(
            !bad_sig.verify(
                &good,
                &DoublePublicKeyScheme::into_double_public_key(&keypair)
            ),
            "Verification of a signature on a different message passed!"
        );
        assert!(
            !good_sig.verify(
                &bad,
                &DoublePublicKeyScheme::into_double_public_key(&keypair)
            ),
            "Verification of a signature on a different message passed!"
        );
    }

    #[test]
    fn test_double_public_key_double_signature_serialization_for_bls12_377() {
        let mut keypair =
            Keypair::<TinyBLS<Bls12_377, ark_bls12_377::Config>>::generate(thread_rng());
        let message = Message::new(b"ctx", b"test message");
        let good_sig0 = DoublePublicKeyScheme::sign(&mut keypair, &message);

        let signed_message = DoubleSignedMessage {
            message: message,
            publickey: DoublePublicKey(
                keypair.into_public_key_in_signature_group().0,
                keypair.public.0,
            ),
            signature: good_sig0,
        };

        assert!(
            signed_message.verify(),
            "valid double signed message should verify"
        );

        let deserialized_signed_message = double_public_serialization_test::<
            TinyBLS<Bls12_377, ark_bls12_377::Config>,
            Bls12_377,
            ark_bls12_377::Config,
        >(signed_message);

        assert!(
            deserialized_signed_message.verify(),
            "deserialized valid double signed message should verify"
        );
    }

    #[test]
    fn test_double_public_key_double_signature_serialization_for_bls12_381() {
        let mut keypair =
            Keypair::<TinyBLS<Bls12_381, ark_bls12_381::Config>>::generate(thread_rng());
        let message = Message::new(b"ctx", b"test message");
        let good_sig0 = DoublePublicKeyScheme::sign(&mut keypair, &message);

        let signed_message = DoubleSignedMessage {
            message: message,
            publickey: DoublePublicKey(
                keypair.into_public_key_in_signature_group().0,
                keypair.public.0,
            ),
            signature: good_sig0,
        };

        assert!(
            signed_message.verify(),
            "valid double signed message should verify"
        );

        let deserialized_signed_message = double_public_serialization_test::<
            TinyBLS<Bls12_381, ark_bls12_381::Config>,
            Bls12_381,
            ark_bls12_381::Config,
        >(signed_message);

        assert!(
            deserialized_signed_message.verify(),
            "deserialized valid double signed message should verify"
        );
    }

    #[test]
    fn test_single_bls_message_double_signature_scheme_for_bls12_377() {
        test_single_bls_message_double_signature_scheme::<
            TinyBLS<Bls12_377, ark_bls12_377::Config>,
            Bls12_377,
            ark_bls12_377::Config,
        >();
    }

    #[test]
    fn test_single_bls_message_double_signature_scheme_for_bls12_381() {
        test_single_bls_message_double_signature_scheme::<
            TinyBLS<Bls12_381, ark_bls12_381::Config>,
            Bls12_381,
            ark_bls12_381::Config,
        >();
    }
}
