//! Tests for the `submit_encrypted` user-facing wrapper extrinsic.

use crate::mock::*;
use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_runtime::traits::Hash;

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
