#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects
)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo};
use frame_support::error::BadOrigin;
use frame_support::traits::OnInitialize;
use frame_support::traits::schedule::DispatchTime;
use frame_support::traits::schedule::v3::Named as ScheduleNamed;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use share_pool::SafeFloat;
use sp_core::{Get, H256, U256};
use sp_runtime::PerU16;
use sp_runtime::traits::Hash;
use sp_runtime::traits::{DispatchInfoOf, DispatchTransaction, TransactionExtension};
use sp_runtime::{DispatchError, traits::TxBaseImplication};
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{
    AlphaBalance, CustomTransactionError, GammaBalance, NetUid, SubnetInfo, TaoBalance, Token,
};
use subtensor_swap_interface::{SwapEngine, SwapHandler};

use super::mock;
use super::mock::*;
use crate::extensions::SubtensorTransactionExtension;
use crate::staking::lock::LockState;
use crate::swap::swap_coldkey::ColdkeySwapPolicyImportMode;
use crate::weights::WeightInfo;
use crate::*;
use crate::{Call, Error};

fn run_to_block(n: u64) {
    System::run_to_block::<AllPalletsWithSystem>(n);
}

#[test]
fn test_announce_coldkey_swap_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let ed = ExistentialDeposit::get();

        assert_eq!(ColdkeySwapAnnouncements::<Test>::iter().count(), 0);

        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&who, swap_cost + ed);
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_hash,
        ));

        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        let now = System::block_number();
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::iter().collect::<Vec<_>>(),
            vec![(who, (now + delay, new_coldkey_hash))]
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), ed);
        assert_eq!(
            last_event(),
            RuntimeEvent::SubtensorModule(Event::ColdkeySwapAnnounced {
                who,
                new_coldkey_hash,
            })
        );
    });
}

#[test]
fn test_announce_coldkey_swap_with_existing_announcement_past_delay_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let new_coldkey_2 = U256::from(3);
        let new_coldkey_2_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey_2);

        assert_eq!(ColdkeySwapAnnouncements::<Test>::iter().count(), 0);

        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&who, swap_cost * 2.into());

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_hash,
        ));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::iter().collect::<Vec<_>>(),
            vec![(who, (now + delay, new_coldkey_hash))]
        );

        let reannouncement_delay = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(now + delay + reannouncement_delay);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_2_hash
        ));

        let now = System::block_number();
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::iter().collect::<Vec<_>>(),
            vec![(who, (now + delay, new_coldkey_2_hash))]
        );
    });
}

#[test]
fn test_announce_coldkey_swap_only_pays_swap_cost_if_no_announcement_exists() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let new_coldkey_2 = U256::from(3);
        let new_coldkey_2_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey_2);
        let ed = ExistentialDeposit::get();

        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&who, swap_cost + ed);
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_hash,
        ));
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), ed);

        let now = System::block_number();
        let base_delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        let reannouncement_delay = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(now + base_delay + reannouncement_delay);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_2_hash
        ));
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), ed);
    });
}

#[test]
fn test_announce_coldkey_swap_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let new_coldkey = U256::from(1);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);

        assert_noop!(
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::none(), new_coldkey_hash),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::root(), new_coldkey_hash),
            BadOrigin
        );
    });
}

#[test]
fn test_announce_coldkey_swap_with_existing_announcement_not_past_delay_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let new_coldkey_2 = U256::from(3);
        let new_coldkey_2_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey_2);

        assert_eq!(ColdkeySwapAnnouncements::<Test>::iter().count(), 0);

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_hash,
        ));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::iter().collect::<Vec<_>>(),
            vec![(who, (now + delay, new_coldkey_hash))]
        );

        assert_noop!(
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::signed(who), new_coldkey_2_hash),
            Error::<Test>::ColdkeySwapReannouncedTooEarly
        );
    });
}

#[test]
fn test_swap_coldkey_announced_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey1 = U256::from(1001);
        let hotkey2 = U256::from(1002);
        let hotkey3 = U256::from(1003);
        let ed = ExistentialDeposit::get();
        let min_stake = DefaultMinStake::<Test>::get();
        let stake1 = min_stake * 10.into();
        let stake2 = min_stake * 20.into();
        let stake3 = min_stake * 30.into();
        let now = System::block_number();

        ColdkeySwapAnnouncements::<Test>::insert(who, (now, new_coldkey_hash));

        // Run some blocks for the announcement to be past the delay
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get() + 1;
        run_to_block(now + delay);

        add_balance_to_coldkey_account(&who, stake1 + stake2 + stake3 + ed);

        let expected_remaining: u64 = ed.to_u64();

        let (
            netuid1,
            netuid2,
            hotkeys,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            identity,
            balance_before,
            total_stake_before,
        ) = comprehensive_setup!(
            who,
            new_coldkey,
            new_coldkey_hash,
            stake1,
            stake2,
            stake3,
            hotkey1,
            hotkey2,
            hotkey3,
            expected_remaining
        );

        assert_ok!(SubtensorModule::swap_coldkey_announced(
            <Test as frame_system::Config>::RuntimeOrigin::signed(who),
            new_coldkey
        ));

        comprehensive_checks!(
            who,
            hotkey1,
            hotkey2,
            hotkey3,
            hotkeys,
            new_coldkey,
            balance_before,
            identity,
            netuid1,
            netuid2,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            total_stake_before,
            0.into() // Charged on announcement
        );
    });
}

// A coldkey swap moves one position per `(hotkey, netuid)` row and is priced for a bounded
// number of them: an oversized list is refused before any write, the declared weight covers
// the bound, and the actual weight is refunded to the positions really moved.
#[test]
fn test_swap_coldkey_bounded_and_priced_per_position() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let hotkey = U256::from(1001);
        let netuid = add_dynamic_network(&hotkey, &who);
        let min_stake = DefaultMinStake::<Test>::get();
        add_balance_to_coldkey_account(&who, min_stake * 100.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(who),
            hotkey,
            netuid,
            min_stake * 10.into()
        ));

        // Too many staking hotkeys: refused, nothing moved.
        let mut oversized = StakingHotkeys::<Test>::get(who);
        let mut next = 50_000_u64;
        while oversized.len() <= MAX_COLDKEY_SWAP_HOTKEYS as usize {
            oversized.push(U256::from(next));
            next += 1;
        }
        let real_list = StakingHotkeys::<Test>::get(who);
        StakingHotkeys::<Test>::insert(who, oversized);
        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey(
                RuntimeOrigin::root(),
                who,
                new_coldkey,
                TaoBalance::ZERO
            ),
            Error::<Test>::ColdkeySwapTooHeavy
        );
        StakingHotkeys::<Test>::insert(who, real_list);

        // Declared weight covers the admitted bound.
        let call = RuntimeCall::SubtensorModule(crate::Call::swap_coldkey {
            old_coldkey: who,
            new_coldkey,
            swap_cost: TaoBalance::ZERO,
        });
        let declared = call.get_dispatch_info().call_weight;
        let per_position = <Test as crate::Config>::WeightInfo::transfer_stake();
        assert!(
            declared.all_gte(per_position.saturating_mul(u64::from(MAX_COLDKEY_SWAP_POSITIONS)))
        );

        let alpha_before =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &who, netuid);
        let post_info = SubtensorModule::swap_coldkey(
            RuntimeOrigin::root(),
            who,
            new_coldkey,
            TaoBalance::ZERO,
        )
        .expect("swap succeeds");
        let actual = post_info.actual_weight.expect("actual weight reported");
        assert!(declared.all_gte(actual));
        // One hotkey, one position: priced as exactly that, far below the envelope.
        assert_eq!(
            actual,
            SubtensorModule::swap_coldkey_actual_weight(
                crate::swap::swap_coldkey::ColdkeySwapWork {
                    hotkeys: 1,
                    positions: 1,
                    pending_registrations: 0,
                    dest_locks: 0,
                    registration_queue_scan: 0,
                    gamma_deposits: 0,
                }
            )
        );
        assert!(actual.all_lt(declared));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &new_coldkey,
                netuid
            ),
            alpha_before
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &who, netuid)
                .is_zero()
        );
    });
}

#[test]
fn test_swap_coldkey_announced_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::none(), new_coldkey),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::root(), new_coldkey),
            BadOrigin
        );
    });
}

#[test]
fn test_swap_coldkey_announced_without_announcement_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(who), new_coldkey),
            Error::<Test>::ColdkeySwapAnnouncementNotFound
        );
    })
}

#[test]
fn test_swap_coldkey_announced_with_mismatched_coldkey_hash_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let other_coldkey = U256::from(3);
        let now = System::block_number();

        ColdkeySwapAnnouncements::<Test>::insert(who, (now, new_coldkey_hash));

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(who), other_coldkey),
            Error::<Test>::AnnouncedColdkeyHashDoesNotMatch
        );
        // Failed prechecks must not consume the announcement.
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
    })
}

#[test]
fn test_swap_coldkey_announced_too_early_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);

        // Now case
        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        ColdkeySwapAnnouncements::<Test>::insert(who, (now + delay, new_coldkey_hash));

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                new_coldkey
            ),
            Error::<Test>::ColdkeySwapTooEarly
        );
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));

        // Now + delay - 1 case
        run_to_block(now + delay - 1);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                new_coldkey
            ),
            Error::<Test>::ColdkeySwapTooEarly
        );
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
    })
}

#[test]
fn test_swap_coldkey_announced_into_already_associated_coldkey_merges() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let dest_hotkey = U256::from(3);
        let source_hotkey = U256::from(4);

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        // Destination is already in use (owns a hotkey / has StakingHotkeys).
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &new_coldkey,
            &dest_hotkey
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &who,
            &source_hotkey
        ));

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            new_coldkey,
        ));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get() + 1;
        run_to_block(now + delay);

        assert_ok!(SubtensorModule::swap_coldkey_announced(
            <Test as frame_system::Config>::RuntimeOrigin::signed(who),
            new_coldkey
        ));

        let staking = StakingHotkeys::<Test>::get(new_coldkey);
        assert!(staking.contains(&dest_hotkey));
        assert!(staking.contains(&source_hotkey));
        assert!(StakingHotkeys::<Test>::get(who).is_empty());
        assert_eq!(Owner::<Test>::get(source_hotkey), new_coldkey);
        assert_eq!(Owner::<Test>::get(dest_hotkey), new_coldkey);
    })
}

#[test]
fn test_swap_coldkey_announced_with_hotkey_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let hotkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&hotkey);
        let now = System::block_number();

        ColdkeySwapAnnouncements::<Test>::insert(who, (now, hotkey_hash));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get() + 1;
        run_to_block(now + delay);

        SubtensorModule::create_account_if_non_existent(&new_coldkey, &hotkey);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                hotkey
            ),
            Error::<Test>::NewColdKeyIsHotkey
        );
        // Destination refusal after announce must leave the announcement in place.
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
    })
}

#[test]
fn test_swap_coldkey_works() {
    new_test_ext(1000).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey1 = U256::from(1001);
        let hotkey2 = U256::from(1002);
        let hotkey3 = U256::from(1003);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();
        let min_stake = DefaultMinStake::<Test>::get();
        let stake1 = min_stake * 10.into();
        let stake2 = min_stake * 20.into();
        let stake3 = min_stake * 30.into();

        // Fund: stake_total + (swap_cost + ED).
        add_balance_to_coldkey_account(&old_coldkey, swap_cost + stake1 + stake2 + stake3 + ed);

        // Some old announcement and dispute that will be cleared
        let now = System::block_number() - 100;
        ColdkeySwapAnnouncements::<Test>::insert(old_coldkey, (now, new_coldkey_hash));
        ColdkeySwapDisputes::<Test>::insert(old_coldkey, now);
        let expected_remaining = swap_cost + ed;

        let (
            netuid1,
            netuid2,
            hotkeys,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            identity,
            balance_before,
            total_stake_before,
        ) = comprehensive_setup!(
            old_coldkey,
            new_coldkey,
            new_coldkey_hash,
            stake1,
            stake2,
            stake3,
            hotkey1,
            hotkey2,
            hotkey3,
            expected_remaining
        );

        assert_ok!(SubtensorModule::swap_coldkey(
            <Test as frame_system::Config>::RuntimeOrigin::root(),
            old_coldkey,
            new_coldkey,
            swap_cost,
        ));

        comprehensive_checks!(
            old_coldkey,
            hotkey1,
            hotkey2,
            hotkey3,
            hotkeys,
            new_coldkey,
            balance_before,
            identity,
            netuid1,
            netuid2,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            total_stake_before,
            swap_cost
        );

        // Check that the old announcement and dispute are cleared
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(old_coldkey));
        assert!(!ColdkeySwapDisputes::<Test>::contains_key(old_coldkey));
    });
}

#[test]
fn test_swap_coldkey_announced_transfers_locked_alpha() {
    new_test_ext(1000).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey1 = U256::from(1001);
        let hotkey2 = U256::from(1002);
        let hotkey3 = U256::from(1003);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();
        let min_stake = DefaultMinStake::<Test>::get();
        let stake1 = min_stake * 10.into();
        let stake2 = min_stake * 20.into();
        let stake3 = min_stake * 30.into();

        add_balance_to_coldkey_account(&old_coldkey, swap_cost + stake1 + stake2 + stake3 + ed);

        let (
            netuid1,
            _netuid2,
            _hotkeys,
            hk1_alpha,
            _hk2_alpha,
            _hk3_alpha,
            _total_ck_stake,
            _identity,
            _balance_before,
            _total_stake_before,
        ) = comprehensive_setup!(
            old_coldkey,
            new_coldkey,
            new_coldkey_hash,
            stake1,
            stake2,
            stake3,
            hotkey1,
            hotkey2,
            hotkey3,
            swap_cost + ed
        );

        let lock_amount = hk1_alpha / 2.into();
        assert!(!lock_amount.is_zero());
        assert_ok!(SubtensorModule::do_lock_stake(
            &old_coldkey,
            netuid1,
            &hotkey1,
            lock_amount,
        ));

        let old_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &old_coldkey,
            netuid1,
        );
        let new_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &new_coldkey,
            netuid1,
        );
        let old_lock_before =
            Lock::<Test>::get((old_coldkey, netuid1, hotkey1)).expect("lock should exist");

        ColdkeySwapAnnouncements::<Test>::insert(
            old_coldkey,
            (System::block_number(), new_coldkey_hash),
        );

        assert_ok!(SubtensorModule::swap_coldkey_announced(
            <Test as frame_system::Config>::RuntimeOrigin::signed(old_coldkey),
            new_coldkey,
        ));

        assert!(ColdkeySwapAnnouncements::<Test>::get(old_coldkey).is_none());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &old_coldkey,
                netuid1,
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &new_coldkey,
                netuid1,
            ),
            old_stake_before + new_stake_before
        );
        assert!(Lock::<Test>::get((old_coldkey, netuid1, hotkey1)).is_none());
        assert_eq!(
            Lock::<Test>::get((new_coldkey, netuid1, hotkey1)),
            Some(old_lock_before)
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::swap_coldkey::test_swap_coldkey_works_with_zero_cost --exact --nocapture
#[test]
fn test_swap_coldkey_works_with_zero_cost() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey1 = U256::from(1001);
        let hotkey2 = U256::from(1002);
        let hotkey3 = U256::from(1003);
        let ed = ExistentialDeposit::get();
        let swap_cost = TaoBalance::from(0);
        let min_stake = DefaultMinStake::<Test>::get();
        let stake1 = min_stake * 10.into();
        let stake2 = min_stake * 20.into();
        let stake3 = min_stake * 30.into();

        add_balance_to_coldkey_account(&old_coldkey, stake1 + stake2 + stake3 + ed);
        let expected_remaining = ed;

        let (
            netuid1,
            netuid2,
            hotkeys,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            identity,
            balance_before,
            total_stake_before,
        ) = comprehensive_setup!(
            old_coldkey,
            new_coldkey,
            new_coldkey_hash,
            stake1,
            stake2,
            stake3,
            hotkey1,
            hotkey2,
            hotkey3,
            expected_remaining
        );

        assert_ok!(SubtensorModule::swap_coldkey(
            <Test as frame_system::Config>::RuntimeOrigin::root(),
            old_coldkey,
            new_coldkey,
            swap_cost.into(),
        ));

        comprehensive_checks!(
            old_coldkey,
            hotkey1,
            hotkey2,
            hotkey3,
            hotkeys,
            new_coldkey,
            balance_before,
            identity,
            netuid1,
            netuid2,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            total_stake_before,
            swap_cost
        );
    });
}

