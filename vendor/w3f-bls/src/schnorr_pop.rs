//! ## Implementation of ProofofPossion trait for BLS keys using schnorr sginature
//! ## TODO: I assume this can also moved to pop.rs but for now I put it separately to help reviews
use ark_ff::field_hashers::{DefaultFieldHasher, HashToField};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

use crate::engine::EngineBLS;
use crate::{ProofOfPossession, ProofOfPossessionGenerator};

use crate::serialize::SerializableToBytes;
use crate::single::{Keypair, PublicKey};

use alloc::vec::Vec;
use digest::DynDigest;

use ark_ec::Group;

pub type SchnorrProof<E> = (<E as EngineBLS>::Scalar, <E as EngineBLS>::Scalar);

#[derive(Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct SchnorrPoP<E: EngineBLS>(SchnorrProof<E>);

impl<E: EngineBLS> Clone for SchnorrPoP<E> {
    fn clone(&self) -> Self {
        SchnorrPoP(self.0)
    }
}

/// Generate Schnorr Signature for an arbitrary message using a key ment to use in BLS scheme
trait BLSSchnorrPoPGenerator<E: EngineBLS, H: DynDigest + Default + Clone>:
    ProofOfPossessionGenerator<E, H, PublicKey<E>, SchnorrPoP<E>>
{
    /// Produce a secret witness scalar `k`, aka nonce, from hash of
    /// H( H(s) | H(public_key)) because our key does not have the
    /// randomness redundacy exists in EdDSA secret key.
    fn witness_scalar(&self) -> <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField;
}

impl<E: EngineBLS, H: DynDigest + Default + Clone> BLSSchnorrPoPGenerator<E, H> for Keypair<E> {
    //The pseudo random witness is generated similar to eddsa witness
    //hash(secret_key|publick_key)
    fn witness_scalar(&self) -> <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField {
        let secret_key_as_bytes = self.secret.to_bytes();
        let public_key_as_bytes = <E as EngineBLS>::public_key_point_to_byte(&self.public.0);

        let mut secret_key_hasher = H::default();
        secret_key_hasher.update(secret_key_as_bytes.as_slice());
        let hashed_secret_key = secret_key_hasher.finalize_reset().to_vec();

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);

        let scalar_seed = [hashed_secret_key, public_key_as_bytes].concat();
        hasher.hash_to_field(scalar_seed.as_slice(), 1)[0]
    }
}

impl<E: EngineBLS, H: DynDigest + Default + Clone>
    ProofOfPossessionGenerator<E, H, PublicKey<E>, SchnorrPoP<E>> for Keypair<E>
{
    //TODO: Message must be equal to public key.
    fn generate_pok(&mut self) -> SchnorrPoP<E> {
        //First we should figure out the base point in E, I think the secret key trait/struct knows about it.

        //choose random scaler k
        //For now we don't we just use a trick similar to Ed25519
        //we use hash of concatination of hash the secret key and the public key

        //schnorr equations

        //R = rG.
        //k = H(R|M)
        //s = k*private_key + r
        // publishing (s, R) verifying that (s*G = H(R|M)*Publickey + R => H(R|M)*Publickey + R - s*G = 0)
        // so either we need to two into_affine and one curve addition or or two curve additions.
        // instead we actually doing H(s*G - H(R|M)*Publickey|M) == H(R|M) == k
        // avoiding one curve addition (or two field divisions) in expense of a hash.
        let mut r = <dyn BLSSchnorrPoPGenerator<E, H>>::witness_scalar(self);

        let mut r_point = <<E as EngineBLS>::PublicKeyGroup as Group>::generator();
        r_point *= r; //todo perhaps we need to mandate E to have  a hard coded point

        let r_point_as_bytes = <E as EngineBLS>::public_key_point_to_byte(&r_point);
        let public_key_as_bytes = <E as EngineBLS>::public_key_point_to_byte(&self.public.0); //it *must* be the public key (fixed) otherwise secret key can be recovered from the two different proves

        let proof_basis = [r_point_as_bytes, public_key_as_bytes].concat();
        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);
        let k = hasher.hash_to_field(proof_basis.as_slice(), 1)[0];

        let s = (k * self.secret.into_vartime().0) + r;

        ::zeroize::Zeroize::zeroize(&mut r); //clear secret witness from memory

        SchnorrPoP::<E>((s, k))
    }
}

