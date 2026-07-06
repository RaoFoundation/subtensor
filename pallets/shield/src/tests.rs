use crate::mock::*;
use crate::{
    AuthorKeys, CurrentKey, Error, ExtrinsicLifetime, HasMigrationRun, MaxExtrinsicWeight,
    MaxPendingExtrinsicsLimit, NextKey, NextKeyExpiresAt, NextPendingExtrinsicIndex,
    OnInitializeWeight, PendingExtrinsic, PendingExtrinsics, PendingKey, PendingKeyExpiresAt,
};
use codec::Encode;
use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_runtime::testing::TestSignature;
use sp_runtime::traits::{Block as BlockT, Hash};
use stp_shield::{MLKEM768_ENC_KEY_LEN, ShieldEncKey, ShieldKeystore, ShieldedTransaction};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ml_kem::{
    EncodedSizeUser, MlKem768Params,
    kem::{Encapsulate, EncapsulationKey},
};
use rand_chacha::{ChaChaRng, rand_core::SeedableRng};
use stc_shield::MemoryShieldKeystore;

/// Simulates a 3-validator round-robin (authors 1, 2, 3) over 5 blocks.
/// Each block calls `announce_next_key` and verifies the full pipeline:
/// CurrentKey, PendingKey, NextKey, AuthorKeys, expirations, and
/// `is_shielded_using_current_key`.
#[test]
fn key_rotation_round_robin() {
    new_test_ext().execute_with(|| {
        let key_of =
            |n: u8| -> ShieldEncKey { BoundedVec::truncate_from(vec![n; MLKEM768_ENC_KEY_LEN]) };
        let hash_of = |pk: &ShieldEncKey| sp_io::hashing::twox_128(&pk[..]);

        // 3 validators in round-robin: 1, 2, 3, 1, 2.
        let authors = [1u8, 2, 3, 1, 2];
        let next_next = |block: usize| -> Option<u8> { authors.get(block + 2).copied() };

        // ── Block 1: author=1, next_next=3 ──────────────────────────────
        // Pipeline is empty; author(3) has no AuthorKeys yet.
        System::set_block_number(1);
        set_authors(Some(author(1)), next_next(0).map(author));
        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(key_of(1)),
        ));

        assert!(CurrentKey::<Test>::get().is_none());
        assert!(PendingKey::<Test>::get().is_none());
        assert!(NextKey::<Test>::get().is_none());
        assert_eq!(AuthorKeys::<Test>::get(author(1)), Some(key_of(1)));
        assert!(PendingKeyExpiresAt::<Test>::get().is_none());
        assert!(NextKeyExpiresAt::<Test>::get().is_none());
        // Nothing in PendingKey → is_shielded always false.
        assert!(!MevShield::is_shielded_using_current_key(&[0xFF; 16]));

        // ── Block 2: author=2, next_next=1 ──────────────────────────────
        // author(1) registered in block 1 → NextKey picks up key_of(1).
        System::set_block_number(2);
        set_authors(Some(author(2)), next_next(1).map(author));
        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(key_of(2)),
        ));

        assert!(CurrentKey::<Test>::get().is_none());
        assert!(PendingKey::<Test>::get().is_none());
        assert_eq!(NextKey::<Test>::get(), Some(key_of(1)));
        assert_eq!(AuthorKeys::<Test>::get(author(2)), Some(key_of(2)));
        assert!(PendingKeyExpiresAt::<Test>::get().is_none());
        assert_eq!(NextKeyExpiresAt::<Test>::get(), Some(5)); // 2 + 3

        // ── Block 3: author=3, next_next=2 ──────────────────────────────
        // NextKey(key_of(1)) → PendingKey; next_next=author(2) has key_of(2) → NextKey.
        System::set_block_number(3);
        set_authors(Some(author(3)), next_next(2).map(author));
        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(key_of(3)),
        ));

        assert!(CurrentKey::<Test>::get().is_none());
        assert_eq!(PendingKey::<Test>::get(), Some(key_of(1)));
        assert_eq!(NextKey::<Test>::get(), Some(key_of(2)));
        assert_eq!(AuthorKeys::<Test>::get(author(3)), Some(key_of(3)));
        assert_eq!(PendingKeyExpiresAt::<Test>::get(), Some(5)); // 3 + 2
        assert_eq!(NextKeyExpiresAt::<Test>::get(), Some(6)); // 3 + 3
        // PendingKey = key_of(1) → is_shielded matches its hash.
        assert!(MevShield::is_shielded_using_current_key(&hash_of(&key_of(
            1
        ))));
        assert!(!MevShield::is_shielded_using_current_key(&hash_of(
            &key_of(2)
        )));
        assert!(!MevShield::is_shielded_using_current_key(&[0xFF; 16]));

        // ── Block 4: author=1, next_next=out of bounds ──────────────────
        // Full pipeline: PendingKey(key_of(1)) → CurrentKey, NextKey(key_of(2)) → PendingKey.
        System::set_block_number(4);
        set_authors(Some(author(1)), next_next(3).map(author));
        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(key_of(1)),
        ));

        assert_eq!(CurrentKey::<Test>::get(), Some(key_of(1)));
        assert_eq!(PendingKey::<Test>::get(), Some(key_of(2)));
        assert!(NextKey::<Test>::get().is_none());
        assert_eq!(AuthorKeys::<Test>::get(author(1)), Some(key_of(1)));
        assert_eq!(PendingKeyExpiresAt::<Test>::get(), Some(6)); // 4 + 2
        assert!(NextKeyExpiresAt::<Test>::get().is_none());
        // PendingKey = key_of(2).
        assert!(MevShield::is_shielded_using_current_key(&hash_of(&key_of(
            2
        ))));
        assert!(!MevShield::is_shielded_using_current_key(&hash_of(
            &key_of(1)
        )));

        // ── Block 5: author=2, next_next=none ───────────────────────────
        // PendingKey(key_of(2)) → CurrentKey; pipeline drains.
        System::set_block_number(5);
        set_authors(Some(author(2)), None);
        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(key_of(2)),
        ));

        assert_eq!(CurrentKey::<Test>::get(), Some(key_of(2)));
        assert!(PendingKey::<Test>::get().is_none());
        assert!(NextKey::<Test>::get().is_none());
        assert!(PendingKeyExpiresAt::<Test>::get().is_none());
        assert!(NextKeyExpiresAt::<Test>::get().is_none());
    });
}