#[test]
fn test_swap_coldkey_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let old_coldkey = U256::from(2);
        let new_coldkey = U256::from(3);
        let swap_cost = SubtensorModule::get_key_swap_cost();

        assert_noop!(
            SubtensorModule::swap_coldkey(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                old_coldkey,
                new_coldkey,
                swap_cost,
            ),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::swap_coldkey(
                <Test as frame_system::Config>::RuntimeOrigin::none(),
                old_coldkey,
                new_coldkey,
                swap_cost
            ),
            BadOrigin
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::swap_coldkey::test_swap_coldkey_with_not_enough_balance_to_pay_swap_cost_fails --exact --nocapture
#[test]
fn test_swap_coldkey_with_not_enough_balance_to_pay_swap_cost_fails() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let swap_cost = SubtensorModule::get_key_swap_cost();

        // No balance to pay swap cost
        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey(
                RuntimeOrigin::root(),
                old_coldkey,
                new_coldkey,
                swap_cost
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapColdKey
        );

        // Needs to preserve ED
        let balance = SubtensorModule::get_key_swap_cost() + ExistentialDeposit::get() - 1.into();
        add_balance_to_coldkey_account(&old_coldkey, balance);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey(
                RuntimeOrigin::root(),
                old_coldkey,
                new_coldkey,
                swap_cost
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapColdKey
        );
    });
}

#[test]
fn test_do_swap_coldkey_preserves_new_coldkey_identity() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);

        let old_identity = ChainIdentityV2 {
            name: b"Old identity".to_vec(),
            ..Default::default()
        };
        IdentitiesV2::<Test>::insert(who, old_identity.clone());

        let new_identity = ChainIdentityV2 {
            name: b"New identity".to_vec(),
            ..Default::default()
        };
        IdentitiesV2::<Test>::insert(new_coldkey, new_identity.clone());

        assert_ok!(SubtensorModule::do_swap_coldkey(&who, &new_coldkey,));

        // Destination identity wins; source identity is cleared (no orphan on drained key).
        assert!(IdentitiesV2::<Test>::get(who).is_none());
        assert_eq!(IdentitiesV2::<Test>::get(new_coldkey), Some(new_identity));
    });
}

#[test]
fn test_announce_coldkey_swap_with_not_enough_balance_to_pay_swap_cost_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);

        // No balance to pay swap cost
        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(who), new_coldkey),
            Error::<Test>::NotEnoughBalanceToPaySwapColdKey
        );

        // Needs to preserve ED
        let balance = SubtensorModule::get_key_swap_cost() + ExistentialDeposit::get() - 1.into();
        add_balance_to_coldkey_account(&who, balance);
        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(who), new_coldkey),
            Error::<Test>::NotEnoughBalanceToPaySwapColdKey
        );
    });
}

#[test]
fn test_do_swap_coldkey_with_no_stake() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&old_coldkey),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            TaoBalance::ZERO
        );
        assert!(!StakingHotkeys::<Test>::contains_key(new_coldkey));
    });
}

#[test]
fn test_do_swap_coldkey_with_max_values() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let old_coldkey2 = U256::from(3);
        let new_coldkey2 = U256::from(4);
        let hotkey = U256::from(5);
        let hotkey2 = U256::from(6);
        let other_coldkey = U256::from(7);
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        // Max possible balance: (21M - EDs - ...) / 2
        let ed = u64::from(ExistentialDeposit::get());
        let locks = 200_000_000_000_u64;
        let max_stake = (21_000_000_000_000_000_u64 - 2 * ed - 100) / 2;

        // Add a network
        add_network(netuid, 1, 0);
        add_network(netuid2, 1, 0);

        // Register hotkey on each subnet.
        // hotkey2 is owned by other_coldkey.
        register_ok_neuron(netuid, hotkey, old_coldkey, 1001000);
        register_ok_neuron(netuid2, hotkey2, other_coldkey, 1001000);

        // Give balance to old_coldkey and old_coldkey2.
        add_balance_to_coldkey_account(&old_coldkey, max_stake.into());
        add_balance_to_coldkey_account(&old_coldkey2, max_stake.into());

        mock::setup_reserves(netuid, max_stake.into(), max_stake.into());
        mock::setup_reserves(netuid2, max_stake.into(), max_stake.into());

        // Stake to hotkey on each subnet.
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(old_coldkey),
            hotkey,
            netuid,
            max_stake.into()
        ));
        let expected_stake1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old_coldkey,
            netuid,
        );

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(old_coldkey2),
            hotkey2,
            netuid2,
            max_stake.into()
        ));
        let expected_stake2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &old_coldkey2,
            netuid2,
        );

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey,));
        assert_ok!(SubtensorModule::do_swap_coldkey(
            &old_coldkey2,
            &new_coldkey2,
        ));

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&old_coldkey),
            TaoBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            expected_stake1.to_u64().into(),
            epsilon = TaoBalance::from(expected_stake1.to_u64()) / 1000.into()
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&old_coldkey2),
            TaoBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey2),
            expected_stake2.to_u64().into(),
            epsilon = TaoBalance::from(expected_stake2.to_u64()) / 1000.into()
        );
    });
}

#[test]
fn test_do_swap_coldkey_effect_on_delegated_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let delegator = U256::from(3);
        let hotkey = U256::from(4);
        let stake = TaoBalance::from(100_000_000_000_u64);

        StakingHotkeys::<Test>::insert(old_coldkey, vec![hotkey]);
        StakingHotkeys::<Test>::insert(delegator, vec![hotkey]);
        SubtensorModule::create_account_if_non_existent(&old_coldkey, &hotkey);
        add_balance_to_coldkey_account(&old_coldkey, stake);
        add_balance_to_coldkey_account(&delegator, stake);

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(old_coldkey),
            hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(delegator),
            hotkey,
            netuid,
            stake.into()
        ));
        let coldkey_stake_before = SubtensorModule::get_total_stake_for_coldkey(&old_coldkey);
        let delegator_stake_before = SubtensorModule::get_total_stake_for_coldkey(&delegator);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey,));

        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            coldkey_stake_before,
            epsilon = 500.into()
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&delegator),
            delegator_stake_before,
            epsilon = 500.into()
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&old_coldkey),
            TaoBalance::ZERO,
            epsilon = 500.into()
        );
    });
}

#[test]
fn test_swap_delegated_stake_for_coldkey() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let other_coldkey = U256::from(3);
        let hotkey1 = U256::from(4);
        let hotkey2 = U256::from(5);
        let stake_amount1 = DefaultMinStake::<Test>::get() * 10.into();
        let stake_amount2 = DefaultMinStake::<Test>::get() * 20.into();
        let netuid = NetUid::from(1);

        // Setup initial state
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, hotkey1, other_coldkey, 0);
        register_ok_neuron(netuid, hotkey2, other_coldkey, 0);

        let reserve = u64::from(stake_amount1 + stake_amount2) * 10;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Notice hotkey1 and hotkey2 are Owned by other_coldkey
        // old_coldkey and new_coldkey therefore delegates stake to them
        // === Give old_coldkey some balance ===
        add_balance_to_coldkey_account(
            &old_coldkey,
            stake_amount1 + stake_amount2 + 1_000_000.into(),
        );

        // === Stake to hotkeys ===
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(old_coldkey),
            hotkey1,
            netuid,
            stake_amount1
        ));
        let expected_stake_alpha1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &old_coldkey,
            netuid,
        );

        let (expected_stake_alpha2, fee) = mock::swap_tao_to_alpha(netuid, stake_amount2.into());
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(old_coldkey),
            hotkey2,
            netuid,
            stake_amount2
        ));
        let expected_stake_alpha2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &old_coldkey,
            netuid,
        );
        let fee = (expected_stake_alpha2.to_u64() as f64 * 0.003) as u64;

        // Record initial values
        let initial_total_issuance = SubtensorModule::get_total_issuance();
        let initial_total_stake = SubtensorModule::get_total_stake();
        let coldkey_stake = SubtensorModule::get_total_stake_for_coldkey(&old_coldkey);
        let stake_coldkey_hotkey1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &old_coldkey,
            netuid,
        );
        let stake_coldkey_hotkey2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &old_coldkey,
            netuid,
        );
        let total_hotkey1_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey1);
        let total_hotkey2_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey2);

        // Perform the swap
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey,));

        // Verify stake transfer
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &new_coldkey,
                netuid
            ),
            expected_stake_alpha1
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey2,
                &new_coldkey,
                netuid
            ),
            expected_stake_alpha2
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &old_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey2,
                &old_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        // Verify TotalColdkeyStake
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            coldkey_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&old_coldkey),
            TaoBalance::ZERO
        );

        // Verify TotalHotkeyStake remains unchanged
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey1),
            total_hotkey1_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey2),
            total_hotkey2_stake
        );

        // Verify total stake and issuance remain unchanged
        assert_eq!(
            SubtensorModule::get_total_stake(),
            initial_total_stake,
            "Total stake changed unexpectedly"
        );
        assert_eq!(
            SubtensorModule::get_total_issuance(),
            initial_total_issuance,
            "Total issuance changed unexpectedly"
        );
    });
}

#[test]
fn test_coldkey_swap_total() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let nominator1 = U256::from(2);
        let nominator2 = U256::from(3);
        let nominator3 = U256::from(4);
        let delegate1 = U256::from(5);
        let delegate2 = U256::from(6);
        let delegate3 = U256::from(7);
        let hotkey1 = U256::from(2);
        let hotkey2 = U256::from(3);
        let hotkey3 = U256::from(4);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let netuid3 = NetUid::from(3);
        let ed = ExistentialDeposit::get();
        let stake = DefaultMinStake::<Test>::get() * 10.into();

        // Initial funding. Burns will reduce these balances.
        add_balance_to_coldkey_account(&coldkey, stake * 6.into() + ed.into());
        add_balance_to_coldkey_account(&delegate1, stake * 2.into() + ed.into());
        add_balance_to_coldkey_account(&delegate2, stake * 2.into() + ed.into());
        add_balance_to_coldkey_account(&delegate3, stake * 2.into() + ed.into());
        add_balance_to_coldkey_account(&nominator1, stake * 2.into() + ed.into());
        add_balance_to_coldkey_account(&nominator2, stake * 2.into() + ed.into());
        add_balance_to_coldkey_account(&nominator3, stake * 2.into() + ed.into());

        let reserve = u64::from(stake) * 10;
        mock::setup_reserves(netuid1, reserve.into(), reserve.into());
        mock::setup_reserves(netuid2, reserve.into(), reserve.into());
        mock::setup_reserves(netuid3, reserve.into(), reserve.into());

        // Setup initial state
        add_network(netuid1, 13, 0);
        add_network(netuid2, 14, 0);
        add_network(netuid3, 15, 0);

        // Registrations (burns happen here)
        register_ok_neuron(netuid1, hotkey1, coldkey, 0);
        register_ok_neuron(netuid2, hotkey2, coldkey, 0);
        register_ok_neuron(netuid3, hotkey3, coldkey, 0);
        register_ok_neuron(netuid1, delegate1, delegate1, 0);
        register_ok_neuron(netuid2, delegate2, delegate2, 0);
        register_ok_neuron(netuid3, delegate3, delegate3, 0);

        // ------------------------------------------------------------
        // After the burn-based registrations, ensure each staking coldkey still
        // has enough free balance to perform its staking actions.
        //
        // Each of these accounts will stake `stake * N`, and we want them to
        // also retain ED so they don't get reaped mid-test.
        // ------------------------------------------------------------
        let ensure_min_balance = |account: &U256, required: TaoBalance| {
            let bal = SubtensorModule::get_coldkey_balance(account);
            if bal < required {
                add_balance_to_coldkey_account(account, required - bal);
            }
        };

        // coldkey stakes 6 times
        ensure_min_balance(&coldkey, stake * 6.into() + ed);

        // each delegate stakes 2 times
        ensure_min_balance(&delegate1, stake * 2.into() + ed);
        ensure_min_balance(&delegate2, stake * 2.into() + ed);
        ensure_min_balance(&delegate3, stake * 2.into() + ed);

        // each nominator stakes 2 times
        ensure_min_balance(&nominator1, stake * 2.into() + ed);
        ensure_min_balance(&nominator2, stake * 2.into() + ed);
        ensure_min_balance(&nominator3, stake * 2.into() + ed);

        Delegates::<Test>::insert(hotkey1, PerU16::from_parts(u16::MAX / 10));
        Delegates::<Test>::insert(hotkey2, PerU16::from_parts(u16::MAX / 10));
        Delegates::<Test>::insert(hotkey3, PerU16::from_parts(u16::MAX / 10));
        Delegates::<Test>::insert(delegate1, PerU16::from_parts(u16::MAX / 10));
        Delegates::<Test>::insert(delegate2, PerU16::from_parts(u16::MAX / 10));
        Delegates::<Test>::insert(delegate3, PerU16::from_parts(u16::MAX / 10));

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            hotkey1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            hotkey2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            hotkey3,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            delegate1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            delegate2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            delegate3,
            netuid1,
            stake.into()
        ));

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate1),
            hotkey1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate2),
            hotkey2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate3),
            hotkey3,
            netuid1,
            stake.into()
        ));

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate1),
            delegate1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate2),
            delegate2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(delegate3),
            delegate3,
            netuid1,
            stake.into()
        ));

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator1),
            hotkey1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator2),
            hotkey2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator3),
            hotkey3,
            netuid1,
            stake.into()
        ));

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator1),
            delegate1,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator2),
            delegate2,
            netuid1,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(nominator3),
            delegate3,
            netuid1,
            stake.into()
        ));

        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&coldkey),
            vec![hotkey1, hotkey2, hotkey3]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&coldkey),
            vec![hotkey1, hotkey2, hotkey3, delegate1, delegate2, delegate3]
        );
        let ck_stake = SubtensorModule::get_total_stake_for_coldkey(&coldkey);
        let hk1_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey1);
        let hk2_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey2);
        let hk3_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey3);
        let d1_stake = SubtensorModule::get_total_stake_for_hotkey(&delegate1);
        let d2_stake = SubtensorModule::get_total_stake_for_hotkey(&delegate2);
        let d3_stake = SubtensorModule::get_total_stake_for_hotkey(&delegate3);

        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate1),
            vec![delegate1]
        );
        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate2),
            vec![delegate2]
        );
        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate3),
            vec![delegate3]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate1),
            vec![delegate1, hotkey1]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate2),
            vec![delegate2, hotkey2]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate3),
            vec![delegate3, hotkey3]
        );

        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator1), vec![]);
        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator2), vec![]);
        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator3), vec![]);

        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator1),
            vec![hotkey1, delegate1]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator2),
            vec![hotkey2, delegate2]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator3),
            vec![hotkey3, delegate3]
        );

        // Perform the swap
        let new_coldkey = U256::from(1100);
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey),
            ck_stake
        );
        assert_ok!(SubtensorModule::do_swap_coldkey(&coldkey, &new_coldkey,));
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            ck_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey),
            TaoBalance::ZERO
        );

        // Check everything is swapped.
        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&new_coldkey),
            vec![hotkey1, hotkey2, hotkey3]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&new_coldkey),
            vec![hotkey1, hotkey2, hotkey3, delegate1, delegate2, delegate3]
        );
        // Shouldn't change.
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey1),
            hk1_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey2),
            hk2_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey3),
            hk3_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate1),
            d1_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate2),
            d2_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate3),
            d3_stake
        );

        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate1),
            vec![delegate1]
        );
        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate2),
            vec![delegate2]
        );
        assert_eq!(
            SubtensorModule::get_owned_hotkeys(&delegate3),
            vec![delegate3]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate1),
            vec![delegate1, hotkey1]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate2),
            vec![delegate2, hotkey2]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&delegate3),
            vec![delegate3, hotkey3]
        );

        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator1), vec![]);
        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator2), vec![]);
        assert_eq!(SubtensorModule::get_owned_hotkeys(&nominator3), vec![]);

        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator1),
            vec![hotkey1, delegate1]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator2),
            vec![hotkey2, delegate2]
        );
        assert_eq!(
            SubtensorModule::get_all_staked_hotkeys(&nominator3),
            vec![hotkey3, delegate3]
        );
    });
}

