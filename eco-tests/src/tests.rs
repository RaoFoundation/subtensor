#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]

use frame_support::assert_ok;
use pallet_subtensor::*;
use sp_core::{Get, U256};

use super::helpers::*;
use super::mock::*;

#[test]
fn test_add_stake_ok_neuron_does_not_belong_to_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(544);
        let hotkey_id = U256::from(54544);
        let other_cold_key = U256::from(99498);
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        let stake = DefaultMinStake::<Test>::get() * 10.into();

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&other_cold_key, stake.into());

        // Perform the request which is signed by a different cold key
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(other_cold_key),
            hotkey_id,
            netuid,
            stake,
        ));
    });
}
