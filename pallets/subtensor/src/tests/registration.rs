#![allow(clippy::unwrap_used)]

use crate::*;
use frame_support::{assert_noop, assert_ok};
use frame_system::Config;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, Token};

use super::mock;
use super::mock::*;
use crate::{AxonInfoOf, Error};

/********************************************
    subscribing::subscribe() tests
*********************************************/

#[test]
fn test_init_new_network_registration_defaults() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;

        SubtensorModule::init_new_network(netuid, tempo);

        assert_eq!(BurnHalfLife::<Test>::get(netuid), 360);
        assert_eq!(
            BurnIncreaseMult::<Test>::get(netuid),
            U64F64::from_num(1.26)
        );

        assert_eq!(
            SubtensorModule::get_burn(netuid),
            TaoBalance::from(RAO_PER_TAO)
        );
    });
}

#[test]
fn test_registration_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;

        add_network(netuid, tempo, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Make burn small and stable for this test.
        let burn = 1_000_u64;
        SubtensorModule::set_burn(netuid, burn.into());

        let hotkey = U256::from(1);
        let coldkey = U256::from(667);

        add_balance_to_coldkey_account(&coldkey, 50_000.into());

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));

        // neuron inserted
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 1);

        // ownership set
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey),
            coldkey
        );

        // uid exists
        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
        assert_eq!(uid, 0);

        // no stake by default
        assert_eq!(
            SubtensorModule::get_stake_for_uid_and_subnetwork(netuid, uid),
            AlphaBalance::ZERO
        );

        // TAO inflow recorded
        assert_eq!(SubnetTaoFlow::<Test>::get(netuid), burn as i64);
    });
}

#[test]
fn test_registration_failed_no_signature() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        let hotkey = U256::from(1);

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::none(),
            netuid,
            hotkey,
        );

        assert_eq!(result, Err(sp_runtime::DispatchError::BadOrigin));
    });
}

#[test]
fn test_registration_disabled() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        SubtensorModule::set_network_registration_allowed(netuid, false);

        let hotkey = U256::from(1);
        let coldkey = U256::from(667);

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey,
        );

        assert_eq!(
            result,
            Err(Error::<Test>::SubNetRegistrationDisabled.into())
        );
    });
}

#[test]
fn test_registration_root_not_permitted() {
    new_test_ext(1).execute_with(|| {
        let tempo: u16 = 13;
        // Ensure root exists in this test env.
        SubtensorModule::init_new_network(NetUid::ROOT, tempo);

        let hotkey = U256::from(1);
        let coldkey = U256::from(2);

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            NetUid::ROOT,
            hotkey,
        );

        assert_eq!(
            result,
            Err(Error::<Test>::RegistrationNotPermittedOnRootSubnet.into())
        );
    });
}

#[test]
fn test_registration_not_enough_balance() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // burn cost is 10_000, but coldkey only has 9_999.
        SubtensorModule::set_burn(netuid, 10_000u64.into());

        let hotkey = U256::from(1);
        let coldkey = U256::from(667);

        add_balance_to_coldkey_account(&coldkey, 9_999.into());

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey,
        );

        assert_eq!(result, Err(Error::<Test>::NotEnoughBalanceToStake.into()));
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 0);
    });
}

#[test]
fn test_registration_non_associated_coldkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let hotkey = U256::from(1);
        let true_owner = U256::from(111);
        let attacker = U256::from(222);

        // Pre-own the hotkey by a different coldkey.
        Owner::<Test>::insert(hotkey, true_owner);

        // Attacker has enough funds, but doesn't own the hotkey.
        add_balance_to_coldkey_account(&attacker, 50_000.into());

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(attacker),
            netuid,
            hotkey,
        );

        assert_eq!(result, Err(Error::<Test>::NonAssociatedColdKey.into()));
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 0);
    });
}

#[test]
fn test_registration_without_neuron_slot_doesnt_burn() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let hotkey = U256::from(1);
        let coldkey = U256::from(667);

        add_balance_to_coldkey_account(&coldkey, 10_000.into());
        let before = SubtensorModule::get_coldkey_balance(&coldkey);

        // No slots => should fail before burning.
        SubtensorModule::set_max_allowed_uids(netuid, 0);

        assert_noop!(
            SubtensorModule::burned_register(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                netuid,
                hotkey
            ),
            Error::<Test>::NoNeuronIdAvailable
        );

        assert_eq!(SubtensorModule::get_coldkey_balance(&coldkey), before);
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 0);
    });
}

#[test]
fn test_registration_already_active_hotkey_error() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);

        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let coldkey = U256::from(667);
        let hotkey = U256::from(1);
        add_balance_to_coldkey_account(&coldkey, 1_000_000.into());

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));

        // Step to a new interval so we don't hit TooManyRegistrationsThisInterval first.
        step_block(1);

        let result = SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey,
        );
        assert_eq!(
            result,
            Err(Error::<Test>::HotKeyAlreadyRegisteredInSubNet.into())
        );
    });
}

// -----------------------------
// Burn price dynamics tests
// -----------------------------

#[test]
fn test_burn_decay() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Make behavior deterministic for this test:
        // - very slow decay
        // - x2 bump on successful registration
        // - neutral min/max clamps so this test isolates bump + decay behavior
        BurnHalfLife::<Test>::insert(netuid, 1_000);
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(2));
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(0u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(u64::MAX));

        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let coldkey = U256::from(100);
        let hotkey = U256::from(200);
        add_balance_to_coldkey_account(&coldkey, 1_000_000.into());

        // Register in this block. Burn updates immediately now.
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));
        assert_eq!(SubtensorModule::get_burn(netuid), 2_000u64.into());

        // Next block only continuous decay applies: 2000 -> 1998.
        step_block(1);
        assert_eq!(SubtensorModule::get_burn(netuid), 1_998u64.into());
    });
}

