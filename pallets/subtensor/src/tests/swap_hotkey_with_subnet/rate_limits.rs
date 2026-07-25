#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::{PerU16, SaturatedConversion};
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_swap_rate_limits --exact --nocapture
#[test]
fn test_swap_hotkey_swap_rate_limits() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let delegate_take_block = 4567;
        let child_key_take_block = 8910;

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Set the last delegate take block for the old hotkey
        SubtensorModule::set_last_tx_block_delegate_take(&old_hotkey, delegate_take_block);
        // Set last childkey take block for the old hotkey
        SubtensorModule::set_last_tx_block_childkey(&old_hotkey, child_key_take_block);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Check for new hotkey (LastTxBlock is no longer transferred: the generic tx rate
        // limit was removed.
        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&new_hotkey),
            delegate_take_block
        );
        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&new_hotkey),
            child_key_take_block
        );
    });
}

#[test]
fn test_swap_owner_failed_interval_not_passed() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        assert_err!(
            SubtensorModule::perform_hotkey_swap(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeySwapOnSubnetIntervalNotPassed,
        );
    });
}

#[test]
fn test_swap_owner_check_swap_block_set() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        let new_block_number = System::block_number() + HotkeySwapOnSubnetInterval::get();
        System::set_block_number(new_block_number);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            new_block_number
        );
    });
}

#[test]
fn test_swap_owner_check_swap_record_clean_up() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        let new_block_number = System::block_number() + HotkeySwapOnSubnetInterval::get();
        System::set_block_number(new_block_number);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            new_block_number
        );

        step_block((HotkeySwapOnSubnetInterval::get() as u16 + u16::from(netuid)) * 2);
        assert!(!LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid, coldkey
        ));
    });
}
