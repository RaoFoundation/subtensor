//! The lending hooks of `pallet_subtensor`: every stake path refuses a lending custody account
//! and the pallet hotkey, a coldkey swap re-keys the holder's positions, and the two hook
//! points whose economics are on hold leave dissolution and the conviction bar as they were.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{
    assert_noop, assert_ok, dispatch::DispatchErrorWithPostInfo, traits::Hooks,
    weights::WeightMeter,
};
use pallet_lending::{
    Due, InterestRetry, InterestTotals, LendingInterface, LendingPoolInterface, PositionCount,
    Positions, SyntheticOutstanding, types::Side,
};
use sp_core::U256;
use sp_runtime::{DispatchError, PerU16, StateVersion};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

use super::lending_adapter::{RAO, alice, bob, custody, lending_subnet, short};
use super::mock::*;
use crate::staking::lock::LockState;
use crate::subnets::dissolution::{DissolveCleanupPhase, DissolveCleanupStatus};
use crate::*;

/// A subnet with lending on and Alice's short open: its id, the short's custody account and
/// the pallet hotkey.
fn with_short() -> (NetUid, U256, U256) {
    let netuid = lending_subnet(50);
    assert_ok!(Lending::open_short(
        RuntimeOrigin::signed(alice()),
        netuid,
        2_000 * RAO,
        5_000 * RAO,
        0
    ));
    let hotkey = Lending::pallet_hotkey_account();
    assert_eq!(<Test as Config>::Lending::pallet_hotkey(), Some(hotkey));
    (netuid, custody(alice(), netuid), hotkey)
}

/// `call` fails with `error` and changes no storage.
fn assert_refused<T: core::fmt::Debug, E: Into<DispatchErrorWithPostInfo>>(
    call: impl FnOnce() -> Result<T, E>,
    error: impl Into<DispatchError>,
) {
    let root = sp_io::storage::root(StateVersion::V1);
    let result = call().map(|_| ()).map_err(|e| e.into().error);
    assert_eq!(result, Err(error.into()));
    assert_eq!(root, sp_io::storage::root(StateVersion::V1));
}

const CUSTODY: Error<Test> = Error::<Test>::LendingCustodyAccount;
const PALLET_HOTKEY: Error<Test> = Error::<Test>::LendingPalletHotkey;

fn signed(who: U256) -> RuntimeOrigin {
    RuntimeOrigin::signed(who)
}

#[test]
fn custody_and_pallet_accounts_are_recognised_without_storage() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        for account in [custody, hotkey, Lending::pallet_account()] {
            assert!(<Test as Config>::Lending::is_lending_custody(&account));
        }
        for account in [
            alice(),
            bob(),
            U256::from(1),
            U256::from(MOCK_BLOCK_BUILDER),
        ] {
            assert!(!<Test as Config>::Lending::is_lending_custody(&account));
        }
        assert!(!<Test as Config>::Lending::is_lending_custody(
            &SubtensorModule::get_subnet_account_id(netuid).unwrap()
        ));
    });
}

#[test]
fn add_stake_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, _) = with_short();
        let amount = TaoBalance::from(100 * RAO);
        assert_refused(
            || SubtensorModule::add_stake(signed(custody), U256::from(1), netuid, amount),
            CUSTODY,
        );
        assert_refused(
            || {
                SubtensorModule::add_stake_limit(
                    signed(custody),
                    U256::from(1),
                    netuid,
                    amount,
                    TaoBalance::from(u64::MAX),
                    true,
                )
            },
            CUSTODY,
        );
    });
}

#[test]
fn add_stake_refuses_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::add_stake(signed(bob()), hotkey, netuid, (100 * RAO).into()),
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn remove_stake_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &custody,
            netuid,
            AlphaBalance::from(10 * RAO),
        );
        assert_refused(
            || SubtensorModule::remove_stake(signed(custody), hotkey, netuid, RAO.into()),
            CUSTODY,
        );
        assert_refused(
            || {
                SubtensorModule::remove_stake_limit(
                    signed(custody),
                    hotkey,
                    netuid,
                    RAO.into(),
                    TaoBalance::from(0u64),
                    true,
                )
            },
            CUSTODY,
        );
    });
}

