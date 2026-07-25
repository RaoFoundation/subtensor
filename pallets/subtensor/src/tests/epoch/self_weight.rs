#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Subnet-owner self-weight allowance during epoch weight setting.

use sp_core::U256;
use subtensor_runtime_common::NetUidStorageIndex;

use super::super::mock::*;
use crate::*;

/// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::epoch::self_weight::test_can_set_self_weight_as_subnet_owner --exact --show-output
#[test]
fn test_can_set_self_weight_as_subnet_owner() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey: U256 = U256::from(1);
        let subnet_owner_hotkey: U256 = U256::from(1 + 456);

        let other_hotkey: U256 = U256::from(2);

        let stake = 5_000_000_000_000_u64; // 5k TAO
        let to_emit: u64 = 1_000_000_000_u64; // 1 TAO

        // Create subnet
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Register the other hotkey
        register_ok_neuron(netuid, other_hotkey, subnet_owner_coldkey, 0);

        // Add stake to owner hotkey.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_owner_hotkey,
            &subnet_owner_coldkey,
            netuid,
            stake.into(),
        );

        // Give vpermits to owner hotkey ONLY
        ValidatorPermit::<Test>::insert(netuid, vec![true, false]);

        // Set weight of 50% to each hotkey.
        // This includes a self-weight
        let fifty_percent: u16 = u16::MAX / 2;
        Weights::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            0,
            vec![(0, fifty_percent), (1, fifty_percent)],
        );

        step_block(1);
        // Set updated so weights are valid
        LastUpdate::<Test>::insert(NetUidStorageIndex::from(netuid), vec![2, 0]);

        // Run epoch
        let hotkey_emission = SubtensorModule::epoch(netuid, to_emit.into());

        // hotkey_emission is [(hotkey, incentive, dividend)]
        assert_eq!(hotkey_emission.len(), 2);
        assert!(
            hotkey_emission
                .iter()
                .any(|(hk, _, _)| *hk == subnet_owner_hotkey)
        );
        assert!(hotkey_emission.iter().any(|(hk, _, _)| *hk == other_hotkey));

        log::debug!("hotkey_emission: {hotkey_emission:?}");
        // Both should have received incentive emission
        assert!(hotkey_emission[0].1 > 0.into());
        assert!(hotkey_emission[1].1 > 0.into());

        // Their incentive should be equal
        assert_eq!(hotkey_emission[0].1, hotkey_emission[1].1);
    });
}