/// AuthorKeys is read *before* being updated, so when current == next_next
/// the NextKey picks up the old key, not the newly announced one.
#[test]
fn announce_rotations_use_pre_update_author_keys() {
    new_test_ext().execute_with(|| {
        set_authors(Some(author(1)), Some(author(1)));

        let old_pk = valid_pk();
        let new_pk = valid_pk_b();
        AuthorKeys::<Test>::insert(author(1), old_pk.clone());

        assert_ok!(MevShield::announce_next_key(
            RuntimeOrigin::none(),
            Some(new_pk.clone()),
        ));

        assert_eq!(NextKey::<Test>::get(), Some(old_pk));
        assert_eq!(AuthorKeys::<Test>::get(author(1)), Some(new_pk));
    });
}

#[test]
fn announce_rejects_signed_origin() {
    new_test_ext().execute_with(|| {
        set_authors(Some(author(1)), None);
        assert_noop!(
            MevShield::announce_next_key(RuntimeOrigin::signed(1), Some(valid_pk())),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

#[test]
fn announce_rejects_bad_pk_length() {
    new_test_ext().execute_with(|| {
        set_authors(Some(author(1)), None);
        let bad_pk: ShieldEncKey = BoundedVec::truncate_from(vec![0x01; 100]);

        assert_noop!(
            MevShield::announce_next_key(RuntimeOrigin::none(), Some(bad_pk)),
            Error::<Test>::BadEncKeyLen
        );
    });
}

#[test]
fn announce_none_pk_removes_author_key() {
    new_test_ext().execute_with(|| {
        set_authors(Some(author(1)), None);
        AuthorKeys::<Test>::insert(author(1), valid_pk());

        assert_ok!(MevShield::announce_next_key(RuntimeOrigin::none(), None));

        assert!(AuthorKeys::<Test>::get(author(1)).is_none());
    });
}

#[test]
fn announce_fails_when_no_current_author() {
    new_test_ext().execute_with(|| {
        set_authors(None, None);

        assert_noop!(
            MevShield::announce_next_key(RuntimeOrigin::none(), Some(valid_pk())),
            Error::<Test>::Unreachable
        );
    });
}

#[test]
fn submit_encrypted_emits_event() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let ciphertext = BoundedVec::truncate_from(vec![0xAA; 64]);
        let who: u64 = 1;

        assert_ok!(MevShield::submit_encrypted(
            RuntimeOrigin::signed(who),
            ciphertext.clone(),
        ));

        let expected_id = <Test as frame_system::Config>::Hashing::hash_of(&(who, &ciphertext));

        System::assert_last_event(
            crate::Event::<Test>::EncryptedSubmitted {
                id: expected_id,
                who,
            }
            .into(),
        );
    });
}

#[test]
fn submit_encrypted_rejects_unsigned() {
    new_test_ext().execute_with(|| {
        let ciphertext = BoundedVec::truncate_from(vec![0xAA; 64]);

        assert_noop!(
            MevShield::submit_encrypted(RuntimeOrigin::none(), ciphertext),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

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

#[test]
fn try_unshield_tx_decrypts_extrinsic() {
    let mut rng = ChaChaRng::from_seed([42u8; 32]);
    let keystore = MemoryShieldKeystore::new();

    // Client side: read the announced encapsulation key and encapsulate.
    let pk_bytes = keystore.next_enc_key().unwrap();
    let enc_key =
        EncapsulationKey::<MlKem768Params>::from_bytes(pk_bytes.as_slice().try_into().unwrap());
    let (kem_ct, shared_secret) = enc_key.encapsulate(&mut rng).unwrap();

    // Build the inner extrinsic that we'll encrypt.
    let inner_call = RuntimeCall::System(frame_system::Call::remark {
        remark: vec![1, 2, 3],
    });
    let inner_uxt = <Block as BlockT>::Extrinsic::new_bare(inner_call);
    let plaintext = inner_uxt.encode();

    // AEAD encrypt the extrinsic bytes.
    let nonce = [42u8; 24];
    let cipher = XChaCha20Poly1305::new(shared_secret.as_slice().into());
    let aead_ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[],
            },
        )
        .unwrap();

    // Roll keystore so next -> current (author side).
    keystore.roll_for_next_slot().unwrap();
    let dec_key_bytes = keystore.current_dec_key().unwrap();

    let shielded_tx = ShieldedTransaction {
        key_hash: [0u8; 16],
        kem_ct: kem_ct.as_slice().to_vec(),
        nonce,
        aead_ct,
    };

    let result = crate::Pallet::<Test>::try_unshield_tx::<Block>(dec_key_bytes, shielded_tx);
    assert!(result.is_some());

    let decoded = result.unwrap();
    assert_eq!(decoded.encode(), inner_uxt.encode());
}

// ---------------------------------------------------------------------------
// Migration tests
// ---------------------------------------------------------------------------

mod migration_tests {
    use super::*;
    use crate::migrations::migrate_clear_v1_storage::migrate_clear_v1_storage;
    use sp_io::hashing::twox_128;

    #[test]
    fn migrate_clear_v1_storage_works() {
        new_test_ext().execute_with(|| {
            // Seed legacy storage that should be cleared.
            seed_legacy_map("Submissions", 5);
            seed_legacy_map("KeyHashByBlock", 3);
            CurrentKey::<Test>::put(valid_pk());

            // Current storage that must survive.
            NextKey::<Test>::put(valid_pk());
            AuthorKeys::<Test>::insert(author(1), valid_pk_b());

            // Sanity: legacy values exist.
            assert_eq!(count_keys("Submissions"), 5);
            assert_eq!(count_keys("KeyHashByBlock"), 3);
            assert!(CurrentKey::<Test>::get().is_some());

            migrate_clear_v1_storage::<Test>();

            // Legacy storage cleared.
            assert_eq!(count_keys("Submissions"), 0);
            assert_eq!(count_keys("KeyHashByBlock"), 0);
            assert!(CurrentKey::<Test>::get().is_none());

            // Current storage untouched.
            assert_eq!(NextKey::<Test>::get(), Some(valid_pk()));
            assert_eq!(AuthorKeys::<Test>::get(author(1)), Some(valid_pk_b()));

            // Migration was recorded.
            let mig_key = BoundedVec::truncate_from(b"migrate_clear_v1_storage".to_vec());
            assert!(HasMigrationRun::<Test>::get(&mig_key));

            // Idempotent: re-run doesn't touch new data.
            CurrentKey::<Test>::put(valid_pk_b());
            migrate_clear_v1_storage::<Test>();
            assert_eq!(CurrentKey::<Test>::get(), Some(valid_pk_b()));
        });
    }

    fn seed_legacy_map(storage_name: &str, count: u32) {
        let mut prefix = Vec::new();
        prefix.extend_from_slice(&twox_128(b"MevShield"));
        prefix.extend_from_slice(&twox_128(storage_name.as_bytes()));

        for i in 0..count {
            let mut key = prefix.clone();
            key.extend_from_slice(&i.to_le_bytes());
            sp_io::storage::set(&key, &[1u8; 32]);
        }
    }

    fn count_keys(storage_name: &str) -> u32 {
        let mut prefix = Vec::new();
        prefix.extend_from_slice(&twox_128(b"MevShield"));
        prefix.extend_from_slice(&twox_128(storage_name.as_bytes()));

        let mut count = 0u32;
        let mut next_key = sp_io::storage::next_key(&prefix);
        while let Some(key) = next_key {
            if !key.starts_with(&prefix) {
                break;
            }
            count += 1;
            next_key = sp_io::storage::next_key(&key);
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Encrypted extrinsics storage tests
// ---------------------------------------------------------------------------

mod encrypted_extrinsics_tests {
    use super::*;
    use frame_support::traits::Hooks;

    #[test]
    fn store_encrypted_works() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            let encoded_call = BoundedVec::truncate_from(call.encode());
            let who: u64 = 1;

            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(who),
                encoded_call.clone(),
            ));

            // Verify the extrinsic was stored at index 0 with account ID
            let expected = PendingExtrinsic::<Test> {
                who,
                encrypted_call: encoded_call,
                submitted_at: 1,
            };
            assert_eq!(PendingExtrinsics::<Test>::get(0), Some(expected));
            assert_eq!(NextPendingExtrinsicIndex::<Test>::get(), 1);
            assert_eq!(PendingExtrinsics::<Test>::count(), 1);

            // Verify event was emitted with index
            System::assert_last_event(
                crate::Event::<Test>::ExtrinsicStored { index: 0, who }.into(),
            );
        });
    }

    #[test]
    fn on_initialize_decodes_and_dispatches() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Store an encoded remark call
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            let encoded_call = BoundedVec::truncate_from(call.encode());

            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                encoded_call,
            ));

            // Verify there's a pending extrinsic
            assert_eq!(NextPendingExtrinsicIndex::<Test>::get(), 1);
            assert_eq!(PendingExtrinsics::<Test>::count(), 1);
            assert!(PendingExtrinsics::<Test>::get(0).is_some());

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify storage was cleared but NextPendingExtrinsicIndex stays (unique auto-increment)
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert_eq!(NextPendingExtrinsicIndex::<Test>::get(), 1);
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);

            // Verify ExtrinsicDispatched event was emitted
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 0 }.into());
        });
    }

    #[test]
    fn on_initialize_handles_decode_failure() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Store invalid bytes that can't be decoded as a call
            let invalid_bytes = BoundedVec::truncate_from(vec![0xFF, 0xFF, 0xFF, 0xFF]);

            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                invalid_bytes,
            ));

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify storage was cleared
            assert!(PendingExtrinsics::<Test>::get(0).is_none());

            // Verify ExtrinsicDecodeFailed event was emitted
            System::assert_has_event(
                crate::Event::<Test>::ExtrinsicDecodeFailed { index: 0 }.into(),
            );
        });
    }

    #[test]
    fn on_initialize_handles_dispatch_failure() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // A root-only call dispatched from a signed origin will fail.
            let failing_call =
                RuntimeCall::System(frame_system::Call::set_heap_pages { pages: 64 });

            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(failing_call.encode()),
            ));

            // Verify there is 1 pending extrinsic
            assert_eq!(NextPendingExtrinsicIndex::<Test>::get(), 1);
            assert_eq!(PendingExtrinsics::<Test>::count(), 1);
            assert!(PendingExtrinsics::<Test>::get(0).is_some());

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify storage was cleared
            assert!(PendingExtrinsics::<Test>::get(0).is_none());

            // Verify the call failed
            System::assert_has_event(
                crate::Event::<Test>::ExtrinsicDispatchFailed {
                    index: 0,
                    error: sp_runtime::DispatchError::BadOrigin,
                }
                .into(),
            );
        });
    }

    #[test]
    fn store_encrypted_rejects_when_full() {
        new_test_ext().execute_with(|| {
            let max = MaxPendingExtrinsicsLimit::<Test>::get();

            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            let encoded_call = BoundedVec::truncate_from(call.encode());

            // Fill up the pending extrinsics storage to max
            for _ in 0..max {
                assert_ok!(MevShield::store_encrypted(
                    RuntimeOrigin::signed(1),
                    encoded_call.clone(),
                ));
            }

            // The next one should fail
            assert_noop!(
                MevShield::store_encrypted(RuntimeOrigin::signed(1), encoded_call),
                Error::<Test>::TooManyPendingExtrinsics
            );
        });
    }

    #[test]
    fn on_initialize_processes_mixed_success_and_failure() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Store a valid call
            let valid_call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(valid_call.encode()),
            ));

            // Store invalid bytes
            let invalid_bytes = BoundedVec::truncate_from(vec![0xFF, 0xFF]);
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                invalid_bytes,
            ));

            // Store another valid call
            let valid_call2 = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![4, 5, 6],
            });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(valid_call2.encode()),
            ));

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify storage was cleared
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert!(PendingExtrinsics::<Test>::get(1).is_none());
            assert!(PendingExtrinsics::<Test>::get(2).is_none());

            // Verify correct events were emitted
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 0 }.into());
            System::assert_has_event(
                crate::Event::<Test>::ExtrinsicDecodeFailed { index: 1 }.into(),
            );
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 2 }.into());
        });
    }

    #[test]
    fn on_initialize_expires_old_extrinsics() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Store an extrinsic at block 1
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            // Verify the extrinsic was stored with submitted_at = 1
            let pending = PendingExtrinsics::<Test>::get(0).unwrap();
            assert_eq!(pending.submitted_at, 1);

            // Run on_initialize at block 12 (1 + 10 + 1 = 12, which is > MAX_EXTRINSIC_LIFETIME)
            // MAX_EXTRINSIC_LIFETIME is 10, so at block 12, age is 11 which exceeds the limit
            System::set_block_number(12);
            MevShield::on_initialize(12);

            // Verify storage was cleared
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);

            // Verify ExtrinsicExpired event was emitted (not ExtrinsicDispatched)
            System::assert_has_event(crate::Event::<Test>::ExtrinsicExpired { index: 0 }.into());
        });
    }

    #[test]
    fn on_initialize_does_not_expire_recent_extrinsics() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Store an extrinsic at block 1
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            // Run on_initialize at block 11 (age is 10, which equals MAX_EXTRINSIC_LIFETIME)
            // Should NOT expire since we check age > MAX, not age >=
            System::set_block_number(11);
            MevShield::on_initialize(11);

            // Verify storage was cleared (extrinsic was dispatched, not expired)
            assert!(PendingExtrinsics::<Test>::get(0).is_none());

            // Verify ExtrinsicDispatched event was emitted (not ExtrinsicExpired)
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 0 }.into());
        });
    }

    #[test]
    fn on_initialize_emits_dispatch_failed_on_bad_origin() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // set_heap_pages requires Root origin, so dispatching with Signed will fail
            let call = RuntimeCall::System(frame_system::Call::set_heap_pages { pages: 10 });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify storage was cleared
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);

            // Verify ExtrinsicDispatchFailed event was emitted
            System::assert_has_event(
                crate::Event::<Test>::ExtrinsicDispatchFailed {
                    index: 0,
                    error: sp_runtime::DispatchError::BadOrigin,
                }
                .into(),
            );
        });
    }

    #[test]
    fn on_initialize_handles_missing_slots() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Manually create a gap in indices by directly manipulating storage
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            let pending = PendingExtrinsic::<Test> {
                who: 1,
                encrypted_call: BoundedVec::truncate_from(call.encode()),
                submitted_at: 1,
            };

            // Insert at index 5, leaving 0-4 empty
            PendingExtrinsics::<Test>::insert(5, pending);
            NextPendingExtrinsicIndex::<Test>::put(6);

            // Run on_initialize - should handle the gap and process index 5
            MevShield::on_initialize(2);

            // Verify the extrinsic at index 5 was processed
            assert!(PendingExtrinsics::<Test>::get(5).is_none());
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);

            // Verify ExtrinsicDispatched event for index 5
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 5 }.into());
        });
    }

    #[test]
    fn multiple_accounts_dispatch_with_correct_origins() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            let user_a: u64 = 100;
            let user_b: u64 = 200;

            // User A submits a remark_with_event
            let call_a =
                RuntimeCall::System(frame_system::Call::remark_with_event { remark: vec![0xAA] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(user_a),
                BoundedVec::truncate_from(call_a.encode()),
            ));

            // User B submits a remark_with_event
            let call_b =
                RuntimeCall::System(frame_system::Call::remark_with_event { remark: vec![0xBB] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(user_b),
                BoundedVec::truncate_from(call_b.encode()),
            ));

            // Run on_initialize
            MevShield::on_initialize(2);

            // Verify both events have correct senders
            let hash_a = <Test as frame_system::Config>::Hashing::hash(&[0xAAu8]);
            let hash_b = <Test as frame_system::Config>::Hashing::hash(&[0xBBu8]);

            System::assert_has_event(
                frame_system::Event::<Test>::Remarked {
                    sender: user_a,
                    hash: hash_a,
                }
                .into(),
            );
            System::assert_has_event(
                frame_system::Event::<Test>::Remarked {
                    sender: user_b,
                    hash: hash_b,
                }
                .into(),
            );
        });
    }

    #[test]
    fn expiration_mixed_with_valid_extrinsics() {
        new_test_ext().execute_with(|| {
            // Submit first extrinsic at block 1
            System::set_block_number(1);
            let old_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![0x01] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(old_call.encode()),
            ));

            // Submit second extrinsic at block 10
            System::set_block_number(10);
            let new_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![0x02] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(2),
                BoundedVec::truncate_from(new_call.encode()),
            ));

            // Run on_initialize at block 12
            // First extrinsic: age = 12 - 1 = 11 > 10, should expire
            // Second extrinsic: age = 12 - 10 = 2 <= 10, should dispatch
            System::set_block_number(12);
            MevShield::on_initialize(12);

            // Verify both were removed from storage
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert!(PendingExtrinsics::<Test>::get(1).is_none());
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);

            // Verify first expired, second dispatched
            System::assert_has_event(crate::Event::<Test>::ExtrinsicExpired { index: 0 }.into());
            System::assert_has_event(crate::Event::<Test>::ExtrinsicDispatched { index: 1 }.into());
        });
    }

    #[test]
    fn set_max_pending_extrinsics_number_works() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Default is 100
            assert_eq!(MaxPendingExtrinsicsLimit::<Test>::get(), 100);

            assert_ok!(MevShield::set_max_pending_extrinsics_number(
                RuntimeOrigin::root(),
                50,
            ));

            assert_eq!(MaxPendingExtrinsicsLimit::<Test>::get(), 50);

            System::assert_last_event(
                crate::Event::<Test>::MaxPendingExtrinsicsNumberSet { value: 50 }.into(),
            );
        });
    }

    #[test]
    fn set_max_pending_extrinsics_number_rejects_signed_origin() {
        new_test_ext().execute_with(|| {
            assert_noop!(
                MevShield::set_max_pending_extrinsics_number(RuntimeOrigin::signed(1), 50),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn set_max_pending_extrinsics_number_enforced_on_store() {
        new_test_ext().execute_with(|| {
            // Set limit to 2
            assert_ok!(MevShield::set_max_pending_extrinsics_number(
                RuntimeOrigin::root(),
                2,
            ));

            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            let encoded_call = BoundedVec::truncate_from(call.encode());

            // First two should succeed
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                encoded_call.clone(),
            ));
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                encoded_call.clone(),
            ));

            // Third should fail
            assert_noop!(
                MevShield::store_encrypted(RuntimeOrigin::signed(1), encoded_call),
                Error::<Test>::TooManyPendingExtrinsics
            );
        });
    }

    #[test]
    fn set_on_initialize_weight_works() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            assert_eq!(
                OnInitializeWeight::<Test>::get(),
                crate::DEFAULT_ON_INITIALIZE_WEIGHT
            );

            assert_ok!(MevShield::set_on_initialize_weight(
                RuntimeOrigin::root(),
                1_000_000,
            ));

            assert_eq!(OnInitializeWeight::<Test>::get(), 1_000_000);

            System::assert_last_event(
                crate::Event::<Test>::OnInitializeWeightSet { value: 1_000_000 }.into(),
            );
        });
    }

    #[test]
    fn set_on_initialize_weight_rejects_signed_origin() {
        new_test_ext().execute_with(|| {
            assert_noop!(
                MevShield::set_on_initialize_weight(RuntimeOrigin::signed(1), 1_000_000),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn set_on_initialize_weight_rejects_above_absolute_max() {
        new_test_ext().execute_with(|| {
            // Exactly at absolute max should succeed
            assert_ok!(MevShield::set_on_initialize_weight(
                RuntimeOrigin::root(),
                crate::MAX_ON_INITIALIZE_WEIGHT,
            ));

            // Above absolute max should fail
            assert_noop!(
                MevShield::set_on_initialize_weight(
                    RuntimeOrigin::root(),
                    crate::MAX_ON_INITIALIZE_WEIGHT + 1,
                ),
                Error::<Test>::WeightExceedsAbsoluteMax
            );
        });
    }

    #[test]
    fn set_on_initialize_weight_enforced_on_processing() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Set weight to 0 so nothing can be processed
            assert_ok!(MevShield::set_on_initialize_weight(
                RuntimeOrigin::root(),
                0,
            ));

            // Store an extrinsic
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            assert_eq!(PendingExtrinsics::<Test>::count(), 1);

            // Run on_initialize — should postpone due to weight limit
            MevShield::on_initialize(2);

            // Extrinsic should still be pending (postponed)
            assert_eq!(PendingExtrinsics::<Test>::count(), 1);
            System::assert_has_event(crate::Event::<Test>::ExtrinsicPostponed { index: 0 }.into());
        });
    }

    #[test]
    fn set_stored_extrinsic_lifetime_works() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            assert_eq!(
                ExtrinsicLifetime::<Test>::get(),
                crate::DEFAULT_EXTRINSIC_LIFETIME
            );

            assert_ok!(MevShield::set_stored_extrinsic_lifetime(
                RuntimeOrigin::root(),
                20
            ));

            assert_eq!(ExtrinsicLifetime::<Test>::get(), 20);

            System::assert_last_event(
                crate::Event::<Test>::ExtrinsicLifetimeSet { value: 20 }.into(),
            );
        });
    }

    #[test]
    fn set_stored_extrinsic_lifetime_rejects_signed_origin() {
        new_test_ext().execute_with(|| {
            assert_noop!(
                MevShield::set_stored_extrinsic_lifetime(RuntimeOrigin::signed(1), 20),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn set_stored_extrinsic_lifetime_enforced_on_expiration() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Set lifetime to 2 blocks
            assert_ok!(MevShield::set_stored_extrinsic_lifetime(
                RuntimeOrigin::root(),
                2
            ));

            // Store an extrinsic at block 1
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            // At block 4: age = 4 - 1 = 3 > 2, should expire
            System::set_block_number(4);
            MevShield::on_initialize(4);

            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);
            System::assert_has_event(crate::Event::<Test>::ExtrinsicExpired { index: 0 }.into());
        });
    }

    #[test]
    fn set_max_extrinsic_weight_works() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            assert_eq!(
                MaxExtrinsicWeight::<Test>::get(),
                crate::DEFAULT_MAX_EXTRINSIC_WEIGHT
            );

            assert_ok!(MevShield::set_max_extrinsic_weight(
                RuntimeOrigin::root(),
                1_000_000,
            ));

            assert_eq!(MaxExtrinsicWeight::<Test>::get(), 1_000_000);

            System::assert_last_event(
                crate::Event::<Test>::MaxExtrinsicWeightSet { value: 1_000_000 }.into(),
            );
        });
    }

    #[test]
    fn set_max_extrinsic_weight_rejects_signed_origin() {
        new_test_ext().execute_with(|| {
            assert_noop!(
                MevShield::set_max_extrinsic_weight(RuntimeOrigin::signed(1), 1_000_000),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn set_max_extrinsic_weight_rejects_above_absolute_max() {
        new_test_ext().execute_with(|| {
            // Exactly at absolute max should succeed
            assert_ok!(MevShield::set_max_extrinsic_weight(
                RuntimeOrigin::root(),
                crate::MAX_ON_INITIALIZE_WEIGHT,
            ));

            // Above absolute max should fail
            assert_noop!(
                MevShield::set_max_extrinsic_weight(
                    RuntimeOrigin::root(),
                    crate::MAX_ON_INITIALIZE_WEIGHT + 1,
                ),
                Error::<Test>::WeightExceedsAbsoluteMax
            );
        });
    }

    #[test]
    fn max_extrinsic_weight_is_enforced() {
        new_test_ext().execute_with(|| {
            System::set_block_number(1);

            // Set per-extrinsic weight to 0 so all extrinsics exceed the limit
            assert_ok!(MevShield::set_max_extrinsic_weight(
                RuntimeOrigin::root(),
                0,
            ));

            // Store an extrinsic
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            assert_ok!(MevShield::store_encrypted(
                RuntimeOrigin::signed(1),
                BoundedVec::truncate_from(call.encode()),
            ));

            assert_eq!(PendingExtrinsics::<Test>::count(), 1);

            // Run on_initialize — should remove the extrinsic (weight exceeded)
            MevShield::on_initialize(2);

            // Extrinsic should be removed (not postponed)
            assert_eq!(PendingExtrinsics::<Test>::count(), 0);
            assert!(PendingExtrinsics::<Test>::get(0).is_none());
            System::assert_has_event(
                crate::Event::<Test>::ExtrinsicWeightExceeded { index: 0 }.into(),
            );
        });
    }
}