#[test]
fn test_do_swap_coldkey_effect_on_delegations() {
    new_test_ext(1).execute_with(|| {
        let new_coldkey = U256::from(0);
        let owner = U256::from(1);
        let coldkey = U256::from(4);
        let delegate = U256::from(2);
        let netuid = NetUid::from(0); // Stake to 0
        let netuid2 = NetUid::from(1); // Stake to 1
        let stake = DefaultMinStake::<Test>::get() * 10.into();
        let reserve = u64::from(stake) * 1000;

        mock::setup_reserves(netuid, reserve.into(), reserve.into());
        mock::setup_reserves(netuid2, reserve.into(), reserve.into());

        add_network(netuid, 13, 0); // root
        add_network(netuid2, 13, 0);

        root_register_ok(delegate, owner); // register on root
        register_ok_neuron(netuid2, delegate, owner, 0);
        add_balance_to_coldkey_account(&coldkey, stake * 10.into());

        // since the reserves are equal and we stake the same amount to both networks, we can reuse
        // this values for different networks. but you should take it into account in case of tests
        // changes
        let (expected_stake, fee) = mock::swap_tao_to_alpha(netuid, stake.into());

        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            delegate,
            netuid,
            stake
        ));

        // Add stake to netuid2
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            delegate,
            netuid2,
            stake
        ));

        // Perform the swap
        assert_ok!(SubtensorModule::do_swap_coldkey(&coldkey, &new_coldkey,));

        // Verify stake was moved for the delegate
        let approx_total_stake = stake * 2.into() - (fee * 2).into();
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate),
            approx_total_stake,
            epsilon = approx_total_stake / 100.into()
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey),
            TaoBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&new_coldkey),
            approx_total_stake,
            epsilon = approx_total_stake / 100.into()
        );
        let actual_stake_new: u64 = AlphaV2::<Test>::get((delegate, new_coldkey, netuid)).into();
        assert_eq!(expected_stake, actual_stake_new.into());
        let actual_stake_old: u64 = AlphaV2::<Test>::get((delegate, coldkey, netuid)).into();
        assert_eq!(actual_stake_old, 0u64);

        let actual_stake_new_2: u64 = AlphaV2::<Test>::get((delegate, new_coldkey, netuid2)).into();
        assert_eq!(expected_stake, actual_stake_new_2.into());
        let actual_stake_old_2: u64 = AlphaV2::<Test>::get((delegate, coldkey, netuid2)).into();
        assert_eq!(actual_stake_old_2, 0u64);
    });
}

#[test]
fn test_dispute_coldkey_swap_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let now = System::block_number();
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);

        assert_ok!(SubtensorModule::dispute_coldkey_swap(
            RuntimeOrigin::signed(who)
        ));

        assert_eq!(ColdkeySwapDisputes::<Test>::get(who), Some(now));
        assert!(matches!(
            last_event(),
            RuntimeEvent::SubtensorModule(Event::ColdkeySwapDisputed { coldkey: _ })
        ));
    });
}

#[test]
fn test_dispute_coldkey_swap_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);

        assert_noop!(
            SubtensorModule::dispute_coldkey_swap(RuntimeOrigin::root()),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::dispute_coldkey_swap(RuntimeOrigin::none()),
            BadOrigin
        );
    });
}

#[test]
fn test_dispute_coldkey_swap_without_announcement_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);

        assert_noop!(
            SubtensorModule::dispute_coldkey_swap(RuntimeOrigin::signed(who)),
            Error::<Test>::ColdkeySwapAnnouncementNotFound
        );
    });
}

#[test]
fn test_dispute_coldkey_swap_already_disputed_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);
        dispute_coldkey_swap(who);

        assert_noop!(
            SubtensorModule::dispute_coldkey_swap(RuntimeOrigin::signed(who)),
            Error::<Test>::ColdkeySwapAlreadyDisputed
        );
    });
}

#[test]
fn test_reset_coldkey_swap_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);
        dispute_coldkey_swap(who);

        assert_ok!(SubtensorModule::reset_coldkey_swap(
            RuntimeOrigin::root(),
            who,
        ));

        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
        assert!(!ColdkeySwapDisputes::<Test>::contains_key(who));
        assert!(matches!(
            last_event(),
            RuntimeEvent::SubtensorModule(Event::ColdkeySwapReset { who })
        ));
    });
}

#[test]
fn test_reset_coldkey_swap_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let coldkey = U256::from(2);

        assert_noop!(
            SubtensorModule::reset_coldkey_swap(RuntimeOrigin::signed(who), coldkey),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::reset_coldkey_swap(RuntimeOrigin::none(), coldkey),
            BadOrigin
        );
    });
}

#[test]
fn test_reset_coldkey_swap_keep_locked_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);
        dispute_coldkey_swap(who);

        assert_ok!(SubtensorModule::reset_coldkey_swap_keep_locked(
            RuntimeOrigin::root(),
            who,
        ));

        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
        assert_eq!(ColdkeySwapDisputes::<Test>::get(who), Some(0));
        assert!(matches!(
            last_event(),
            RuntimeEvent::SubtensorModule(Event::ColdkeySwapReset { who })
        ));
    });
}

#[test]
fn test_reset_coldkey_swap_keep_locked_with_bad_origin_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let coldkey = U256::from(2);

        assert_noop!(
            SubtensorModule::reset_coldkey_swap_keep_locked(RuntimeOrigin::signed(who), coldkey),
            BadOrigin
        );

        assert_noop!(
            SubtensorModule::reset_coldkey_swap_keep_locked(RuntimeOrigin::none(), coldkey),
            BadOrigin
        );
    });
}

#[test]
fn test_clear_coldkey_swap_announcement_works() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);

        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(who).unwrap();
        let delay = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(when + delay);

        assert_ok!(SubtensorModule::clear_coldkey_swap_announcement(
            RuntimeOrigin::signed(who)
        ));

        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
        System::assert_last_event(Event::ColdkeySwapCleared { who }.into());
    });
}

#[test]
fn test_clear_coldkey_swap_announcement_not_found() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);

        assert_noop!(
            SubtensorModule::clear_coldkey_swap_announcement(RuntimeOrigin::signed(who)),
            Error::<Test>::ColdkeySwapAnnouncementNotFound
        );
    });
}

#[test]
fn test_clear_coldkey_swap_announcement_too_early() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        announce_coldkey_swap(who, new_coldkey);

        // Advance by less than the full delay — one block short
        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(who).unwrap();
        let delay = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(when + delay - 1);

        assert_noop!(
            SubtensorModule::clear_coldkey_swap_announcement(RuntimeOrigin::signed(who)),
            Error::<Test>::ColdkeySwapClearTooEarly
        );

        // Announcement is still present
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
    });
}

#[test]
fn test_clear_coldkey_swap_announcement_bad_origin() {
    new_test_ext(1).execute_with(|| {
        assert_noop!(
            SubtensorModule::clear_coldkey_swap_announcement(RuntimeOrigin::root()),
            BadOrigin
        );
        assert_noop!(
            SubtensorModule::clear_coldkey_swap_announcement(RuntimeOrigin::none()),
            BadOrigin
        );
    });
}

#[test]
#[allow(deprecated)]
fn test_schedule_swap_coldkey_deprecated() {
    new_test_ext(1).execute_with(|| {
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);

        assert_noop!(
            SubtensorModule::schedule_swap_coldkey(
                <<Test as Config>::RuntimeOrigin>::root(),
                new_coldkey,
            ),
            Error::<Test>::Deprecated
        );
    });
}

#[macro_export]
macro_rules! comprehensive_setup {
    (
        $who:expr,
        $new_coldkey:expr,
        $new_coldkey_hash:expr,
        $stake1:expr,
        $stake2:expr,
        $stake3:expr,
        $hotkey1:expr,
        $hotkey2:expr,
        $hotkey3:expr
    ) => {{
        comprehensive_setup!(
            $who,
            $new_coldkey,
            $new_coldkey_hash,
            $stake1,
            $stake2,
            $stake3,
            $hotkey1,
            $hotkey2,
            $hotkey3,
            ExistentialDeposit::get()
        )
    }};

    // New form: caller specifies exactly how much free balance must remain
    // after staking (e.g. ED + swap_cost, or ED).
    (
        $who:expr,
        $new_coldkey:expr,
        $new_coldkey_hash:expr,
        $stake1:expr,
        $stake2:expr,
        $stake3:expr,
        $hotkey1:expr,
        $hotkey2:expr,
        $hotkey3:expr,
        $expected_remaining_balance:expr
    ) => {{
        let stake1: TaoBalance = $stake1.into();
        let stake2: TaoBalance = $stake2.into();
        let stake3: TaoBalance = $stake3.into();
        let expected_remaining: TaoBalance = $expected_remaining_balance.into();

        // Setup networks and subnet ownerships
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        SubnetOwner::<Test>::insert(netuid1, $who);
        SubnetOwner::<Test>::insert(netuid2, $who);

        // Setup reserves
        let reserve1 = u64::from(stake1 + stake3) * 10;
        let reserve2 = u64::from(stake2) * 10;
        mock::setup_reserves(netuid1, reserve1.into(), reserve1.into());
        mock::setup_reserves(netuid2, reserve2.into(), reserve2.into());

        // Setup auto stake destinations
        AutoStakeDestination::<Test>::insert($who, netuid1, $hotkey1);
        AutoStakeDestination::<Test>::insert($who, netuid2, $hotkey2);
        AutoStakeDestinationColdkeys::<Test>::insert(
            $hotkey1,
            netuid1,
            vec![$who, U256::from(3), U256::from(4)],
        );
        AutoStakeDestinationColdkeys::<Test>::insert(
            $hotkey2,
            netuid2,
            vec![U256::from(7), U256::from(8), $who],
        );

        // Setup neurons with stake
        register_ok_neuron(netuid1, $hotkey1, $who, 0);
        register_ok_neuron(netuid2, $hotkey2, $who, 0);
        register_ok_neuron(netuid1, $hotkey3, $who, 0);

        let hotkeys = vec![$hotkey1, $hotkey2, $hotkey3];
        assert_eq!(StakingHotkeys::<Test>::get($who), hotkeys);
        assert_eq!(OwnedHotkeys::<Test>::get($who), hotkeys);
        assert_eq!(Owner::<Test>::get($hotkey1), $who);
        assert_eq!(Owner::<Test>::get($hotkey2), $who);
        assert_eq!(Owner::<Test>::get($hotkey3), $who);

        // ------------------------------------------------------------
        // After registrations, ensure $who has enough free balance to:
        //   (stake1 + stake2 + stake3) + expected_remaining_balance
        // so the add_stake calls won't fail AND the remaining free balance
        // after staking is exactly what the tests expect.
        // ------------------------------------------------------------
        let stake_total = stake1 + stake2 + stake3;
        let required_free = stake_total + expected_remaining;

        let current_free = SubtensorModule::get_coldkey_balance(&$who);
        if current_free < required_free {
            add_balance_to_coldkey_account(&$who, required_free - current_free);
        }

        // Now staking will succeed and leave exactly expected_remaining behind.
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed($who),
            $hotkey1,
            netuid1,
            stake1.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed($who),
            $hotkey2,
            netuid2,
            stake2.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed($who),
            $hotkey3,
            netuid1,
            stake3.into()
        ));
        let hk1_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey1, &$who, netuid1);
        let hk2_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey2, &$who, netuid2);
        let hk3_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey3, &$who, netuid1);
        let total_ck_stake = SubtensorModule::get_total_stake_for_coldkey(&$who);

        // Setup identity
        let identity = ChainIdentityV2::default();
        IdentitiesV2::<Test>::insert($who, identity.clone());
        assert_eq!(IdentitiesV2::<Test>::get($who), Some(identity.clone()));
        assert!(IdentitiesV2::<Test>::get($new_coldkey).is_none());

        let balance_before = SubtensorModule::get_coldkey_balance(&$who);
        let total_stake_before = SubtensorModule::get_total_stake();

        (
            netuid1,
            netuid2,
            hotkeys,
            hk1_alpha,
            hk2_alpha,
            hk3_alpha,
            total_ck_stake,
            identity,
            balance_before,
            total_stake_before,
        )
    }};
}

