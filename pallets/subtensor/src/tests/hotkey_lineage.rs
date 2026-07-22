#![allow(clippy::unwrap_used, clippy::expect_used)]

use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::AlphaBalance;

use super::mock::*;
use crate::*;

#[test]
fn test_hotkey_swap_records_lineage_on_subnet_only() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);
        let other_hk = U256::from(4);

        let netuid_a = add_dynamic_network(&h0, &coldkey);
        let netuid_b = add_dynamic_network(&other_hk, &coldkey);
        register_ok_neuron(netuid_b, h0, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid_a),
            false,
        ));

        assert_eq!(HotkeySuccessor::<Test>::get(netuid_a, h0), Some(h1));
        assert_eq!(SubtensorModule::hotkey_root(netuid_a, &h1), h0);
        assert!(HotkeySuccessor::<Test>::get(netuid_b, h0).is_none());
        // h0 remains registered on netuid_b; no lineage written there.
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid_b, &h0
        ));
    });
}

#[test]
fn test_hotkey_swap_lineage_chain_and_tip() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);
        let h2 = U256::from(4);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));

        // Cooldown is strict `<`: need interval + 1 after the recorded swap block.
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h1,
            &h2,
            Some(netuid),
            false,
        ));

        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h0), Some(h1));
        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h1), Some(h2));
        assert_eq!(SubtensorModule::hotkey_root(netuid, &h2), h0);
        assert!(SubtensorModule::same_hotkey_lineage(netuid, &h0, &h2));
        assert_eq!(SubtensorModule::hotkey_lineage_tip(netuid, &h0), h2);
    });
}

#[test]
fn test_hotkey_swap_all_subnets_records_lineage_on_each() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);

        let netuid_a = add_dynamic_network(&h0, &coldkey);
        let netuid_b = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            None,
            false,
        ));

        for netuid in [netuid_a, netuid_b] {
            assert_eq!(HotkeySuccessor::<Test>::get(netuid, h0), Some(h1));
            assert_eq!(SubtensorModule::hotkey_root(netuid, &h1), h0);
        }
    });
}

#[test]
fn test_all_subnets_swap_records_lineage_for_residual_collateral() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);
        let other_hk = U256::from(4);

        let netuid_live = add_dynamic_network(&h0, &coldkey);
        // Different owner hotkey so h0 is not a member on the residual subnet.
        let netuid_residual = add_dynamic_network(&other_hk, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let locked = AlphaBalance::from(10_000_000_000u64);
        MinerCollateral::<Test>::insert(
            (netuid_residual, h0, coldkey),
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid_residual, coldkey, locked);
        ColdkeyCollateralHotkeys::<Test>::mutate(netuid_residual, coldkey, |hotkeys| {
            hotkeys.try_push(h0).unwrap();
        });

        assert!(!SubtensorModule::is_hotkey_registered_on_network(
            netuid_residual,
            &h0
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            None,
            false,
        ));

        assert_eq!(HotkeySuccessor::<Test>::get(netuid_live, h0), Some(h1));
        assert_eq!(
            HotkeySuccessor::<Test>::get(netuid_residual, h0),
            Some(h1),
            "lineage must be recorded where residual collateral migrates"
        );
        assert!(MinerCollateral::<Test>::get((netuid_residual, h0, coldkey)).is_none());
        assert_eq!(
            MinerCollateral::<Test>::get((netuid_residual, h1, coldkey))
                .expect("bond moved")
                .locked,
            locked
        );
    });
}

#[test]
fn test_bonded_hotkey_swap_migrates_collateral_keep_stake_blocked() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);
        let h2 = U256::from(4);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &h0,
            &coldkey,
            netuid,
            100_000_000_000u64.into(),
        );
        let locked = AlphaBalance::from(40_000_000_000u64);
        MinerCollateral::<Test>::insert(
            (netuid, h0, coldkey),
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, coldkey, locked);
        ColdkeyCollateralHotkeys::<Test>::mutate(netuid, coldkey, |hotkeys| {
            hotkeys.try_push(h0).unwrap();
        });

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &h0,
                &h1,
                Some(netuid),
                true,
            ),
            Error::<Test>::KeepStakeBlockedByCollateral
        );

        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));

        assert!(MinerCollateral::<Test>::get((netuid, h0, coldkey)).is_none());
        let migrated = MinerCollateral::<Test>::get((netuid, h1, coldkey))
            .expect("collateral must follow the UID");
        assert_eq!(migrated.locked, locked);
        assert_eq!(ColdkeyMinerCollateral::<Test>::get(netuid, coldkey), locked);

        // Validator permit must not reopen the keep_stake escape.
        let uid = Uids::<Test>::get(netuid, h1).expect("registered");
        SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &h1,
                &h2,
                Some(netuid),
                true,
            ),
            Error::<Test>::KeepStakeBlockedByCollateral
        );
    });
}

