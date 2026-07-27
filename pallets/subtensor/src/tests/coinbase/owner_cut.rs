#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Subnet owner cut base case and disabled-owner-cut redistribution.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::owner_cut::test_owner_cut_base --exact --show-output --nocapture
#[test]
fn test_owner_cut_base() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );
        SubtensorModule::set_tempo_unchecked(netuid, 10000); // Large number (dont drain)
        SubtensorModule::set_subnet_owner_cut(0);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), 0.into()); // No cut
        SubtensorModule::set_subnet_owner_cut(u16::MAX);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), 1_000_000_000.into()); // Full cut.
    });
}

// cargo test --package pallet-subtensor --lib -- tests::coinbase::owner_cut::test_disabling_owner_cut_sends_subnet_emission_to_miners_and_validators --exact --nocapture
#[test]
fn test_disabling_owner_cut_sends_subnet_emission_to_miners_and_validators() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        let subnet_tempo = 10;
        let stake = 100_000_000_000u64;

        SubtensorModule::set_tempo_unchecked(netuid, subnet_tempo);
        setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 1);

        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_max_allowed_validators(netuid, 1);
        step_block(subnet_tempo);

        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        SubtensorModule::set_owner_cut_enabled_flag(netuid, false);

        let owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &subnet_owner_hotkey).unwrap();
        let validator_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &validator_hotkey).unwrap();
        let miner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &miner_hotkey).unwrap();
        let uid_count = [
            owner_uid as usize,
            validator_uid as usize,
            miner_uid as usize,
        ]
        .into_iter()
        .max()
        .unwrap()
            + 1;

        Weights::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            validator_uid,
            vec![(miner_uid, 0xFFFF)],
        );
        BlockAtRegistration::<Test>::set(netuid, owner_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, validator_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, miner_uid, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2; uid_count]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX);
        let mut validator_permit = vec![false; uid_count];
        validator_permit[validator_uid as usize] = true;
        ValidatorPermit::<Test>::insert(netuid, validator_permit);

        let owner_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_owner_hotkey,
            &subnet_owner_coldkey,
            netuid,
        );
        let validator_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &validator_hotkey,
            &validator_coldkey,
            netuid,
        );
        let miner_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        // Disabling owner cut removes the subnet owner from emission distribution, so the
        // subnet emission is fully distributed across the validator and miner paths instead.
        step_block(subnet_tempo);

        let owner_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_owner_hotkey,
            &subnet_owner_coldkey,
            netuid,
        );
        let validator_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &validator_hotkey,
            &validator_coldkey,
            netuid,
        );
        let miner_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        assert_eq!(owner_stake_after, owner_stake_before);
        assert!(validator_stake_after > validator_stake_before);
        assert!(miner_stake_after > miner_stake_before);
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );
    });
}