#[macro_export]
macro_rules! comprehensive_checks {
    (
        $who:expr,
        $hotkey1:expr,
        $hotkey2:expr,
        $hotkey3:expr,
        $hotkeys:expr,
        $new_coldkey:expr,
        $balance_before:expr,
        $identity:expr,
        $netuid1:expr,
        $netuid2:expr,
        $hk1_alpha:expr,
        $hk2_alpha:expr,
        $hk3_alpha:expr,
        $total_ck_stake:expr,
        $total_stake_before:expr,
        $swap_cost:expr
    ) => {
        // Ensure the announcement has been consumed
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key($who));

        // Ensure the cost has been withdrawn from the old coldkey and recycled
        let balance_after = SubtensorModule::get_coldkey_balance(&$who);
        let ed = ExistentialDeposit::get();
        assert_eq!($balance_before - $swap_cost, balance_after + ed);

        // Ensure the identity is correctly swapped
        assert!(IdentitiesV2::<Test>::get($who).is_none());
        assert_eq!(IdentitiesV2::<Test>::get($new_coldkey), Some($identity));

        // Ensure the subnet ownerships are correctly swapped
        assert_eq!(SubnetOwner::<Test>::get($netuid1), $new_coldkey);
        assert_eq!(SubnetOwner::<Test>::get($netuid2), $new_coldkey);

        // Ensure the auto stake destinations are correctly swapped
        assert!(AutoStakeDestination::<Test>::get($who, $netuid1).is_none());
        assert!(AutoStakeDestination::<Test>::get($who, $netuid2).is_none());
        assert_eq!(
            AutoStakeDestination::<Test>::get($new_coldkey, $netuid1),
            Some($hotkey1)
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get($new_coldkey, $netuid2),
            Some($hotkey2)
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get($hotkey1, $netuid1),
            vec![U256::from(3), U256::from(4), $new_coldkey]
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get($hotkey2, $netuid2),
            vec![U256::from(7), U256::from(8), $new_coldkey]
        );

        // Ensure the coldkey stake is correctly swapped
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey1, &$who, $netuid1),
            0.into(),
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey2, &$who, $netuid2),
            0.into(),
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&$hotkey3, &$who, $netuid1),
            0.into(),
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &$hotkey1,
                &$new_coldkey,
                $netuid1
            ),
            $hk1_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &$hotkey2,
                &$new_coldkey,
                $netuid2
            ),
            $hk2_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &$hotkey3,
                &$new_coldkey,
                $netuid1
            ),
            $hk3_alpha
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&$who),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&$new_coldkey),
            $total_ck_stake,
        );

        // Ensure the staking hotkeys are correctly swapped (order-incensitive)
        assert!(StakingHotkeys::<Test>::get($who).is_empty());
        let mut actual_st_hots = StakingHotkeys::<Test>::get($new_coldkey);
        let mut expected_st_hots = $hotkeys.clone();
        actual_st_hots.sort();
        expected_st_hots.sort();
        assert_eq!(actual_st_hots, expected_st_hots);

        // Ensure the hotkey ownership is correctly swapped
        assert!(OwnedHotkeys::<Test>::get($who).is_empty());
        assert_eq!(OwnedHotkeys::<Test>::get($new_coldkey), $hotkeys);
        assert_eq!(Owner::<Test>::get($hotkey1), $new_coldkey);
        assert_eq!(Owner::<Test>::get($hotkey2), $new_coldkey);
        assert_eq!(Owner::<Test>::get($hotkey3), $new_coldkey);

        // Ensure the remaining balance is transferred to the new coldkey
        assert_eq!(SubtensorModule::get_coldkey_balance(&$who), 0.into());
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&$new_coldkey),
            ExistentialDeposit::get()
        );

        // Ensure total stake is unchanged
        assert_eq!(
            SubtensorModule::get_total_stake(),
            $total_stake_before,
            "Total stake changed unexpectedly"
        );

        // Verify event emission
        System::assert_last_event(
            Event::ColdkeySwapped {
                old_coldkey: $who,
                new_coldkey: $new_coldkey,
            }
            .into(),
        );
    };
}

fn coldkey_hash_of(coldkey: U256) -> H256 {
    <Test as frame_system::Config>::Hashing::hash_of(&coldkey)
}

fn announce_coldkey_swap(who: U256, new_coldkey: U256) {
    let ed = ExistentialDeposit::get();
    let swap_cost = SubtensorModule::get_key_swap_cost();
    add_balance_to_coldkey_account(&who, ed + swap_cost);

    assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
        RuntimeOrigin::signed(who),
        new_coldkey,
    ));
}

fn dispute_coldkey_swap(who: U256) {
    assert_ok!(SubtensorModule::dispute_coldkey_swap(
        RuntimeOrigin::signed(who),
    ));
}

fn seed_collateral_row(netuid: NetUid, hotkey: U256, coldkey: U256, state: MinerCollateralState) {
    let locked = state.locked;
    MinerCollateral::<Test>::insert((netuid, hotkey, coldkey), state);
    let aggregate = ColdkeyMinerCollateral::<Test>::get(netuid, coldkey) + locked;
    ColdkeyMinerCollateral::<Test>::insert(netuid, coldkey, aggregate);
    ColdkeyCollateralHotkeys::<Test>::mutate(netuid, coldkey, |hotkeys| {
        if !hotkeys.contains(&hotkey) {
            hotkeys
                .try_push(hotkey)
                .expect("test collateral index within bound");
        }
    });
}

/// Opens deposits at `hotkeys` for `coldkey` on `netuid`, each with its hotkey as balance.
fn seed_gamma_deposits(netuid: NetUid, coldkey: U256, hotkeys: core::ops::Range<u64>) {
    for hotkey in hotkeys {
        GammaDeposits::<Test>::insert(
            netuid,
            U256::from(hotkey),
            GammaDeposit {
                coldkey,
                spender: U256::from(77),
                balance: GammaBalance::from(hotkey),
                daily_spend_cap: Some(GammaBalance::from(5u64)),
                bucket: (GammaBalance::from(3u64), 9),
            },
        );
        ColdkeyGammaHotkeys::<Test>::mutate(netuid, coldkey, |index| {
            index.try_push(U256::from(hotkey)).unwrap()
        });
    }
}

// A coldkey swap moves the coldkey's gamma deposits with their terms.
#[test]
fn test_do_swap_coldkey_migrates_gamma_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        let (old_coldkey, new_coldkey) = (U256::from(1), U256::from(2));
        seed_gamma_deposits(netuid, old_coldkey, 100..102);

        let work = SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey).unwrap();
        assert_eq!(work.gamma_deposits, 2);
        assert!(!ColdkeyGammaHotkeys::<Test>::contains_key(
            netuid,
            old_coldkey
        ));
        assert_eq!(
            ColdkeyGammaHotkeys::<Test>::get(netuid, new_coldkey).into_inner(),
            vec![U256::from(100), U256::from(101)]
        );
        for hotkey in [100, 101] {
            assert_eq!(
                GammaDeposits::<Test>::get(netuid, U256::from(hotkey)),
                Some(GammaDeposit {
                    coldkey: new_coldkey,
                    spender: U256::from(77),
                    balance: GammaBalance::from(hotkey),
                    daily_spend_cap: Some(GammaBalance::from(5u64)),
                    bucket: (GammaBalance::from(3u64), 9),
                })
            );
        }
    });
}

// Gamma deposits merge into the new coldkey's, up to `MAX_GAMMA_DEPOSITS` per subnet.
#[test]
fn test_do_swap_coldkey_merges_gamma_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        let (old_coldkey, new_coldkey) = (U256::from(1), U256::from(2));
        let held = 200..200 + u64::from(MAX_GAMMA_DEPOSITS) - 1;
        seed_gamma_deposits(netuid, old_coldkey, 100..102);
        seed_gamma_deposits(netuid, new_coldkey, held.clone());
        assert_noop!(
            SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey),
            Error::<Test>::TooManyGammaDeposits
        );

        let closed = U256::from(held.end - 1);
        GammaDeposits::<Test>::remove(netuid, closed);
        ColdkeyGammaHotkeys::<Test>::mutate(netuid, new_coldkey, |index| {
            index.retain(|hotkey| *hotkey != closed)
        });
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));
        assert_eq!(
            ColdkeyGammaHotkeys::<Test>::get(netuid, new_coldkey).into_inner(),
            (held.start..held.end - 1)
                .chain(100..102)
                .map(U256::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            GammaDeposits::<Test>::get(netuid, U256::from(100)).map(|deposit| deposit.coldkey),
            Some(new_coldkey)
        );
    });
}

// More gamma deposits than one swap moves: refused before any write.
#[test]
fn test_do_swap_coldkey_refuses_too_many_gamma_deposits() {
    new_test_ext(1).execute_with(|| {
        let (heavy, target) = (U256::from(3), U256::from(4));
        let per_subnet = MAX_GAMMA_DEPOSITS as usize;
        let subnets = (MAX_COLDKEY_SWAP_GAMMA_DEPOSITS as usize).div_ceil(per_subnet) + 1;
        for index in 0..subnets {
            let netuid = NetUid::from(100 + index as u16);
            NetworksAdded::<Test>::insert(netuid, true);
            ColdkeyGammaHotkeys::<Test>::insert(
                netuid,
                heavy,
                BoundedVec::truncate_from(
                    (0..per_subnet as u64)
                        .map(|hotkey| U256::from(10_000 + hotkey))
                        .collect(),
                ),
            );
        }
        assert_noop!(
            SubtensorModule::do_swap_coldkey(&heavy, &target),
            Error::<Test>::ColdkeySwapTooHeavy
        );
    });
}

// A swap that would leave the destination with more gamma deposits than one swap moves is
// refused, so no one can make a coldkey too heavy to swap away.
#[test]
fn test_do_swap_coldkey_refuses_a_destination_left_too_heavy_to_swap() {
    new_test_ext(1).execute_with(|| {
        let (source, destination) = (U256::from(3), U256::from(4));
        let per_subnet = u64::from(MAX_GAMMA_DEPOSITS);
        let first = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        let second = add_dynamic_network(&U256::from(1004), &U256::from(1003));
        let third = add_dynamic_network(&U256::from(1006), &U256::from(1005));
        seed_gamma_deposits(first, source, 100..100 + per_subnet);
        seed_gamma_deposits(second, source, 200..200 + per_subnet);
        seed_gamma_deposits(third, destination, 300..301);
        assert_noop!(
            SubtensorModule::do_swap_coldkey(&source, &destination),
            Error::<Test>::ColdkeySwapTooHeavy
        );

        GammaDeposits::<Test>::remove(third, U256::from(300));
        ColdkeyGammaHotkeys::<Test>::remove(third, destination);
        assert_eq!(
            SubtensorModule::do_swap_coldkey(&source, &destination)
                .unwrap()
                .gamma_deposits,
            MAX_COLDKEY_SWAP_GAMMA_DEPOSITS
        );
    });
}

// Regression: coldkey swap must migrate miner collateral with the stake so the
// new coldkey cannot withdraw the locked bond.
#[test]
fn test_do_swap_coldkey_migrates_miner_collateral() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&old_coldkey, &hotkey);
        add_balance_to_coldkey_account(&old_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &old_coldkey,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old_coldkey,
            netuid,
        );
        let locked = alpha / 2.into();
        assert!(!locked.is_zero());

        seed_collateral_row(
            netuid,
            hotkey,
            old_coldkey,
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::from(7u64),
            },
        );

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        assert!(
            MinerCollateral::<Test>::get((netuid, hotkey, old_coldkey)).is_none(),
            "old coldkey collateral row must be cleared"
        );
        assert!(ColdkeyCollateralHotkeys::<Test>::get(netuid, old_coldkey).is_empty());
        assert_eq!(
            ColdkeyCollateralHotkeys::<Test>::get(netuid, new_coldkey).into_inner(),
            vec![hotkey]
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, old_coldkey),
            AlphaBalance::ZERO
        );

        let migrated = MinerCollateral::<Test>::get((netuid, hotkey, new_coldkey))
            .expect("collateral must move to new coldkey");
        assert_eq!(migrated.locked, locked);
        assert_eq!(migrated.drain_ratio, U64F64::from_num(1));
        assert_eq!(migrated.min_locked, AlphaBalance::ZERO);
        assert_eq!(migrated.earned, AlphaBalance::from(7u64));
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, new_coldkey),
            locked
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &new_coldkey,
                netuid
            ),
            alpha
        );

        // Locked bond remains non-withdrawable under the new coldkey.
        assert_err!(
            SubtensorModule::ensure_hotkey_covers_collateral(&new_coldkey, &hotkey, netuid, alpha),
            Error::<Test>::StakeUnavailable
        );
        assert_ok!(SubtensorModule::ensure_hotkey_covers_collateral(
            &new_coldkey,
            &hotkey,
            netuid,
            alpha.saturating_sub(locked)
        ));
    });
}

// Regression: a zero-locked floor (`locked == 0`, `min_locked > 0`) still has a
// standing MinerCollateral row and index entry, but ColdkeyMinerCollateral is
// already zero. Coldkey swap must migrate that row — not early-return on the
// locked aggregate.
#[test]
fn test_do_swap_coldkey_migrates_zero_locked_min_collateral_floor() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let min_locked = AlphaBalance::from(40_000_000_000u64);
        let earned = AlphaBalance::from(9u64);

        let _ = SubtensorModule::create_account_if_non_existent(&old_coldkey, &hotkey);
        seed_collateral_row(
            netuid,
            hotkey,
            old_coldkey,
            MinerCollateralState {
                locked: AlphaBalance::ZERO,
                drain_ratio: U64F64::from_num(1),
                min_locked,
                earned,
            },
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, old_coldkey),
            AlphaBalance::ZERO,
            "zero-locked floor must leave the locked aggregate empty"
        );
        assert!(
            !ColdkeyCollateralHotkeys::<Test>::get(netuid, old_coldkey).is_empty(),
            "zero-locked floor must remain indexed"
        );

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        assert!(
            MinerCollateral::<Test>::get((netuid, hotkey, old_coldkey)).is_none(),
            "old coldkey zero-locked floor must be cleared"
        );
        assert!(ColdkeyCollateralHotkeys::<Test>::get(netuid, old_coldkey).is_empty());
        assert_eq!(
            ColdkeyCollateralHotkeys::<Test>::get(netuid, new_coldkey).into_inner(),
            vec![hotkey]
        );

        let migrated = MinerCollateral::<Test>::get((netuid, hotkey, new_coldkey))
            .expect("zero-locked floor must move to new coldkey");
        assert_eq!(migrated.locked, AlphaBalance::ZERO);
        assert_eq!(migrated.min_locked, min_locked);
        assert_eq!(migrated.earned, earned);
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, new_coldkey),
            AlphaBalance::ZERO
        );
    });
}

// Regression: a late failure inside do_swap_coldkey must roll back collateral
// migration together with the stake move (storage transaction).
#[test]
fn test_do_swap_coldkey_rolls_back_collateral_on_failure() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let blocked_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&old_coldkey, &hotkey);
        add_balance_to_coldkey_account(&old_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &old_coldkey,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old_coldkey,
            netuid,
        );
        let locked = alpha / 2.into();

        seed_collateral_row(
            netuid,
            hotkey,
            old_coldkey,
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::from(3u64),
            },
        );

        // Destination has an active lock → swap_coldkey_locks fails after
        // stake/collateral migration; the storage transaction must roll back.
        SubtensorModule::insert_lock_state(
            &new_coldkey,
            netuid,
            &blocked_hotkey,
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(0),
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        assert_noop!(
            SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey),
            Error::<Test>::ActiveLockExists
        );

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &old_coldkey,
                netuid
            ),
            alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &new_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        let remaining = MinerCollateral::<Test>::get((netuid, hotkey, old_coldkey))
            .expect("collateral must remain on old coldkey after rollback");
        assert_eq!(remaining.locked, locked);
        assert_eq!(remaining.earned, AlphaBalance::from(3u64));
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, old_coldkey),
            locked
        );
        assert!(MinerCollateral::<Test>::get((netuid, hotkey, new_coldkey)).is_none());
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, new_coldkey),
            AlphaBalance::ZERO
        );
    });
}

// Orphaned bond (aggregate non-zero but missing from the collateral-hotkey
// index) fails closed — unbounded association / prefix scans are not used.
#[test]
fn test_do_swap_coldkey_fails_closed_on_orphaned_miner_collateral() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);
        let orphan_hotkey = U256::from(99);
        let locked = AlphaBalance::from(25_000_000_000u64);

        MinerCollateral::<Test>::insert(
            (netuid, orphan_hotkey, old_coldkey),
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, old_coldkey, locked);
        assert!(ColdkeyCollateralHotkeys::<Test>::get(netuid, old_coldkey).is_empty());

        assert_noop!(
            SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey),
            Error::<Test>::ColdkeyCollateralIncomplete
        );

        // Transaction rolls back: orphan row and aggregate stay on old coldkey.
        assert_eq!(
            MinerCollateral::<Test>::get((netuid, orphan_hotkey, old_coldkey))
                .expect("orphan remains")
                .locked,
            locked
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, old_coldkey),
            locked
        );
        assert!(MinerCollateral::<Test>::get((netuid, orphan_hotkey, new_coldkey)).is_none());
    });
}