#[test]
fn test_burn_halves_every_half_life() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        BurnHalfLife::<Test>::insert(netuid, 2);
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(0u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(u64::MAX));

        SubtensorModule::set_burn(netuid, 1_024u64.into());

        step_block(2);
        assert_eq!(SubtensorModule::get_burn(netuid), 511u64.into());

        step_block(2);
        assert_eq!(SubtensorModule::get_burn(netuid), 255u64.into());
    });
}

#[test]
fn test_burn_min_and_max_clamps_prevent_zero_stuck_and_cap_bump() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Half-life every block; multiplier 2.
        BurnHalfLife::<Test>::insert(netuid, 1);
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(2));

        // Explicitly test the new clamp behavior:
        // - min burn prevents decay from getting stuck at zero
        // - max burn caps the immediate post-registration bump
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(100_000u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(150_000u64));

        // Start at 1 => halving would go to 0, but the min-burn clamp keeps it at 100_000.
        SubtensorModule::set_burn(netuid, 1u64.into());

        // Step one block => halving applies, but min clamp => burn becomes 100_000.
        step_block(1);
        assert_eq!(SubtensorModule::get_burn(netuid), 100_000u64.into());

        // Register now; bump should apply immediately but be capped by max burn.
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        add_balance_to_coldkey_account(&coldkey, 1_000_000u64.into());

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));

        // Immediate bump would be 200_000, but max burn caps it at 150_000.
        assert_eq!(SubtensorModule::get_burn(netuid), 150_000u64.into());

        // Next block decays 150_000 -> 75_000, but min clamp raises it back to 100_000.
        step_block(1);
        assert_eq!(SubtensorModule::get_burn(netuid), 100_000u64.into());

        // One more block proves it does not get stuck below the configured minimum.
        step_block(1);
        assert_eq!(SubtensorModule::get_burn(netuid), 100_000u64.into());
    });
}

#[test]
fn test_registration_increases_recycled_rao_per_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        BurnHalfLife::<Test>::insert(netuid, 1); // allow 1 reg / block
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1)); // keep burn stable aside from halving
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let coldkey = U256::from(667);
        add_balance_to_coldkey_account(&coldkey, 1_000_000.into());

        // First registration
        let burn1 = SubtensorModule::get_burn(netuid);
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            U256::from(1)
        ));
        assert_eq!(SubtensorModule::get_rao_recycled(netuid), burn1);

        // Next interval
        step_block(1);

        // Second registration (burn may have changed; read it)
        let burn2 = SubtensorModule::get_burn(netuid);
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            U256::from(2)
        ));

        assert_eq!(SubtensorModule::get_rao_recycled(netuid), burn1 + burn2);
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_registration_get_uid_to_prune_all_in_immunity_period() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Neutralize safety floor and owner-immortality for deterministic selection
        SubtensorModule::set_min_non_immune_uids(netuid, 0);
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);
        SubnetOwner::<Test>::insert(netuid, U256::from(999_999u64));

        // Register uid0 @ block 0
        register_ok_neuron(netuid, U256::from(0), U256::from(0), 0);

        // Move to next block so interval resets; register uid1 @ block 1
        step_block(1);
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 0);

        // Both immune with immunity_period=2 at current block=1.
        SubtensorModule::set_immunity_period(netuid, 2);
        assert_eq!(SubtensorModule::get_current_block_as_u64(), 1);

        // Both immune; prune lowest emission among immune (uid0=100, uid1=110 => uid0)
        Emission::<Test>::mutate(netuid, |v| {
            v[0] = 100u64.into();
            v[1] = 110u64.into();
        });

        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), Some(0));
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_registration_get_uid_to_prune_none_in_immunity_period() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Neutralize safety floor and owner-immortality for deterministic selection
        SubtensorModule::set_min_non_immune_uids(netuid, 0);
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);
        SubnetOwner::<Test>::insert(netuid, U256::from(999_999u64));

        // Register uid0 @ block 0
        register_ok_neuron(netuid, U256::from(0), U256::from(0), 0);

        // Register uid1 @ block 1
        step_block(1);
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 0);

        SubtensorModule::set_immunity_period(netuid, 2);

        // Advance beyond immunity -> both non-immune (current=1, step 3 => 4)
        step_block(3);
        assert_eq!(SubtensorModule::get_current_block_as_u64(), 4);

        Emission::<Test>::mutate(netuid, |v| {
            v[0] = 100u64.into();
            v[1] = 110u64.into();
        });

        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), Some(0));
    });
}

// These owner-immortality tests do not depend on registration paths; keep as-is.

#[test]
fn test_registration_get_uid_to_prune_owner_immortality() {
    new_test_ext(1).execute_with(|| {
        [
            (1, 1), // limit=1 => prune other owner hk (uid1)
            (2, 2), // limit=2 => both owner hks immortal => prune non-owner (uid2)
        ]
        .iter()
        .for_each(|(limit, uid_to_prune)| {
            let subnet_owner_ck = U256::from(0);
            let subnet_owner_hk = U256::from(1);

            let other_owner_hk = U256::from(2);
            Owner::<Test>::insert(other_owner_hk, subnet_owner_ck);
            OwnedHotkeys::<Test>::insert(subnet_owner_ck, vec![subnet_owner_hk, other_owner_hk]);

            let non_owner_hk = U256::from(3);

            let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

            BlockAtRegistration::<Test>::insert(netuid, 1, 1);
            BlockAtRegistration::<Test>::insert(netuid, 2, 2);
            Uids::<Test>::insert(netuid, other_owner_hk, 1);
            Uids::<Test>::insert(netuid, non_owner_hk, 2);
            Keys::<Test>::insert(netuid, 1, other_owner_hk);
            Keys::<Test>::insert(netuid, 2, non_owner_hk);
            ImmunityPeriod::<Test>::insert(netuid, 1);
            SubnetworkN::<Test>::insert(netuid, 3);

            SubtensorModule::set_min_non_immune_uids(netuid, 0);

            step_block(10); // all non-immune

            ImmuneOwnerUidsLimit::<Test>::insert(netuid, *limit);

            Emission::<Test>::insert(
                netuid,
                vec![AlphaBalance::from(0), 0u64.into(), 1u64.into()],
            );

            assert_eq!(
                SubtensorModule::get_neuron_to_prune(netuid),
                Some(*uid_to_prune)
            );
        });
    });
}

