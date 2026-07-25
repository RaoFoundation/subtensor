//! Tests for `announce_next_key` key rotation and author-key bookkeeping.

use crate::mock::*;
use crate::{
    AuthorKeys, CurrentKey, Error, NextKey, NextKeyExpiresAt, PendingKey, PendingKeyExpiresAt,
};
use frame_support::{BoundedVec, assert_noop, assert_ok};
use stp_shield::{MLKEM768_ENC_KEY_LEN, ShieldEncKey};

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
        set_mock_authors(Some(author(1)), next_next(0).map(author));
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
        set_mock_authors(Some(author(2)), next_next(1).map(author));
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
        set_mock_authors(Some(author(3)), next_next(2).map(author));
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
        set_mock_authors(Some(author(1)), next_next(3).map(author));
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
        set_mock_authors(Some(author(2)), None);
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
        set_mock_authors(Some(author(1)), Some(author(1)));

        let old_pk = valid_shield_enc_key();
        let new_pk = valid_shield_enc_key_b();
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
        set_mock_authors(Some(author(1)), None);
        assert_noop!(
            MevShield::announce_next_key(RuntimeOrigin::signed(1), Some(valid_shield_enc_key())),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

#[test]
fn announce_rejects_bad_pk_length() {
    new_test_ext().execute_with(|| {
        set_mock_authors(Some(author(1)), None);
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
        set_mock_authors(Some(author(1)), None);
        AuthorKeys::<Test>::insert(author(1), valid_shield_enc_key());

        assert_ok!(MevShield::announce_next_key(RuntimeOrigin::none(), None));

        assert!(AuthorKeys::<Test>::get(author(1)).is_none());
    });
}

#[test]
fn announce_fails_when_no_current_author() {
    new_test_ext().execute_with(|| {
        set_mock_authors(None, None);

        assert_noop!(
            MevShield::announce_next_key(RuntimeOrigin::none(), Some(valid_shield_enc_key())),
            Error::<Test>::Unreachable
        );
    });
}