#[test]
fn test_coldkey_collateral_positions_cap() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);

        for i in 0..MAX_COLDKEY_COLLATERAL_HOTKEYS {
            let hotkey = U256::from(10_000u64 + u64::from(i));
            seed_collateral_row(
                netuid,
                hotkey,
                coldkey,
                MinerCollateralState {
                    locked: AlphaBalance::from(1u64),
                    drain_ratio: U64F64::from_num(1),
                    min_locked: AlphaBalance::ZERO,
                    earned: AlphaBalance::ZERO,
                },
            );
        }

        let overflow_hotkey = U256::from(99_999);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &overflow_hotkey);
        assert_noop!(
            SubtensorModule::do_set_min_collateral(
                RuntimeOrigin::signed(coldkey),
                netuid,
                overflow_hotkey,
                AlphaBalance::from(1u64),
            ),
            Error::<Test>::ColdkeyCollateralPositionsFull
        );
    });
}

#[test]
fn test_transfer_miner_collateral_coldkey_merges_existing_dest() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(3);
        let old_coldkey = U256::from(1);
        let new_coldkey = U256::from(2);

        let old_locked = AlphaBalance::from(40u64);
        let dest_locked = AlphaBalance::from(10u64);

        seed_collateral_row(
            netuid,
            hotkey,
            old_coldkey,
            MinerCollateralState {
                locked: old_locked,
                drain_ratio: U64F64::from_num(0.5),
                min_locked: AlphaBalance::from(2u64),
                earned: AlphaBalance::from(5u64),
            },
        );
        seed_collateral_row(
            netuid,
            hotkey,
            new_coldkey,
            MinerCollateralState {
                locked: dest_locked,
                drain_ratio: U64F64::from_num(0.25),
                min_locked: AlphaBalance::from(3u64),
                earned: AlphaBalance::from(8u64),
            },
        );

        assert_ok!(SubtensorModule::transfer_miner_collateral_coldkey(
            netuid,
            &hotkey,
            &old_coldkey,
            &new_coldkey,
        ));

        assert!(MinerCollateral::<Test>::get((netuid, hotkey, old_coldkey)).is_none());
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, old_coldkey),
            AlphaBalance::ZERO
        );

        let merged = MinerCollateral::<Test>::get((netuid, hotkey, new_coldkey))
            .expect("dest row must exist after merge");
        assert_eq!(merged.locked, old_locked.saturating_add(dest_locked));
        // Keep the slower (min) drain ratio.
        assert_eq!(merged.drain_ratio, U64F64::from_num(0.25));
        assert_eq!(
            merged.min_locked,
            AlphaBalance::from(2u64).saturating_add(AlphaBalance::from(3u64))
        );
        assert_eq!(
            merged.earned,
            AlphaBalance::from(5u64).saturating_add(AlphaBalance::from(8u64))
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, new_coldkey),
            old_locked.saturating_add(dest_locked)
        );
    });
}

// ---------------------------------------------------------------------------
// Coldkey-swap merge into an already-used destination
// ---------------------------------------------------------------------------

#[test]
fn test_merge_swap_sums_overlapping_alpha_and_unions_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let shared_hotkey = U256::from(100);
        let old_only_hotkey = U256::from(101);
        let dest_only_hotkey = U256::from(102);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old,
            &shared_hotkey
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old,
            &old_only_hotkey
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest,
            &dest_only_hotkey
        ));
        // Dest also stakes to the shared hotkey (third-party stake association).
        let mut dest_staking = StakingHotkeys::<Test>::get(dest);
        if !dest_staking.contains(&shared_hotkey) {
            dest_staking.push(shared_hotkey);
            StakingHotkeys::<Test>::insert(dest, dest_staking);
        }

        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));
        SubtokenEnabled::<Test>::insert(netuid, true);

        let old_shared = AlphaBalance::from(1_000_000_u64);
        let dest_shared = AlphaBalance::from(2_500_000_u64);
        let old_only = AlphaBalance::from(750_000_u64);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &shared_hotkey,
            &old,
            netuid,
            old_shared,
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &shared_hotkey,
            &dest,
            netuid,
            dest_shared,
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &old_only_hotkey,
            &old,
            netuid,
            old_only,
        );

        // Overlapping basket watermarks must sum (anti double-claim).
        BasketClaimed::<Test>::insert(shared_hotkey, old, 40_i128);
        BasketClaimed::<Test>::insert(shared_hotkey, dest, 25_i128);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hotkey,
                &dest,
                netuid
            ),
            old_shared.saturating_add(dest_shared)
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_only_hotkey,
                &dest,
                netuid
            ),
            old_only
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hotkey,
                &old,
                netuid
            )
            .is_zero()
        );
        assert_eq!(BasketClaimed::<Test>::get(shared_hotkey, dest), 65_i128);
        assert_eq!(BasketClaimed::<Test>::get(shared_hotkey, old), 0_i128);

        let staking = StakingHotkeys::<Test>::get(dest);
        assert!(staking.contains(&shared_hotkey));
        assert!(staking.contains(&old_only_hotkey));
        assert!(staking.contains(&dest_only_hotkey));
        assert!(StakingHotkeys::<Test>::get(old).is_empty());
        assert_eq!(Owner::<Test>::get(old_only_hotkey), dest);
        assert_eq!(Owner::<Test>::get(shared_hotkey), dest);
        assert_eq!(Owner::<Test>::get(dest_only_hotkey), dest);
    });
}

#[test]
fn test_merge_swap_moves_subnet_ownership_and_keeps_dest_auto_stake() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let dest_hot = U256::from(11);
        let auto_old = U256::from(20);
        let auto_dest = U256::from(21);

        let netuid_owned = add_dynamic_network(&old_hot, &old);
        let netuid_auto = add_dynamic_network(&dest_hot, &dest);

        AutoStakeDestination::<Test>::insert(old, netuid_auto, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid_auto, vec![old]);
        AutoStakeDestination::<Test>::insert(dest, netuid_auto, auto_dest);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_dest, netuid_auto, vec![dest]);

        assert_eq!(SubnetOwner::<Test>::get(netuid_owned), old);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert_eq!(SubnetOwner::<Test>::get(netuid_owned), dest);
        // Destination auto-stake wins on the overlapping netuid.
        assert_eq!(
            AutoStakeDestination::<Test>::get(dest, netuid_auto),
            Some(auto_dest)
        );
        assert!(AutoStakeDestination::<Test>::get(old, netuid_auto).is_none());
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid_auto).contains(&old));
        assert!(AutoStakeDestinationColdkeys::<Test>::get(auto_dest, netuid_auto).contains(&dest));
    });
}

/// Merge into a used destination that has no auto-stake setting must keep the
/// unset/default reward route — source policy is dropped (Skeptic fid:5f22d507).
#[test]
fn test_merge_swap_does_not_import_source_auto_stake_onto_unset_dest() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let dest_hot = U256::from(11);
        let auto_old = U256::from(20);

        let _netuid_owned = add_dynamic_network(&old_hot, &old);
        let netuid_auto = add_dynamic_network(&dest_hot, &dest);
        assert!(SubtensorModule::coldkey_already_in_use(&dest));

        AutoStakeDestination::<Test>::insert(old, netuid_auto, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid_auto, vec![old]);
        // Destination deliberately has no AutoStakeDestination row.

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(AutoStakeDestination::<Test>::get(old, netuid_auto).is_none());
        assert!(AutoStakeDestination::<Test>::get(dest, netuid_auto).is_none());
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid_auto).contains(&old));
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid_auto).contains(&dest));
    });
}

/// Funded destination without staking associations also keeps unset auto-stake
/// (Skeptic fid:a2c90852) — free balance alone is prior presence.
#[test]
fn test_funded_dest_without_staking_does_not_import_source_auto_stake() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let auto_old = U256::from(20);

        let netuid_auto = add_dynamic_network(&old_hot, &old);
        add_balance_to_coldkey_account(&dest, ExistentialDeposit::get() + 1_000.into());
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));
        assert!(!SubtensorModule::get_coldkey_balance(&dest).is_zero());

        AutoStakeDestination::<Test>::insert(old, netuid_auto, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid_auto, vec![old]);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(AutoStakeDestination::<Test>::get(old, netuid_auto).is_none());
        assert!(AutoStakeDestination::<Test>::get(dest, netuid_auto).is_none());
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid_auto).contains(&dest));
    });
}

/// Destination presence is decided when V2 announces, so an unrelated balance
/// transfer during the delay cannot turn a blank rename into a policy-dropping
/// merge.
#[test]
fn test_v2_announcement_snapshot_ignores_post_announce_destination_dust() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let auto_old = U256::from(20);

        let netuid = add_dynamic_network(&old_hot, &old);
        AutoStakeDestination::<Test>::insert(old, netuid, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid, vec![old]);

        announce_coldkey_swap(old, dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(old).expect("announced");
        assert_eq!(
            announcement_hash,
            SubtensorModule::coldkey_swap_v2_announcement_hash(&dest, true)
        );
        assert_eq!(ColdkeySwapV2PolicySnapshots::<Test>::get(old), Some(true));

        // This balance can come from any account; the destination does not sign.
        add_balance_to_coldkey_account(&dest, ExistentialDeposit::get());
        assert!(SubtensorModule::coldkey_rejects_unsolicited_policy_import(
            &dest
        ));

        run_to_block(when);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(old),
            dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(old, netuid).is_none());
        assert_eq!(
            AutoStakeDestination::<Test>::get(dest, netuid),
            Some(auto_old)
        );
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid).contains(&old));
        assert!(AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid).contains(&dest));
        assert!(!ColdkeySwapV2PolicySnapshots::<Test>::contains_key(old));
    });
}

/// A destination funded before V2 announces remains an intentional merge and
/// keeps its unset/default reward route.
#[test]
fn test_v2_announcement_snapshot_preserves_funded_destination_default() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let auto_old = U256::from(20);

        let netuid = add_dynamic_network(&old_hot, &old);
        AutoStakeDestination::<Test>::insert(old, netuid, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid, vec![old]);
        add_balance_to_coldkey_account(&dest, ExistentialDeposit::get());

        announce_coldkey_swap(old, dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(old).expect("announced");
        assert_eq!(
            announcement_hash,
            SubtensorModule::coldkey_swap_v2_announcement_hash(&dest, false)
        );
        assert_eq!(ColdkeySwapV2PolicySnapshots::<Test>::get(old), Some(false));

        run_to_block(when);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(old),
            dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(old, netuid).is_none());
        assert!(AutoStakeDestination::<Test>::get(dest, netuid).is_none());
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid).contains(&old));
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid).contains(&dest));
        assert!(!ColdkeySwapV2PolicySnapshots::<Test>::contains_key(old));
    });
}

/// A legacy caller cannot opt into V2 snapshot semantics merely by submitting
/// a correctly formed V2 commitment: the dedicated V2 marker is also required.
#[test]
fn test_legacy_announcement_cannot_forge_v2_policy_snapshot() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&old, swap_cost + ExistentialDeposit::get());

        let forged_v2_hash = SubtensorModule::coldkey_swap_v2_announcement_hash(&dest, true);
        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(old),
            forged_v2_hash,
        ));
        assert!(!ColdkeySwapV2PolicySnapshots::<Test>::contains_key(old));
        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(old).expect("announced");
        run_to_block(when);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(old), dest,),
            Error::<Test>::AnnouncedColdkeyHashDoesNotMatch
        );
    });
}

/// Destination with only reserved funds (zero reducible) is still prior
/// presence for the import gate (Skeptic fid:c135ad77).
#[test]
fn test_reserved_only_dest_rejects_unsolicited_policy_import_gate() {
    use frame_support::traits::tokens::{Fortitude, Preservation, fungible::Inspect};

    new_test_ext(1).execute_with(|| {
        // Fresh id — avoid colliding with mock accounts that already hold refs.
        let dest = U256::from(9_001);
        let total = ExistentialDeposit::get() + 500.into();
        add_balance_to_coldkey_account(&dest, total);
        // Force free→reserved without Balances::reserve's ED consumer checks,
        // matching a pending multisig-style deposit that leaves no transferable
        // balance while total funds remain.
        frame_system::Account::<Test>::mutate(dest, |a| {
            a.data.reserved = a.data.reserved.saturating_add(a.data.free);
            a.data.free = 0.into();
        });
        assert!(Balances::free_balance(dest).is_zero());
        assert!(!Balances::reserved_balance(dest).is_zero());
        assert!(
            Balances::reducible_balance(&dest, Preservation::Expendable, Fortitude::Polite)
                .is_zero(),
            "reducible must be zero so the old gate would have missed this"
        );
        assert!(
            !Balances::total_balance(&dest).is_zero(),
            "total funds (incl. reserved) must remain"
        );
        // `Inspect::balance` is free-only — must not be used as the presence signal.
        assert!(Balances::balance(&dest).is_zero());
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));
        assert!(
            SubtensorModule::coldkey_rejects_unsolicited_policy_import(&dest),
            "reserved-only dest must reject policy import"
        );
    });
}

/// End-to-end: locked (non-reducible) funds on dest still block auto-stake
/// import under the total-balance gate (Skeptic fid:c135ad77).
#[test]
fn test_locked_nonzero_total_dest_does_not_import_source_auto_stake() {
    use frame_support::traits::tokens::{Fortitude, Preservation, fungible::Inspect};
    use frame_support::traits::{LockableCurrency, WithdrawReasons};

    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let auto_old = U256::from(20);

        let netuid_auto = add_dynamic_network(&old_hot, &old);
        let locked = ExistentialDeposit::get() + 500.into();
        add_balance_to_coldkey_account(&dest, locked);
        Balances::set_lock(*b"mskdep00", &dest, locked, WithdrawReasons::all());
        assert!(
            Balances::reducible_balance(&dest, Preservation::Expendable, Fortitude::Polite)
                .is_zero(),
            "polite reducible must be zero under a full lock"
        );
        assert!(!Balances::total_balance(&dest).is_zero());
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));

        AutoStakeDestination::<Test>::insert(old, netuid_auto, auto_old);
        AutoStakeDestinationColdkeys::<Test>::insert(auto_old, netuid_auto, vec![old]);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(AutoStakeDestination::<Test>::get(old, netuid_auto).is_none());
        assert!(AutoStakeDestination::<Test>::get(dest, netuid_auto).is_none());
        assert!(!AutoStakeDestinationColdkeys::<Test>::get(auto_old, netuid_auto).contains(&dest));
    });
}

#[test]
fn test_merge_swap_destination_account_flags_win() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let dest_hot = U256::from(20);

        // Destination is already in use so its AccountFlags win over the source.
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        SubtensorModule::set_accept_locked_alpha(&old, true);
        // Destination rejects locked alpha (default / cleared).
        AccountFlags::<Test>::remove(dest);
        assert!(SubtensorModule::account_rejects_locked_alpha(&dest));
        assert!(!SubtensorModule::account_rejects_locked_alpha(&old));
        assert!(SubtensorModule::coldkey_already_in_use(&dest));

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(AccountFlags::<Test>::get(old) == 0 || !AccountFlags::<Test>::contains_key(old));
        assert!(SubtensorModule::account_rejects_locked_alpha(&dest));
    });
}

#[test]
fn test_rename_onto_fresh_destination_moves_source_account_flags() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hot = U256::from(10);

        assert_ok!(SubtensorModule::create_account_if_non_existent(&old, &hot));
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(old),
            true,
        ));
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(SubtensorModule::account_rejects_staking_hotkey_growth(
            &dest
        ));
        assert!(!SubtensorModule::account_rejects_staking_hotkey_growth(
            &old
        ));
        assert_eq!(Owner::<Test>::get(hot), dest);
    });
}

