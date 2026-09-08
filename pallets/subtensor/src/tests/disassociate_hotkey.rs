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
    SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 34, None)
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
                SubtensorModule::disassociate_hotkey(origin, hotkey, 34, None),
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
        SubtensorModule::note_hotkey_index_length(
            &StakingHotkeys::<Test>::hashed_key_for(coldkey),
            3,
        );
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 4, None),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            5,
            None
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
            SubtensorModule::note_hotkey_index_length(
                &AutoStakeDestinationColdkeys::<Test>::hashed_key_for(hotkey, netuid),
                3,
            );
        }
        AutoStakeDestinationColdkeys::<Test>::insert(hotkey, NetUid::from(2), Vec::<U256>::new());
        SubtensorModule::note_hotkey_index_length(
            &AutoStakeDestinationColdkeys::<Test>::hashed_key_for(hotkey, NetUid::from(2)),
            0,
        );
        // Underestimation discovered on the last subnet must not partially clean the first.
        assert_noop!(
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 10, None),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            11,
            None
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
            legacy_proof: None,
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
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 13, None),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            14,
            None
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
            legacy_proof: None,
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
fn review_rejected_zero_witness_must_cover_owner_vector_proof() {
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(|| {
        let pair = associate();
        let mut keys = vec![pair.1];
        for id in 100..10_100 {
            let other = U256::from(id);
            Owner::<Test>::insert(other, pair.0);
            keys.push(other);
        }
        SubtensorModule::note_hotkey_index_length(
            &OwnedHotkeys::<Test>::hashed_key_for(pair.0),
            keys.len(),
        );
        SubtensorModule::note_hotkey_index_length(
            &StakingHotkeys::<Test>::hashed_key_for(pair.0),
            keys.len(),
        );
        OwnedHotkeys::<Test>::insert(pair.0, &keys);
        StakingHotkeys::<Test>::insert(pair.0, &keys);
        pair
    });
    ext.commit_all().unwrap();
    let call = RuntimeCall::SubtensorModule(Call::disassociate_hotkey {
        hotkey,
        max_items: 0,
        legacy_proof: None,
    });
    let charged = call.get_dispatch_info().call_weight.proof_size();
    let (result, proof) = ext.execute_and_prove(|| {
        SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 0, None)
    });
    assert_eq!(
        result,
        Err(Error::<Test>::InvalidDisassociationWitness.into())
    );
    ext.execute_with(|| assert_eq!(Owner::<Test>::get(hotkey), coldkey));
    let actual: usize = proof.iter_nodes().map(Vec::len).sum();
    assert!(
        actual as u64 <= charged,
        "owner vectors: recorded proof {actual} bytes exceeds declared {charged} bytes"
    );
}

#[test]
fn review_rejected_autostake_witness_must_cover_vector_proof() {
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(|| {
        let pair = associate();
        let netuid = NetUid::from(65535);
        let keys: Vec<_> = (100..10_100).map(U256::from).collect();
        for staker in &keys {
            AutoStakeDestination::<Test>::insert(staker, netuid, pair.1);
        }
        SubtensorModule::note_hotkey_index_length(
            &AutoStakeDestinationColdkeys::<Test>::hashed_key_for(pair.1, netuid),
            keys.len(),
        );
        AutoStakeDestinationColdkeys::<Test>::insert(pair.1, netuid, keys);
        pair
    });
    ext.commit_all().unwrap();
    let call = RuntimeCall::SubtensorModule(Call::disassociate_hotkey {
        hotkey,
        max_items: 3,
        legacy_proof: None,
    });
    let charged = call.get_dispatch_info().call_weight.proof_size();
    let (result, proof) = ext.execute_and_prove(|| {
        SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 3, None)
    });
    assert_eq!(
        result,
        Err(Error::<Test>::InvalidDisassociationWitness.into())
    );
    ext.execute_with(|| assert_eq!(Owner::<Test>::get(hotkey), coldkey));
    let actual: usize = proof.iter_nodes().map(Vec::len).sum();
    assert!(
        actual as u64 <= charged,
        "autostake vector: recorded proof {actual} bytes exceeds declared {charged} bytes"
    );
}

