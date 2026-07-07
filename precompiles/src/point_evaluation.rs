#![allow(clippy::arithmetic_side_effects)]
extern crate alloc;

use alloc::vec::Vec;

use fp_evm::{
    ExitError, ExitSucceed, Precompile, PrecompileFailure, PrecompileHandle, PrecompileOutput,
    PrecompileResult,
};

use ark_bls12_381::{Bls12_381, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::pairing::Pairing;
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{One, PrimeField};
use ark_serialize::CanonicalDeserialize;

use hex_literal::hex;
use sha2::{Digest, Sha256};

const GAS_COST: u64 = 50_000;

const INPUT_LEN: usize = 192;

const G2_SRS: [u8; 96] = hex!(
    "b5bfd7dd8cdeb128843bc287230af38926187075cbfbefa81009a2ce615ac53d2914e5870cb452d2afaaab24f3499f72185cbfee53492714734429b7b38608e23926c911cceceac9a36851477ba4c60b087041de621000edc98edada20c1def2"
);

const FIELD_ELEMENTS_PER_BLOB_BYTES: [u8; 32] = {
    let mut buf = [0u8; 32];
    buf[30] = 0x10;
    buf[31] = 0x00;
    buf
};

const BLS_MODULUS_BYTES: [u8; 32] =
    hex!("73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001");

pub struct PointEvaluation;

impl PointEvaluation {
    fn execute_impl(input: &[u8], g2_srs: &[u8]) -> PrecompileResult {
        if input.len() != INPUT_LEN {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("input must be 192 bytes".into()),
            });
        }

        let mut buf = [0u8; INPUT_LEN];
        buf.copy_from_slice(input);

        let mut versioned_hash = [0u8; 32];
        versioned_hash.copy_from_slice(&buf[0..32]);

        let z = &buf[32..64];
        let y = &buf[64..96];
        let commitment_bytes = &buf[96..144];
        let proof_bytes = &buf[144..INPUT_LEN];

        let commitment_hash = Sha256::digest(commitment_bytes);
        if versioned_hash[0] != 0x01 || versioned_hash.get(1..) != commitment_hash.get(1..) {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("versioned hash mismatch".into()),
            });
        }

        let deserialize_bls_scalar = |bytes: &[u8]| {
            if bytes >= BLS_MODULUS_BYTES.as_slice() {
                return Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other("invalid field element".into()),
                });
            }
            Ok(Fr::from_be_bytes_mod_order(bytes))
        };
        let z_fr = deserialize_bls_scalar(z)?;
        let y_fr = deserialize_bls_scalar(y)?;

        let commitment = G1Affine::deserialize_compressed(commitment_bytes).map_err(|_| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other("invalid commitment".into()),
            }
        })?;

        let proof = G1Affine::deserialize_compressed(proof_bytes).map_err(|_| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other("invalid proof".into()),
            }
        })?;

        let g2_srs =
            G2Affine::deserialize_compressed(g2_srs).map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other("invalid G2 SRS".into()),
            })?;

        let g1_gen = G1Affine::generator();
        let g2_gen = G2Affine::generator();

        #[allow(clippy::arithmetic_side_effects)]
        let p_minus_y =
            (G1Projective::from(commitment) - G1Projective::from(g1_gen) * y_fr).into_affine();

        let neg_g2 = -g2_gen;

        let s_minus_z =
            (G2Projective::from(g2_srs) + G2Projective::from(g2_gen) * (-z_fr)).into_affine();

        let valid = Bls12_381::multi_pairing([p_minus_y, proof], [neg_g2, s_minus_z])
            .0
            .is_one();

        if !valid {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("KZG proof verification failed".into()),
            });
        }

        let mut output = Vec::with_capacity(64);
        output.extend_from_slice(&FIELD_ELEMENTS_PER_BLOB_BYTES);
        output.extend_from_slice(&BLS_MODULUS_BYTES);

        Ok(PrecompileOutput {
            exit_status: ExitSucceed::Returned,
            output,
        })
    }
}