#[test]
fn test_registration_get_uid_to_prune_owner_immortality_all_immune() {
    new_test_ext(1).execute_with(|| {
        let limit = 2;
        let uid_to_prune = 2;
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let other_owner_hk = U256::from(2);
        Owner::<Test>::insert(other_owner_hk, subnet_owner_ck);
        OwnedHotkeys::<Test>::insert(subnet_owner_ck, vec![subnet_owner_hk, other_owner_hk]);

        let non_owner_hk = U256::from(3);
        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        BlockAtRegistration::<Test>::insert(netuid, 0, 12);
        BlockAtRegistration::<Test>::insert(netuid, 1, 11);
        BlockAtRegistration::<Test>::insert(netuid, 2, 10);
        Uids::<Test>::insert(netuid, other_owner_hk, 1);
        Uids::<Test>::insert(netuid, non_owner_hk, 2);
        Keys::<Test>::insert(netuid, 1, other_owner_hk);
        Keys::<Test>::insert(netuid, 2, non_owner_hk);
        ImmunityPeriod::<Test>::insert(netuid, 100);
        SubnetworkN::<Test>::insert(netuid, 3);

        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        step_block(20); // all still immune

        ImmuneOwnerUidsLimit::<Test>::insert(netuid, limit);

        Emission::<Test>::insert(
            netuid,
            vec![AlphaBalance::from(0), 0u64.into(), 1u64.into()],
        );

        assert_eq!(
            SubtensorModule::get_neuron_to_prune(netuid),
            Some(uid_to_prune)
        );
    });
}

#[test]
fn test_registration_get_neuron_metadata() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        let hotkey = U256::from(1);
        let coldkey = U256::from(667);
        add_balance_to_coldkey_account(&coldkey, 100_000.into());

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));

        let neuron: AxonInfoOf = SubtensorModule::get_axon_info(netuid, &hotkey);
        assert_eq!(neuron.ip, 0);
        assert_eq!(neuron.version, 0);
        assert_eq!(neuron.port, 0);
    });
}

#[test]
fn test_last_update_correctness() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id = U256::from(1);
        let burn_cost: u64 = 1_000;
        let coldkey_account_id = U256::from(667); // Neighbour of the beast, har har

        // Add network first, then override burn so init_new_network doesn't clobber it.
        add_network(netuid, tempo, 0);
        SubtensorModule::set_burn(netuid, burn_cost.into());

        let reserve: u64 = 1_000_000_000_000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Simulate existing neurons
        let existing_neurons = 3;
        SubnetworkN::<Test>::insert(netuid, existing_neurons);

        // Simulate no LastUpdate so far (can happen on mechanisms)
        LastUpdate::<Test>::remove(NetUidStorageIndex::from(netuid));

        // Give enough balance for the burn path.
        add_balance_to_coldkey_account(&coldkey_account_id, 10_000.into());

        // Register and ensure LastUpdate is expanded correctly.
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid,
            hotkey_account_id
        ));

        assert_eq!(
            LastUpdate::<Test>::get(NetUidStorageIndex::from(netuid)).len(),
            (existing_neurons + 1) as usize
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_registration_pruning_1() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(5);
        add_network(netuid, 10_000, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);

        MaxRegistrationsPerBlock::<Test>::insert(netuid, 1024);
        SubtensorModule::set_max_allowed_uids(netuid, 3);

        let coldkeys = [U256::from(20_001), U256::from(20_002), U256::from(20_003)];
        let hotkeys = [U256::from(30_001), U256::from(30_002), U256::from(30_003)];

        for i in 0..3 {
            register_ok_neuron(netuid, hotkeys[i], coldkeys[i], 0);
            step_block(1);
        }

        let uid0 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[0]).unwrap();
        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[1]).unwrap();
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[2]).unwrap();

        assert_eq!(uid0, 0);
        assert_eq!(uid1, 1);
        assert_eq!(uid2, 2);
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 3);

        let now: u64 = 1_000;
        frame_system::Pallet::<Test>::set_block_number(now);

        SubtensorModule::set_immunity_period(netuid, 100);

        BlockAtRegistration::<Test>::insert(netuid, uid0, now - 150);
        BlockAtRegistration::<Test>::insert(netuid, uid1, now - 200);
        BlockAtRegistration::<Test>::insert(netuid, uid2, now - 10);

        assert!(!SubtensorModule::get_neuron_is_immune(netuid, uid0));
        assert!(!SubtensorModule::get_neuron_is_immune(netuid, uid1));
        assert!(SubtensorModule::get_neuron_is_immune(netuid, uid2));

        Emission::<Test>::mutate(netuid, |v| {
            v[uid0 as usize] = 10u64.into();
            v[uid1 as usize] = 10u64.into();
            v[uid2 as usize] = 1u64.into();
        });

        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), Some(uid1));

        let new_hotkey = U256::from(40_000);
        let new_coldkey = U256::from(50_000);

        // Ensure interval allows another registration
        step_block(1);

        register_ok_neuron(netuid, new_hotkey, new_coldkey, 0);

        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 3);

        assert!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[1]).is_err(),
            "Hotkey for pruned UID should no longer be registered"
        );

        let new_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hotkey).unwrap();
        assert_eq!(new_uid, uid1);

        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[0]).unwrap(),
            uid0
        );
        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[2]).unwrap(),
            uid2
        );
    });
}