#[test]
fn review_fully_settled_basket_must_not_prevent_release() {
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

fn historical_index_proof(
    ext: &mut sp_io::TestExternalities,
    keys: &[Vec<u8>],
) -> DisassociationProof<Test> {
    use sp_runtime::traits::Header;
    ext.execute_with(|| {
        System::set_block_number(0);
        SubtensorModule::start_hotkey_index_tracking();
        System::set_block_number(1);
        for key in keys {
            HotkeyIndexLengths::<Test>::remove(sp_io::hashing::blake2_256(key));
        }
    });
    ext.commit_all().unwrap();
    let root = *ext.backend.root();
    let (_, proof) = ext.execute_and_prove(|| {
        for key in keys {
            assert!(sp_io::storage::get(key).is_some());
        }
    });
    let header = frame_system::pallet_prelude::HeaderFor::<Test>::new(
        1,
        Default::default(),
        root,
        Default::default(),
        Default::default(),
    );
    ext.execute_with(|| {
        System::set_block_number(2);
        frame_system::BlockHash::<Test>::insert(1, header.hash());
    });
    (header, proof.into_iter_nodes().collect())
}

#[test]
fn legacy_indexes_require_authenticated_post_activation_proof() {
    use sp_runtime::traits::Header;
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(associate);
    let keys = [
        OwnedHotkeys::<Test>::hashed_key_for(coldkey),
        StakingHotkeys::<Test>::hashed_key_for(coldkey),
    ];
    let proof = historical_index_proof(&mut ext, &keys);
    ext.execute_with(|| {
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::InvalidDisassociationWitness
        );
        let release = |proof| {
            SubtensorModule::disassociate_hotkey(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                2,
                Some(proof),
            )
        };
        let mut tampered = proof.clone();
        tampered.0.set_state_root(Default::default());
        assert_noop!(
            release(tampered),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_noop!(
            release((proof.0.clone(), vec![])),
            Error::<Test>::InvalidDisassociationWitness
        );
        HotkeyIndexTrackingSince::<Test>::put(2);
        assert_noop!(
            release(proof.clone()),
            Error::<Test>::InvalidDisassociationWitness
        );
        HotkeyIndexTrackingSince::<Test>::put(1);
        frame_system::BlockHash::<Test>::remove(1);
        assert_noop!(
            release(proof.clone()),
            Error::<Test>::InvalidDisassociationWitness
        );
        frame_system::BlockHash::<Test>::insert(1, proof.0.hash());
        assert_ok!(release(proof));
        assert!(!Owner::<Test>::contains_key(hotkey));
    });
}

#[test]
fn historical_proof_cannot_underestimate_indexes_that_have_grown() {
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(associate);
    let proof = historical_index_proof(
        &mut ext,
        &[
            OwnedHotkeys::<Test>::hashed_key_for(coldkey),
            StakingHotkeys::<Test>::hashed_key_for(coldkey),
        ],
    );
    ext.execute_with(|| {
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey),
            U256::from(12)
        ));
        assert_noop!(
            SubtensorModule::disassociate_hotkey(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                2,
                Some(proof.clone())
            ),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            4,
            Some(proof)
        ));
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![U256::from(12)]);
    });
}

#[test]
fn missing_length_metadata_rejects_without_reading_large_legacy_value() {
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(|| {
        let pair = associate();
        let key = OwnedHotkeys::<Test>::hashed_key_for(pair.0);
        OwnedHotkeys::<Test>::insert(pair.0, vec![pair.1; 10_000]);
        HotkeyIndexLengths::<Test>::remove(sp_io::hashing::blake2_256(&key));
        pair
    });
    ext.commit_all().unwrap();
    let (_, proof) = ext.execute_and_prove(|| {
        assert_noop!(
            disassociate(coldkey, hotkey),
            Error::<Test>::InvalidDisassociationWitness
        );
    });
    // Reading the legacy value would record over 320 kB, independently of max_items.
    assert!(proof.iter_nodes().map(Vec::len).sum::<usize>() < 20_000);
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
            SubtensorModule::disassociate_hotkey(RuntimeOrigin::signed(coldkey), hotkey, 4, None),
            Error::<Test>::InvalidDisassociationWitness
        );
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            5,
            None
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
fn tracking_activation_is_idempotent_and_does_not_scan_legacy_indexes() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, _) = associate();
        HotkeyIndexLengths::<Test>::remove(sp_io::hashing::blake2_256(
            &OwnedHotkeys::<Test>::hashed_key_for(coldkey),
        ));
        System::set_block_number(100);
        SubtensorModule::start_hotkey_index_tracking();
        System::set_block_number(200);
        SubtensorModule::start_hotkey_index_tracking();
        assert_eq!(HotkeyIndexTrackingSince::<Test>::get(), Some(101));
        assert!(
            HotkeyIndexLengths::<Test>::get(sp_io::hashing::blake2_256(
                &OwnedHotkeys::<Test>::hashed_key_for(coldkey)
            ))
            .is_none()
        );
    });
}

#[test]
fn autostake_growth_and_retargeting_preserve_length_bounds() {
    new_test_ext(1).execute_with(|| {
        let (coldkey, hotkey) = associate();
        let other = U256::from(30);
        let netuid = NetUid::from(1);
        add_network(netuid, 100, 0);
        Uids::<Test>::insert(netuid, hotkey, 0);
        Uids::<Test>::insert(netuid, other, 1);
        let key = AutoStakeDestinationColdkeys::<Test>::hashed_key_for(hotkey, netuid);
        // Legacy inverse entry: a real growth must install metadata for its full length.
        AutoStakeDestinationColdkeys::<Test>::insert(hotkey, netuid, vec![U256::from(20)]);
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey
        ));
        assert_eq!(
            HotkeyIndexLengths::<Test>::get(sp_io::hashing::blake2_256(&key)),
            Some(2)
        );
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            other
        ));
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hotkey, netuid).len(),
            1
        );
        assert_ok!(SubtensorModule::check_hotkey_index_lengths());
        HotkeyIndexLengths::<Test>::insert(sp_io::hashing::blake2_256(&key), 0);
        assert!(SubtensorModule::check_hotkey_index_lengths().is_err());
    });
}

#[test]
fn historical_length_remains_safe_after_untracked_cleanup() {
    let mut ext = new_test_ext(1);
    let (coldkey, hotkey) = ext.execute_with(associate);
    let proof = historical_index_proof(
        &mut ext,
        &[
            OwnedHotkeys::<Test>::hashed_key_for(coldkey),
            StakingHotkeys::<Test>::hashed_key_for(coldkey),
        ],
    );
    ext.execute_with(|| {
        SubtensorModule::maybe_remove_staking_hotkey(&hotkey, &coldkey);
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey));
        assert_ok!(SubtensorModule::disassociate_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            2,
            Some(proof)
        ));
    });
}

#[test]
fn fresh_genesis_enables_index_proofs_without_a_runtime_upgrade() {
    use frame_support::traits::BuildGenesisConfig;
    new_test_ext(0).execute_with(|| {
        crate::GenesisConfig::<Test>::default().build();
        assert_eq!(HotkeyIndexTrackingSince::<Test>::get(), Some(0));
        assert_ok!(SubtensorModule::check_hotkey_index_lengths());
    });
}