#[test]
fn unstake_all_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &custody,
            netuid,
            AlphaBalance::from(10 * RAO),
        );
        assert_refused(
            || SubtensorModule::unstake_all(signed(custody), hotkey),
            CUSTODY,
        );
    });
}

#[test]
fn unstake_all_alpha_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &custody,
            netuid,
            AlphaBalance::from(10 * RAO),
        );
        assert_refused(
            || SubtensorModule::unstake_all_alpha(signed(custody), hotkey),
            CUSTODY,
        );
    });
}

#[test]
fn move_stake_refuses_custody_and_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        let other = U256::from(1);
        assert_refused(
            || {
                SubtensorModule::move_stake(
                    signed(custody),
                    hotkey,
                    other,
                    netuid,
                    netuid,
                    RAO.into(),
                )
            },
            CUSTODY,
        );
        // Nobody moves stake onto the pallet hotkey, or off it.
        assert_refused(
            || {
                SubtensorModule::move_stake(
                    signed(bob()),
                    other,
                    hotkey,
                    netuid,
                    netuid,
                    RAO.into(),
                )
            },
            PALLET_HOTKEY,
        );
        assert_refused(
            || {
                SubtensorModule::move_stake(
                    signed(bob()),
                    hotkey,
                    other,
                    netuid,
                    netuid,
                    RAO.into(),
                )
            },
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn transfer_stake_refuses_custody_on_either_end() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, _) = with_short();
        let hotkey = U256::from(1);
        assert_refused(
            || {
                SubtensorModule::transfer_stake(
                    signed(custody),
                    bob(),
                    hotkey,
                    netuid,
                    netuid,
                    RAO.into(),
                )
            },
            CUSTODY,
        );
        assert_refused(
            || {
                SubtensorModule::transfer_stake(
                    signed(bob()),
                    custody,
                    hotkey,
                    netuid,
                    netuid,
                    RAO.into(),
                )
            },
            CUSTODY,
        );
    });
}

#[test]
fn swap_stake_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        let other = add_dynamic_network(&U256::from(11), &U256::from(12));
        assert_refused(
            || SubtensorModule::swap_stake(signed(custody), hotkey, netuid, other, RAO.into()),
            CUSTODY,
        );
    });
}

#[test]
fn recycle_alpha_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::recycle_alpha(signed(custody), hotkey, RAO.into(), netuid),
            CUSTODY,
        );
    });
}

#[test]
fn burn_alpha_refuses_a_custody_coldkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::burn_alpha(signed(custody), hotkey, RAO.into(), netuid),
            CUSTODY,
        );
    });
}

#[test]
fn lock_stake_refuses_custody_and_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::lock_stake(signed(custody), U256::from(1), netuid, RAO.into()),
            CUSTODY,
        );
        assert_refused(
            || SubtensorModule::lock_stake(signed(bob()), hotkey, netuid, RAO.into()),
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn move_lock_refuses_custody_and_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::move_lock(signed(custody), U256::from(1), netuid),
            CUSTODY,
        );
        assert_refused(
            || SubtensorModule::move_lock(signed(bob()), hotkey, netuid),
            PALLET_HOTKEY,
        );
    });
}

/// The chokepoint under every unstake, recycle and burn: alpha lending keeps locked is not
/// available, whoever asks.
#[test]
fn locked_custody_alpha_is_not_available_to_unstake() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        let held = AlphaBalance::from(10 * RAO);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &custody, netuid, held);
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::held_alpha(&custody, netuid),
            10 * RAO
        );
        assert_eq!(
            <Test as Config>::Lending::locked_alpha(&custody, netuid),
            10 * RAO
        );
        assert_eq!(
            SubtensorModule::ensure_available_to_unstake(&custody, netuid, 1u64.into()),
            Err(Error::<Test>::LendingLockedAlpha)
        );
        assert_eq!(
            SubtensorModule::ensure_alpha_not_lending_locked(&custody, netuid, held),
            Err(Error::<Test>::LendingLockedAlpha)
        );
        // An ordinary staker has no locked alpha and is not affected.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(&U256::from(1), &bob(), netuid, held);
        assert_eq!(<Test as Config>::Lending::locked_alpha(&bob(), netuid), 0);
        assert_ok!(SubtensorModule::ensure_available_to_unstake(
            &bob(),
            netuid,
            held
        ));
    });
}

