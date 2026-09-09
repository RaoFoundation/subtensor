#![allow(clippy::unwrap_used, deprecated)]

use super::mock::*;
use crate::*;
use frame_support::{
    assert_noop, assert_ok,
    dispatch::{GetDispatchInfo, Pays},
};
use sp_core::U256;
use sp_runtime::{DispatchError, PerU16};
use substrate_fixed::types::{I96F32, U64F64};

fn associate() -> (U256, U256) {
    let coldkey = U256::from(10);
    let hotkey = U256::from(11);
    assert_ok!(SubtensorModule::try_associate_hotkey(
        RuntimeOrigin::signed(coldkey),
        hotkey
    ));
    (coldkey, hotkey)
}

fn disassociate(coldkey: U256, hotkey: U256) -> DispatchResult {
    SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 34)
}

#[test]
fn releases_ownership_and_allows_reassociation() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        Delegates::<Test>::insert(hotkey, PerU16::from_percent(10));
        AutoParentDelegationEnabled::<Test>::insert(hotkey, false);
        LastTxBlockDelegateTake::<Test>::insert(hotkey, 123);
        HotkeySuccessor::<Test>::insert(NetUid::from(1), hotkey, U256::from(99));
        HotkeyRoot::<Test>::insert(NetUid::from(1), hotkey, U256::from(9));
        assert_ok!(disassociate(coldkey, hotkey));
        assert!(!Owner::<Test>::contains_key(hotkey));
        assert!(!OwnedHotkeys::<Test>::contains_key(coldkey));
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey));
        assert!(!Delegates::<Test>::contains_key(hotkey));
        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(hotkey));
        assert_eq!(LastTxBlockDelegateTake::<Test>::get(hotkey), 123);
        assert_eq!(
            HotkeyRoot::<Test>::get(NetUid::from(1), hotkey),
            Some(U256::from(9))
        );
        assert_eq!(
            HotkeySuccessor::<Test>::get(NetUid::from(1), hotkey),
            Some(U256::from(99))
        );
        System::assert_last_event(Event::<Test>::HotkeyDisassociated { coldkey, hotkey }.into());
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::HotKeyAccountNotExists
        );
        let new_owner = U256::from(12);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(new_owner),
            hotkey
        ));
        assert_eq!(Owner::<Test>::get(hotkey), new_owner);
        assert_eq!(OwnedHotkeys::<Test>::get(new_owner), vec![hotkey]);
        assert_eq!(StakingHotkeys::<Test>::get(new_owner), vec![hotkey]);
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn requires_signed_existing_owner_even_for_default_account() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        for origin in [RuntimeOrigin::none(), RuntimeOrigin::root()] {
            assert_noop!(
                SubtensorModule::disassociate_hotkey(origin, hotkey, 34),
                DispatchError::BadOrigin
            );
        }
        assert_noop!(
            disassociate(hotkey, hotkey),
            Error::<Test>::NonAssociatedColdKey
        );
        assert_noop!(
            disassociate(U256::from(12), hotkey),
            Error::<Test>::NonAssociatedColdKey
        );
        assert_noop!(
            disassociate(U256::zero(), U256::from(99)),
            Error::<Test>::HotKeyAccountNotExists
        );
        assert_eq!(Owner::<Test>::get(hotkey), coldkey);
    });
}

#[test]
fn rejects_membership_on_root_and_other_subnets() {
    for netuid in [NetUid::ROOT, NetUid::from(1), NetUid::from(4095)] {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            IsNetworkMember::<Test>::insert(hotkey, netuid, true);
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::HotkeyIsStillRegistered
            );
        });
    }
}

#[test]
fn also_checks_uid_index() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        Uids::<Test>::insert(NetUid::from(4095), hotkey, 1);
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::HotkeyIsStillRegistered
        );
    });
}

