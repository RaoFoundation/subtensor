//! Mode-specific authorization and exact wire compatibility checks.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::keys::{verify_hashed_authorization, Keypair, CRYPTO_HASHED, CRYPTO_HASHED_ED25519};
use crate::runtime::ConstantInfo;

fn supported_runtime() -> Runtime {
    let mut runtime = super::hashed_tests::runtime(true);
    runtime
        .pallets
        .iter_mut()
        .find(|p| p.name == "HashedAccounts")
        .unwrap()
        .constants
        .push(ConstantInfo {
            name: "ClassicalSignerVersions".into(),
            ty: 0,
            value: vec![1, subtensor_hashed::WRAPPED_SIGNER_VERSION],
            docs: vec![],
        });
    runtime
}

fn parameters() -> TxParams {
    TxParams {
        era: Value::str("00"),
        nonce: 7,
        tip: 0,
        tip_asset_id: None,
        genesis_hash: [8; 32],
        era_block_hash: [8; 32],
        metadata_hash: None,
    }
}

#[test]
fn wrapped_classical_proofs_preserve_wire_shape_and_bind_mode_and_full_payload() {
    for crypto in [CRYPTO_HASHED, CRYPTO_HASHED_ED25519] {
        let runtime = supported_runtime();
        let key = Keypair::from_seed(&[91; 32], crypto)
            .unwrap()
            .at_generation(129)
            .unwrap();
        let descriptor = key.hashed_descriptor().unwrap();
        let commitment = key.hashed_current_commitment().unwrap();
        let call = runtime
            .compose_call(
                "System",
                "remark",
                &Value::record(vec![("remark".into(), Value::Bytes(vec![42; 300]))]),
            )
            .unwrap();
        let (extra, implicit) = runtime.signature_payload_parts(&parameters()).unwrap();
        let payload = [call.as_slice(), extra.as_slice(), implicit.as_slice()].concat();
        let raw = key.sign_hashed(&payload).unwrap();
        let wrapped = key.sign_hashed_wrapped(&payload).unwrap();
        assert_eq!(raw.len(), 136);
        // Generation 129 must not be mistaken for a wrapped proof tag.
        assert_eq!(raw[0], subtensor_hashed::WRAPPED_SIGNER_VERSION);
        assert_eq!(wrapped.len(), 137);
        assert_eq!(wrapped[0], subtensor_hashed::WRAPPED_SIGNER_VERSION);
        for proof in [&raw, &wrapped] {
            assert!(
                verify_hashed_authorization(proof, &payload, &descriptor, 129, &commitment)
                    .unwrap()
            );
            let (encoded, _) = runtime
                .encode_signed_extrinsic(
                    &call,
                    key.public_key_bytes(),
                    proof,
                    crypto,
                    &parameters(),
                )
                .unwrap();
            let mut cursor = Cursor::new(&encoded);
            compact_u128(&mut cursor).unwrap();
            assert_eq!(cursor.byte().unwrap(), 0x84);
            assert_eq!(cursor.byte().unwrap(), 2);
            assert_eq!(compact_u128(&mut cursor).unwrap(), 106);
            let address =
                subtensor_hashed::SignerAddress::from_bytes(cursor.take(106).unwrap()).unwrap();
            assert_eq!(address.account, key.public_key_bytes());
            assert_eq!(address.version, if proof.len() == 137 { 0x81 } else { 1 });
            assert_eq!(
                cursor.byte().unwrap(),
                if crypto == CRYPTO_HASHED { 1 } else { 0 }
            );
            assert!(runtime.decode_extrinsic(&encoded, true).is_ok());
            assert!(encoded.ends_with(&call));
        }
        assert!(!verify_hashed_authorization(
            &wrapped[1..],
            &payload,
            &descriptor,
            129,
            &commitment
        )
        .unwrap());
        let relabelled = [vec![0x81], raw.clone()].concat();
        assert!(
            !verify_hashed_authorization(&relabelled, &payload, &descriptor, 129, &commitment)
                .unwrap()
        );
        let mut changed = payload.clone();
        changed[300] ^= 1;
        assert!(
            !verify_hashed_authorization(&wrapped, &changed, &descriptor, 129, &commitment)
                .unwrap()
        );
        let mut wrong_next = wrapped.clone();
        wrong_next[1 + 40] ^= 1;
        assert!(
            !verify_hashed_authorization(&wrong_next, &payload, &descriptor, 129, &commitment)
                .unwrap()
        );
        assert!(
            !verify_hashed_authorization(&wrapped, &payload, &descriptor, 130, &commitment)
                .unwrap()
        );
        for tag in [0, 1, 2, 0x80, 0x82, 0xff] {
            let mut malformed = wrapped.clone();
            malformed[0] = tag;
            assert!(!verify_hashed_authorization(
                &malformed,
                &payload,
                &descriptor,
                129,
                &commitment
            )
            .unwrap());
            assert!(runtime
                .encode_signed_extrinsic(
                    &call,
                    key.public_key_bytes(),
                    &malformed,
                    crypto,
                    &parameters()
                )
                .is_err());
        }
    }
}

#[test]
fn wrapped_proof_requires_explicit_runtime_support_while_raw_stays_compatible() {
    let runtime = super::hashed_tests::runtime(true);
    let key = Keypair::from_seed(&[92; 32], CRYPTO_HASHED_ED25519).unwrap();
    let call = [0, 0];
    let raw = key.sign_hashed(b"full payload").unwrap();
    let wrapped = key.sign_hashed_wrapped(b"full payload").unwrap();
    assert!(runtime
        .encode_signed_extrinsic(
            &call,
            key.public_key_bytes(),
            &raw,
            CRYPTO_HASHED_ED25519,
            &parameters()
        )
        .is_ok());
    assert!(runtime
        .encode_signed_extrinsic(
            &call,
            key.public_key_bytes(),
            &wrapped,
            CRYPTO_HASHED_ED25519,
            &parameters()
        )
        .is_err());
}