#[test]
fn children_refuse_the_pallet_hotkey_as_parent() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        assert_refused(
            || {
                SubtensorModule::do_schedule_children(
                    signed(Lending::pallet_account()),
                    hotkey,
                    netuid,
                    vec![(u64::MAX, U256::from(1))],
                )
            },
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn children_refuse_the_pallet_hotkey_as_child() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        assert_refused(
            || {
                SubtensorModule::do_schedule_children(
                    signed(U256::from(2)),
                    U256::from(1),
                    netuid,
                    vec![(u64::MAX / 2, U256::from(77)), (u64::MAX / 2, hotkey)],
                )
            },
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn childkey_take_refuses_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        assert_refused(
            || {
                SubtensorModule::set_childkey_take(
                    signed(Lending::pallet_account()),
                    hotkey,
                    netuid,
                    PerU16::from_percent(5),
                )
            },
            PALLET_HOTKEY,
        );
    });
}

/// Registration, network registration and hotkey swaps all take ownership of a hotkey through
/// `create_account_if_non_existent` or `set_hotkey_owner`.
#[test]
fn hotkey_ownership_paths_refuse_lending_accounts() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody, hotkey) = with_short();
        for account in [hotkey, custody, Lending::pallet_account()] {
            assert_refused(
                || SubtensorModule::create_account_if_non_existent(&bob(), &account),
                PALLET_HOTKEY,
            );
            assert_refused(
                || SubtensorModule::set_hotkey_owner(&bob(), &account),
                PALLET_HOTKEY,
            );
        }
        // The pallet hotkey is still owned by the pallet account alone.
        assert_eq!(Owner::<Test>::get(hotkey), Lending::pallet_account());
        assert!(!Uids::<Test>::contains_key(netuid, hotkey));
    });
}

#[test]
fn registration_refuses_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        SubtensorModule::set_burn(netuid, TaoBalance::from(0u64));
        let result = SubtensorModule::burned_register(signed(bob()), netuid, hotkey);
        assert_eq!(
            result.map(|_| ()).map_err(|e| e.error),
            Err(PALLET_HOTKEY.into())
        );
        assert!(!Uids::<Test>::contains_key(netuid, hotkey));
        assert_eq!(Owner::<Test>::get(hotkey), Lending::pallet_account());
    });
}

#[test]
fn swap_hotkey_refuses_the_pallet_hotkey_as_new_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        // `lending_subnet` registers hotkey 1 under coldkey 2.
        let owner = U256::from(2);
        let old = U256::from(1);
        add_balance_to_coldkey_account(&owner, (10_000 * RAO).into());
        assert_refused(
            || SubtensorModule::do_swap_hotkey(signed(owner), &old, &hotkey, Some(netuid), false),
            PALLET_HOTKEY,
        );
        assert_refused(
            || SubtensorModule::do_swap_hotkey(signed(owner), &old, &hotkey, None, false),
            PALLET_HOTKEY,
        );
        assert_eq!(Owner::<Test>::get(hotkey), Lending::pallet_account());
        assert_eq!(Owner::<Test>::get(old), owner);
        assert!(!Uids::<Test>::contains_key(netuid, hotkey));
    });
}

#[test]
fn subnet_owner_hotkey_refuses_the_pallet_hotkey() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, hotkey) = with_short();
        assert_refused(
            || SubtensorModule::set_subnet_owner_hotkey(netuid, &hotkey),
            PALLET_HOTKEY,
        );
    });
}

#[test]
fn coldkey_swap_refuses_a_custody_account_on_either_end() {
    new_test_ext(1).execute_with(|| {
        let (_, custody, _) = with_short();
        System::set_block_number(System::block_number() + 1);
        let fresh = U256::from(4_242);
        let out = SubtensorModule::do_swap_coldkey(&custody, &fresh);
        assert_eq!(out.map(|_| ()), Err(CUSTODY.into()));
        let into = SubtensorModule::do_swap_coldkey(&bob(), &custody);
        assert_eq!(into.map(|_| ()), Err(CUSTODY.into()));
    });
}

