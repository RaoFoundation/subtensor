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

impl Precompile for PointEvaluation {
    fn execute(handle: &mut impl PrecompileHandle) -> PrecompileResult {
        handle.record_cost(GAS_COST)?;

        if handle.input().len() != INPUT_LEN {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("input must be 192 bytes".into()),
            });
        }

        let mut input = [0u8; INPUT_LEN];
        input.copy_from_slice(handle.input());

        let mut versioned_hash = [0u8; 32];
        versioned_hash.copy_from_slice(&input[0..32]);

        let z = &input[32..64];
        let y = &input[64..96];
        let commitment_bytes = &input[96..144];
        let proof_bytes = &input[144..INPUT_LEN];

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

        let g2_srs = G2Affine::deserialize_compressed(&G2_SRS[..]).map_err(|_| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other("invalid G2 SRS".into()),
            }
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
    fn point_evaluation_success() {
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
        });
    }
}
