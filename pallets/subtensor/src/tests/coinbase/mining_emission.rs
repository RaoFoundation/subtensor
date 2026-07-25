#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Mining emission distribution with/without root sell.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::mining_emission::test_mining_emission_distribution_with_no_root_sell --exact --show-output --nocapture
#[test]
fn test_mining_emission_distribution_with_no_root_sell() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let validator_miner_coldkey = U256::from(3);
        let validator_miner_hotkey = U256::from(4);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = NetUid::from(1);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        SubtensorModule::set_tao_weight(0); // Start tao weight at 0
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        add_network(netuid, subnet_tempo, 0);
        SubnetMechanism::<Test>::insert(netuid, 1); // Set mechanism to 1

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, validator_miner_hotkey, validator_miner_coldkey, 1);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 2);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &validator_miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Setup stakes:
        //   Stake from validator
        //   Stake from valiminer
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_miner_coldkey),
            validator_miner_hotkey,
            netuid,
            stake.into()
        ));

        // Setup YUMA so that it creates emissions
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
        BlockAtRegistration::<Test>::set(netuid, 0, 1);
        BlockAtRegistration::<Test>::set(netuid, 1, 1);
        BlockAtRegistration::<Test>::set(netuid, 2, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell NOT happen
        // set price very low, e.g. a lot of alpha in
        let alpha = AlphaBalance::from(1_000_000_000_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        // Make sure we ARE NOT root selling, so we do not have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(!root_sell_flag, "Root sell flag should be false");

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        let old_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        let per_block_emission = SubtensorModule::get_block_emission_for_issuance(
            SubtensorModule::get_alpha_issuance(netuid).into(),
        )
        .unwrap_or(0);

        // step by one block
        step_block(1);
        // Verify that root alpha divs
        let new_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        // Check that we are indeed NOT root selling, i.e. that root alpha divs are NOT increasing
        assert_eq!(
            new_root_alpha_divs, old_root_alpha_divs,
            "Root alpha divs should not increase"
        );
        // Check root divs are zero
        assert_eq!(
            new_root_alpha_divs,
            AlphaBalance::ZERO,
            "Root alpha divs should be zero"
        );
        step_block(1);
        // Drain to a clean epoch boundary so accumulation starts fresh.
        step_epochs(1, netuid);
        let miner_stake_before_epoch = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );
        // Run again but with some root stake
        step_block(subnet_tempo - 1);
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid).to_u64(),
            U96F32::saturating_from_num(per_block_emission)
                .saturating_mul(U96F32::saturating_from_num((subnet_tempo - 1) as u64))
                .saturating_mul(U96F32::saturating_from_num(0.5)) // miner cut
                .saturating_mul(U96F32::saturating_from_num(0.90))
                .saturating_to_num::<u64>(),
            epsilon = 100_000_u64.into()
        );
        step_block(1);
        assert!(
            BlocksSinceLastStep::<Test>::get(netuid) == 0,
            "Blocks since last step should be 0"
        );

        let miner_uid = Uids::<Test>::get(netuid, miner_hotkey).unwrap_or(0);
        log::info!("Miner uid: {miner_uid:?}");
        let miner_incentive: AlphaBalance = {
            let miner_incentive = Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .get(miner_uid as usize)
                .copied();

            assert!(miner_incentive.is_some());

            (miner_incentive.unwrap_or_default().deconstruct() as u64).into()
        };
        log::info!("Miner incentive: {miner_incentive:?}");

        // Miner emissions
        let miner_emission_1: u64 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        )
        .to_u64()
            - miner_stake_before_epoch.to_u64();

        assert_abs_diff_eq!(
            Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .iter()
                .map(|p| p.deconstruct())
                .sum::<u16>(),
            u16::MAX,
            epsilon = 10
        );

        assert_abs_diff_eq!(
            miner_emission_1,
            U96F32::saturating_from_num(miner_incentive)
                .saturating_div(u16::MAX.into())
                .saturating_mul(U96F32::saturating_from_num(per_block_emission))
                .saturating_mul(U96F32::saturating_from_num(subnet_tempo))
                .saturating_mul(U96F32::saturating_from_num(0.45)) // miner cut
                .saturating_to_num::<u64>(),
            epsilon = 1_000_000_u64
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::mining_emission::test_mining_emission_distribution_with_root_sell --exact --show-output --nocapture
#[test]
fn test_mining_emission_distribution_with_root_sell() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let validator_miner_coldkey = U256::from(3);
        let validator_miner_hotkey = U256::from(4);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        SubtensorModule::set_tao_weight(0); // Start tao weight at 0
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        // Period is `tempo`; `tempo = 2` keeps a one-block gap between epochs so
        // pending root-alpha-divs can be observed accumulating before a drain.
        Tempo::<Test>::insert(netuid, 2);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 0);

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, validator_miner_hotkey, validator_miner_coldkey, 1);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 2);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &validator_miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Setup stakes:
        //   Stake from validator
        //   Stake from valiminer
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_miner_coldkey),
            validator_miner_hotkey,
            netuid,
            stake.into()
        ));

        // Setup YUMA so that it creates emissions
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
        BlockAtRegistration::<Test>::set(netuid, 0, 1);
        BlockAtRegistration::<Test>::set(netuid, 1, 1);
        BlockAtRegistration::<Test>::set(netuid, 2, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell happen
        // Set moving price > 1.0
        // Set price > 1.0
        let alpha = AlphaBalance::from(100_000_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        let old_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        let miner_stake_before_epoch = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        // step by one block
        step_block(1);
        // Verify root alpha divs
        let new_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        // Check that we ARE root selling, i.e. that root alpha divs are changing
        assert_ne!(
            new_root_alpha_divs, old_root_alpha_divs,
            "Root alpha divs should be changing"
        );
        assert!(
            new_root_alpha_divs > AlphaBalance::ZERO,
            "Root alpha divs should be greater than 0"
        );

        // Run again but with some root stake
        step_block(subnet_tempo - 1);

        let miner_uid = Uids::<Test>::get(netuid, miner_hotkey).unwrap_or(0);
        let miner_incentive: AlphaBalance = {
            let miner_incentive = Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .get(miner_uid as usize)
                .copied();

            assert!(miner_incentive.is_some());

            (miner_incentive.unwrap_or_default().deconstruct() as u64).into()
        };
        log::info!("Miner incentive: {miner_incentive:?}");

        let per_block_emission = SubtensorModule::get_block_emission_for_issuance(
            SubtensorModule::get_alpha_issuance(netuid).into(),
        )
        .unwrap_or(0);

        // Miner emissions
        let miner_emission_1: u64 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        )
        .to_u64()
            - miner_stake_before_epoch.to_u64();

        assert_abs_diff_eq!(
            miner_emission_1,
            U96F32::saturating_from_num(miner_incentive)
                .saturating_div(u16::MAX.into())
                .saturating_mul(U96F32::saturating_from_num(per_block_emission))
                .saturating_mul(U96F32::saturating_from_num(subnet_tempo))
                .saturating_mul(U96F32::saturating_from_num(0.45)) // miner cut
                .saturating_to_num::<u64>(),
            epsilon = 1_000_000_u64
        );
    });
}