/// The pallet hotkey is registered once, under the pallet account, and registering again
/// changes nothing.
#[test]
fn pallet_hotkey_registration_is_idempotent() {
    new_test_ext(1).execute_with(|| {
        let coldkey = Lending::pallet_account();
        let hotkey = Lending::pallet_hotkey_account();
        assert!(!Owner::<Test>::contains_key(hotkey));
        assert_ok!(Lending::initialize_custody());
        assert_eq!(Owner::<Test>::get(hotkey), coldkey);
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![hotkey]);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![hotkey]);
        let root = sp_io::storage::root(StateVersion::V1);
        assert_ok!(Lending::initialize_custody());
        assert_eq!(root, sp_io::storage::root(StateVersion::V1));
        // The runtime migration registers it too, and leaves one record.
        Lending::on_runtime_upgrade();
        assert_eq!(Owner::<Test>::get(hotkey), coldkey);
        assert_eq!(OwnedHotkeys::<Test>::get(coldkey), vec![hotkey]);
        assert_eq!(StakingHotkeys::<Test>::get(coldkey), vec![hotkey]);
    });
}

#[test]
fn pallet_hotkey_registration_fails_if_the_hotkey_has_another_owner() {
    new_test_ext(1).execute_with(|| {
        let hotkey = Lending::pallet_hotkey_account();
        Owner::<Test>::insert(hotkey, bob());
        assert_refused(
            Lending::initialize_custody,
            Error::<Test>::NonAssociatedColdKey,
        );
    });
}

/// A coldkey swap moves the position, its `Due` entry, its retry count and its custody to the
/// new key. The old key has nothing left, interest keeps its schedule, and the new key closes.
#[test]
fn coldkey_swap_moves_positions_and_the_interest_schedule() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody_account, _) = with_short();
        let carol = U256::from(1_003);
        let old = (alice(), netuid, Side::Short);
        let new = (carol, netuid, Side::Short);
        let pos = short(alice(), netuid).unwrap();
        InterestRetry::<Test>::insert(old, 2);
        let retries = InterestRetry::<Test>::get(old);
        let held = SubtensorModule::get_coldkey_balance(&custody_account);
        let alice_tao = SubtensorModule::get_coldkey_balance(&alice());

        // The swap is never refused for lending, not even in the block of the open.
        assert_ok!(SubtensorModule::swap_coldkey(
            RuntimeOrigin::root(),
            alice(),
            carol,
            TaoBalance::from(0u64)
        ));

        // The old key has nothing left.
        assert!(!Positions::<Test>::contains_key(old));
        assert!(!Due::<Test>::contains_key(pos.charge_at, old));
        assert!(!InterestRetry::<Test>::contains_key(old));
        assert_eq!(
            Lending::custody_account(&alice(), netuid, Side::Short),
            None
        );
        assert_eq!(SubtensorModule::get_coldkey_balance(&alice()), 0u64.into());
        // The new key has all of it, in the same custody account, and no funds moved.
        assert_eq!(short(carol, netuid), Some(pos));
        assert!(Due::<Test>::contains_key(pos.charge_at, new));
        assert_eq!(InterestRetry::<Test>::get(new), retries);
        assert_eq!(custody(carol, netuid), custody_account);
        assert_eq!(SubtensorModule::get_coldkey_balance(&custody_account), held);
        assert_eq!(SubtensorModule::get_coldkey_balance(&carol), alice_tao);
        assert_eq!(PositionCount::<Test>::get(netuid), 1);
        assert_ok!(Lending::do_try_state());

        // The old key opens nothing more in the block of the swap: it would derive the custody
        // account the moved position uses.
        add_balance_to_coldkey_account(&alice(), (10_000 * RAO).into());
        assert_noop!(
            Lending::open_short(signed(alice()), netuid, 1_000 * RAO, 3_000 * RAO, 0),
            pallet_lending::Error::<Test>::SwappedOutThisBlock
        );

        // Interest is charged on the original schedule.
        System::set_block_number(pos.charge_at);
        Lending::on_initialize(pos.charge_at);
        let charged = u64::try_from(InterestTotals::<Test>::get(netuid).charged).unwrap();
        assert!(charged > 0);
        let after = short(carol, netuid).unwrap();
        assert_eq!(after.collateral, pos.collateral - charged);
        assert!(after.charge_at > pos.charge_at);
        assert!(Due::<Test>::contains_key(after.charge_at, new));
        assert!(!InterestRetry::<Test>::contains_key(new));
        assert_ok!(Lending::do_try_state());

        // Alice can no longer close; Carol can, and gets the custody's remainder.
        assert!(Lending::close_short(signed(alice()), netuid, 2_000 * RAO, u64::MAX).is_err());
        assert_ok!(Lending::close_short(
            signed(carol),
            netuid,
            2_000 * RAO,
            u64::MAX
        ));
        assert!(short(carol, netuid).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&custody_account),
            0u64.into()
        );
        assert!(SubtensorModule::get_coldkey_balance(&carol) > alice_tao);
        assert_ok!(Lending::do_try_state());
    });
}

