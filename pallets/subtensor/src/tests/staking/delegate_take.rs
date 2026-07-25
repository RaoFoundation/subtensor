#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::increase_take`] / [`crate::staking::decrease_take`].

use frame_support::dispatch::{DispatchClass, GetDispatchInfo, Pays};
use frame_support::{assert_err, assert_ok};
use sp_core::U256;
use sp_runtime::PerU16;
use subtensor_runtime_common::NetUid;

use super::super::mock::*;
use crate::*;

/***********************************************************
    staking::delegate_take tests
************************************************************/

#[test]
fn test_delegate_take_dispatch_info_pays_fee() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        let take = PerU16::from_parts(SubtensorModule::get_min_delegate_take());

        let decrease_take_call =
            RuntimeCall::SubtensorModule(SubtensorCall::decrease_take { hotkey, take });
        let decrease_take_dispatch_info = decrease_take_call.get_dispatch_info();
        assert_eq!(decrease_take_dispatch_info.class, DispatchClass::Normal);
        assert_eq!(decrease_take_dispatch_info.pays_fee, Pays::Yes);

        let increase_take_call =
            RuntimeCall::SubtensorModule(SubtensorCall::increase_take { hotkey, take });
        let increase_take_dispatch_info = increase_take_call.get_dispatch_info();
        assert_eq!(increase_take_dispatch_info.class, DispatchClass::Normal);
        assert_eq!(increase_take_dispatch_info.pays_fee, Pays::Yes);
    });
}

// Verify delegate take can be decreased
#[test]
fn test_delegate_take_can_be_decreased() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        // Coldkey / hotkey 0 decreases take to 5%. This should fail as the minimum take is 9%
        assert_err!(
            SubtensorModule::do_decrease_take(
                RuntimeOrigin::signed(coldkey0),
                hotkey0,
                PerU16::from_parts(u16::MAX / 20)
            ),
            Error::<Test>::DelegateTakeTooLow
        );
    });
}

// Verify delegate take can be decreased
#[test]
fn test_can_set_min_take_ok() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates
        Delegates::<Test>::insert(hotkey0, PerU16::from_parts(u16::MAX / 10));

        // Coldkey / hotkey 0 decreases take to min
        assert_ok!(SubtensorModule::do_decrease_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take())
        ));
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );
    });
}

// Verify delegate take can not be increased with do_decrease_take
#[test]
fn test_delegate_take_can_not_be_increased_with_decrease_take() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Set min take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );

        // Coldkey / hotkey 0 tries to increase take to 12.5%
        assert_eq!(
            SubtensorModule::do_decrease_take(
                RuntimeOrigin::signed(coldkey0),
                hotkey0,
                PerU16::from_parts(SubtensorModule::get_max_delegate_take())
            ),
            Err(Error::<Test>::DelegateTakeTooLow.into())
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );
    });
}

// Verify delegate take can be increased
#[test]
fn test_delegate_take_can_be_increased() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        step_block(1 + InitialTxDelegateTakeRateLimit::get() as u16);

        // Coldkey / hotkey 0 decreases take to 12.5%
        assert_ok!(SubtensorModule::do_increase_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(u16::MAX / 8)
        ));
        assert_eq!(SubtensorModule::get_hotkey_take(&hotkey0), u16::MAX / 8);
    });
}

// Verify delegate take can not be decreased with increase_take
#[test]
fn test_delegate_take_can_not_be_decreased_with_increase_take() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        // Coldkey / hotkey 0 tries to decrease take to 5%
        assert_eq!(
            SubtensorModule::do_increase_take(
                RuntimeOrigin::signed(coldkey0),
                hotkey0,
                PerU16::from_parts(u16::MAX / 20)
            ),
            Err(Error::<Test>::DelegateTakeTooLow.into())
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );
    });
}

// Verify delegate take can be increased up to InitialDefaultDelegateTake (18%)
#[test]
fn test_delegate_take_can_be_increased_to_limit() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        step_block(1 + InitialTxDelegateTakeRateLimit::get() as u16);

        // Coldkey / hotkey 0 tries to increase take to InitialDefaultDelegateTake+1
        assert_ok!(SubtensorModule::do_increase_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(InitialDefaultDelegateTake::get())
        ));
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            InitialDefaultDelegateTake::get()
        );
    });
}

// Verify delegate take can not be increased above InitialDefaultDelegateTake (18%)
#[test]
fn test_delegate_take_can_not_be_increased_beyond_limit() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        // Coldkey / hotkey 0 tries to increase take to InitialDefaultDelegateTake+1
        // (Disable this check if InitialDefaultDelegateTake is u16::MAX)
        if InitialDefaultDelegateTake::get() != u16::MAX {
            assert_eq!(
                SubtensorModule::do_increase_take(
                    RuntimeOrigin::signed(coldkey0),
                    hotkey0,
                    PerU16::from_parts(InitialDefaultDelegateTake::get() + 1)
                ),
                Err(Error::<Test>::DelegateTakeTooHigh.into())
            );
        }
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );
    });
}

// Test rate-limiting on increase_take
#[test]
fn test_rate_limits_enforced_on_increase_take() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        // Increase take first time
        assert_ok!(SubtensorModule::do_increase_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 1)
        ));

        // Increase again
        assert_eq!(
            SubtensorModule::do_increase_take(
                RuntimeOrigin::signed(coldkey0),
                hotkey0,
                PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 2)
            ),
            Err(Error::<Test>::DelegateTxRateLimitExceeded.into())
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take() + 1
        );

        step_block(1 + InitialTxDelegateTakeRateLimit::get() as u16);

        // Can increase after waiting
        assert_ok!(SubtensorModule::do_increase_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 2)
        ));
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take() + 2
        );
    });
}

// Test rate-limiting on an increase take just after a decrease take
// Prevents a Validator from decreasing take and then increasing it immediately after.
#[test]
fn test_rate_limits_enforced_on_decrease_before_increase_take() {
    new_test_ext(1).execute_with(|| {
        // Make account
        let hotkey0 = U256::from(1);
        let coldkey0 = U256::from(3);

        // Add balance
        add_balance_to_coldkey_account(&coldkey0, 100000.into());

        // Register the neuron to a new network
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey0, coldkey0, 124124);

        // Coldkey / hotkey 0 become delegates with 9% take
        Delegates::<Test>::insert(
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 1),
        );
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take() + 1
        );

        // Decrease take
        assert_ok!(SubtensorModule::do_decrease_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take())
        )); // Verify decrease
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        // Increase take immediately after
        assert_eq!(
            SubtensorModule::do_increase_take(
                RuntimeOrigin::signed(coldkey0),
                hotkey0,
                PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 1)
            ),
            Err(Error::<Test>::DelegateTxRateLimitExceeded.into())
        ); // Verify no change
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take()
        );

        step_block(1 + InitialTxDelegateTakeRateLimit::get() as u16);

        // Can increase after waiting
        assert_ok!(SubtensorModule::do_increase_take(
            RuntimeOrigin::signed(coldkey0),
            hotkey0,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take() + 1)
        )); // Verify increase
        assert_eq!(
            SubtensorModule::get_hotkey_take(&hotkey0),
            SubtensorModule::get_min_delegate_take() + 1
        );
    });
}
