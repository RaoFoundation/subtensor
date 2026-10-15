#![allow(clippy::unwrap_used)]
use super::mock::*;
use crate::{Error, OwnedHotkeys, Owner, StakingHotkeys};
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::NetUid;

#[test]
fn protected_hotkey_cannot_be_associated_or_reassigned_to_classical_owner() {
    new_test_ext(1).execute_with(|| {
        let classical = U256::from(10);
        let protected_cold = U256::from(11);
        let protected_hot = U256::from(12);
        ProtectedOwnerAccounts::set(vec![protected_cold, protected_hot]);
        assert_noop!(
            SubtensorModule::try_associate_hotkey(RuntimeOrigin::signed(classical), protected_hot),
            Error::<Test>::HotkeyOwnerPolicyViolation
        );
        assert!(!Owner::<Test>::contains_key(protected_hot));
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(protected_cold),
            protected_hot
        ));
        assert_eq!(Owner::<Test>::get(protected_hot), protected_cold);
        assert_noop!(
            SubtensorModule::set_hotkey_owner(&classical, &protected_hot),
            Error::<Test>::HotkeyOwnerPolicyViolation
        );
        assert_eq!(Owner::<Test>::get(protected_hot), protected_cold);
        // Ordinary ownership remains unchanged for ordinary accounts.
        let ordinary_hot = U256::from(13);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(classical),
            ordinary_hot
        ));
        assert_eq!(Owner::<Test>::get(ordinary_hot), classical);
    });
}

#[test]
fn hotkey_swap_cannot_claim_a_protected_destination() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(20);
        let old_hot = U256::from(21);
        let protected_hot = U256::from(22);
        ProtectedOwnerAccounts::set(vec![protected_hot]);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &cold, &old_hot
        ));
        for netuid in [None, Some(NetUid::from(1))] {
            let result = SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(cold),
                &old_hot,
                &protected_hot,
                netuid,
                false,
            );
            assert_eq!(
                result.unwrap_err().error,
                Error::<Test>::HotkeyOwnerPolicyViolation.into()
            );
            assert_eq!(Owner::<Test>::get(old_hot), cold);
            assert!(!Owner::<Test>::contains_key(protected_hot));
        }
    });
}

#[test]
fn coldkey_swap_refuses_downgrade_before_moving_any_ownership() {
    new_test_ext(1).execute_with(|| {
        let protected_cold = U256::from(30);
        let protected_hot = U256::from(31);
        let classical = U256::from(32);
        ProtectedOwnerAccounts::set(vec![protected_cold, protected_hot]);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &protected_cold,
            &protected_hot
        ));
        assert_noop!(
            SubtensorModule::do_swap_coldkey(&protected_cold, &classical),
            Error::<Test>::HotkeyOwnerPolicyViolation
        );
        assert_eq!(Owner::<Test>::get(protected_hot), protected_cold);
        assert_eq!(
            OwnedHotkeys::<Test>::get(protected_cold),
            vec![protected_hot]
        );
        assert_eq!(
            StakingHotkeys::<Test>::get(protected_cold),
            vec![protected_hot]
        );
        assert!(OwnedHotkeys::<Test>::get(classical).is_empty());
        assert!(StakingHotkeys::<Test>::get(classical).is_empty());
    });
}