impl Precompile for PointEvaluation {
    fn execute(handle: &mut impl PrecompileHandle) -> PrecompileResult {
        handle.record_cost(GAS_COST)?;
        Self::execute_impl(handle.input(), &G2_SRS)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]
    use super::*;
    use crate::mock::{addr_from_index, new_test_ext};
    use precompile_utils::testing::MockHandle;
    use sp_core::{H160, U256};

    fn execute_point_eval(input: Vec<u8>) -> Result<Vec<u8>, PrecompileFailure> {
        let addr = H160::from_low_u64_be(10);
        let caller = addr_from_index(1);
        let mut handle = MockHandle::new(
            addr,
            fp_evm::Context {
                address: addr,
                caller,
                apparent_value: U256::zero(),
            },
        );
        handle.input = input;
        match PointEvaluation::execute(&mut handle) {
            Ok(output) => Ok(output.output),
            Err(e) => Err(e),
        }
    }

    const EXPECTED_OUTPUT: [u8; 64] = hex!(
        "0000000000000000000000000000000000000000000000000000000000001000"
        "73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001"
    );

    #[test]
    fn point_evaluation_requires_192_bytes() {
        new_test_ext().execute_with(|| {
            let result = execute_point_eval(vec![0u8; 100]);
            assert!(result.is_err());
        });
    }

    #[test]
    fn point_evaluation_rejects_invalid_versioned_hash() {
        new_test_ext().execute_with(|| {
            let mut input = vec![0u8; 192];
            input[0] = 0x00;
            let result = execute_point_eval(input);
            assert!(result.is_err());
        });
    }

    #[test]
    fn point_evaluation_rejects_versioned_hash_mismatch() {
        new_test_ext().execute_with(|| {
            let mut input = vec![0u8; 192];
            input[0] = 0x01;
            input[1] = 0x42;
            let result = execute_point_eval(input);
            assert!(result.is_err());
        });
    }

    #[test]
    fn point_evaluation_success_identity() {
        new_test_ext().execute_with(|| {
            use ark_serialize::CanonicalSerialize;

            // Constant polynomial f(x) = 100.
            // Commitment = 100 * G1, so C - y*G1 = 0.
            // Proof = identity (G1 point at infinity).
            // Verification: e(0, S-z*G2) = e(inf, G2) = 1.
            let z_bytes = {
                let mut buf = [0u8; 32];
                buf[31] = 42;
                buf
            };
            let y_bytes = {
                let mut buf = [0u8; 32];
                buf[31] = 100;
                buf
            };

            let _z_fr = Fr::from_be_bytes_mod_order(&z_bytes);
            let y_fr = Fr::from_be_bytes_mod_order(&y_bytes);

            let g1_gen = G1Affine::generator();
            let commitment = (G1Projective::from(g1_gen) * y_fr).into_affine();

            let mut commitment_bytes = [0u8; 48];
            commitment
                .serialize_compressed(&mut commitment_bytes[..])
                .unwrap();

            let commitment_hash = Sha256::digest(commitment_bytes);

            let mut versioned_hash = [0u8; 32];
            versioned_hash[0] = 0x01;
            versioned_hash[1..].copy_from_slice(&commitment_hash[1..]);

            let mut proof_bytes = [0u8; 48];
            G1Affine::identity()
                .serialize_compressed(&mut proof_bytes[..])
                .unwrap();

            let mut input = Vec::with_capacity(192);
            input.extend_from_slice(&versioned_hash);
            input.extend_from_slice(&z_bytes);
            input.extend_from_slice(&y_bytes);
            input.extend_from_slice(&commitment_bytes);
            input.extend_from_slice(&proof_bytes);

            let result = execute_point_eval(input);
            assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
            assert_eq!(result.unwrap(), EXPECTED_OUTPUT);
        });
    }