#[test]
fn test_bump_registration_price_after_registration_applies_multiplier_immediately() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(3));
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(0u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(u64::MAX));
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        SubtensorModule::bump_registration_price_after_registration(netuid);

        assert_eq!(SubtensorModule::get_burn(netuid), 3_000u64.into());
    });
}

#[test]
fn test_update_registration_prices_for_networks_runs_on_next_block_step() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        // Gentle decay so the one-block result is deterministic.
        // Use neutral min/max clamps so this test isolates decay + counter reset.
        BurnHalfLife::<Test>::insert(netuid, 1_000);
        BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(2));
        SubtensorModule::set_min_burn(netuid, TaoBalance::from(0u64));
        SubtensorModule::set_max_burn(netuid, TaoBalance::from(u64::MAX));
        SubtensorModule::set_burn(netuid, 1_000u64.into());

        // Seed the per-block counter to prove that stepping one block only
        // performs decay + reset now, and does not apply a delayed bump.
        RegistrationsThisBlock::<Test>::insert(netuid, 3);
        assert_eq!(RegistrationsThisBlock::<Test>::get(netuid), 3);

        step_block(1);

        // `update_registration_prices_for_networks()` is invoked from on_initialize.
        // With the immediate-bump registration fix, the next block should only decay:
        // 1000 -> 999, and reset the bookkeeping counter.
        assert_eq!(SubtensorModule::get_burn(netuid), 999u64.into());
        assert_eq!(RegistrationsThisBlock::<Test>::get(netuid), 0);
    });
}

