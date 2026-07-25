#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Distribute-emission edge cases (no miners, zero emission).

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_distribute_emission_no_miners_all_drained() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        remove_owner_registration_stake(netuid);
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let init_stake = 1;
        SubtensorModule::set_burn(netuid, TaoBalance::from(0));
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        // Set the emission to be 1 million.
        let emission = AlphaBalance::from(1_000_000);
        // Run drain pending without any miners.
        SubtensorModule::distribute_emission(
            netuid,
            emission.saturating_div(2.into()).into(),
            emission.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Get the new stake of the hotkey.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect this neuron to get *all* the emission.
        // Slight epsilon due to rounding (hotkey_take).
        assert_abs_diff_eq!(
            new_stake,
            u64::from(emission + init_stake.into()).into(),
            epsilon = 1.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::coinbase::distribute_emission::test_distribute_emission_zero_emission --exact --show-output
#[test]
fn test_distribute_emission_zero_emission() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network_disable_commit_reveal(&U256::from(1), &U256::from(2));
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let miner_hk = U256::from(5);
        let miner_ck = U256::from(6);
        let init_stake: u64 = 100_000_000_000_000;
        let tempo = 2;
        SubtensorModule::set_tempo_unchecked(netuid, tempo);
        // Set weight-set limit to 0.
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, miner_hk, miner_ck, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        run_to_block_no_epoch(netuid, 50);

        // Run epoch for initial setup.
        SubtensorModule::epoch(netuid, AlphaBalance::ZERO);

        // Set weights on miner
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![0, 1, 2],
            vec![0, 0, 1],
            0,
        ));

        run_to_block_no_epoch(netuid, 50);

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Capture stake right before the zero-emission distribution so the assertion
        // isolates that call (the subnet legitimately accrues emission during the
        // preceding block runs under price-based shares).
        let stake_before_distribute = SubtensorModule::get_total_stake_for_hotkey(&hotkey);

        // Set the emission to be ZERO.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Get the new stake of the hotkey.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged by the zero-emission distribution.
        assert_eq!(new_stake, stake_before_distribute);

        // Check that the incentive and dividends are set by epoch.
        assert!(
            Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .iter()
                .map(|p| p.deconstruct())
                .sum::<u16>()
                > 0
        );
        assert!(
            Dividends::<Test>::get(netuid)
                .iter()
                .map(|p| p.deconstruct())
                .sum::<u16>()
                > 0
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::distribute_emission::test_zero_shares_zero_emission --exact --show-output --nocapture
#[test]
fn test_zero_shares_zero_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);
        let netuid1 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        let netuid2 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        // Setup prices 1 and 1
        let initial: u64 = 1_000_000;
        SubnetTAO::<Test>::insert(netuid1, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid1, AlphaBalance::from(initial));
        SubnetTAO::<Test>::insert(netuid2, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid2, AlphaBalance::from(initial));
        // Set subnet prices so that both are
        //   - cut off by lower limit for tao flow method
        //   - zeroed out for price ema method
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(0));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(0));
        // Run coinbase
        SubtensorModule::run_coinbase(emission_credit);
        // Netuid 1 is cut off by lower limit, all emission goes to netuid2
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid1), initial.into());
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid2), initial.into());
    });
}
