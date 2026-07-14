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
use sp_runtime::traits::Hash;
use sp_runtime::traits::{DispatchInfoOf, DispatchTransaction, TransactionExtension};
use sp_runtime::{DispatchError, traits::TxBaseImplication};
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{
    AlphaBalance, CustomTransactionError, SubnetInfo, TaoBalance, Token,
};
use subtensor_swap_interface::{SwapEngine, SwapHandler};

use super::mock;
use super::mock::*;
use crate::extensions::SubtensorTransactionExtension;
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
            new_coldkey_2_hash,
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
            new_coldkey_2_hash,
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
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::signed(who), new_coldkey_2_hash,),
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

        assert_noop!(
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

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(who), other_coldkey),
            Error::<Test>::AnnouncedColdkeyHashDoesNotMatch
        );
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

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                new_coldkey
            ),
            Error::<Test>::ColdkeySwapTooEarly
        );

        // Now + delay - 1 case
        run_to_block(now + delay - 1);

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                new_coldkey
            ),
            Error::<Test>::ColdkeySwapTooEarly
        );
    })
}

#[test]
fn test_swap_coldkey_announced_with_already_associated_coldkey_fails() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let new_coldkey = U256::from(2);
        let new_coldkey_hash = <Test as frame_system::Config>::Hashing::hash_of(&new_coldkey);
        let hotkey = U256::from(3);

        let swap_cost = SubtensorModule::get_key_swap_cost();
        let ed = ExistentialDeposit::get();
        add_balance_to_coldkey_account(&who, swap_cost + ed);

        assert_ok!(SubtensorModule::announce_coldkey_swap(
            RuntimeOrigin::signed(who),
            new_coldkey_hash,
        ));

        let now = System::block_number();
        let delay = ColdkeySwapAnnouncementDelay::<Test>::get() + 1;
        run_to_block(now + delay);

        SubtensorModule::create_account_if_non_existent(&new_coldkey, &hotkey);

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                new_coldkey
            ),
            Error::<Test>::ColdKeyAlreadyAssociated
        );
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

        assert_noop!(
            SubtensorModule::swap_coldkey_announced(
                <Test as frame_system::Config>::RuntimeOrigin::signed(who),
                hotkey
            ),
            Error::<Test>::NewColdKeyIsHotkey
        );
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
        assert_noop!(
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

        assert_noop!(
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

        // Identity is preserved
        assert_eq!(IdentitiesV2::<Test>::get(who), Some(old_identity));
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
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::signed(who), new_coldkey_hash),
            Error::<Test>::NotEnoughBalanceToPaySwapColdKey
        );

        // Needs to preserve ED
        let balance = SubtensorModule::get_key_swap_cost() + ExistentialDeposit::get() - 1.into();
        add_balance_to_coldkey_account(&who, balance);
        assert_noop!(
            SubtensorModule::announce_coldkey_swap(RuntimeOrigin::signed(who), new_coldkey_hash),
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

        Delegates::<Test>::insert(hotkey1, u16::MAX / 10);
        Delegates::<Test>::insert(hotkey2, u16::MAX / 10);
        Delegates::<Test>::insert(hotkey3, u16::MAX / 10);
        Delegates::<Test>::insert(delegate1, u16::MAX / 10);
        Delegates::<Test>::insert(delegate2, u16::MAX / 10);
        Delegates::<Test>::insert(delegate3, u16::MAX / 10);

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

        assert_ok!(SubtensorModule::root_register(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            delegate
        )); // register on root
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

    assert_ok!(SubtensorModule::announce_coldkey_swap(
        RuntimeOrigin::signed(who),
        coldkey_hash_of(new_coldkey),
    ));
}

fn dispute_coldkey_swap(who: U256) {
    assert_ok!(SubtensorModule::dispute_coldkey_swap(
        RuntimeOrigin::signed(who),
    ));
}

fn ref_count(coldkey: &U256) -> u32 {
    RootRegisteredHotkeyCount::<Test>::get(coldkey)
}

#[test]
fn swap_coldkey_transfers_ref_count_for_root_registered_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let old_coldkey = U256::from(10);
        let new_coldkey = U256::from(20);
        let h1 = U256::from(11);
        let h2 = U256::from(12);
        let h_not_root = U256::from(13);

        // Two root-registered hotkeys plus one non-root-registered hotkey,
        // all owned by old_coldkey.
        root_register_with_stake(&old_coldkey, &h1, alpha);
        root_register_with_stake(&old_coldkey, &h2, alpha);
        register_ok_neuron(alpha, h_not_root, old_coldkey, 0);

        assert_eq!(ref_count(&old_coldkey), 2);
        assert_eq!(ref_count(&new_coldkey), 0);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        assert_eq!(ref_count(&old_coldkey), 0);
        assert_eq!(ref_count(&new_coldkey), 2);
        assert!(!SubtensorModule::coldkey_has_root_hotkey(&old_coldkey));
        assert!(SubtensorModule::coldkey_has_root_hotkey(&new_coldkey));
    });
}

#[test]
fn swap_coldkey_with_no_root_hotkeys_is_noop_for_ref_count() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let old_coldkey = U256::from(10);
        let new_coldkey = U256::from(20);
        let hot = U256::from(11);

        // Hotkey registered on alpha only, not on root.
        register_ok_neuron(alpha, hot, old_coldkey, 0);
        assert_eq!(ref_count(&old_coldkey), 0);

        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        assert_eq!(ref_count(&old_coldkey), 0);
        assert_eq!(ref_count(&new_coldkey), 0);
    });
}

#[test]
fn swap_coldkey_fires_removed_for_source_and_added_for_target() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let from = U256::from(10);
        let to = U256::from(99);
        root_register_with_stake(&from, &U256::from(11), alpha);
        root_register_with_stake(&from, &U256::from(12), alpha);
        let _ = take_root_registration_log();

        assert_ok!(SubtensorModule::do_swap_coldkey(&from, &to));

        let log = take_root_registration_log();
        assert!(log.contains(&RootRegistrationChange::Removed(from)));
        assert!(log.contains(&RootRegistrationChange::Added(to)));
    });
}