/// Destination with configured AccountFlags but no staking associations keeps
/// those flags on merge (Auditor fid:9411d39b).
#[test]
fn test_merge_preserves_configured_dest_flags_without_staking_associations() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);

        // Empty source (no staking associations) with a non-default flag that
        // would follow on a true rename — must not overwrite dest's opt-out.
        add_balance_to_coldkey_account(&old, ExistentialDeposit::get());
        SubtensorModule::set_accept_locked_alpha(&old, true);
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(dest),
            true,
        ));
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));
        assert!(!SubtensorModule::coldkey_already_in_use(&old));
        assert!(AccountFlags::<Test>::contains_key(dest));
        assert!(SubtensorModule::account_rejects_staking_hotkey_growth(
            &dest
        ));

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(SubtensorModule::account_rejects_staking_hotkey_growth(
            &dest
        ));
        // Dest did not inherit source's accept-locked bit.
        assert!(SubtensorModule::account_rejects_locked_alpha(&dest));
    });
}

/// Exactly MAX dest exhausted locks admit; one past the cap fails closed
/// (Auditor fid:9792420a).
#[test]
fn test_swap_dest_lock_cap_admits_max_rejects_sentinel() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let now = SubtensorModule::get_current_block_as_u64();
        add_balance_to_coldkey_account(&old, ExistentialDeposit::get());

        for i in 0..MAX_COLDKEY_SWAP_DEST_LOCKS {
            let netuid = NetUid::from(1 + i as u16);
            let dhot = U256::from(100 + u64::from(i));
            Lock::<Test>::insert(
                (dest, netuid, dhot),
                LockState {
                    locked_mass: AlphaBalance::ZERO,
                    conviction: U64F64::from_num(0),
                    last_update: now,
                },
            );
        }

        let work = SubtensorModule::do_swap_coldkey_tracked(&old, &dest).expect("at cap");
        assert_eq!(work.dest_locks, MAX_COLDKEY_SWAP_DEST_LOCKS);

        // Re-seed an empty source and one extra exhausted dest row.
        let old2 = U256::from(3);
        add_balance_to_coldkey_account(&old2, ExistentialDeposit::get());
        // Dest still has the purged... wait, prior swap purged them. Re-insert
        // MAX+1 exhausted rows.
        for i in 0..MAX_COLDKEY_SWAP_DEST_LOCKS.saturating_add(1) {
            let netuid = NetUid::from(1 + i as u16);
            let dhot = U256::from(200 + u64::from(i));
            Lock::<Test>::insert(
                (dest, netuid, dhot),
                LockState {
                    locked_mass: AlphaBalance::ZERO,
                    conviction: U64F64::from_num(0),
                    last_update: now,
                },
            );
        }
        let err = SubtensorModule::do_swap_coldkey_tracked(&old2, &dest).expect_err("over cap");
        assert_eq!(err.1, Error::<Test>::ColdkeySwapTooHeavy.into());
    });
}

#[test]
fn test_merge_swap_blocked_by_destination_active_lock() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        let netuid = add_dynamic_network(&hotkey, &dest);

        let now = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (dest, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(1_000_000_u64),
                conviction: U64F64::from_num(0),
                last_update: now,
            },
        );

        assert_err!(
            SubtensorModule::do_swap_coldkey(&old, &dest),
            Error::<Test>::ActiveLockExists
        );
    });
}

#[test]
fn test_announce_rejects_hotkey_destination_before_fee() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let hotkey = U256::from(2);
        let cold = U256::from(3);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &cold, &hotkey
        ));

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);
        let before = SubtensorModule::get_coldkey_balance(&who);

        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(who), hotkey),
            Error::<Test>::NewColdKeyIsHotkey
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), before);
        assert!(ColdkeySwapAnnouncements::<Test>::get(who).is_none());
    });
}

#[test]
fn test_announce_rejects_destination_with_active_lock_before_fee() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        let netuid = add_dynamic_network(&hotkey, &dest);

        let now = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (dest, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(500_000_u64),
                conviction: U64F64::from_num(0),
                last_update: now,
            },
        );

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);
        let before = SubtensorModule::get_coldkey_balance(&who);

        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(who), dest),
            Error::<Test>::ActiveLockExists
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), before);
    });
}

#[test]
fn test_announce_and_merge_purge_exhausted_destination_lock_rows() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let who_hot = U256::from(20);
        let dest_hot = U256::from(3);
        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &who, &who_hot
        ));
        // Destination is already in use (owns a hotkey) but does not opt out of
        // receiving locked alpha — fully exhausted dust rows alone must not
        // block the merge.
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        // Opt in: default AccountFlags reject locked alpha on used destinations.
        SubtensorModule::set_accept_locked_alpha(&dest, true);
        assert!(SubtensorModule::coldkey_already_in_use(&dest));
        assert!(!SubtensorModule::account_rejects_locked_alpha(&dest));

        let now = SubtensorModule::get_current_block_as_u64();
        let active = AlphaBalance::from(2_500_000_u64);
        // Fully exhausted dust row on dest; sorts before source hotkey.
        Lock::<Test>::insert(
            (dest, netuid, dest_hot),
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::from_num(0),
                last_update: now,
            },
        );
        LockingColdkeys::<Test>::insert((netuid, dest_hot, dest), ());
        Lock::<Test>::insert(
            (who, netuid, who_hot),
            LockState {
                locked_mass: active,
                conviction: U64F64::from_num(7),
                last_update: now,
            },
        );

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));
        // Announce admission is read-only: dust rows remain until execute.
        assert!(Lock::<Test>::get((dest, netuid, dest_hot)).is_some());

        assert_ok!(SubtensorModule::do_swap_coldkey(&who, &dest));
        assert!(Lock::<Test>::get((dest, netuid, dest_hot)).is_none());
        assert_eq!(Lock::<Test>::iter_prefix((dest, netuid)).count(), 1);
        assert_eq!(SubtensorModule::get_current_locked(&dest, netuid), active);
    });
}

#[test]
fn test_third_party_announce_cannot_wipe_destination_conviction_only_lock() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let dest = U256::from(2);
        let dest_hot = U256::from(3);
        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));

        let now = SubtensorModule::get_current_block_as_u64();
        let conviction = U64F64::from_num(12_345);
        let individual = LockState {
            locked_mass: AlphaBalance::ZERO,
            conviction,
            last_update: now,
        };
        // Seed individual + aggregate conviction used by subnet-king scoring.
        SubtensorModule::insert_lock_state(&dest, netuid, &dest_hot, individual);
        HotkeyLock::<Test>::insert(
            netuid,
            dest_hot,
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction,
                last_update: now,
            },
        );
        let aggregate_before = HotkeyLock::<Test>::get(netuid, dest_hot)
            .expect("aggregate present")
            .conviction;

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&attacker, swap_cost + ed);
        let before = SubtensorModule::get_coldkey_balance(&attacker);

        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(attacker), dest),
            Error::<Test>::ActiveLockExists
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&attacker), before);
        assert!(ColdkeySwapAnnouncements::<Test>::get(attacker).is_none());

        // Destination conviction-only lock and aggregate must be untouched.
        let preserved = Lock::<Test>::get((dest, netuid, dest_hot))
            .expect("conviction-only lock must survive third-party announce");
        assert_eq!(preserved.locked_mass, AlphaBalance::ZERO);
        assert_eq!(preserved.conviction, conviction);
        assert_eq!(
            HotkeyLock::<Test>::get(netuid, dest_hot)
                .expect("aggregate must survive")
                .conviction,
            aggregate_before
        );
    });
}

#[test]
fn test_merge_swap_combined_hotkeys_too_heavy() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);

        // Half the cap on each side with no overlap → union exceeds the bound.
        let half = (MAX_COLDKEY_SWAP_HOTKEYS / 2) + 1;
        let mut old_list = Vec::new();
        let mut dest_list = Vec::new();
        for i in 0..half {
            old_list.push(U256::from(10_000 + u64::from(i)));
            dest_list.push(U256::from(20_000 + u64::from(i)));
        }
        StakingHotkeys::<Test>::insert(old, old_list);
        StakingHotkeys::<Test>::insert(dest, dest_list);

        assert_err!(
            SubtensorModule::do_swap_coldkey(&old, &dest),
            Error::<Test>::ColdkeySwapTooHeavy
        );
    });
}

#[test]
fn test_merge_swap_keeps_later_hotkey_swap_cooldown() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        let netuid = add_dynamic_network(&hotkey, &old);

        LastHotkeySwapOnNetuid::<Test>::insert(netuid, old, 100u64);
        LastHotkeySwapOnNetuid::<Test>::insert(netuid, dest, 250u64);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert!(!LastHotkeySwapOnNetuid::<Test>::contains_key(netuid, old));
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(netuid, dest), 250u64);
    });
}

#[test]
fn test_merge_swap_root_stake_and_rate_limits() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &hotkey
        ));

        let root = NetUid::ROOT;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old,
            root,
            AlphaBalance::from(1_000_000_u64),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &dest,
            root,
            AlphaBalance::from(500_000_u64),
        );

        LastRateLimitedBlock::<Test>::insert(RateLimitKey::LastTxBlock(old), 40u64);
        LastRateLimitedBlock::<Test>::insert(RateLimitKey::LastTxBlock(dest), 90u64);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &dest, root),
            AlphaBalance::from(1_500_000_u64)
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &old, root)
                .is_zero()
        );
        assert!(!LastRateLimitedBlock::<Test>::contains_key(
            RateLimitKey::LastTxBlock(old)
        ));
        assert_eq!(
            LastRateLimitedBlock::<Test>::get(RateLimitKey::LastTxBlock(dest)),
            90u64
        );
    });
}

#[test]
fn test_merge_swap_honors_dest_reject_locked_alpha() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let dest_hot = U256::from(11);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &old_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        // Destination is in use and keeps the default reject-locked-alpha posture.
        assert!(SubtensorModule::coldkey_already_in_use(&dest));
        assert!(SubtensorModule::account_rejects_locked_alpha(&dest));

        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));
        let now = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (old, netuid, old_hot),
            LockState {
                locked_mass: AlphaBalance::from(1_000_000_u64),
                conviction: U64F64::from_num(0),
                last_update: now,
            },
        );

        assert_err!(
            SubtensorModule::do_swap_coldkey(&old, &dest),
            Error::<Test>::AccountRejectsLockedAlpha
        );
        // Source lock untouched.
        assert_eq!(
            Lock::<Test>::get((old, netuid, old_hot))
                .expect("source lock remains")
                .locked_mass,
            AlphaBalance::from(1_000_000_u64)
        );
    });
}

#[test]
fn test_merge_swap_honors_dest_reject_staking_hotkey_growth() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let dest_hot = U256::from(11);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &old_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(dest),
            true,
        ));

        assert_err!(
            SubtensorModule::do_swap_coldkey(&old, &dest),
            Error::<Test>::AccountRejectsStakingHotkeyGrowth
        );
        assert_eq!(Owner::<Test>::get(old_hot), old);
        assert!(StakingHotkeys::<Test>::get(dest).contains(&dest_hot));
        assert!(!StakingHotkeys::<Test>::get(dest).contains(&old_hot));
    });
}

#[test]
fn test_announce_rejects_dest_opt_outs_before_fee() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let who_hot = U256::from(10);
        let dest_hot = U256::from(11);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &who, &who_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(dest),
            true,
        ));

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);
        let before = SubtensorModule::get_coldkey_balance(&who);

        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(who), dest),
            Error::<Test>::AccountRejectsStakingHotkeyGrowth
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&who), before);
        assert!(ColdkeySwapAnnouncements::<Test>::get(who).is_none());
    });
}

#[test]
fn test_fresh_destination_still_receives_source_locks() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(10);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &hotkey
        ));
        // Destination is unused (no staking / owned hotkeys).
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));

        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));
        let now = SubtensorModule::get_current_block_as_u64();
        let locked = AlphaBalance::from(2_000_000_u64);
        Lock::<Test>::insert(
            (old, netuid, hotkey),
            LockState {
                locked_mass: locked,
                conviction: U64F64::from_num(0),
                last_update: now,
            },
        );

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));
        assert!(Lock::<Test>::get((old, netuid, hotkey)).is_none());
        assert_eq!(
            Lock::<Test>::get((dest, netuid, hotkey))
                .expect("lock moved to fresh dest")
                .locked_mass,
            locked
        );
    });
}

#[test]
fn test_announced_swap_preserves_announcement_when_dest_opts_out_after_announce() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let who_hot = U256::from(10);
        let dest_hot = U256::from(11);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &who, &who_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));

        // Destination opts out after the fee was charged.
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(dest),
            true,
        ));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get() + 1;
        run_to_block(now + delay);

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(who), dest),
            Error::<Test>::AccountRejectsStakingHotkeyGrowth
        );
        // Announcement (and the paid fee) must remain usable for a later retry /
        // clear, not silently burned by a destination grief.
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
        assert_eq!(Owner::<Test>::get(who_hot), who);
    });
}

#[test]
fn test_merge_swap_basket_claimed_overflow_fails_closed() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let old_hot = U256::from(10);
        let dest_hot = U256::from(11);
        let fund_hot = U256::from(12);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &old_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));

        // Claim-only associations so the swap visits the shared fund watermark.
        HotkeySwapBasketCredits::<Test>::insert(old, fund_hot, ());
        HotkeySwapBasketCredits::<Test>::insert(dest, fund_hot, ());
        BasketClaimed::<Test>::insert(fund_hot, old, i128::MAX);
        BasketClaimed::<Test>::insert(fund_hot, dest, 1_i128);

        assert_err!(
            SubtensorModule::do_swap_coldkey(&old, &dest),
            DispatchError::Arithmetic(sp_runtime::ArithmeticError::Overflow)
        );
        // Rolled back: both watermarks remain, no silent saturate.
        assert_eq!(BasketClaimed::<Test>::get(fund_hot, old), i128::MAX);
        assert_eq!(BasketClaimed::<Test>::get(fund_hot, dest), 1_i128);
        assert!(HotkeySwapBasketCredits::<Test>::contains_key(old, fund_hot));
        assert_eq!(Owner::<Test>::get(old_hot), old);
    });
}

#[test]
fn test_merge_swap_rolls_source_locks_under_source_policy() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(10);
        let dest_hot = U256::from(11);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &hotkey
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        // Destination is in use and accepts locked alpha, but is perpetual on
        // this netuid with no active locks of its own.
        assert_ok!(SubtensorModule::set_reject_locked_alpha(
            RuntimeOrigin::signed(dest),
            false,
        ));
        let netuid = add_dynamic_network(&U256::from(200), &U256::from(201));
        DecayingLock::<Test>::insert(dest, netuid, false);
        assert!(SubtensorModule::is_perpetual_lock(&dest, netuid));
        assert!(!SubtensorModule::is_perpetual_lock(&old, netuid));

        UnlockRate::<Test>::put(200u64);
        MaturityRate::<Test>::put(200u64);
        let start = SubtensorModule::get_current_block_as_u64();
        let initial = AlphaBalance::from(1_000_000_u64);
        Lock::<Test>::insert(
            (old, netuid, hotkey),
            LockState {
                locked_mass: initial,
                conviction: U64F64::from_num(0),
                last_update: start,
            },
        );
        // Advance so a decaying roll reduces mass; a perpetual roll would not.
        System::set_block_number(start + UnlockRate::<Test>::get());

        let expected = crate::staking::lock::ConvictionModel::roll_forward_lock(
            Lock::<Test>::get((old, netuid, hotkey)).expect("source lock"),
            SubtensorModule::get_current_block_as_u64(),
            UnlockRate::<Test>::get(),
            MaturityRate::<Test>::get(),
            false,
            false, // source decaying policy
        )
        .0
        .locked_mass;

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        let moved = Lock::<Test>::get((dest, netuid, hotkey)).expect("lock on dest");
        assert_eq!(
            moved.locked_mass, expected,
            "merge must not reinflate mass by rolling history under dest perpetual policy"
        );
        assert!(moved.locked_mass < initial);
    });
}

