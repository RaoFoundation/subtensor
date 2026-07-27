//! Tests for root-configurable shield queue weight and lifetime limits.

use crate::mock::*;
use crate::{
    Error, ExtrinsicLifetime, MaxExtrinsicWeight, MaxPendingExtrinsicsLimit, OnInitializeWeight,
    PendingExtrinsics,
};
use codec::Encode;
use frame_support::traits::Hooks;
use frame_support::{BoundedVec, assert_noop, assert_ok};

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

        System::assert_last_event(crate::Event::<Test>::ExtrinsicLifetimeSet { value: 20 }.into());
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
        System::assert_has_event(crate::Event::<Test>::ExtrinsicWeightExceeded { index: 0 }.into());
    });
}