#[test]
fn rejects_legacy_and_current_stake_including_nominators_and_zero_rows() {
    for legacy in [true, false] {
        for staker in [U256::from(10), U256::from(20)] {
            for shares in [0u64, 10] {
                new_test_ext(1).execute_with(|| {
                    let (coldkey, hotkey) = associate();
                    let key = (hotkey, staker, NetUid::from(4095));
                    if legacy {
                        Alpha::<Test>::insert(key, U64F64::from_num(shares));
                    } else {
                        AlphaV2::<Test>::insert(key, share_pool::SafeFloat::from(shares));
                    }
                    assert_noop!(
                        disassociate(coldkey, hotkey),
                        Error::<Test>::HotkeyHasOutstandingStake
                    );
                });
            }
        }
    }
}

#[test]
fn rejects_outstanding_rewards_without_root_stake() {
    for state in [0, 2, 3, 4, 5] {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            match state {
                0 => BasketShares::<Test>::insert(hotkey, 1),
                2 => BasketClaimed::<Test>::insert(hotkey, U256::from(20), -1),
                3 => PendingBasketDeposits::<Test>::insert(
                    hotkey,
                    NetUid::from(1),
                    AlphaBalance::from(1),
                ),
                4 => RootClaimable::<Test>::insert(
                    hotkey,
                    std::collections::BTreeMap::from([(NetUid::from(1), I96F32::from_num(1))]),
                ),
                _ => RootClaimed::<Test>::insert((NetUid::from(4095), hotkey, U256::from(20)), 1),
            }
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::HotkeyHasOutstandingRewards
            );
        });
    }
}

#[test]
fn rejects_subnet_ownership_and_child_relationships() {
    for state in 0..4 {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            let netuid = NetUid::from(4095);
            let related = vec![(u64::MAX, U256::from(20))];
            match state {
                0 => SubnetOwnerHotkey::<Test>::insert(netuid, hotkey),
                1 => ChildKeys::<Test>::insert(hotkey, netuid, related),
                2 => ParentKeys::<Test>::insert(hotkey, netuid, related),
                _ => PendingChildKeys::<Test>::insert(netuid, hotkey, (related, 100)),
            }
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::HotkeyHasActiveRelationships
            );
        });
    }
}

#[test]
fn rejects_residual_locks_on_dissolved_subnets() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        LockingColdkeys::<Test>::insert((NetUid::from(4095), hotkey, U256::from(20)), ());
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::HotkeyHasOutstandingStake
        );
    });
}

#[test]
fn validates_both_vector_lengths_and_preserves_other_relationships() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let other = U256::from(30);
        let staked = U256::from(31);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey),
            other
        ));
        StakingHotkeys::<Test>::append(coldkey, staked);
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 4),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            5
        ));
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![other]);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![other, staked]);
        assert_eq!(Owner::<Test>::get(other), coldkey);
    });
}

#[test]
fn cleans_autostake_for_all_coldkeys_but_preserves_retargeted_entries() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let staker = U256::from(20);
        let retargeted = U256::from(21);
        let other_hotkey = U256::from(30);
        for netuid in [NetUid::from(1), NetUid::from(4095)] {
            AutoStakeDestination::<Test>::insert(coldkey, netuid, hotkey);
            AutoStakeDestination::<Test>::insert(staker, netuid, hotkey);
            AutoStakeDestination::<Test>::insert(retargeted, netuid, other_hotkey);
            AutoStakeDestinationColdkeys::<Test>::insert(
                hotkey,
                netuid,
                vec![coldkey, staker, retargeted],
            );
        }
        AutoStakeDestinationColdkeys::<Test>::insert(hotkey, NetUid::from(2), Vec::<U256>::new());
        // Underestimation discovered on the last subnet must not partially clean the first.
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 10),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            11
        ));
        for netuid in [NetUid::from(1), NetUid::from(4095)] {
            assert!(!AutoStakeDestination::<Test>::contains_key(coldkey, netuid));
            assert!(!AutoStakeDestination::<Test>::contains_key(staker, netuid));
            assert_eq!(
                AutoStakeDestination::<Test>::get(retargeted, netuid),
                Some(other_hotkey)
            );
        }
        assert!(
            AutoStakeDestinationColdkeys::<Test>::iter_prefix(hotkey)
                .next()
                .is_none()
        );
    });
}