#[test]
fn test_coldkey_swap_work_stops_after_position_cap() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(10);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old, &hotkey
        ));
        // Insert more unique netuid stake rows than the admission cap allows.
        let over = MAX_COLDKEY_SWAP_POSITIONS.saturating_add(8);
        for i in 0..over {
            let netuid = NetUid::from(i as u16);
            AlphaV2::<Test>::insert((hotkey, old, netuid), SafeFloat::from(1u64));
        }

        let work = SubtensorModule::coldkey_swap_work(&old, &dest);
        assert_eq!(
            work.positions,
            MAX_COLDKEY_SWAP_POSITIONS.saturating_add(1),
            "admission scan must stop one past the cap"
        );
        assert!(work.positions > MAX_COLDKEY_SWAP_POSITIONS);
    });
}

#[test]
fn test_coldkey_swap_work_union_not_undercounted_by_overlap() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let shared = (MAX_COLDKEY_SWAP_HOTKEYS / 2) as usize;
        let dest_only = shared + 2; // union would exceed MAX if fully discovered

        // Source holds the shared prefix of dest's list.
        let mut dest_list = Vec::new();
        for i in 0..(shared + dest_only) {
            dest_list.push(U256::from(10_000 + i as u64));
        }
        let source_list: Vec<_> = dest_list.iter().take(shared).cloned().collect();
        StakingHotkeys::<Test>::insert(old, source_list);
        // Dest list starts with the shared hotkeys, then many dest-only ones.
        StakingHotkeys::<Test>::insert(dest, dest_list);

        let work = SubtensorModule::coldkey_swap_work(&old, &dest);
        assert!(
            work.hotkeys > MAX_COLDKEY_SWAP_HOTKEYS,
            "overlapping prefix must not hide dest-only hotkeys from the union count"
        );
    });
}

/// Announce reveals the destination in call data. Without a reservation a third
/// party could register that address as their hotkey during the delay and force
/// `NewColdKeyIsHotkey` at execute — repeatedly delaying recovery.
#[test]
fn test_announce_reserves_destination_against_third_party_hotkey_registration() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let attacker = U256::from(3);
        let attacker_cold = U256::from(4);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();

        add_balance_to_coldkey_account(&who, swap_cost + ed);
        add_balance_to_coldkey_account(&attacker_cold, swap_cost + ed);
        add_balance_to_coldkey_account(&attacker, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(dest),
            Some(who)
        );
        assert_eq!(ColdkeySwapReservedDestination::<Test>::get(who), Some(dest));

        // Third party cannot claim the reserved destination as a hotkey.
        assert_noop!(
            SubtensorModule::create_account_if_non_existent(&attacker_cold, &dest),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        assert!(!SubtensorModule::hotkey_account_exists(&dest));

        // Another coldkey cannot announce to the same reserved destination
        // (refused before the fee, so attacker's balance is untouched).
        let attacker_before = SubtensorModule::get_coldkey_balance(&attacker);
        assert_noop!(
            SubtensorModule::announce_coldkey_swap_v2(RuntimeOrigin::signed(attacker), dest,),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&attacker),
            attacker_before
        );

        // After the delay the owner can still execute the swap.
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        let now = System::block_number();
        run_to_block(now + delay);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(who),
            dest,
        ));
        assert!(!ColdkeySwapDestinationReservations::<Test>::contains_key(
            dest
        ));
        assert!(!ColdkeySwapReservedDestination::<Test>::contains_key(who));
        // Reservation released — a later registration of an unrelated key works.
        let free_hot = U256::from(99);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &attacker_cold,
            &free_hot
        ));
    });
}

/// A competing legacy hash announcement must not execute into another
/// announcer's V2-reserved destination (Skeptic fid:fb099e4c).
#[test]
fn test_legacy_announced_swap_cannot_merge_into_reserved_destination() {
    new_test_ext(1).execute_with(|| {
        let victim = U256::from(1);
        let dest = U256::from(2);
        let attacker = U256::from(3);
        let attacker_hot = U256::from(4);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 20;

        // Attacker holds locked stake they would otherwise dump onto dest.
        let netuid = add_dynamic_network(&attacker_hot, &attacker);
        add_balance_to_coldkey_account(
            &attacker,
            TaoBalance::from(stake_amount * 2) + swap_cost + ed,
        );
        assert_ok!(SubtensorModule::stake_into_subnet(
            &attacker_hot,
            &attacker,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        ));
        let now = SubtensorModule::get_current_block_as_u64();
        SubtensorModule::insert_lock_state(
            &attacker,
            netuid,
            &attacker_hot,
            LockState {
                locked_mass: AlphaBalance::from(stake_amount / 2),
                conviction: U64F64::from_num(1),
                last_update: now,
            },
        );
        SubtensorModule::set_accept_locked_alpha(&attacker, true);

        add_balance_to_coldkey_account(&victim, swap_cost + ed);
        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(victim),
            dest,
        ));
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(dest),
            Some(victim)
        );

        // Attacker announces the same dest via legacy hash (no reservation).
        let dest_hash = <Test as frame_system::Config>::Hashing::hash_of(&dest);
        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(attacker),
            dest_hash,
        ));
        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(attacker).expect("announced");
        run_to_block(when + 1);

        // Competing execute must fail closed on the live reservation.
        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(attacker), dest,),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        // Dest still reserved for the victim; no attacker lock landed.
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(dest),
            Some(victim)
        );
        assert!(Lock::<Test>::get((dest, netuid, attacker_hot)).is_none());
    });
}

/// Third parties must not grow a reserved destination's stake associations
/// during the announce delay (would trip TooHeavy / dest opt-outs at execute).
#[test]
fn test_reserved_destination_blocks_third_party_stake_transfer() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let attacker = U256::from(3);
        let attacker_hot = U256::from(4);
        let dest_hot = U256::from(5);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 20;

        // Attacker holds stake they could otherwise dump onto the reserved dest.
        let netuid = add_dynamic_network(&attacker_hot, &attacker);
        add_balance_to_coldkey_account(&attacker, (stake_amount * 2).into());
        assert_ok!(SubtensorModule::stake_into_subnet(
            &attacker_hot,
            &attacker,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        ));

        add_balance_to_coldkey_account(&who, swap_cost + ed);
        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(dest),
            Some(who)
        );

        // Cross-coldkey transfer onto the reserved destination is refused.
        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(attacker),
                dest,
                attacker_hot,
                netuid,
                netuid,
                stake_amount.into(),
            ),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        assert_noop!(
            SubtensorModule::do_transfer_stake_and_hotkey(
                RuntimeOrigin::signed(attacker),
                dest,
                attacker_hot,
                dest_hot,
                netuid,
                netuid,
                stake_amount.into(),
            ),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        // Stake remains with the attacker; dest was not marked in-use.
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &attacker_hot,
                &attacker,
                netuid
            ) > AlphaBalance::ZERO
        );
        assert!(!SubtensorModule::coldkey_already_in_use(&dest));

        // Destination itself can still stake with its own funds (same-coldkey).
        add_balance_to_coldkey_account(&dest, (stake_amount * 2).into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        assert_ok!(SubtensorModule::stake_into_subnet(
            &dest_hot,
            &dest,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        ));
        assert!(SubtensorModule::coldkey_already_in_use(&dest));
    });
}

/// Hotkey-swap must not assign Owner to a reserved coldkey-swap destination
/// (covers the path that bypasses create_account_if_non_existent).
#[test]
fn test_swap_hotkey_refuses_reserved_coldkey_swap_destination() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let attacker_cold = U256::from(4);
        let attacker_hot = U256::from(5);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();

        add_balance_to_coldkey_account(&who, swap_cost + ed);
        add_balance_to_coldkey_account(&attacker_cold, swap_cost * 10.into() + ed);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &attacker_cold,
            &attacker_hot
        ));

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));

        assert_noop!(
            SubtensorModule::swap_hotkey_v2(
                RuntimeOrigin::signed(attacker_cold),
                attacker_hot,
                dest,
                None,
                false,
            ),
            Error::<Test>::ColdkeySwapDestinationReserved
        );
        assert!(!SubtensorModule::hotkey_account_exists(&dest));
    });
}

/// Destination can free an exclusive reservation so merges into an existing
/// coldkey cannot be squat-blocked forever by a third-party announce.
#[test]
fn test_destination_can_release_coldkey_swap_reservation() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let victim_dest = U256::from(2);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();

        add_balance_to_coldkey_account(&attacker, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(attacker),
            victim_dest,
        ));
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(victim_dest),
            Some(attacker)
        );

        // Only the reserved destination may release.
        assert_noop!(
            SubtensorModule::release_own_coldkey_swap_destination_reservation(
                RuntimeOrigin::signed(attacker)
            ),
            Error::<Test>::ColdkeySwapDestinationReservationNotFound
        );

        assert_ok!(
            SubtensorModule::release_own_coldkey_swap_destination_reservation(
                RuntimeOrigin::signed(victim_dest)
            )
        );
        assert!(!ColdkeySwapDestinationReservations::<Test>::contains_key(
            victim_dest
        ));
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(attacker).expect("announced");
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &attacker,
                &victim_dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::CurrentDestination),
            "release must revoke balance-ignore privilege and use current eligibility"
        );
        // Announcement remains for the attacker, but exclusivity is gone so
        // another party can announce into this destination again.
        assert!(ColdkeySwapAnnouncements::<Test>::contains_key(attacker));
        let other = U256::from(7);
        add_balance_to_coldkey_account(&other, swap_cost + ed);
        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(other),
            victim_dest,
        ));
        assert_eq!(
            ColdkeySwapDestinationReservations::<Test>::get(victim_dest),
            Some(other)
        );
    });
}

/// A blank-destination snapshot may ignore unsolicited balance dust only. Once
/// the destination releases its reservation and becomes an active coldkey, the
/// stale snapshot cannot install the source's auto-stake policy.
#[test]
fn test_v2_policy_snapshot_does_not_bypass_destination_state_after_release() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let victim_dest = U256::from(2);
        let source_hotkey = U256::from(10);
        let source_auto_stake = U256::from(20);
        let victim_hotkey = U256::from(30);

        let netuid = add_dynamic_network(&source_hotkey, &attacker);
        AutoStakeDestination::<Test>::insert(attacker, netuid, source_auto_stake);
        AutoStakeDestinationColdkeys::<Test>::insert(source_auto_stake, netuid, vec![attacker]);

        announce_coldkey_swap(attacker, victim_dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(attacker).expect("announced");
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &attacker,
                &victim_dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::IgnoreDestinationBalance)
        );

        assert_ok!(
            SubtensorModule::release_own_coldkey_swap_destination_reservation(
                RuntimeOrigin::signed(victim_dest)
            )
        );
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &victim_dest,
            &victim_hotkey,
        ));
        assert!(SubtensorModule::coldkey_already_in_use(&victim_dest));
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &attacker,
                &victim_dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::CurrentDestination)
        );

        run_to_block(when);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(attacker),
            victim_dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(attacker, netuid).is_none());
        assert!(AutoStakeDestination::<Test>::get(victim_dest, netuid).is_none());
        assert!(
            !AutoStakeDestinationColdkeys::<Test>::get(source_auto_stake, netuid)
                .contains(&victim_dest)
        );
    });
}

/// Releasing the reservation removes the right to ignore dust, but a destination
/// that is still blank at execution remains a rename target and receives the
/// source policy.
#[test]
fn test_v2_policy_snapshot_preserves_blank_rename_after_release() {
    new_test_ext(1).execute_with(|| {
        let source = U256::from(1);
        let dest = U256::from(2);
        let source_hotkey = U256::from(10);
        let source_auto_stake = U256::from(20);

        let netuid = add_dynamic_network(&source_hotkey, &source);
        AutoStakeDestination::<Test>::insert(source, netuid, source_auto_stake);
        AutoStakeDestinationColdkeys::<Test>::insert(source_auto_stake, netuid, vec![source]);

        announce_coldkey_swap(source, dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(source).expect("announced");
        assert_ok!(
            SubtensorModule::release_own_coldkey_swap_destination_reservation(
                RuntimeOrigin::signed(dest)
            )
        );
        assert!(!SubtensorModule::coldkey_rejects_unsolicited_policy_import(
            &dest
        ));
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &source,
                &dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::CurrentDestination)
        );

        run_to_block(when);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(source),
            dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(source, netuid).is_none());
        assert_eq!(
            AutoStakeDestination::<Test>::get(dest, netuid),
            Some(source_auto_stake)
        );
        assert!(
            AutoStakeDestinationColdkeys::<Test>::get(source_auto_stake, netuid).contains(&dest)
        );
    });
}

/// Even while the original reservation is live, its balance-ignore privilege
/// must not bypass destination state acquired after announcement.
#[test]
fn test_v2_policy_snapshot_rechecks_non_balance_state_while_reserved() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let victim_dest = U256::from(2);
        let source_hotkey = U256::from(10);
        let source_auto_stake = U256::from(20);
        let victim_hotkey = U256::from(30);

        let netuid = add_dynamic_network(&source_hotkey, &attacker);
        AutoStakeDestination::<Test>::insert(attacker, netuid, source_auto_stake);
        AutoStakeDestinationColdkeys::<Test>::insert(source_auto_stake, netuid, vec![attacker]);

        announce_coldkey_swap(attacker, victim_dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(attacker).expect("announced");

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &victim_dest,
            &victim_hotkey,
        ));
        assert!(SubtensorModule::coldkey_already_in_use(&victim_dest));
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &attacker,
                &victim_dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::IgnoreDestinationBalance),
            "the reservation remains live; execution must apply the current-state gate"
        );

        run_to_block(when);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(attacker),
            victim_dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(attacker, netuid).is_none());
        assert!(AutoStakeDestination::<Test>::get(victim_dest, netuid).is_none());
        assert!(
            !AutoStakeDestinationColdkeys::<Test>::get(source_auto_stake, netuid)
                .contains(&victim_dest)
        );
    });
}

/// Expiry revokes the same balance-ignore privilege even if the stale V2
/// announcement remains executable. Later destination activity must therefore
/// keep its unset/default auto-stake route.
#[test]
fn test_v2_policy_snapshot_does_not_bypass_destination_state_after_expiry() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let victim_dest = U256::from(2);
        let source_hotkey = U256::from(10);
        let source_auto_stake = U256::from(20);
        let victim_hotkey = U256::from(30);

        let netuid = add_dynamic_network(&source_hotkey, &attacker);
        AutoStakeDestination::<Test>::insert(attacker, netuid, source_auto_stake);
        AutoStakeDestinationColdkeys::<Test>::insert(source_auto_stake, netuid, vec![attacker]);

        announce_coldkey_swap(attacker, victim_dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(attacker).expect("announced");
        let ttl = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(when + ttl + 1);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &victim_dest,
            &victim_hotkey,
        ));
        assert!(SubtensorModule::coldkey_already_in_use(&victim_dest));
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &attacker,
                &victim_dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::CurrentDestination)
        );

        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(attacker),
            victim_dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(attacker, netuid).is_none());
        assert!(AutoStakeDestination::<Test>::get(victim_dest, netuid).is_none());
        assert!(
            !AutoStakeDestinationColdkeys::<Test>::get(source_auto_stake, netuid)
                .contains(&victim_dest)
        );
    });
}

