//! ## Implementation of ProofofPossion trait for Double BLS public keys using
//! the scheme described in [https://eprint.iacr.org/2022/1611] which also
//! complies with the proof of possession proposed in
//! [draft-irtf-cfrg-bls-signature-05](https://www.ietf.org/archive/id/draft-irtf-cfrg-bls-signature-05.html)

use crate::engine::EngineBLS;
use crate::{Message, ProofOfPossession, ProofOfPossessionGenerator};

use crate::double::DoublePublicKey;
use crate::serialize::SerializableToBytes;
use crate::single::Keypair;

use alloc::vec::Vec;
use digest::DynDigest;

use ark_ec::Group;
use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};

const PROOF_OF_POSSESSION_CONTEXT: &'static [u8] = b"POP_";
const BLS_CONTEXT: &'static [u8] = b"BLS_";

/// Proof Of Possession of the secret key as the secret scaler genarting both public
/// keys in G1 and G2 by generating a BLS Signature of public key (in G2)
pub struct BLSPoP<E: EngineBLS>(pub E::SignatureGroup);

impl<E: EngineBLS> BLSPoP<E> {
    fn bls_pop_context<H: DynDigest + Default + Clone>() -> Vec<u8> {
        [
            <BLSPoP<E> as ProofOfPossession<E, H, DoublePublicKey<E>>>::POP_DOMAIN_SEPARATION_TAG,
            E::CURVE_NAME,
            E::SIG_GROUP_NAME,
            E::CIPHER_SUIT_DOMAIN_SEPARATION,
            PROOF_OF_POSSESSION_CONTEXT,
        ]
        .concat()
    }
}

//The bls proof of possession for single or double public key schemes are the same
impl<E: EngineBLS, H: DynDigest + Default + Clone>
    ProofOfPossessionGenerator<E, H, DoublePublicKey<E>, BLSPoP<E>> for Keypair<E>
{
    fn generate_pok(&mut self) -> BLSPoP<E> {
        //We simply classicaly BLS sign public key in G2 based on https://eprint.iacr.org/2022/1611
        let public_key_as_bytes = self.public.to_bytes();
        let sigma_pop = self.sign(&Message::new(
            <BLSPoP<E>>::bls_pop_context::<H>().as_slice(),
            &public_key_as_bytes.as_slice(),
        ));

        BLSPoP::<E>(sigma_pop.0)
    }
}

/// The verification process for verifying both possession of one secret key
/// for two public key is different.
impl<E: EngineBLS, H: DynDigest + Default + Clone> ProofOfPossession<E, H, DoublePublicKey<E>>
    for BLSPoP<E>
{
    const POP_DOMAIN_SEPARATION_TAG: &'static [u8] = b"BLS_POP_";
    //E::CURVE_NAME, E::SIG_GROUP_NAME, E::CIPHER_SUIT_DOMAIN_SEPARATION,
    // will do in runtime.
    /// verify the validity of PoP by performing the following Pairing
    /// e(H_pop(pk_2) + t.g_1, pk_2) = e(sign(H_pop(pk_2))+ t.pk_1, g_2)
    /// we verifying by calling the verify_prepared ⎈function from the
    /// engine.
    fn verify(&self, public_key_of_prover: &DoublePublicKey<E>) -> bool {
        //First we need to generate our randomness in a way that
        //prover is unable to predict. We assume g1 and g2 are fixed.

        let public_key_as_bytes =
            <E as EngineBLS>::public_key_point_to_byte(&public_key_of_prover.1);
        let public_key_in_signature_group = public_key_of_prover.0;
        let public_key_in_signature_group_as_bytes =
            E::signature_point_to_byte(&public_key_in_signature_group);

        let public_key_hashed_to_signature_group = Message::new(
            <BLSPoP<E>>::bls_pop_context::<H>().as_slice(),
            &public_key_as_bytes,
        )
        .hash_to_signature_curve::<E>();
        let public_key_hashed_to_signature_group_as_bytes =
            E::signature_point_to_byte(&public_key_hashed_to_signature_group);
        let mut random_oracle_seed = Vec::with_capacity(
            public_key_hashed_to_signature_group_as_bytes.len()
                + public_key_as_bytes.len()
                + public_key_in_signature_group_as_bytes.len()
                + E::SIGNATURE_SERIALIZED_SIZE,
        );

        random_oracle_seed.extend_from_slice(&public_key_hashed_to_signature_group_as_bytes);
        random_oracle_seed.extend_from_slice(&public_key_as_bytes);
        random_oracle_seed.extend_from_slice(&public_key_in_signature_group_as_bytes);
        random_oracle_seed.extend_from_slice(&E::signature_point_to_byte(&self.0));

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);

        let randomization_coefficient: E::Scalar =
            hasher.hash_to_field(random_oracle_seed.as_slice(), 1)[0];

        let mut randomized_pub_in_g1 = public_key_in_signature_group;
        randomized_pub_in_g1 *= randomization_coefficient;
        let signature = E::prepare_signature(self.0 + randomized_pub_in_g1);
        let prepared_public_key = E::prepare_public_key(public_key_of_prover.1);
        let prepared = [
            (
                prepared_public_key.clone(),
                E::prepare_signature(public_key_hashed_to_signature_group),
            ),
            (
                prepared_public_key.clone(),
                E::prepare_signature(E::generator_of_signature_group() * randomization_coefficient),
            ),
        ];
        E::verify_prepared(signature, prepared.iter())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use crate::double::DoublePublicKeyScheme;
    use crate::engine::ZBLS;
    use crate::single::Keypair;
    use crate::{double_pop::BLSPoP, DoublePublicKey};
    use crate::{ProofOfPossession, ProofOfPossessionGenerator};

    use rand::thread_rng;
    use sha2::Sha256;

    #[test]
    fn nugget_bls_pop_sign() {
        let mut keypair = Keypair::<ZBLS>::generate(thread_rng());
        <Keypair<ZBLS> as ProofOfPossessionGenerator<
            ZBLS,
            Sha256,
            DoublePublicKey<ZBLS>,
            BLSPoP<ZBLS>,
        >>::generate_pok(&mut keypair);
    }

    #[test]
    fn nugget_bls_pop_sign_and_verify() {
        let mut keypair = Keypair::<ZBLS>::generate(thread_rng());
        let proof_pair = <dyn ProofOfPossessionGenerator<
            ZBLS,
            Sha256,
            DoublePublicKey<ZBLS>,
            BLSPoP<ZBLS>,
        >>::generate_pok(&mut keypair);
        assert!(
            ProofOfPossession::<ZBLS, Sha256, DoublePublicKey::<ZBLS>>::verify(
                &proof_pair,
                &DoublePublicKeyScheme::into_double_public_key(&keypair)
            ),
            "valid pok does not verify"
        );
    }

    #[test]
    fn nugget_bls_pop_of_random_public_key_should_fail() {
        use crate::{ProofOfPossession, ProofOfPossessionGenerator};

        let mut keypair_good = Keypair::<ZBLS>::generate(thread_rng());
        let proof_pair = <dyn ProofOfPossessionGenerator<
            ZBLS,
            Sha256,
            DoublePublicKey<ZBLS>,
            BLSPoP<ZBLS>,
        >>::generate_pok(&mut keypair_good);
        let keypair_bad = Keypair::<ZBLS>::generate(thread_rng());
        assert!(
            !ProofOfPossession::<ZBLS, Sha256, DoublePublicKey::<ZBLS>>::verify(
                &proof_pair,
                &DoublePublicKeyScheme::into_double_public_key(&keypair_bad)
            ),
            "invalid pok of unrelated public key should not verify"
        );
    }
}
