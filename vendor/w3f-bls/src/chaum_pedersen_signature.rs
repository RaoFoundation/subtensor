use alloc::vec::Vec;

use ark_ec::Group;
use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};

use digest::DynDigest;

use crate::double::{DoublePublicKeyScheme, PublicKeyInSignatureGroup};
use crate::engine::EngineBLS;
use crate::schnorr_pop::SchnorrProof;
use crate::serialize::SerializableToBytes;
use crate::single::Signature;
use crate::{Message, SecretKeyVT};

pub type ChaumPedersenSignature<E> = (Signature<E>, SchnorrProof<E>);

/// ProofOfPossion trait which should be implemented by secret
pub trait ChaumPedersenSigner<E: EngineBLS, H: DynDigest + Default + Clone> {
    /// The proof of possession generator is supposed to
    /// to produce a schnoor signature of the message using
    /// the secret key which it claim to possess.
    fn generate_cp_signature(&mut self, message: &Message) -> ChaumPedersenSignature<E>;

    fn generate_witness_scaler(
        &self,
        message_point_as_bytes: &Vec<u8>,
    ) -> <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField;

    fn generate_dleq_proof(
        &mut self,
        message: &Message,
        bls_signature: E::SignatureGroup,
    ) -> SchnorrProof<E>;
}

/// This should be implemented by public key
pub trait ChaumPedersenVerifier<E: EngineBLS, H: DynDigest + Default + Clone> {
    fn verify_cp_signature(
        &self,
        message: &Message,
        signature_proof: ChaumPedersenSignature<E>,
    ) -> bool;
}

impl<E: EngineBLS, H: DynDigest + Default + Clone> ChaumPedersenSigner<E, H> for SecretKeyVT<E> {
    fn generate_cp_signature(&mut self, message: &Message) -> ChaumPedersenSignature<E> {
        //First we generate a vanila BLS Signature;
        let bls_signature = SecretKeyVT::sign(self, message);
        (
            bls_signature,
            <SecretKeyVT<E> as ChaumPedersenSigner<E, H>>::generate_dleq_proof(
                self,
                message,
                bls_signature.0,
            ),
        )
    }

    #[allow(non_snake_case)]
    fn generate_dleq_proof(
        &mut self,
        message: &Message,
        bls_signature: E::SignatureGroup,
    ) -> SchnorrProof<E> {
        let signature_point = bls_signature;
        let message_point = message.hash_to_signature_curve::<E>();

        let signature_point_as_bytes = E::signature_point_to_byte(&signature_point);
        let message_point_as_bytes = E::signature_point_to_byte(&message_point);
        let public_key_in_signature_group_as_bytes = E::signature_point_to_byte(
            &DoublePublicKeyScheme::<E>::into_public_key_in_signature_group(self).0,
        );

        let mut k = <SecretKeyVT<E> as ChaumPedersenSigner<E, H>>::generate_witness_scaler(
            self,
            &message_point_as_bytes,
        );

        let A_point = <<E as EngineBLS>::SignatureGroup as Group>::generator() * k;
        let B_point = message_point * k;

        let A_point_as_bytes = E::signature_point_to_byte(&A_point);
        let B_point_as_bytes = E::signature_point_to_byte(&B_point);

        let proof_basis = [
            message_point_as_bytes,
            public_key_in_signature_group_as_bytes,
            signature_point_as_bytes,
            A_point_as_bytes,
            B_point_as_bytes,
        ]
        .concat();

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);

        let c = hasher.hash_to_field(proof_basis.as_slice(), 1)[0];

        let s = k - c * self.0;

        ::zeroize::Zeroize::zeroize(&mut k); //clear secret witness from memory

        (c, s)
    }

    fn generate_witness_scaler(
        &self,
        message_point_as_bytes: &Vec<u8>,
    ) -> <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField {
        let secret_key_as_bytes = self.to_bytes();

        let mut secret_key_hasher = H::default();
        secret_key_hasher.update(secret_key_as_bytes.as_slice());
        let hashed_secret_key = secret_key_hasher.finalize_reset().to_vec();

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);
        let scalar_seed = [hashed_secret_key, message_point_as_bytes.clone()].concat();
        hasher.hash_to_field(scalar_seed.as_slice(), 1)[0]
    }
}

/// This should be implemented by public key
#[allow(non_snake_case)]
impl<E: EngineBLS, H: DynDigest + Default + Clone> ChaumPedersenVerifier<E, H>
    for PublicKeyInSignatureGroup<E>
{
    fn verify_cp_signature(
        &self,
        message: &Message,
        signature_proof: ChaumPedersenSignature<E>,
    ) -> bool {
        let A_check_point = <<E as EngineBLS>::SignatureGroup as Group>::generator()
            * signature_proof.1 .1
            + self.0 * signature_proof.1 .0;

        let B_check_point = message.hash_to_signature_curve::<E>() * signature_proof.1 .1
            + signature_proof.0 .0 * signature_proof.1 .0;

        let A_point_as_bytes = E::signature_point_to_byte(&A_check_point);
        let B_point_as_bytes = E::signature_point_to_byte(&B_check_point);

        let signature_point_as_bytes = signature_proof.0.to_bytes();
        let message_point_as_bytes =
            E::signature_point_to_byte(&message.hash_to_signature_curve::<E>());
        let public_key_in_signature_group_as_bytes = E::signature_point_to_byte(&self.0);

        let resulting_proof_basis = [
            message_point_as_bytes,
            public_key_in_signature_group_as_bytes,
            signature_point_as_bytes,
            A_point_as_bytes,
            B_point_as_bytes,
        ]
        .concat();

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);
        let c_check: <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField =
            hasher.hash_to_field(resulting_proof_basis.as_slice(), 1)[0];

        c_check == signature_proof.1 .0
    }
}