/// Expiry likewise falls back to current eligibility instead of treating the
/// missing reservation as evidence that an otherwise blank destination is used.
#[test]
fn test_v2_policy_snapshot_preserves_blank_rename_after_expiry() {
    new_test_ext(1).execute_with(|| {
        let source = U256::from(1);
        let dest = U256::from(2);
        let source_hotkey = U256::from(10);
        let source_auto_stake = U256::from(20);

        let netuid = add_dynamic_network(&source_hotkey, &source);
        AutoStakeDestination::<Test>::insert(source, netuid, source_auto_stake);
        AutoStakeDestinationColdkeys::<Test>::insert(source_auto_stake, netuid, vec![source]);

        announce_coldkey_swap(source, dest);
        let (when, announcement_hash) =
            ColdkeySwapAnnouncements::<Test>::get(source).expect("announced");
        let ttl = ColdkeySwapReannouncementDelay::<Test>::get();
        run_to_block(when + ttl + 1);

        assert!(!SubtensorModule::coldkey_rejects_unsolicited_policy_import(
            &dest
        ));
        assert_eq!(
            SubtensorModule::coldkey_swap_v2_policy_snapshot(
                &source,
                &dest,
                &announcement_hash,
                when,
            ),
            Some(ColdkeySwapPolicyImportMode::CurrentDestination)
        );
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(source),
            dest,
        ));

        assert!(AutoStakeDestination::<Test>::get(source, netuid).is_none());
        assert_eq!(
            AutoStakeDestination::<Test>::get(dest, netuid),
            Some(source_auto_stake)
        );
        assert!(
            AutoStakeDestinationColdkeys::<Test>::get(source_auto_stake, netuid).contains(&dest)
        );
    });
}

/// Abandoned reservations expire after when + reannouncement delay so unused
/// destinations cannot be squat forever.
#[test]
fn test_coldkey_swap_destination_reservation_expires() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let other = U256::from(3);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();

        add_balance_to_coldkey_account(&who, swap_cost + ed);
        add_balance_to_coldkey_account(&other, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(who),
            dest,
        ));
        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(who).unwrap();
        let ttl = ColdkeySwapReannouncementDelay::<Test>::get();

        // Still live just at the TTL boundary.
        run_to_block(when + ttl);
        assert_noop!(
            SubtensorModule::create_account_if_non_existent(&other, &dest),
            Error::<Test>::ColdkeySwapDestinationReserved
        );

        // One block past TTL: purge on access, registration succeeds.
        run_to_block(when + ttl + 1);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &other, &dest
        ));
        assert!(!ColdkeySwapDestinationReservations::<Test>::contains_key(
            dest
        ));
    });
}

/// Destination dust-lock rows contribute independent weight even when the
/// source has no stake positions (Auditor fid:7bea5c54).
#[test]
fn test_swap_weight_grows_with_destination_exhausted_locks() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let dest_hot_a = U256::from(10);
        let dest_hot_b = U256::from(11);

        // Empty source (no stake positions); destination accepts locks.
        add_balance_to_coldkey_account(&old, ExistentialDeposit::get());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot_a
        ));
        SubtensorModule::set_accept_locked_alpha(&dest, true);

        let light_work =
            SubtensorModule::do_swap_coldkey_tracked(&old, &dest).expect("light merge");
        assert_eq!(light_work.dest_locks, 0);
        let light = SubtensorModule::swap_coldkey_actual_weight(light_work);

        // Re-fund a fresh empty source for the heavy case.
        let old2 = U256::from(3);
        add_balance_to_coldkey_account(&old2, ExistentialDeposit::get());
        // Seed exhausted dust rows on dest (insert_lock_state would drop is_zero).
        let now = SubtensorModule::get_current_block_as_u64();
        for (hot, i) in [(dest_hot_a, 0u64), (dest_hot_b, 1u64)] {
            let netuid = NetUid::from(1 + i as u16);
            Lock::<Test>::insert(
                (dest, netuid, hot),
                LockState {
                    locked_mass: AlphaBalance::ZERO,
                    conviction: U64F64::from_num(0),
                    last_update: now,
                },
            );
            LockingColdkeys::<Test>::insert((netuid, hot, dest), ());
        }

        let heavy_work =
            SubtensorModule::do_swap_coldkey_tracked(&old2, &dest).expect("heavy merge");
        assert_eq!(heavy_work.dest_locks, 2);
        let heavy = SubtensorModule::swap_coldkey_actual_weight(heavy_work);
        // Same source work shape (no positions); heavier dest lock cleanup
        // must report strictly more weight.
        assert!(
            heavy.ref_time() > light.ref_time(),
            "dest lock cleanup must be charged independently of source positions: light={}, heavy={}",
            light.ref_time(),
            heavy.ref_time()
        );
        // Full path accounting: 16 reads + 6 writes per dest_lock (Skeptic
        // fid:bcf7b5a5 — admission + re-check + purge + aggregate).
        let per_row = <Test as frame_system::Config>::DbWeight::get().reads_writes(16, 6);
        assert_eq!(
            heavy.ref_time().saturating_sub(light.ref_time()),
            per_row.ref_time().saturating_mul(2),
            "dest_lock weight must charge the full cleanup path per row"
        );
    });
}

/// Oversized global registration queue fails closed even when the source owns
/// zero pending rows (Auditor fid:cf0fb7c5).
#[test]
fn test_swap_rejects_oversized_registration_queue_dominated_by_unrelated() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let amount = TaoBalance::from(1_000_u64);

        let mut queue = Vec::new();
        let over = MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN.saturating_add(1);
        for i in 0..over {
            queue.push(crate::subnets::subnet::NetworkRegistrationInfo {
                coldkey: U256::from(10_000 + u64::from(i)),
                hotkey: U256::from(20_000 + u64::from(i)),
                mechid: 0,
                identity: None,
                lock_amount: amount,
                median_subnet_alpha_price: Default::default(),
                registration_block: 1,
                lock_id: i,
            });
        }
        NetworkRegistrationQueue::<Test>::put(queue);

        let err = SubtensorModule::do_swap_coldkey_tracked(&old, &dest).expect_err("too heavy");
        let (done, dispatch_err) = err;
        assert_eq!(dispatch_err, Error::<Test>::ColdkeySwapTooHeavy.into());
        // Failure charges the recorded queue-scan axis (not a bare precheck).
        let bare = <Test as frame_system::Config>::DbWeight::get().reads(4);
        assert!(
            done.ref_time() > bare.ref_time(),
            "queue-scan failure must charge scanned rows: done={}, bare={}",
            done.ref_time(),
            bare.ref_time()
        );
    });
}

/// Oversized queue is rejected from the length prefix alone — admission must
/// not SCALE-decode the Vec body (Auditor fid:185b04ac).
#[test]
fn test_swap_rejects_oversized_registration_queue_via_decode_len_without_body() {
    new_test_ext(1).execute_with(|| {
        use codec::{Compact, Encode};
        use frame_support::storage::unhashed;

        let old = U256::from(1);
        let dest = U256::from(2);
        // Compact length ≫ cap, but no element body. `get()` would fail to
        // decode; `decode_len` alone must drive the TooHeavy refusal.
        let huge = MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN.saturating_mul(40);
        assert!(huge > MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN.saturating_add(1));
        unhashed::put_raw(
            NetworkRegistrationQueue::<Test>::hashed_key().as_ref(),
            &Compact(huge).encode(),
        );
        assert_eq!(
            NetworkRegistrationQueue::<Test>::decode_len(),
            Some(huge as usize)
        );

        let err = SubtensorModule::do_swap_coldkey_tracked(&old, &dest).expect_err("too heavy");
        let (done, dispatch_err) = err;
        assert_eq!(dispatch_err, Error::<Test>::ColdkeySwapTooHeavy.into());
        let bare = <Test as frame_system::Config>::DbWeight::get().reads(4);
        assert!(
            done.ref_time() > bare.ref_time(),
            "decode_len refusal must still charge the queue-scan sentinel: done={}, bare={}",
            done.ref_time(),
            bare.ref_time()
        );
    });
}

#[test]
fn test_swap_failure_weight_grows_with_destination_lock_scan() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest_light = U256::from(2);
        let dest_heavy = U256::from(3);
        let hot = U256::from(4);

        assert_ok!(SubtensorModule::create_account_if_non_existent(&old, &hot));
        // Both destinations reject staking growth so admission fails after the
        // dest-lock scan (source introduces a new hotkey).
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest_light,
            &U256::from(12)
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest_heavy,
            &U256::from(13)
        ));
        for dest in [dest_light, dest_heavy] {
            assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
                RuntimeOrigin::signed(dest),
                true,
            ));
        }

        let now = SubtensorModule::get_current_block_as_u64();
        // Heavy dest: several exhausted dust rows scanned before opt-out refusal.
        for i in 0..8u64 {
            let netuid = NetUid::from(10 + i as u16);
            let dhot = U256::from(100 + i);
            Lock::<Test>::insert(
                (dest_heavy, netuid, dhot),
                LockState {
                    locked_mass: AlphaBalance::ZERO,
                    conviction: U64F64::from_num(0),
                    last_update: now,
                },
            );
        }

        let (light_done, _) =
            SubtensorModule::do_swap_coldkey_tracked(&old, &dest_light).expect_err("opt-out");
        let (heavy_done, _) =
            SubtensorModule::do_swap_coldkey_tracked(&old, &dest_heavy).expect_err("opt-out");
        assert!(
            heavy_done.ref_time() > light_done.ref_time(),
            "failure weight must grow with dest lock rows scanned: light={}, heavy={}",
            light_done.ref_time(),
            heavy_done.ref_time()
        );
        // Opt-out fails after bound admission (`admitted=true`), so failure
        // weight uses the full dest-lock cleanup schedule (16r/6w per row).
        let per_row = <Test as frame_system::Config>::DbWeight::get().reads_writes(16, 6);
        assert_eq!(
            heavy_done.ref_time().saturating_sub(light_done.ref_time()),
            per_row.ref_time().saturating_mul(8),
            "admitted failure must charge full dest-lock cleanup path per row"
        );
    });
}

/// Pending network-registration escrow follows the queue `lock_id` only —
/// never a global escrow map scan (Auditor: unbounded registration cost).
#[test]
fn test_merge_swap_rewrites_pending_registration_via_queue_lock_id() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        let lock_id = 7u32;
        let amount = TaoBalance::from(1_000_000_u64);

        NetworkRegistrationQueue::<Test>::put(vec![
            crate::subnets::subnet::NetworkRegistrationInfo {
                coldkey: old,
                hotkey,
                mechid: 0,
                identity: None,
                lock_amount: amount,
                median_subnet_alpha_price: Default::default(),
                registration_block: 1,
                lock_id,
            },
        ]);
        NetworkRegistrationEscrow::<Test>::insert(lock_id, (old, amount));
        // Unrelated escrow row must stay untouched (proves we do not iter-all).
        NetworkRegistrationEscrow::<Test>::insert(99u32, (U256::from(9), amount));

        assert_ok!(SubtensorModule::do_swap_coldkey(&old, &dest));

        let queue = NetworkRegistrationQueue::<Test>::get();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].coldkey, dest);
        assert_eq!(
            NetworkRegistrationEscrow::<Test>::get(lock_id),
            Some((dest, amount))
        );
        assert_eq!(
            NetworkRegistrationEscrow::<Test>::get(99u32),
            Some((U256::from(9), amount))
        );
    });
}

/// Opt-out failures after admission still charge the work already scanned
/// (Auditor: do not refund admission on AccountRejects*).
#[test]
fn test_swap_failure_charges_admission_work_on_dest_opt_out() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let source_hot = U256::from(3);
        let dest_hot = U256::from(4);

        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &old,
            &source_hot
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &dest, &dest_hot
        ));
        assert_ok!(SubtensorModule::set_reject_staking_hotkey_growth(
            RuntimeOrigin::signed(dest),
            true,
        ));

        let err = SubtensorModule::do_swap_coldkey_tracked(&old, &dest).expect_err("opt-out");
        let (done, dispatch_err) = err;
        assert_eq!(
            dispatch_err,
            Error::<Test>::AccountRejectsStakingHotkeyGrowth.into()
        );
        // Must charge more than the bare precheck (4 reads) — admission scanned
        // both sides before the opt-out refusal.
        let bare = frame_support::weights::Weight::from_parts(0, 0)
            .saturating_add(<Test as frame_system::Config>::DbWeight::get().reads(4));
        assert!(done.all_gte(bare));
        assert!(done.ref_time() > bare.ref_time());
    });
}

/// Legacy hash announce preserves commitment secrecy (no cleartext dest /
/// reservation); V2 remains the validated+reserved path.

#[test]
fn test_swap_coldkey_announced_declared_weight_fits_block() {
    new_test_ext(1).execute_with(|| {
        let declared = SubtensorModule::swap_coldkey_announced_declared_weight();
        let max = <Test as frame_system::Config>::BlockWeights::get()
            .get(frame_support::dispatch::DispatchClass::Normal)
            .max_extrinsic
            .expect("normal max_extrinsic");
        assert!(
            declared.all_lte(max),
            "declared={:?} exceeds max_extrinsic={:?}",
            declared,
            max
        );
        // Full dest-lock cleanup accounting (16r/6w × MAX_COLDKEY_SWAP_DEST_LOCKS)
        // must still leave headroom under the normal-class extrinsic cap.
        let dest_cap = crate::MAX_COLDKEY_SWAP_DEST_LOCKS;
        let dest_charge = <Test as frame_system::Config>::DbWeight::get().reads_writes(
            u64::from(dest_cap).saturating_mul(16),
            u64::from(dest_cap).saturating_mul(6),
        );
        assert!(
            dest_charge.ref_time() > 0 && declared.ref_time() > dest_charge.ref_time(),
            "declared envelope must include the full dest-lock cleanup charge"
        );
    });
}

#[test]
fn test_e2e_happy_path_subnet_owner_swap_to_fresh_destination() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let dest = U256::from(2);
        let hotkey = U256::from(3);
        let netuid = add_dynamic_network(&hotkey, &old);
        // Mimic start_call + stake like the zombienet happy path.
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 200;
        add_balance_to_coldkey_account(&old, (stake_amount * 2).into());
        assert_ok!(SubtensorModule::stake_into_subnet(
            &hotkey,
            &old,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        ));
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &old, netuid)
                > AlphaBalance::ZERO
        );

        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&old, swap_cost + ExistentialDeposit::get());
        assert_ok!(SubtensorModule::announce_coldkey_swap_v2(
            RuntimeOrigin::signed(old),
            dest,
        ));
        let (when, _) = ColdkeySwapAnnouncements::<Test>::get(old).expect("announced");
        run_to_block(when + 1);
        assert_ok!(SubtensorModule::swap_coldkey_announced(
            RuntimeOrigin::signed(old),
            dest,
        ));
        assert_eq!(SubnetOwner::<Test>::get(netuid), dest);
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &dest, netuid)
                > AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_legacy_hash_announce_does_not_reserve_destination() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let dest = U256::from(2);
        let hash = <Test as frame_system::Config>::Hashing::hash_of(&dest);
        let ed = ExistentialDeposit::get();
        let swap_cost = SubtensorModule::get_key_swap_cost();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            hash,
        ));
        assert!(ColdkeySwapReservedDestination::<Test>::get(who).is_none());
        assert!(!ColdkeySwapDestinationReservations::<Test>::contains_key(
            dest
        ));
        // Destination can still be claimed as a hotkey under the legacy path.
        let other = U256::from(3);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &other, &dest
        ));
    });
}