impl<E: EngineBLS, H: DynDigest + Default + Clone> ProofOfPossession<E, H, PublicKey<E>>
    for SchnorrPoP<E>
{
    const POP_DOMAIN_SEPARATION_TAG: &'static [u8] = b"SCHNORR_POP_XMD:SHA-256_RO_POP_";
    /// verify the validity of schnoor proof for a given publick key by
    /// making sure this is equal to zero
    /// H(+s*G - k*Publkey|M) ==  k  
    fn verify(&self, public_key_of_prover: &PublicKey<E>) -> bool {
        let mut schnorr_point = <<E as EngineBLS>::PublicKeyGroup as Group>::generator();
        schnorr_point *= self.0 .0;
        let mut k_public_key = public_key_of_prover.0;
        k_public_key *= -self.0 .1;
        schnorr_point += k_public_key;

        let schnorr_point_as_bytes = <E as EngineBLS>::public_key_point_to_byte(&schnorr_point);
        let public_key_as_bytes =
            <E as EngineBLS>::public_key_point_to_byte(&public_key_of_prover.0); //it *must* be the public key (fixed) otherwise secret key can be recovered from the two different proves

        let resulting_proof_basis = [schnorr_point_as_bytes, public_key_as_bytes].concat();

        let hasher = <DefaultFieldHasher<H> as HashToField<
            <<E as EngineBLS>::PublicKeyGroup as Group>::ScalarField,
        >>::new(&[]);
        let random_scalar: E::Scalar = hasher.hash_to_field(resulting_proof_basis.as_slice(), 1)[0];
        random_scalar == self.0 .1
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::SchnorrPoP;
    use crate::engine::ZBLS;
    use crate::single::{Keypair, PublicKey};
    use crate::ProofOfPossessionGenerator;
    use rand::thread_rng;
    use sha2::Sha512;

    #[test]
    fn schnorr_bls_pop_sign() {
        let mut keypair = Keypair::<ZBLS>::generate(thread_rng());
        <Keypair<ZBLS> as ProofOfPossessionGenerator<
            ZBLS,
            Sha512,
            PublicKey<ZBLS>,
            SchnorrPoP<ZBLS>,
        >>::generate_pok(&mut keypair);
    }

    #[test]
    fn schnorr_bls_pop_sign_and_verify() {
        use crate::{ProofOfPossession, ProofOfPossessionGenerator};

        let mut keypair = Keypair::<ZBLS>::generate(thread_rng());
        let proof_pair = <dyn ProofOfPossessionGenerator<
            ZBLS,
            Sha512,
            PublicKey<ZBLS>,
            SchnorrPoP<ZBLS>,
        >>::generate_pok(&mut keypair);
        assert!(
            ProofOfPossession::<ZBLS, Sha512, PublicKey::<ZBLS>>::verify(
                &proof_pair,
                &keypair.public
            ),
            "valid pok does not verify"
        );
    }

    #[test]
    fn schnorr_bls_pop_of_random_public_key_should_fail() {
        use crate::{ProofOfPossession, ProofOfPossessionGenerator};

        let mut keypair_good = Keypair::<ZBLS>::generate(thread_rng());
        let proof_pair = <dyn ProofOfPossessionGenerator<
            ZBLS,
            Sha512,
            PublicKey<ZBLS>,
            SchnorrPoP<ZBLS>,
        >>::generate_pok(&mut keypair_good);
        let keypair_bad = Keypair::<ZBLS>::generate(thread_rng());
        assert!(
            !ProofOfPossession::<ZBLS, Sha512, PublicKey::<ZBLS>>::verify(
                &proof_pair,
                &keypair_bad.public
            ),
            "invalid pok of unrelated public key should not verify"
        );
    }
}
