#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! reset inactive subnet state.

use super::helpers::*;
use super::prelude::*;

fn do_setup_unactive_sn() -> (Vec<NetUid>, Vec<NetUid>) {
    // Register some subnets
    let netuid0 = add_dynamic_network_without_emission_block(&U256::from(0), &U256::from(0));
    let netuid1 = add_dynamic_network_without_emission_block(&U256::from(1), &U256::from(1));
    let netuid2 = add_dynamic_network_without_emission_block(&U256::from(2), &U256::from(2));
    let inactive_netuids = vec![netuid0, netuid1, netuid2];
    // Add active subnets
    let netuid3 = add_dynamic_network_without_emission_block(&U256::from(3), &U256::from(3));
    let netuid4 = add_dynamic_network_without_emission_block(&U256::from(4), &U256::from(4));
    let netuid5 = add_dynamic_network_without_emission_block(&U256::from(5), &U256::from(5));
    let active_netuids = vec![netuid3, netuid4, netuid5];
    let netuids: Vec<NetUid> = inactive_netuids
        .iter()
        .chain(active_netuids.iter())
        .copied()
        .collect();

    let initial_tao = Pallet::<Test>::get_network_min_lock();
    let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

    const EXTRA_POOL_TAO: u64 = 123_123_u64;
    const EXTRA_POOL_ALPHA: u64 = 123_123_u64;

    // Add stake to the subnet pools
    for netuid in &netuids {
        let extra_for_pool = TaoBalance::from(EXTRA_POOL_TAO);
        let stake_in_pool = TaoBalance::from(
            u64::from(initial_tao)
                .checked_add(EXTRA_POOL_TAO)
                .expect("initial_tao + extra_for_pool overflow"),
        );
        SubnetTAO::<Test>::insert(netuid, stake_in_pool);
        TotalStake::<Test>::mutate(|total_stake| {
            let updated_total = u64::from(*total_stake)
                .checked_add(EXTRA_POOL_TAO)
                .expect("total stake overflow");
            *total_stake = updated_total.into();
        });
        TotalIssuance::<Test>::mutate(|total_issuance| {
            let updated_total = u64::from(*total_issuance)
                .checked_add(EXTRA_POOL_TAO)
                .expect("total issuance overflow");
            *total_issuance = updated_total.into();
        });

        let subnet_alpha_in = AlphaBalance::from(
            u64::from(initial_alpha)
                .checked_add(EXTRA_POOL_ALPHA)
                .expect("initial alpha + extra alpha overflow"),
        );
        SubnetAlphaIn::<Test>::insert(netuid, subnet_alpha_in);
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(EXTRA_POOL_ALPHA));
        SubnetVolume::<Test>::insert(netuid, 123123_u128);

        // Try registering on the subnet to simulate a real network
        // give balance to the coldkey
        let coldkey_account_id = U256::from(1111);
        let hotkey_account_id = U256::from(1111);
        let burn_cost = SubtensorModule::get_burn(*netuid);
        // Registration requires keep-alive coverage above the burn (Preservation::Preserve).
        let fund = TaoBalance::from(
            u64::from(burn_cost)
                .checked_add(u64::from(ExistentialDeposit::get()))
                .and_then(|value| value.checked_add(10))
                .expect("burn funding overflow"),
        );
        add_balance_to_coldkey_account(&coldkey_account_id, fund);
        TotalIssuance::<Test>::mutate(|total_issuance| {
            let updated_total = u64::from(*total_issuance)
                .checked_add(u64::from(fund))
                .expect("total issuance overflow (burn)");
            *total_issuance = updated_total.into();
        });

        // register the neuron
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            *netuid,
            hotkey_account_id
        ));
    }

    for netuid in &active_netuids {
        // Set the FirstEmissionBlockNumber for the active subnet
        FirstEmissionBlockNumber::<Test>::insert(netuid, 100);
        // Also set SubtokenEnabled to true
        SubtokenEnabled::<Test>::insert(netuid, true);
    }

    let alpha_amt = AlphaBalance::from(123123_u64);
    // Create some Stake entries
    for netuid in &netuids {
        for hotkey in 0..10 {
            let hk = U256::from(hotkey);
            TotalHotkeyAlpha::<Test>::insert(hk, netuid, alpha_amt);
            TotalHotkeyShares::<Test>::insert(hk, netuid, U64F64::from(123123_u64));
            TotalHotkeyAlphaLastEpoch::<Test>::insert(hk, netuid, alpha_amt);

            RootClaimable::<Test>::mutate(hk, |claimable| {
                claimable.insert(*netuid, I96F32::from(alpha_amt.to_u64()));
            });
            for coldkey in 0..10 {
                let ck = U256::from(coldkey);
                Alpha::<Test>::insert((hk, ck, netuid), U64F64::from(123_u64));
                RootClaimed::<Test>::insert((netuid, hk, ck), 222_u128);
            }
        }
    }
    // Add some pending emissions
    let alpha_em_amt = AlphaBalance::from(355555_u64);
    for netuid in &netuids {
        PendingServerEmission::<Test>::insert(netuid, alpha_em_amt);
        PendingValidatorEmission::<Test>::insert(netuid, alpha_em_amt);
        PendingRootAlphaDivs::<Test>::insert(netuid, alpha_em_amt);
        PendingOwnerCut::<Test>::insert(netuid, alpha_em_amt);

        SubnetTaoInEmission::<Test>::insert(netuid, TaoBalance::from(12345678_u64));
        SubnetAlphaInEmission::<Test>::insert(netuid, AlphaBalance::from(12345678_u64));
        SubnetAlphaOutEmission::<Test>::insert(netuid, AlphaBalance::from(12345678_u64));
    }

    (active_netuids, inactive_netuids)
}