/// At the collateral-hotkey cap, a rename must reserve the destination slot by
/// rewriting the old index entry in place — never mutate first and fail after.
#[test]
fn test_bonded_hotkey_swap_renames_index_at_cap() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Fill the index to capacity with filler hotkeys, then overwrite the
        // last slot with the bonded hotkey we will swap.
        for i in 0..MAX_COLDKEY_COLLATERAL_HOTKEYS {
            let hot = U256::from(10_000u64 + u64::from(i));
            MinerCollateral::<Test>::insert(
                (netuid, hot, coldkey),
                MinerCollateralState {
                    locked: AlphaBalance::from(1u64),
                    drain_ratio: U64F64::from_num(1),
                    min_locked: AlphaBalance::ZERO,
                    earned: AlphaBalance::ZERO,
                },
            );
            ColdkeyCollateralHotkeys::<Test>::mutate(netuid, coldkey, |hotkeys| {
                hotkeys.try_push(hot).unwrap();
            });
        }
        // Replace the last indexed filler with h0 (the registered bonded hotkey).
        let last = U256::from(10_000u64 + u64::from(MAX_COLDKEY_COLLATERAL_HOTKEYS - 1));
        MinerCollateral::<Test>::remove((netuid, last, coldkey));
        MinerCollateral::<Test>::insert(
            (netuid, h0, coldkey),
            MinerCollateralState {
                locked: AlphaBalance::from(40_000_000_000u64),
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyCollateralHotkeys::<Test>::mutate(netuid, coldkey, |hotkeys| {
            let idx = hotkeys.iter().position(|h| *h == last).unwrap();
            hotkeys[idx] = h0;
        });
        ColdkeyMinerCollateral::<Test>::insert(
            netuid,
            coldkey,
            AlphaBalance::from(40_000_000_031u64),
        );

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));

        assert!(MinerCollateral::<Test>::get((netuid, h0, coldkey)).is_none());
        assert!(MinerCollateral::<Test>::get((netuid, h1, coldkey)).is_some());
        let indexed = ColdkeyCollateralHotkeys::<Test>::get(netuid, coldkey);
        assert!(indexed.contains(&h1));
        assert!(!indexed.contains(&h0));
        assert_eq!(indexed.len(), MAX_COLDKEY_COLLATERAL_HOTKEYS as usize);
    });
}

#[test]
fn test_hotkey_lineage_reverse_swap_does_not_cycle() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));
        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h0), Some(h1));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h1,
            &h0,
            Some(netuid),
            false,
        ));

        // Destination tip clears its outgoing edge; no A↔B cycle.
        assert!(HotkeySuccessor::<Test>::get(netuid, h0).is_none());
        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h1), Some(h0));
        assert_eq!(SubtensorModule::hotkey_lineage_tip(netuid, &h1), h0);
        assert_eq!(SubtensorModule::hotkey_lineage_tip(netuid, &h0), h0);
        assert!(SubtensorModule::same_hotkey_lineage(netuid, &h0, &h1));
    });
}

#[test]
fn test_reregister_clears_stale_successor_for_tip() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);
        let h2 = U256::from(4);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));
        assert_eq!(SubtensorModule::hotkey_lineage_tip(netuid, &h0), h1);

        // h0 becomes live again; tip must not keep following the old rename.
        register_ok_neuron(netuid, h0, coldkey, 0);
        assert!(HotkeySuccessor::<Test>::get(netuid, h0).is_none());
        assert_eq!(SubtensorModule::hotkey_lineage_tip(netuid, &h0), h0);
        // Root-based identity still links the prior tip.
        assert!(SubtensorModule::same_hotkey_lineage(netuid, &h0, &h1));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h2,
            Some(netuid),
            false,
        ));
        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h0), Some(h2));
        assert_eq!(SubtensorModule::hotkey_root(netuid, &h2), h0);
        assert!(SubtensorModule::same_hotkey_lineage(netuid, &h1, &h2));
    });
}

#[test]
fn test_dissolve_clears_hotkey_lineage_maps() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let h0 = U256::from(2);
        let h1 = U256::from(3);

        let netuid = add_dynamic_network(&h0, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get() + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &h0,
            &h1,
            Some(netuid),
            false,
        ));
        assert_eq!(HotkeySuccessor::<Test>::get(netuid, h0), Some(h1));
        assert_eq!(HotkeyRoot::<Test>::get(netuid, h1), Some(h0));

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        let mut guard = 0u32;
        while CurrentDissolveCleanupStatus::<Test>::get().is_some()
            || DissolveCleanupQueue::<Test>::get().contains(&netuid)
        {
            guard = guard.saturating_add(1);
            assert!(guard < 256, "dissolve cleanup stalled (guard={guard})");
            run_block_idle();
        }

        assert!(HotkeySuccessor::<Test>::get(netuid, h0).is_none());
        assert!(HotkeyRoot::<Test>::get(netuid, h1).is_none());
    });
}