#[test]
fn test_neuron_registration_disabled() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id: U256 = U256::from(668);
        let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
            netuid,
            block_number,
            0,
            &hotkey_account_id,
        );

        //add network
        add_network(netuid, tempo, 0);
        SubtensorModule::set_network_registration_allowed(netuid, false);

        let result = SubtensorModule::register(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            block_number,
            nonce,
            work.clone(),
            hotkey_account_id,
            coldkey_account_id,
        );
        assert_eq!(
            result,
            Err(Error::<Test>::SubNetRegistrationDisabled.into())
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_registration_pruning() {
    new_test_ext(1).execute_with(|| {
        // --- Setup a simple non-root subnet.
        let netuid = NetUid::from(5);
        add_network(netuid, 10_000, 0);

        // No owner-based immortality: we want to test time-based immunity only.
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 0);

        // Allow registrations freely.
        MaxRegistrationsPerBlock::<Test>::insert(netuid, 1024);
        SubtensorModule::set_target_registrations_per_interval(netuid, u16::MAX);

        // Cap the subnet at 3 UIDs so the 4th registration *must* prune.
        SubtensorModule::set_max_allowed_uids(netuid, 3);

        // --- Register three neurons (uids 0, 1, 2).
        let coldkeys = [U256::from(20_001), U256::from(20_002), U256::from(20_003)];
        let hotkeys = [U256::from(30_001), U256::from(30_002), U256::from(30_003)];

        for i in 0..3 {
            register_ok_neuron(netuid, hotkeys[i], coldkeys[i], 0);
        }

        // Sanity: ensure we got sequential UIDs.
        let uid0 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[0]).unwrap();
        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[1]).unwrap();
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[2]).unwrap();

        assert_eq!(uid0, 0);
        assert_eq!(uid1, 1);
        assert_eq!(uid2, 2);
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 3);

        // --- Craft immunity and tie‑breaking conditions.

        // Fixed "current" block.
        let now: u64 = 1_000;
        frame_system::Pallet::<Test>::set_block_number(now);

        // Immunity lasts 100 blocks.
        SubtensorModule::set_immunity_period(netuid, 100);

        // Registration blocks:
        //  - uid0: now - 150  -> non‑immune
        //  - uid1: now - 200  -> non‑immune (older than uid0)
        //  - uid2: now - 10   -> immune
        BlockAtRegistration::<Test>::insert(netuid, uid0, now - 150);
        BlockAtRegistration::<Test>::insert(netuid, uid1, now - 200);
        BlockAtRegistration::<Test>::insert(netuid, uid2, now - 10);

        // Check immunity flags: the 3rd neuron is immune, the first two are not.
        assert!(!SubtensorModule::get_neuron_is_immune(netuid, uid0));
        assert!(!SubtensorModule::get_neuron_is_immune(netuid, uid1));
        assert!(SubtensorModule::get_neuron_is_immune(netuid, uid2));

        // Emissions:
        //  - uid0: 10
        //  - uid1: 10  (same emission as uid0)
        //  - uid2: 1   (better emission, but immune)
        //
        // Among *non‑immune* neurons, emission ties -> break on reg_block:
        // uid1 registered earlier (now-200 < now-150), so uid1 should be pruned.
        // The immune uid2 should **not** be chosen even though it has lower emission.
        Emission::<Test>::mutate(netuid, |v| {
            v[uid0 as usize] = 10u64.into();
            v[uid1 as usize] = 10u64.into();
            v[uid2 as usize] = 1u64.into();
        });

        // Allow pruning of any non‑immune UID (no safety floor).
        SubtensorModule::set_min_non_immune_uids(netuid, 0);

        // Check that pruning decision respects:
        //  1. Prefer non‑immune over immune.
        //  2. Then lowest emission.
        //  3. Then earliest registration block.
        //  4. Then uid (not needed here).
        assert_eq!(
            SubtensorModule::get_neuron_to_prune(netuid),
            Some(uid1),
            "Expected pruning to choose the oldest non‑immune neuron \
             when emissions tie, even if an immune neuron has lower emission"
        );

        // --- Now actually perform a registration that forces pruning.

        let new_hotkey = U256::from(40_000);
        let new_coldkey = U256::from(50_000);

        // This should internally call do_burned_registration -> register_neuron,
        // which must reuse the UID returned by get_neuron_to_prune (uid1).
        register_ok_neuron(netuid, new_hotkey, new_coldkey, 0);

        // Still capped at 3 UIDs.
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 3);

        // Old uid1 hotkey should be gone.
        assert!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[1]).is_err(),
            "Hotkey for pruned UID should no longer be registered"
        );

        // New hotkey should reuse uid1 (the pruned slot).
        let new_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &new_hotkey).unwrap();
        assert_eq!(
            new_uid, uid1,
            "New registration should reuse the UID selected by get_neuron_to_prune"
        );

        // The other two original neurons (uid0 and uid2) must remain registered.
        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[0]).unwrap(),
            uid0
        );
        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkeys[2]).unwrap(),
            uid2
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn test_update_registration_prices_for_networks_many_half_lives_over_thousands_of_blocks() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        netuid: u16,
        half_life: u16,
        initial_burn: u64,
    }

    fn ref_mul_by_q32(value: u64, factor_q32: u64) -> u64 {
        let product: u128 = (value as u128) * (factor_q32 as u128);
        let shifted: u128 = product >> 32;
        core::cmp::min(shifted, u64::MAX as u128) as u64
    }

    fn ref_pow_q32(base_q32: u64, exp: u16) -> u64 {
        let mut result: u64 = 1u64 << 32; // 1.0 in Q32
        let mut factor: u64 = base_q32;
        let mut power: u32 = u32::from(exp);

        while power > 0 {
            if (power & 1) == 1 {
                result = ref_mul_by_q32(result, factor);
            }

            power >>= 1;

            if power > 0 {
                factor = ref_mul_by_q32(factor, factor);
            }
        }

        result
    }

    fn ref_decay_factor_q32(half_life: u16) -> u64 {
        if half_life == 0 {
            return 1u64 << 32; // 1.0
        }

        let one_q32: u64 = 1u64 << 32;
        let half_q32: u64 = 1u64 << 31; // 0.5
        let mut lo: u64 = 0;
        let mut hi: u64 = one_q32;

        while lo + 1 < hi {
            let mid = lo + ((hi - lo) >> 1);
            let mid_pow = ref_pow_q32(mid, half_life);

            if mid_pow > half_q32 {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        lo
    }

    fn ref_next_burn(
        prev: u64,
        factor_q32: u64,
        half_life: u16,
        current_block: u64,
        min_burn: u64,
        max_burn: u64,
    ) -> u64 {
        let next = if half_life == 0 || current_block <= 1 {
            prev
        } else {
            ref_mul_by_q32(prev, factor_q32)
        };

        next.clamp(min_burn, max_burn)
    }

    new_test_ext(1).execute_with(|| {
        // Use neutral burn bounds so this test stays focused on the
        // registration-price decay/reset path triggered from on_initialize.
        let test_min_burn: u64 = 0;
        let test_max_burn: u64 = u64::MAX;

        // Non-root subnets: many half-lives and many different burn shapes.
        // Use high tempo + no emission block so the test stays focused on the
        // registration-price update path triggered from on_initialize.
        let cases: [Case; 17] = [
            Case {
                netuid: 1,
                half_life: 0,
                initial_burn: 777_777,
            },
            Case {
                netuid: 2,
                half_life: 0,
                initial_burn: 0,
            },
            Case {
                netuid: 3,
                half_life: 1,
                initial_burn: 0,
            },
            Case {
                netuid: 4,
                half_life: 1,
                initial_burn: 1,
            },
            Case {
                netuid: 5,
                half_life: 1,
                initial_burn: 2,
            },
            Case {
                netuid: 6,
                half_life: 2,
                initial_burn: 1_024,
            },
            Case {
                netuid: 7,
                half_life: 3,
                initial_burn: 4_096,
            },
            Case {
                netuid: 8,
                half_life: 4,
                initial_burn: 65_536,
            },
            Case {
                netuid: 9,
                half_life: 5,
                initial_burn: 123_456,
            },
            Case {
                netuid: 10,
                half_life: 7,
                initial_burn: 999_999,
            },
            Case {
                netuid: 11,
                half_life: 8,
                initial_burn: 1_000_000,
            },
            Case {
                netuid: 12,
                half_life: 16,
                initial_burn: 1_000_000,
            },
            Case {
                netuid: 13,
                half_life: 64,
                initial_burn: 1_000_000,
            },
            Case {
                netuid: 14,
                half_life: 360,
                initial_burn: 10_000_000_000,
            },
            Case {
                netuid: 15,
                half_life: 1_000,
                initial_burn: 100_000_000_000,
            },
            Case {
                netuid: 16,
                half_life: 4_096,
                initial_burn: 1_000_000_000_000,
            },
            Case {
                netuid: 17,
                half_life: u16::MAX,
                initial_burn: 123_000_000_000,
            },
        ];

        let mut expected_burns: Vec<u64> = Vec::with_capacity(cases.len());
        let mut factors_q32: Vec<u64> = Vec::with_capacity(cases.len());

        for (idx, case) in cases.iter().enumerate() {
            let netuid = NetUid::from(case.netuid);

            add_network_without_emission_block(netuid, 10_000, 0);
            BurnHalfLife::<Test>::insert(netuid, case.half_life);

            // Vary this on purpose to prove this path is pure decay + reset.
            BurnIncreaseMult::<Test>::insert(
                netuid,
                U64F64::from_num(idx).saturating_add(U64F64::from_num(2)),
            );
            SubtensorModule::set_min_burn(netuid, TaoBalance::from(test_min_burn));
            SubtensorModule::set_max_burn(netuid, TaoBalance::from(test_max_burn));
            SubtensorModule::set_burn(netuid, case.initial_burn.into());

            expected_burns.push(case.initial_burn);
            factors_q32.push(ref_decay_factor_q32(case.half_life));
        }

        // Root subnet: only root gets RegistrationsThisInterval reset here.
        let root = NetUid::from(0);
        if !SubtensorModule::if_subnet_exist(root) {
            SubtensorModule::init_new_network(root, 2);
        }

        let root_half_life: u16 = 17;
        let root_initial_burn: u64 = 123_456;
        let root_factor_q32 = ref_decay_factor_q32(root_half_life);
        let mut root_expected_burn = root_initial_burn;

        BurnHalfLife::<Test>::insert(root, root_half_life);
        BurnIncreaseMult::<Test>::insert(root, U64F64::from_num(97));
        SubtensorModule::set_min_burn(root, TaoBalance::from(test_min_burn));
        SubtensorModule::set_max_burn(root, TaoBalance::from(test_max_burn));
        SubtensorModule::set_burn(root, root_initial_burn.into());

        let total_blocks: u16 = 5_000;
        let mut root_interval_resets_seen: u32 = 0;
        let mut root_interval_non_resets_seen: u32 = 0;

        for step in 0..total_blocks {
            // Seed per-block counters before stepping so the next block's
            // on_initialize must clear them.
            for (idx, case) in cases.iter().enumerate() {
                let netuid = NetUid::from(case.netuid);
                let seed_this_block: u16 =
                    (step.wrapping_mul(17).wrapping_add(idx as u16) % 29).saturating_add(1);

                RegistrationsThisBlock::<Test>::insert(netuid, seed_this_block);
            }

            let root_this_block_seed: u16 = (step.wrapping_mul(19) % 31).saturating_add(1);
            let root_interval_seed: u16 = (step.wrapping_mul(23) % 37).saturating_add(1);

            RegistrationsThisBlock::<Test>::insert(root, root_this_block_seed);
            RegistrationsThisInterval::<Test>::insert(root, root_interval_seed);

            let next_block: u64 = System::block_number() + 1;
            let root_should_reset_interval = SubtensorModule::should_run_epoch(root, next_block);

            step_block(1);

            assert_eq!(System::block_number(), next_block);

            for (idx, case) in cases.iter().enumerate() {
                let netuid = NetUid::from(case.netuid);

                expected_burns[idx] = ref_next_burn(
                    expected_burns[idx],
                    factors_q32[idx],
                    case.half_life,
                    next_block,
                    test_min_burn,
                    test_max_burn,
                );

                let actual_burn: u64 = SubtensorModule::get_burn(netuid).into();

                assert_eq!(
                    actual_burn,
                    expected_burns[idx],
                    "burn mismatch: netuid={:?}, half_life={}, block={}",
                    netuid,
                    case.half_life,
                    next_block
                );

                assert_eq!(
                    RegistrationsThisBlock::<Test>::get(netuid),
                    0,
                    "RegistrationsThisBlock not reset: netuid={:?}, half_life={}, block={}",
                    netuid,
                    case.half_life,
                    next_block
                );
            }

            root_expected_burn = ref_next_burn(
                root_expected_burn,
                root_factor_q32,
                root_half_life,
                next_block,
                test_min_burn,
                test_max_burn,
            );

            let actual_root_burn: u64 = SubtensorModule::get_burn(root).into();
            assert_eq!(
                actual_root_burn,
                root_expected_burn,
                "root burn mismatch at block={}",
                next_block
            );

            assert_eq!(
                RegistrationsThisBlock::<Test>::get(root),
                0,
                "root RegistrationsThisBlock not reset at block={}",
                next_block
            );

            let actual_root_interval = RegistrationsThisInterval::<Test>::get(root);
            if root_should_reset_interval {
                root_interval_resets_seen = root_interval_resets_seen.saturating_add(1);
                assert_eq!(
                    actual_root_interval,
                    0,
                    "root RegistrationsThisInterval should reset on epoch boundary at block={}",
                    next_block
                );
            } else {
                root_interval_non_resets_seen =
                    root_interval_non_resets_seen.saturating_add(1);
                assert_eq!(
                    actual_root_interval,
                    root_interval_seed,
                    "root RegistrationsThisInterval should remain unchanged off epoch boundary at block={}",
                    next_block
                );
            }
        }

        // Prove that the root-only interval branch was exercised both ways.
        assert!(root_interval_resets_seen > 0);
        assert!(root_interval_non_resets_seen > 0);

        // Final assertion for every non-root case.
        for (idx, case) in cases.iter().enumerate() {
            let netuid = NetUid::from(case.netuid);
            let final_actual: u64 = SubtensorModule::get_burn(netuid).into();

            assert_eq!(
                final_actual,
                expected_burns[idx],
                "final burn mismatch: netuid={:?}, half_life={}, initial_burn={}, after {} blocks",
                netuid,
                case.half_life,
                case.initial_burn,
                total_blocks
            );
        }

        // Final assertion for root too.
        let final_root_burn: u64 = SubtensorModule::get_burn(root).into();
        assert_eq!(
            final_root_burn,
            root_expected_burn,
            "final root burn mismatch after {} blocks",
            total_blocks
        );
    });
}

#[test]
fn test_burned_register_immediately_bumps_price_many_multipliers_and_same_block_registrations() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        netuid: u16,
        initial_burn: u64,
        mult: u64,
        min_burn: u64,
        max_burn: u64,
        registrations: u16,
    }

    fn ref_bump(prev: u64, mult: u64, min_burn: u64, max_burn: u64) -> u64 {
        let mult = U64F64::from_num(mult.max(1));
        let next = (U64F64::from_num(prev) * mult).floor().to_num::<u64>();
        next.clamp(min_burn, max_burn)
    }

    fn ensure_spendable_balance(coldkey: U256, burn: u64) {
        // Leave a small remainder so the real burned_register path does not trip
        // balance-after-withdrawal checks. Use saturating_add so zero-burn cases
        // remain valid and never underflow.
        let min_remaining: u64 = 1;
        let buffer: u64 = 10;
        let needed: u64 = burn + min_remaining + buffer;

        let current: u64 = SubtensorModule::get_coldkey_balance(&coldkey).into();
        if current < needed {
            add_balance_to_coldkey_account(&coldkey, (needed - current).into());
        }
    }

    new_test_ext(1).execute_with(|| {
        // Keep reserves large so swap mechanics are never the limiting factor
        // in this burn-price-path test.
        let reserve: u64 = 1_000_000_000_000_000;

        let cases: [Case; 12] = [
            Case {
                netuid: 1,
                initial_burn: 0,
                mult: 0,
                min_burn: 100_000,
                max_burn: u64::MAX,
                registrations: 7,
            },
            Case {
                netuid: 2,
                initial_burn: 5,
                mult: 0,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 7,
            },
            Case {
                netuid: 3,
                initial_burn: 0,
                mult: 1,
                min_burn: 100_000,
                max_burn: u64::MAX,
                registrations: 7,
            },
            Case {
                netuid: 4,
                initial_burn: 7,
                mult: 1,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 7,
            },
            Case {
                netuid: 5,
                initial_burn: 2,
                mult: 2,
                min_burn: 1,
                max_burn: 400,
                registrations: 8,
            },
            Case {
                netuid: 6,
                initial_burn: 3,
                mult: 3,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 6,
            },
            Case {
                netuid: 7,
                initial_burn: 4,
                mult: 4,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 6,
            },
            Case {
                netuid: 8,
                initial_burn: 7,
                mult: 10,
                min_burn: 1,
                max_burn: 500_000,
                registrations: 5,
            },
            Case {
                netuid: 9,
                initial_burn: 1,
                mult: 100,
                min_burn: 1,
                max_burn: 5_000_000_000,
                registrations: 5,
            },
            Case {
                netuid: 10,
                initial_burn: 1,
                mult: 1_000,
                min_burn: 1,
                max_burn: 50_000_000_000,
                registrations: 4,
            },
            Case {
                netuid: 11,
                initial_burn: 5,
                mult: 1,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 32,
            },
            Case {
                netuid: 12,
                initial_burn: 1,
                mult: 2,
                min_burn: 1,
                max_burn: u64::MAX,
                registrations: 20,
            },
        ];

        for case in cases {
            let netuid = NetUid::from(case.netuid);
            let coldkey = U256::from(100_000u64 + u64::from(case.netuid));
            let seeded_interval: u16 = case.netuid.saturating_mul(11).saturating_add(7);

            add_network(netuid, 13, 0);
            mock::setup_reserves(netuid, reserve.into(), reserve.into());

            // Disable decay entirely so this test isolates the immediate
            // per-registration bump path.
            BurnHalfLife::<Test>::insert(netuid, 0);
            BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(case.mult));
            SubtensorModule::set_min_burn(netuid, TaoBalance::from(case.min_burn));
            SubtensorModule::set_max_burn(netuid, TaoBalance::from(case.max_burn));
            SubtensorModule::set_burn(netuid, case.initial_burn.into());

            // Seed this to prove burned_register does not mutate it for non-root.
            RegistrationsThisInterval::<Test>::insert(netuid, seeded_interval);

            let mut expected_burn: u64 = case.initial_burn;
            let mut expected_recycled: u64 = 0;
            let mut expected_total_regs: u16 = 0;

            assert_eq!(
                RegistrationsThisBlock::<Test>::get(netuid),
                0,
                "RegistrationsThisBlock should start at zero for netuid={}",
                case.netuid
            );
            assert_eq!(
                RegistrationsThisInterval::<Test>::get(netuid),
                seeded_interval,
                "RegistrationsThisInterval seed mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_burn(netuid),
                case.initial_burn.into(),
                "initial burn mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_min_burn(netuid),
                case.min_burn.into(),
                "min burn mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_max_burn(netuid),
                case.max_burn.into(),
                "max burn mismatch for netuid={}",
                case.netuid
            );

            // All successful registrations for this case intentionally happen in
            // the same block. Each one should pay the current burn at call entry,
            // and then immediately bump the burn for the very next registration.
            for reg_idx in 0..case.registrations {
                let hotkey = U256::from(
                    u64::from(case.netuid) * 10_000u64 + u64::from(reg_idx) + 1u64,
                );
                let expected_uid = expected_total_regs;

                ensure_spendable_balance(coldkey, expected_burn);

                let balance_before: u64 = SubtensorModule::get_coldkey_balance(&coldkey).into();

                assert_eq!(
                    SubtensorModule::get_burn(netuid),
                    expected_burn.into(),
                    "unexpected burn before registration for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    BurnHalfLife::<Test>::get(netuid),
                    0,
                    "half-life changed unexpectedly for netuid={}",
                    case.netuid
                );
                assert_eq!(
                    BurnIncreaseMult::<Test>::get(netuid),
                    U64F64::from_num(case.mult),
                    "multiplier changed unexpectedly for netuid={}",
                    case.netuid
                );
                assert_eq!(
                    RegistrationsThisBlock::<Test>::get(netuid),
                    expected_total_regs,
                    "RegistrationsThisBlock mismatch before registration for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    RegistrationsThisInterval::<Test>::get(netuid),
                    seeded_interval,
                    "non-root interval counter changed before registration for netuid={}",
                    case.netuid
                );

                assert_ok!(SubtensorModule::burned_register(
                    <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                    netuid,
                    hotkey
                ));

                let expected_after_burn =
                    ref_bump(expected_burn, case.mult, case.min_burn, case.max_burn);
                let balance_after: u64 = SubtensorModule::get_coldkey_balance(&coldkey).into();

                expected_recycled = expected_recycled.saturating_add(expected_burn);
                expected_total_regs = expected_total_regs.saturating_add(1);

                // The real registration should charge the pre-bump burn.
                assert_eq!(
                    balance_after,
                    balance_before.saturating_sub(expected_burn),
                    "coldkey balance delta used the wrong burn price for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    SubtensorModule::get_rao_recycled(netuid),
                    expected_recycled.into(),
                    "rao_recycled mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );

                // The real registration should bump the burn immediately for the
                // next registration in the same block.
                assert_eq!(
                    SubtensorModule::get_burn(netuid),
                    expected_after_burn.into(),
                    "post-registration burn mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );

                // Bookkeeping: only the per-block counter should change here.
                assert_eq!(
                    RegistrationsThisBlock::<Test>::get(netuid),
                    expected_total_regs,
                    "RegistrationsThisBlock mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    RegistrationsThisInterval::<Test>::get(netuid),
                    seeded_interval,
                    "non-root RegistrationsThisInterval changed unexpectedly for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );

                // Registration side effects should still be correct.
                assert_eq!(
                    SubtensorModule::get_subnetwork_n(netuid),
                    expected_total_regs,
                    "subnetwork_n mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );

                let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
                assert_eq!(
                    uid,
                    expected_uid,
                    "uid mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey),
                    coldkey,
                    "owner mismatch for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );
                assert_eq!(
                    SubtensorModule::get_stake_for_uid_and_subnetwork(netuid, uid),
                    AlphaBalance::ZERO,
                    "newly registered neuron should start with zero stake for netuid={} reg_idx={}",
                    case.netuid,
                    reg_idx
                );

                // Edge-case invariants from bump_registration_price_after_registration().
                if expected_burn == 0 {
                    assert_eq!(
                        expected_after_burn,
                        case.min_burn,
                        "zero burn should recover to min burn after a successful registration for netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                    assert_eq!(
                        balance_after,
                        balance_before,
                        "zero burn should not withdraw balance for netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                if case.mult == 0 && expected_burn != 0 {
                    let clamped_expected = expected_burn.clamp(case.min_burn, case.max_burn);
                    assert_eq!(
                        expected_after_burn,
                        clamped_expected,
                        "multiplier=0 should behave like multiplier=1, subject to min/max clamp, on netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                if case.mult == 1 && expected_burn != 0 {
                    let clamped_expected = expected_burn.clamp(case.min_burn, case.max_burn);
                    assert_eq!(
                        expected_after_burn,
                        clamped_expected,
                        "multiplier=1 should preserve burn subject to min/max clamp on netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                if expected_after_burn == case.min_burn {
                    assert!(
                        expected_after_burn >= case.min_burn,
                        "burn fell below min burn on netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                if expected_after_burn == case.max_burn {
                    assert!(
                        expected_after_burn <= case.max_burn,
                        "burn exceeded max burn on netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                if case.mult > 1 && expected_burn != 0 && expected_burn < case.max_burn {
                    assert!(
                        expected_after_burn >= expected_burn || expected_after_burn == case.max_burn,
                        "multiplier>1 should not decrease burn before max clamp on netuid={} reg_idx={}",
                        case.netuid,
                        reg_idx
                    );
                }

                expected_burn = expected_after_burn;
            }

            assert_eq!(
                RegistrationsThisBlock::<Test>::get(netuid),
                case.registrations,
                "same-block registration count mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                RegistrationsThisInterval::<Test>::get(netuid),
                seeded_interval,
                "same-block registrations should not touch non-root interval counter for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_burn(netuid),
                expected_burn.into(),
                "final same-block burn mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_subnetwork_n(netuid),
                case.registrations,
                "final same-block subnet population mismatch for netuid={}",
                case.netuid
            );

            // One block later, the per-block counter should reset. With half-life
            // disabled, burn should remain unchanged across the block step.
            step_block(1);

            assert_eq!(
                RegistrationsThisBlock::<Test>::get(netuid),
                0,
                "RegistrationsThisBlock should reset on next block for netuid={}",
                case.netuid
            );
            assert_eq!(
                RegistrationsThisInterval::<Test>::get(netuid),
                seeded_interval,
                "RegistrationsThisInterval should remain unchanged after block step for netuid={}",
                case.netuid
            );
            assert_eq!(
                BurnHalfLife::<Test>::get(netuid),
                0,
                "final half-life mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                BurnIncreaseMult::<Test>::get(netuid),
                U64F64::from_num(case.mult),
                "final multiplier mismatch for netuid={}",
                case.netuid
            );
            assert_eq!(
                SubtensorModule::get_burn(netuid),
                expected_burn.into(),
                "burn changed across block step even though half-life is disabled for netuid={}",
                case.netuid
            );
        }

        // Exact long-run spot checks after all real burned_register calls.
        assert_eq!(SubtensorModule::get_burn(NetUid::from(1u16)), 100_000u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(1u16)),
            600_000u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(2u16)), 5u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(2u16)),
            35u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(3u16)), 100_000u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(3u16)),
            600_000u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(4u16)), 7u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(4u16)),
            49u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(5u16)), 400u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(5u16)),
            510u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(6u16)), 2_187u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(6u16)),
            1_092u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(7u16)), 16_384u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(7u16)),
            5_460u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(8u16)), 500_000u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(8u16)),
            77_777u64.into()
        );

        assert_eq!(
            SubtensorModule::get_burn(NetUid::from(9u16)),
            5_000_000_000u64.into()
        );
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(9u16)),
            101_010_101u64.into()
        );

        assert_eq!(
            SubtensorModule::get_burn(NetUid::from(10u16)),
            50_000_000_000u64.into()
        );
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(10u16)),
            1_001_001_001u64.into()
        );

        assert_eq!(SubtensorModule::get_burn(NetUid::from(11u16)), 5u64.into());
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(11u16)),
            160u64.into()
        );

        assert_eq!(
            SubtensorModule::get_burn(NetUid::from(12u16)),
            1_048_576u64.into()
        );
        assert_eq!(
            SubtensorModule::get_rao_recycled(NetUid::from(12u16)),
            1_048_575u64.into()
        );
    });
}
