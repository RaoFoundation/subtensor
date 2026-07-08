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
    #![allow(
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used
    )]
    use super::*;
    use crate::mock::{addr_from_index, new_test_ext};
    use precompile_utils::testing::MockHandle;
    use serde::Deserialize;
    use sp_core::{H160, U256};

    /// KZG infinity point (`0xc0 << 376`), from EEST `common.INF_POINT`.
    const INF_POINT: [u8; 48] = hex!(
        "c00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    );

    /// Valid evaluation point from EEST `common.Z`.
    const Z: [u8; 32] = hex!("623ce31cf9759a5c8daf3a357992f9f3dd7f9339d8998bc8e68373e54f00b75e");

    const EXPECTED_OUTPUT: [u8; 64] = hex!(
        "0000000000000000000000000000000000000000000000000000000000001000"
        "73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001"
    );

    #[derive(Deserialize)]
    struct VectorInput {
        commitment: String,
        proof: String,
        z: String,
        y: String,
    }

    #[derive(Deserialize)]
    struct ExternalVector {
        name: String,
        input: VectorInput,
        /// `true` = success, `false`/`null` = failure (null usually means
        /// deserialization reject).
        output: Option<bool>,
    }

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

    fn decode_hex(s: &str) -> Vec<u8> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(s).expect("valid hex")
    }

    /// Mirrors EEST `Spec.kzg_to_versioned_hash`.
    fn kzg_to_versioned_hash(commitment: &[u8], version: u8) -> [u8; 32] {
        let commitment_hash = Sha256::digest(commitment);
        let mut versioned_hash = [0u8; 32];
        versioned_hash[0] = version;
        versioned_hash[1..].copy_from_slice(&commitment_hash[1..]);
        versioned_hash
    }

    /// Mirrors EEST `precompile_input` fixture.
    fn precompile_input(
        versioned_hash: Option<&[u8]>,
        z: &[u8],
        y: &[u8],
        commitment: &[u8],
        proof: &[u8],
    ) -> Vec<u8> {
        let vh = versioned_hash
            .map(|v| v.to_vec())
            .unwrap_or_else(|| kzg_to_versioned_hash(commitment, 0x01).to_vec());
        let mut input =
            Vec::with_capacity(vh.len() + z.len() + y.len() + commitment.len() + proof.len());
        input.extend_from_slice(&vh);
        input.extend_from_slice(z);
        input.extend_from_slice(y);
        input.extend_from_slice(commitment);
        input.extend_from_slice(proof);
        input
    }

    fn bls_modulus_be() -> [u8; 32] {
        BLS_MODULUS_BYTES
    }

    fn assert_success(input: Vec<u8>) {
        let result = execute_point_eval(input);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
        assert_eq!(result.unwrap(), EXPECTED_OUTPUT);
    }

    fn assert_failure(input: Vec<u8>) {
        let result = execute_point_eval(input);
        assert!(result.is_err(), "Expected Err, got Ok({:?})", result.ok());
    }

    // ── Length / versioned-hash sanity ───────────────────────────────────────

    #[test]
    fn point_evaluation_requires_192_bytes() {
        new_test_ext().execute_with(|| {
            assert_failure(vec![0u8; 100]);
        });
    }

    #[test]
    fn point_evaluation_rejects_invalid_versioned_hash() {
        new_test_ext().execute_with(|| {
            let mut input = vec![0u8; 192];
            input[0] = 0x00;
            assert_failure(input);
        });
    }

    #[test]
    fn point_evaluation_rejects_versioned_hash_mismatch() {
        new_test_ext().execute_with(|| {
            let mut input = vec![0u8; 192];
            input[0] = 0x01;
            input[1] = 0x42;
            assert_failure(input);
        });
    }

    // ── EEST `test_valid_inputs` ─────────────────────────────────────────────

    #[test]
    fn eest_valid_in_bounds_z() {
        // Spec.BLS_MODULUS - 1, y=0, INF_POINT commitment/proof.
        new_test_ext().execute_with(|| {
            let mut z = bls_modulus_be();
            // subtract 1 from big-endian modulus
            let mut i = 31;
            loop {
                if z[i] > 0 {
                    z[i] -= 1;
                    break;
                }
                z[i] = 0xff;
                if i == 0 {
                    break;
                }
                i -= 1;
            }
            let y = [0u8; 32];
            assert_success(precompile_input(None, &z, &y, &INF_POINT, &INF_POINT));
        });
    }

    #[test]
    fn eest_valid_mainnet_1() {
        // Mainnet tx
        // https://etherscan.io/tx/0xcb3dc8f3b14f1cda0c16a619a112102a8ec70dce1b3f1b28272227cf8d5fbb0e
        new_test_ext().execute_with(|| {
            let z = hex!("019123bcb9d06356701f7be08b4494625b87a7b02edc566126fb81f6306e915f");
            let y = hex!("6c2eb1e94c2532935b8465351ba1bd88eabe2b3fa1aadff7d1cd816e8315bd38");
            let commitment = hex!(
                "a9546d41993e10df2a7429b8490394ea9ee62807bae6f326d1044a51581306f58d4b9dfd5931e044688855280ff3799e"
            );
            let proof = hex!(
                "a2ea83d9391e0ee42e0c650acc7a1f842a7d385189485ddb4fd54ade3d9fd50d608167dca6c776aad4b8ad5c20691bfe"
            );
            let versioned_hash =
                hex!("018156b94fe9735e573bab36dad05d60feb720d424ccd20aaf719343c31e4246");
            assert_success(precompile_input(
                Some(&versioned_hash),
                &z,
                &y,
                &commitment,
                &proof,
            ));
        });
    }

    // ── EEST `test_invalid_inputs` ───────────────────────────────────────────

    #[test]
    fn eest_invalid_out_of_bounds_z() {
        new_test_ext().execute_with(|| {
            let z = bls_modulus_be();
            let y = [0u8; 32];
            assert_failure(precompile_input(None, &z, &y, &INF_POINT, &INF_POINT));
        });
    }

    #[test]
    fn eest_invalid_out_of_bounds_y() {
        new_test_ext().execute_with(|| {
            let z = [0u8; 32];
            let y = bls_modulus_be();
            assert_failure(precompile_input(None, &z, &y, &INF_POINT, &INF_POINT));
        });
    }

    #[test]
    fn eest_invalid_proof_input_too_short() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let proof = &INF_POINT[..47];
            assert_failure(precompile_input(None, &Z, &y, &INF_POINT, proof));
        });
    }

    #[test]
    fn eest_invalid_proof_input_too_short_2() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let proof = &INF_POINT[..1];
            assert_failure(precompile_input(None, &Z, &y, &INF_POINT, proof));
        });
    }

    #[test]
    fn eest_invalid_proof_input_too_long() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let mut proof = INF_POINT.to_vec();
            proof.push(0);
            assert_failure(precompile_input(None, &Z, &y, &INF_POINT, &proof));
        });
    }

    #[test]
    fn eest_invalid_proof_input_extra_long() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let mut proof = INF_POINT.to_vec();
            proof.extend(vec![0u8; 1023]);
            assert_failure(precompile_input(None, &Z, &y, &INF_POINT, &proof));
        });
    }

    #[test]
    fn eest_invalid_null_inputs() {
        new_test_ext().execute_with(|| {
            assert_failure(precompile_input(Some(&[]), &[], &[], &[], &[]));
        });
    }

    #[test]
    fn eest_invalid_zeros_inputs() {
        new_test_ext().execute_with(|| {
            let zeros32 = [0u8; 32];
            let zeros48 = [0u8; 48];
            assert_failure(precompile_input(
                Some(&zeros32),
                &zeros32,
                &zeros32,
                &zeros48,
                &zeros48,
            ));
        });
    }

    #[test]
    fn eest_invalid_zeros_inputs_correct_versioned_hash() {
        new_test_ext().execute_with(|| {
            let zeros32 = [0u8; 32];
            let zeros48 = [0u8; 48];
            assert_failure(precompile_input(
                None, &zeros32, &zeros32, &zeros48, &zeros48,
            ));
        });
    }

    #[test]
    fn eest_invalid_versioned_hash_version_0x00() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let vh = kzg_to_versioned_hash(&INF_POINT, 0x00);
            assert_failure(precompile_input(Some(&vh), &Z, &y, &INF_POINT, &INF_POINT));
        });
    }

    #[test]
    fn eest_invalid_versioned_hash_version_0x02() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let vh = kzg_to_versioned_hash(&INF_POINT, 0x02);
            assert_failure(precompile_input(Some(&vh), &Z, &y, &INF_POINT, &INF_POINT));
        });
    }

    #[test]
    fn eest_invalid_versioned_hash_version_0xff() {
        new_test_ext().execute_with(|| {
            let y = [0u8; 32];
            let vh = kzg_to_versioned_hash(&INF_POINT, 0xff);
            assert_failure(precompile_input(Some(&vh), &Z, &y, &INF_POINT, &INF_POINT));
        });
    }

    // ── EEST `test_external_vectors` (go-kzg-4844 / consensus-specs) ─────────

    #[test]
    fn eest_external_go_kzg_4844_verify_kzg_proof_vectors() {
        new_test_ext().execute_with(|| {
            let raw = include_str!("testdata/go_kzg_4844_verify_kzg_proof.json");
            let vectors: Vec<ExternalVector> =
                serde_json::from_str(raw).expect("parse go_kzg_4844 vectors");
            assert_eq!(
                vectors.len(),
                122,
                "expected all well-known external vectors"
            );

            let mut success_count = 0usize;
            let mut failure_count = 0usize;

            for vector in vectors {
                let commitment = decode_hex(&vector.input.commitment);
                let proof = decode_hex(&vector.input.proof);
                let z = decode_hex(&vector.input.z);
                let y = decode_hex(&vector.input.y);
                let input = precompile_input(None, &z, &y, &commitment, &proof);
                let result = execute_point_eval(input);
                let expect_success = vector.output == Some(true);
                if expect_success {
                    assert!(
                        result.is_ok(),
                        "vector {} should succeed, got {:?}",
                        vector.name,
                        result.err()
                    );
                    assert_eq!(
                        result.unwrap(),
                        EXPECTED_OUTPUT,
                        "vector {} output mismatch",
                        vector.name
                    );
                    success_count += 1;
                } else {
                    assert!(
                        result.is_err(),
                        "vector {} should fail, got Ok",
                        vector.name
                    );
                    failure_count += 1;
                }
            }

            assert_eq!(success_count, 54);
            assert_eq!(failure_count, 68);
        });
    }

    // ── Local synthetic vectors (pre-existing) ───────────────────────────────

    #[test]
    fn point_evaluation_success_identity() {
        new_test_ext().execute_with(|| {
            use ark_serialize::CanonicalSerialize;

            // Constant polynomial f(x) = 100.
            // Commitment = 100 * G1, so C - y*G1 = 0.
            // Proof = identity (G1 point at infinity).
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

            let y_fr = Fr::from_be_bytes_mod_order(&y_bytes);
            let g1_gen = G1Affine::generator();
            let commitment = (G1Projective::from(g1_gen) * y_fr).into_affine();

            let mut commitment_bytes = [0u8; 48];
            commitment
                .serialize_compressed(&mut commitment_bytes[..])
                .unwrap();

            let mut proof_bytes = [0u8; 48];
            G1Affine::identity()
                .serialize_compressed(&mut proof_bytes[..])
                .unwrap();

            assert_success(precompile_input(
                None,
                &z_bytes,
                &y_bytes,
                &commitment_bytes,
                &proof_bytes,
            ));
        });
    }

    #[test]
    fn point_evaluation_nontrivial() {
        new_test_ext().execute_with(|| {
            use ark_serialize::CanonicalSerialize;

            // Build a test-only SRS whose secret is derived from a public
            // nothing-up-my-sleeve string.  This is NOT the production
            // G2_SRS.
            let sk = Fr::from_be_bytes_mod_order(&Sha256::digest(
                b"KZG test vector for subtensor point_evaluation",
            ));
            let a = Fr::from(7u64);
            let b = Fr::from(42u64);
            let z_fr = Fr::from(3u64);
            let y_fr = a * z_fr + b;

            let g1_gen = G1Affine::generator();
            let g2_gen = G2Affine::generator();

            let s_g1 = (G1Projective::from(g1_gen) * sk).into_affine();
            let s_g2 = (G2Projective::from(g2_gen) * sk).into_affine();

            let commitment =
                (G1Projective::from(s_g1) * a + G1Projective::from(g1_gen) * b).into_affine();
            let proof = (G1Projective::from(g1_gen) * a).into_affine();

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
            assert_ne!(p_minus_y, G1Affine::identity());
            assert_ne!(proof, G1Affine::identity());

            let mut z_bytes = [0u8; 32];
            z_bytes[31] = 3;
            let mut y_bytes = [0u8; 32];
            y_bytes[31] = 63;

            let mut commitment_bytes = [0u8; 48];
            commitment
                .serialize_compressed(&mut commitment_bytes[..])
                .unwrap();
            let mut proof_bytes = [0u8; 48];
            proof.serialize_compressed(&mut proof_bytes[..]).unwrap();
            let mut srs_bytes = [0u8; 96];
            s_g2.serialize_compressed(&mut srs_bytes[..]).unwrap();

            let input = precompile_input(None, &z_bytes, &y_bytes, &commitment_bytes, &proof_bytes);

            let result = PointEvaluation::execute_impl(&input, &srs_bytes);
            assert!(result.is_ok(), "nontrivial vector must pass with known SRS");
            assert_eq!(result.unwrap().output, EXPECTED_OUTPUT);

            let real_reject = PointEvaluation::execute_impl(&input, &G2_SRS);
            assert!(real_reject.is_err(), "must fail with wrong SRS");
        });
    }
}
