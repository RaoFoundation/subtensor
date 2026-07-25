#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! run_coinbase gating before subnet start block.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_run_coinbase_not_started() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo = 2;

        let sn_owner_hk = U256::from(7);
        let sn_owner_ck = U256::from(8);

        add_network_without_emission_block(netuid, tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);

        SubnetOwner::<Test>::insert(netuid, sn_owner_ck);
        SubnetOwnerHotkey::<Test>::insert(netuid, sn_owner_hk);

        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let miner_hk = U256::from(5);
        let miner_ck = U256::from(6);
        let init_stake: u64 = 100_000_000_000_000;
        let tempo = 2;
        SubtensorModule::set_tempo_unchecked(netuid, tempo);
        // Set weight-set limit to 0.
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let reserve = init_stake * 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, miner_hk, miner_ck, 0);
        register_ok_neuron(netuid, sn_owner_hk, sn_owner_ck, 0);
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

        run_to_block_no_epoch(netuid, 30);

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

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Step so tempo should run.
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        let current_block = System::block_number();
        assert!(SubtensorModule::should_run_epoch(netuid, current_block));

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);

        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        // Get the new stake of the hotkey. We expect no emissions.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged.
        assert_eq!(new_stake, init_stake.into());

        // Check that the incentive and dividends are set.
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

#[test]
fn test_run_coinbase_not_started_start_after() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo = 2;

        let sn_owner_hk = U256::from(7);
        let sn_owner_ck = U256::from(8);

        add_network_without_emission_block(netuid, tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);

        SubnetOwner::<Test>::insert(netuid, sn_owner_ck);
        SubnetOwnerHotkey::<Test>::insert(netuid, sn_owner_hk);

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
        register_ok_neuron(netuid, sn_owner_hk, sn_owner_ck, 0);
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

        run_to_block_no_epoch(netuid, 30);

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

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Step so tempo should run.
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        let current_block = System::block_number();
        assert!(SubtensorModule::should_run_epoch(netuid, current_block));

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);
        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        let block_number = StartCallDelay::<Test>::get();
        run_to_block_no_epoch(netuid, block_number);

        let current_block = System::block_number();

        // Run start call.
        assert_ok!(SubtensorModule::start_call(
            RuntimeOrigin::signed(sn_owner_ck),
            netuid
        ));
        assert_eq!(
            FirstEmissionBlockNumber::<Test>::get(netuid),
            Some(current_block + 1)
        );

        // Advance the block past `LastEpochBlock + tempo` so the state-based
        // scheduler is due again (the previous `run_coinbase` advanced it).
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);
        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        // Get the new stake of the hotkey. We expect no emissions.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged.
        assert!(new_stake > init_stake.into());
        log::info!("new_stake: {new_stake}");
    });
}