#[test]
fn charge_scales_with_work_limit_and_pays_fees() {
    let info = |max_items| {
        RuntimeCall::SubtensorModule(Call::disassociate_hotkey {
            hotkey: U256::from(11),
            max_items,
        })
        .get_dispatch_info()
    };
    let base = info(2);
    assert_eq!(base.pays_fee, Pays::Yes);
    assert!(base.call_weight.ref_time() > 0);
    assert!(base.call_weight.proof_size() > 0);
    assert!(info(100).call_weight.all_gt(base.call_weight));
}

#[test]
fn work_limit_counts_subnet_buckets_not_unrelated_accounts() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        // Little-endian Identity keys are not ordered by numeric netuid.
        for raw in [0, 1, 255, 256, 4096, u16::MAX] {
            let netuid = NetUid::from(raw);
            for id in 100..200 {
                let other = U256::from(id);
                Uids::<Test>::insert(netuid, other, 0);
                LockingColdkeys::<Test>::insert((netuid, other, other), ());
            }
        }
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 13),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            14
        ));
        assert_eq!(Uids::<Test>::iter().count(), 600);
        assert_eq!(LockingColdkeys::<Test>::iter().count(), 600);
    });
}

#[test]
fn checks_full_u16_namespace_and_residual_collateral() {
    for raw in [256, 4096, u16::MAX] {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            let key = (NetUid::from(raw), hotkey, coldkey);
            MinerCollateral::<Test>::insert(
                key,
                MinerCollateralState {
                    locked: 1.into(),
                    drain_ratio: U64F64::from_num(1),
                    min_locked: 0.into(),
                    earned: 0.into(),
                },
            );
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::HotkeyHasOutstandingStake
            );
            MinerCollateral::<Test>::remove(key);
            PendingChildKeys::<Test>::insert(
                NetUid::from(raw),
                hotkey,
                (vec![(u64::MAX, U256::from(20))], 100),
            );
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::HotkeyHasActiveRelationships
            );
        });
    }
}

#[test]
fn refuses_disassociation_during_basket_seed() {
    use crate::migrations::migrate_seed_beta_basket::{
        SeedBetaBasketV2Migration, SeedBetaBasketV2Progress,
    };
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        SeedBetaBasketV2Migration::<Test>::put(SeedBetaBasketV2Progress::ClearClaimed {
            cursor: None,
        });
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::BetaBasketSeedInProgress
        );
    });
}

#[test]
fn swap_guard_blocks_disassociation_of_a_frozen_coldkey() {
    use frame_support::traits::ExtendedDispatchable;
    use sp_runtime::traits::Hash;
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        ColdkeySwapAnnouncements::<Test>::insert(
            coldkey,
            (
                System::block_number(),
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(99)),
            ),
        );
        let call = RuntimeCall::SubtensorModule(Call::disassociate_hotkey {
            hotkey,
            max_items: 2,
        });
        let err =
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(coldkey),
                call,
            )
            .unwrap_err();
        assert_eq!(err.error, Error::<Test>::ColdkeySwapAnnounced.into());
        assert_eq!(Owner::<Test>::get(hotkey), coldkey);
    });
}

#[test]
fn fully_settled_basket_must_not_prevent_release() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let staker = U256::from(20);
        add_network(NetUid::ROOT, 100, 0);
        AutoParentDelegationEnabled::<Test>::insert(hotkey, false);
        crate::tests::claim_root::zero_claim_threshold();
        add_balance_to_coldkey_account(&coldkey, 100_000_000_000u64.into());
        add_balance_to_coldkey_account(&staker, 100_000_000u64.into());
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let uid = Uids::<Test>::get(NetUid::ROOT, hotkey).unwrap();
        assert_ok!(SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(staker),
            hotkey,
            10_000_000u64.into()
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(staker),
            hotkey
        ));
        let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &staker,
            NetUid::ROOT,
        );
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(staker),
            hotkey,
            NetUid::ROOT,
            stake
        ));
        let replacement = U256::from(30);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey),
            replacement
        ));
        SubtensorModule::replace_neuron(NetUid::ROOT, uid, &replacement, 2);
        assert!(
            IsNetworkMember::<Test>::iter_key_prefix(hotkey)
                .next()
                .is_none()
        );
        assert!(AlphaV2::<Test>::iter_key_prefix((hotkey,)).next().is_none());
        assert_eq!(BasketShares::<Test>::get(hotkey), 0);
        assert_eq!(BasketClaimed::<Test>::get(hotkey, staker), 0);
        assert!(BasketClaimed::<Test>::contains_key(hotkey, staker));
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &staker), 0);
        assert_ok!(disassociate(coldkey, hotkey));
    });
}

