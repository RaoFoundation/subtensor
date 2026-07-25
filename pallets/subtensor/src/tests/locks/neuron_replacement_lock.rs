#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Neuron replacement.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 18: Neuron replacement
// =========================================================================

#[test]
fn test_neuron_replacement_does_not_affect_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        // Register the hotkey as a neuron
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_set_perpetual_lock(
            &coldkey, netuid, false,
        ));

        let total_before = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked_before = SubtensorModule::get_current_locked(&coldkey, netuid);

        // Replace the neuron with a different hotkey
        let new_hotkey = U256::from(99);
        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
        SubtensorModule::replace_neuron(
            netuid,
            uid,
            &new_hotkey,
            SubtensorModule::get_current_block_as_u64(),
        );

        // Alpha and lock should be unaffected by neuron replacement
        let total_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let locked_after = SubtensorModule::get_current_locked(&coldkey, netuid);

        assert_eq!(total_after, total_before);
        assert_eq!(locked_after, locked_before);

        // Lock still references original hotkey
        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_some());

        // Aggregate lock still references original hotkey
        assert!(DecayingHotkeyLock::<Test>::get(netuid, hotkey).is_some());
    });
}
