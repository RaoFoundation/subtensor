//! Tests for `Pallet::try_decode_shielded_tx` extrinsic parsing.

use crate::mock::*;
use frame_support::BoundedVec;
use sp_runtime::testing::TestSignature;

#[test]
fn try_decode_shielded_tx_parses_bare_submit_encrypted() {
    new_test_ext().execute_with(|| {
        let key_hash = [0xAB; 16];
        let kem_ct = vec![0xCC; 32];
        let nonce = [0xDD; 24];
        let aead_ct = vec![0xEE; 64];

        let ciphertext = build_wire_ciphertext(&key_hash, &kem_ct, &nonce, &aead_ct);
        let call = RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: BoundedVec::truncate_from(ciphertext),
        });
        let uxt = DecodableExtrinsic::new_bare(call);

        let result = crate::Pallet::<Test>::try_decode_shielded_tx::<
            DecodableBlock,
            frame_system::ChainContext<Test>,
        >(uxt);
        assert!(result.is_some());

        let shielded = result.unwrap();
        assert_eq!(shielded.key_hash, key_hash);
        assert_eq!(shielded.kem_ct, kem_ct);
        assert_eq!(shielded.nonce, nonce);
        assert_eq!(shielded.aead_ct, aead_ct);
    });
}

#[test]
fn try_decode_shielded_tx_returns_none_for_non_shield_call() {
    new_test_ext().execute_with(|| {
        let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
        let uxt = DecodableExtrinsic::new_bare(call);

        let result = crate::Pallet::<Test>::try_decode_shielded_tx::<
            DecodableBlock,
            frame_system::ChainContext<Test>,
        >(uxt);
        assert!(result.is_none());
    });
}

#[test]
fn try_decode_shielded_tx_returns_none_for_bad_signature() {
    new_test_ext().execute_with(|| {
        let ciphertext = build_wire_ciphertext(&[0xAB; 16], &[0xCC; 32], &[0xDD; 24], &[0xEE; 64]);
        let call = RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: BoundedVec::truncate_from(ciphertext),
        });
        let bad_sig = TestSignature(1, vec![0xFF; 32]);
        let uxt = DecodableExtrinsic::new_signed(call, 1u64, bad_sig, ());

        let result = crate::Pallet::<Test>::try_decode_shielded_tx::<
            DecodableBlock,
            frame_system::ChainContext<Test>,
        >(uxt);
        assert!(result.is_none());
    });
}

#[test]
fn try_decode_shielded_tx_returns_none_for_malformed_ciphertext() {
    new_test_ext().execute_with(|| {
        let call = RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: BoundedVec::truncate_from(vec![0u8; 5]),
        });
        let uxt = DecodableExtrinsic::new_bare(call);

        let result = crate::Pallet::<Test>::try_decode_shielded_tx::<
            DecodableBlock,
            frame_system::ChainContext<Test>,
        >(uxt);
        assert!(result.is_none());
    });
}

#[test]
fn try_decode_shielded_tx_returns_none_when_depth_exceeded() {
    new_test_ext().execute_with(|| {
        let ciphertext = build_wire_ciphertext(&[0xAB; 16], &[0xCC; 32], &[0xDD; 24], &[0xEE; 64]);
        let inner = RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: BoundedVec::truncate_from(ciphertext),
        });
        let call = nest_call(inner, 8);
        let uxt = DecodableExtrinsic::new_bare(call);

        let result = crate::Pallet::<Test>::try_decode_shielded_tx::<
            DecodableBlock,
            frame_system::ChainContext<Test>,
        >(uxt);
        assert!(result.is_none());
    });
}
