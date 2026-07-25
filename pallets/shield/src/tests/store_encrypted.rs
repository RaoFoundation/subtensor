//! Tests for `store_encrypted` queueing and `on_initialize` pending-extrinsic processing.

use crate::mock::*;
use crate::{
    Error, MaxPendingExtrinsicsLimit, NextPendingExtrinsicIndex, PendingExtrinsic,
    PendingExtrinsics,
};
use codec::Encode;
use frame_support::traits::Hooks;
use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_runtime::traits::Hash;

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
        System::assert_last_event(crate::Event::<Test>::ExtrinsicStored { index: 0, who }.into());
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
        System::assert_has_event(crate::Event::<Test>::ExtrinsicDecodeFailed { index: 0 }.into());
    });
}

#[test]
fn on_initialize_handles_dispatch_failure() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        // A root-only call dispatched from a signed origin will fail.
        let failing_call = RuntimeCall::System(frame_system::Call::set_heap_pages { pages: 64 });

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
        System::assert_has_event(crate::Event::<Test>::ExtrinsicDecodeFailed { index: 1 }.into());
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