    #[test]
    fn point_evaluation_nontrivial() {
        new_test_ext().execute_with(|| {
            use ark_serialize::CanonicalSerialize;

            // Build a test-only SRS whose secret is derived from a public
            // nothing-up-my-sleeve string.  This is NOT the production
            // G2_SRS — nobody can forge a proof against that one (that is
            // the point of the trusted setup).  Here we verify that the
            // pairing-based verification logic correctly accepts a valid
            // degree-1 KZG proof and correctly rejects it against the
            // production SRS.
            let sk = Fr::from_be_bytes_mod_order(&Sha256::digest(b"KZG test vector for subtensor point_evaluation"));
            let a = Fr::from(7u64);
            let b = Fr::from(42u64);
            let z_fr = Fr::from(3u64);
            let y_fr = a * z_fr + b;

            let g1_gen = G1Affine::generator();
            let g2_gen = G2Affine::generator();

            let s_g1 = (G1Projective::from(g1_gen) * sk).into_affine();
            let s_g2 = (G2Projective::from(g2_gen) * sk).into_affine();

            // KZG: commitment = a*(s*G1) + b*G1, proof = a*G1
            let commitment =
                (G1Projective::from(s_g1) * a + G1Projective::from(g1_gen) * b).into_affine();
            let proof = (G1Projective::from(g1_gen) * a).into_affine();

            // Verify internal pairing math first (out of band)
            let p_minus_y =
                (G1Projective::from(commitment) - G1Projective::from(g1_gen) * y_fr).into_affine();
            let neg_g2 = -g2_gen;
            let s_minus_z =
                (G2Projective::from(s_g2) + G2Projective::from(g2_gen) * (-z_fr)).into_affine();
            assert!(
                Bls12_381::multi_pairing([p_minus_y, proof], [neg_g2, s_minus_z])
                    .0
                    .is_one(),
                "internal pairing equation must hold for the known-SRS vector"
            );

            // Verify the vector is nontrivial
            assert_ne!(p_minus_y, G1Affine::identity(), "p_minus_y must be nonzero");
            assert_ne!(proof, G1Affine::identity(), "proof must be nonzero");

            // Build the 192-byte input for execute_impl
            let mut z_bytes = [0u8; 32];
            // Fr from_be_bytes_mod_order expects big-endian bytes.  Fr::from(3) is just 3,
            // and the big-endian representation is [0,...,0,3].
            z_bytes[31] = 3;

            let mut y_bytes = [0u8; 32];
            // y = 7*3 + 42 = 63
            y_bytes[31] = 63;

            let mut commitment_bytes = [0u8; 48];
            commitment
                .serialize_compressed(&mut commitment_bytes[..])
                .unwrap();

            let commitment_hash = Sha256::digest(commitment_bytes);

            let mut versioned_hash = [0u8; 32];
            versioned_hash[0] = 0x01;
            versioned_hash[1..].copy_from_slice(&commitment_hash[1..]);

            let mut proof_bytes = [0u8; 48];
            proof.serialize_compressed(&mut proof_bytes[..]).unwrap();

            let mut srs_bytes = [0u8; 96];
            s_g2.serialize_compressed(&mut srs_bytes[..]).unwrap();

            let mut input = Vec::with_capacity(192);
            input.extend_from_slice(&versioned_hash);
            input.extend_from_slice(&z_bytes);
            input.extend_from_slice(&y_bytes);
            input.extend_from_slice(&commitment_bytes);
            input.extend_from_slice(&proof_bytes);

            // Must use execute_impl with the known SRS; the real precompile
            // uses the hardcoded SRS and would reject this.
            let result = PointEvaluation::execute_impl(&input, &srs_bytes);
            assert!(result.is_ok(), "nontrivial vector must pass with known SRS");
            assert_eq!(result.unwrap().output, EXPECTED_OUTPUT);

            // Verify the real precompile rejects it (wrong SRS)
            let real_reject = PointEvaluation::execute_impl(&input, &G2_SRS);
            assert!(real_reject.is_err(), "must fail with wrong SRS");
        });
    }
}