/// Admission: a holder of lending positions cannot swap onto a coldkey that holds lending
/// positions. The swap is refused whole when it is announced and when it executes, so no
/// position is ever left with the old key. A key without positions swaps onto a holder.
#[test]
fn coldkey_swap_refuses_a_destination_with_lending_positions() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody_account, _) = with_short();
        assert_ok!(Lending::open_short(
            signed(bob()),
            netuid,
            1_000 * RAO,
            3_000 * RAO,
            0
        ));
        System::set_block_number(System::block_number() + 1);
        let refused = || Error::<Test>::ColdkeySwapDestinationHasLendingPositions;

        // The admission check announce and execute share.
        assert_refused(
            || {
                SubtensorModule::ensure_coldkey_swap_destination(&alice(), &bob())
                    .map_err(|(_, error)| DispatchError::from(error))
            },
            refused(),
        );
        // Announce.
        assert_refused(
            || SubtensorModule::announce_coldkey_swap_v2(signed(alice()), bob()),
            refused(),
        );
        // Execute.
        assert_refused(
            || {
                SubtensorModule::swap_coldkey(
                    RuntimeOrigin::root(),
                    alice(),
                    bob(),
                    TaoBalance::from(0u64),
                )
            },
            refused(),
        );
        assert_eq!(custody(alice(), netuid), custody_account);

        // A coldkey without positions swaps onto a holder, and a holder onto one without.
        let dave = U256::from(1_004);
        assert_ok!(SubtensorModule::swap_coldkey(
            RuntimeOrigin::root(),
            dave,
            bob(),
            TaoBalance::from(0u64)
        ));
        let carol = U256::from(1_003);
        assert_ok!(SubtensorModule::swap_coldkey(
            RuntimeOrigin::root(),
            alice(),
            carol,
            TaoBalance::from(0u64)
        ));
        assert!(short(alice(), netuid).is_none());
        assert_eq!(custody(carol, netuid), custody_account);
        assert!(pallet_lending::HolderPositions::<Test>::get(alice()).is_empty());
        assert_ok!(Lending::do_try_state());
    });
}

/// The declared weight of a coldkey swap carries lending's bound, and the bound covers the
/// re-key of a holder with a position.
#[test]
fn coldkey_swap_weight_is_within_the_declared_bound() {
    new_test_ext(1).execute_with(|| {
        let _ = with_short();
        System::set_block_number(System::block_number() + 1);
        let bound = <Test as Config>::Lending::coldkey_swap_weight();
        assert!(bound.ref_time() > 0);
        let used = <Test as Config>::Lending::on_coldkey_swap(&alice(), &U256::from(1_003));
        assert!(used.all_lte(bound));
    });
}

/// HOLD(2.6): with lending state on the subnet the takeover bar is still exactly 18% of the
/// eligible alpha: conviction at the bar does not take the subnet, one unit above does.
#[test]
fn conviction_bar_is_unchanged_with_lending_state() {
    for (conviction, takes_over) in [(1_260u64, false), (1_261, true)] {
        new_test_ext(1).execute_with(|| {
            let (netuid, _, _) = with_short();
            assert!(SyntheticOutstanding::<Test>::get(netuid) > 0);
            assert_eq!(
                <Test as Config>::Lending::conviction_bar_extra(netuid),
                AlphaBalance::from(0u64)
            );
            let owner = SubnetOwner::<Test>::get(netuid);
            let challenger = U256::from(5);
            let king = U256::from(6);
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                &challenger,
                &king
            ));
            // Eligible alpha is 10,000 - 2,000 - 1,000 = 7,000 and 18% of it is 1,260.
            let now = crate::staking::lock::ONE_YEAR + 1;
            System::set_block_number(now);
            NetworkRegisteredAt::<Test>::insert(netuid, 1);
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000u64));
            SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(2_000u64));
            pallet_alpha_assets::AlphaBurned::<Test>::insert(netuid, AlphaBalance::from(1_000u64));
            let lock = LockState {
                locked_mass: AlphaBalance::from(conviction),
                conviction: U64F64::from_num(conviction),
                last_update: now,
            };
            Lock::<Test>::insert((challenger, netuid, king), lock.clone());
            HotkeyLock::<Test>::insert(netuid, king, lock);

            SubtensorModule::change_subnet_owner_if_needed(netuid);

            let expected = if takes_over { challenger } else { owner };
            assert_eq!(SubnetOwner::<Test>::get(netuid), expected);
        });
    }
}