#[test]
fn settled_claim_rows_are_bounded_and_cleared_before_reassociation() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(10));
        for id in 20..23 {
            BasketClaimed::<Test>::insert(hotkey, U256::from(id), 0);
        }
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 4),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            5
        ));
        assert!(!BasketRate::<Test>::contains_key(hotkey));
        assert!(
            BasketClaimed::<Test>::iter_key_prefix(hotkey)
                .next()
                .is_none()
        );
        let new_owner = U256::from(30);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(new_owner),
            hotkey
        ));
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &new_owner),
            0
        );
    });
}

#[test]
fn rejects_underestimated_large_owner_indexes_without_cleanup() {
    for owned in [true, false] {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            let mut keys = vec![hotkey];
            keys.extend((100..10_100).map(U256::from));
            let key = if owned {
                OwnedHotkeys::<Test>::hashed_key_for(coldkey)
            } else {
                StakingHotkeys::<Test>::hashed_key_for(coldkey)
            };
            let encoded = keys.encode();
            sp_io::storage::set(&key, &encoded);
            assert_noop!(
                SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 2),
                Error::<Test>::InvalidDisassociationWitness
            );
            assert_eq!(
                sp_io::storage::get(&key).unwrap().as_ref(),
                encoded.as_slice()
            );
            assert_eq!(Owner::<Test>::get(hotkey), coldkey);
        });
    }
}

#[test]
fn rejects_underestimated_large_autostake_index_without_cleanup() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let netuid = NetUid::from(u16::MAX);
        let stakers: Vec<_> = (100..10_100).map(U256::from).collect();
        AutoStakeDestination::<Test>::insert(stakers[0], netuid, hotkey);
        AutoStakeDestinationColdkeys::<Test>::insert(hotkey, netuid, &stakers);
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 3),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_eq!(Owner::<Test>::get(hotkey), coldkey);
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![hotkey]);
        assert_eq!(
            AutoStakeDestination::<Test>::get(stakers[0], netuid),
            Some(hotkey)
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hotkey, netuid),
            stakers
        );
    });
}

#[test]
fn rejects_malformed_index_lengths_without_releasing_ownership() {
    for value in [vec![], vec![1], vec![2, 0], vec![3, 0, 0, 0]] {
        new_test_ext(1).execute_with(|| {
            let (coldkey, hotkey) = associate();
            let key = OwnedHotkeys::<Test>::hashed_key_for(coldkey);
            sp_io::storage::set(&key, &value);
            assert_noop!(
                disassociate(coldkey, hotkey),
                Error::<Test>::InvalidDisassociationWitness
            );
            assert_eq!(Owner::<Test>::get(hotkey), coldkey);
            assert_eq!(
                sp_io::storage::get(&key).unwrap().as_ref(),
                value.as_slice()
            );
        });
    }
}

#[test]
fn rejects_stale_work_estimates_after_index_growth() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let other = U256::from(30);
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey),
            other
        ));
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 2),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            4
        ));
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![other]);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![other]);
        assert_eq!(Owner::<Test>::get(other), coldkey);
    });
}

#[test]
fn releases_a_hotkey_whose_staking_index_was_already_cleaned() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        SubtensorModule::maybe_remove_staking_hotkey(&hotkey, &coldkey);
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey));
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            1
        ));
        assert!(!Owner::<Test>::contains_key(hotkey));
        assert!(!OwnedHotkeys::<Test>::contains_key(coldkey));
    });
}