#[test]
fn test_migrate_reset_unactive_sn_get_unactive_netuids() {
    new_test_ext(1).execute_with(|| {
        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();

        let initial_tao = Pallet::<Test>::get_network_min_lock();
        let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

        let (unactive_netuids, w) =
            crate::migrations::migrate_reset_unactive_sn::get_unactive_sn_netuids::<Test>(
                initial_alpha,
            );
        // Make sure ALL the inactive subnets are in the unactive netuids
        assert!(
            inactive_netuids
                .iter()
                .all(|netuid| unactive_netuids.contains(netuid))
        );
        // Make sure the active subnets are not in the unactive netuids
        assert!(
            active_netuids
                .iter()
                .all(|netuid| !unactive_netuids.contains(netuid))
        );
    });
}

#[test]
fn test_migrate_reset_unactive_sn() {
    new_test_ext(1).execute_with(|| {
        use sp_std::collections::btree_map::BTreeMap;

        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();

        let initial_tao = Pallet::<Test>::get_network_min_lock();
        let initial_alpha: AlphaBalance = initial_tao.to_u64().into();

        let mut locked_before: BTreeMap<NetUid, TaoBalance> = BTreeMap::new();
        let mut rao_recycled_before: BTreeMap<NetUid, TaoBalance> = BTreeMap::new();

        for netuid in active_netuids.iter().chain(inactive_netuids.iter()) {
            locked_before.insert(*netuid, SubnetLocked::<Test>::get(*netuid));
            rao_recycled_before.insert(*netuid, RAORecycledForRegistration::<Test>::get(netuid));
        }

        // Run the migration
        let w = crate::migrations::migrate_reset_unactive_sn::migrate_reset_unactive_sn::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Verify the results
        for netuid in &inactive_netuids {
            let netuid = *netuid;

            assert_eq!(
                SubnetLocked::<Test>::get(netuid),
                *locked_before.get(&netuid).unwrap(),
                "SubnetLocked unexpectedly changed for inactive subnet {netuid:?}"
            );
            assert_eq!(
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap(),
                "RAORecycledForRegistration unexpectedly changed for inactive subnet {netuid:?}"
            );

            assert_eq!(
                PendingServerEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                PendingValidatorEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                PendingRootAlphaDivs::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                // not modified
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap()
            );
            assert_eq!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetTAO::<Test>::get(netuid), initial_tao);
            assert_ne!(SubnetAlphaIn::<Test>::get(netuid), initial_alpha);
            assert_ne!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_eq!(SubnetTaoInEmission::<Test>::get(netuid), TaoBalance::ZERO);
            assert_eq!(
                SubnetAlphaInEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                SubnetAlphaOutEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(SubnetVolume::<Test>::get(netuid), 0u128);
            for hotkey in 0..10 {
                let hk = U256::from(hotkey);
                assert_ne!(
                    TotalHotkeyAlpha::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(
                    TotalHotkeyShares::<Test>::get(hk, netuid),
                    U64F64::from_num(0.0)
                );
                assert_ne!(
                    TotalHotkeyAlphaLastEpoch::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(RootClaimable::<Test>::get(hk).get(&netuid), None);
                for coldkey in 0..10 {
                    let ck = U256::from(coldkey);
                    assert_ne!(Alpha::<Test>::get((hk, ck, netuid)), U64F64::from_num(0.0));
                    assert_ne!(RootClaimed::<Test>::get((netuid, hk, ck)), 0u128);
                }
            }

            // Don't touch SubnetLocked
            assert_ne!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
        }

        // !!! Make sure the active subnets were not reset
        for netuid in &active_netuids {
            let netuid = *netuid;

            assert_eq!(
                SubnetLocked::<Test>::get(netuid),
                *locked_before.get(&netuid).unwrap(),
                "SubnetLocked unexpectedly changed for active subnet {netuid:?}"
            );
            assert_eq!(
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap(),
                "RAORecycledForRegistration unexpectedly changed for active subnet {netuid:?}"
            );

            assert_ne!(
                PendingServerEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                PendingValidatorEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                PendingRootAlphaDivs::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_eq!(
                // unchanged (already asserted above via snapshot)
                RAORecycledForRegistration::<Test>::get(netuid),
                *rao_recycled_before.get(&netuid).unwrap()
            );
            assert_ne!(SubnetTaoInEmission::<Test>::get(netuid), TaoBalance::ZERO);
            assert_ne!(
                SubnetAlphaInEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(
                SubnetAlphaOutEmission::<Test>::get(netuid),
                AlphaBalance::ZERO
            );
            assert_ne!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetTAO::<Test>::get(netuid), initial_tao);
            assert_ne!(SubnetAlphaIn::<Test>::get(netuid), initial_alpha);
            assert_ne!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_ne!(SubnetVolume::<Test>::get(netuid), 0u128);
            for hotkey in 0..10 {
                let hk = U256::from(hotkey);
                assert_ne!(
                    TotalHotkeyAlpha::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert_ne!(
                    TotalHotkeyShares::<Test>::get(hk, netuid),
                    U64F64::from_num(0.0)
                );
                assert_ne!(
                    TotalHotkeyAlphaLastEpoch::<Test>::get(hk, netuid),
                    AlphaBalance::ZERO
                );
                assert!(RootClaimable::<Test>::get(hk).contains_key(&netuid));
                for coldkey in 0..10 {
                    let ck = U256::from(coldkey);
                    assert_ne!(Alpha::<Test>::get((hk, ck, netuid)), U64F64::from_num(0.0));
                    assert_ne!(RootClaimed::<Test>::get((netuid, hk, ck)), 0u128);
                }
            }
            // Don't touch SubnetLocked
            assert_ne!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
        }
    });
}

#[test]
fn test_migrate_reset_unactive_sn_idempotence() {
    new_test_ext(1).execute_with(|| {
        let (active_netuids, inactive_netuids) = do_setup_unactive_sn();
        let netuids = inactive_netuids
            .iter()
            .chain(active_netuids.iter())
            .copied()
            .collect::<Vec<_>>();

        // Run total issuance migration *before* running the migration.
        crate::migrations::migrate_init_total_issuance::migrate_init_total_issuance::<Test>();

        // Run the migration
        let w = crate::migrations::migrate_reset_unactive_sn::migrate_reset_unactive_sn::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Store the values after running the migration
        let mut subnet_tao_before = BTreeMap::new();
        for netuid in &netuids {
            subnet_tao_before.insert(netuid, SubnetTAO::<Test>::get(netuid));
        }
        let total_stake_before = TotalStake::<Test>::get();
        let total_issuance_before = TotalIssuance::<Test>::get();

        // Run total issuance migration again, to make sure no changes happen from it.
        crate::migrations::migrate_init_total_issuance::migrate_init_total_issuance::<Test>();

        // Verify that none of the values are different
        for netuid in &netuids {
            assert_eq!(
                SubnetTAO::<Test>::get(netuid),
                *subnet_tao_before.get(netuid).unwrap_or(&TaoBalance::ZERO)
            );
        }
        assert_eq!(TotalStake::<Test>::get(), total_stake_before);
        assert_eq!(TotalIssuance::<Test>::get(), total_issuance_before);
    });
}