/// The cleanup of a subnet with an open short settles the short: the short, its custody and its
/// books are gone when the subnet is.
#[test]
fn dissolution_settles_the_short_and_runs_to_completion() {
    new_test_ext(1).execute_with(|| {
        let (netuid, custody_account, _) = with_short();
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        let mut guard = 0u32;
        while CurrentDissolveCleanupStatus::<Test>::get().is_some()
            || DissolveCleanupQueue::<Test>::get().contains(&netuid)
        {
            guard += 1;
            assert!(guard < 256, "dissolve cleanup stalled");
            run_block_idle();
        }
        assert!(!SubtensorModule::if_subnet_exist(netuid));
        assert!(short(alice(), netuid).is_none());
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&custody_account),
            TaoBalance::ZERO
        );
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), 0);
        assert_eq!(PositionCount::<Test>::get(netuid), 0);
    });
}

/// For a dissolved subnet, with an open short or without lending: the weight of the
/// root-claimed step metered alone (zero when the driver runs that step itself), and the weight
/// the dissolution driver meters from `phase` to the end of the cleanup.
fn metered_dissolution_from(phase: DissolveCleanupPhase, open_short: bool) -> (Weight, Weight) {
    new_test_ext(1).execute_with(|| {
        let netuid = if open_short {
            with_short().0
        } else {
            add_dynamic_network(&U256::from(1), &U256::from(2))
        };
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        // The phase's own step, metered alone when the driver starts after it.
        let mut step = WeightMeter::new();
        if phase != DissolveCleanupPhase::SubnetRootDividendsRootClaimed {
            assert!(SubtensorModule::clean_up_root_claimed_for_subnet(
                netuid, &mut step
            ));
        }
        let mut status = DissolveCleanupStatus::new(netuid);
        status.set_phase(phase);
        let mut meter = WeightMeter::new();
        let (_, used) =
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter, &mut status);
        assert_eq!(used, meter.consumed());
        (step.consumed(), used)
    })
}

/// The dissolve hook meters its own work and nothing else. The phase that calls it meters its
/// step, then the hook, then the rest of the cleanup: the driver started at the calling phase
/// uses what the step uses alone, plus what the driver uses from the next phase, plus the hook.
/// With no lending the hook is its read of the subnet's books (the measured `on_dissolve(0)`);
/// with a short it also meters the short's settlement.
#[test]
fn dissolve_hook_meters_its_own_work_only() {
    let scan = <<Test as pallet_lending::Config>::WeightInfo as pallet_lending::weights::WeightInfo>::on_dissolve(0);

    let (_, bare_with_hook) =
        metered_dissolution_from(DissolveCleanupPhase::SubnetRootDividendsRootClaimed, false);
    let (bare_step, bare_after) = metered_dissolution_from(
        DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue,
        false,
    );
    assert!(bare_after.ref_time() > 0);
    assert_eq!(
        bare_with_hook,
        bare_step.saturating_add(bare_after).saturating_add(scan)
    );

    let (_, short_with_hook) =
        metered_dissolution_from(DissolveCleanupPhase::SubnetRootDividendsRootClaimed, true);
    let (short_step, short_after) = metered_dissolution_from(
        DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue,
        true,
    );
    let short_hook = short_with_hook
        .saturating_sub(short_step)
        .saturating_sub(short_after);
    assert!(
        short_hook.ref_time() > scan.ref_time(),
        "a short adds its settlement to the hook's weight"
    );
}
