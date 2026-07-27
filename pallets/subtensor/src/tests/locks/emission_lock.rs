#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Emission interaction.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 17: Emission interaction
// =========================================================================

#[test]
fn test_emissions_do_not_break_lock_invariant() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total_alpha_before = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            total_alpha_before
        ));

        // Simulate emission: directly increase alpha for the hotkey on subnet
        // This increases the pool value for all share holders (including our coldkey)
        let emission_amount: AlphaBalance = 10_000_000u64.into();
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey, netuid, emission_amount);

        // After emission, total alpha should increase by emission_amount
        let total_alpha_after = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_eq!(total_alpha_after, total_alpha_before + emission_amount);

        // Lock invariant still holds: total_alpha >= locked_mass
        let locked = SubtensorModule::get_current_locked(&coldkey, netuid);
        assert!(total_alpha_after >= locked);

        // Available becomes emission_amount
        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, emission_amount);
    });
}

#[test]
fn test_epoch_distribution_auto_locks_owner_cut() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let subnet_tempo = 10;
        let stake = 100_000_000_000u64;

        SubtensorModule::set_tempo_unchecked(netuid, subnet_tempo);
        SubtensorModule::set_ck_burn(0);
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
        OwnerCutAutoLockEnabled::<Test>::insert(netuid, true);

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

        // Setup YUMA so that the next epoch produces non-zero subnet emissions.
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

        let owner_stake_before = get_alpha(&subnet_owner_hotkey, &subnet_owner_coldkey, netuid);
        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );

        // Advance to the next epoch so owner cut is distributed and auto-locked.
        step_epochs(1, netuid);

        let owner_stake_after = get_alpha(&subnet_owner_hotkey, &subnet_owner_coldkey, netuid);
        let owner_cut_locked = owner_stake_after - owner_stake_before;
        assert!(owner_cut_locked > AlphaBalance::ZERO);

        let owner_lock = Lock::<Test>::get((subnet_owner_coldkey, netuid, subnet_owner_hotkey))
            .expect("owner cut should be auto-locked to the subnet owner's hotkey");
        assert_eq!(owner_lock.locked_mass, owner_cut_locked);
    });
}

#[test]
fn test_auto_lock_owner_cut_is_disabled_by_default_and_can_be_enabled() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid =
            setup_subnet_with_stake(subnet_owner_coldkey, subnet_owner_hotkey, 100_000_000_000);
        let owner_cut: AlphaBalance = 10_000_000u64.into();

        assert!(!SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
        SubtensorModule::auto_lock_owner_cut(netuid, owner_cut);

        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );

        OwnerCutAutoLockEnabled::<Test>::insert(netuid, true);
        assert!(SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
        SubtensorModule::auto_lock_owner_cut(netuid, owner_cut);

        let owner_lock = Lock::<Test>::get((subnet_owner_coldkey, netuid, subnet_owner_hotkey))
            .expect("owner cut should be auto-locked when enabled");
        assert_eq!(owner_lock.locked_mass, owner_cut);
    });
}
