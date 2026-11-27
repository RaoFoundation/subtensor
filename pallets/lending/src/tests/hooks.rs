//! The hooks `pallet_subtensor` calls (`hooks.rs`): the pallet hotkey, the custody test and
//! the coldkey-swap re-key.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok, traits::Hooks, weights::WeightMeter};
use sp_runtime::Weight;
use subtensor_runtime_common::{AlphaBalance, NetUid};

use super::mock::*;
use crate::{
    Custody, Due, Error, Event, HolderPositions, InterestRetry, InterestTotals, LendingInterface,
    Positions, SwappedOutAt, SyntheticOutstanding,
    hooks::MAX_POSITIONS_PER_HOLDER,
    params::RAO,
    types::{PositionId, Side},
};

const ALICE: AccountId = 1;
const BOB: AccountId = 2;
const CAROL: AccountId = 3;
const SN1: u16 = 1;

fn sn1() -> NetUid {
    NetUid::from(SN1)
}

fn id(who: AccountId, side: Side) -> PositionId<AccountId> {
    (who, sn1(), side)
}

/// Subnet 1 with a 5,000 TAO / 50,000 alpha pool, lending and longs on, everybody funded.
fn live() -> sp_io::TestExternalities {
    let mut ext = migrated_ext();
    ext.execute_with(|| {
        set_pool(SN1, 5_000 * RAO, 50_000 * RAO);
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Lending::set_longs_enabled(RuntimeOrigin::root(), true));
        for who in [ALICE, BOB, CAROL] {
            set_balance(who, 100 * RAO);
        }
    });
    ext
}

fn open_short(who: AccountId) {
    assert_ok!(Lending::open_short(
        RuntimeOrigin::signed(who),
        sn1(),
        200 * RAO,
        45 * RAO,
        0
    ));
}

fn destination_ok(old: AccountId, new: AccountId) -> bool {
    <Lending as LendingInterface<AccountId>>::coldkey_swap_destination_ok(&old, &new)
}

/// The coldkey-swap hook, after the admission check the swap runs first. It has no error to
/// return; every call must stay inside the bound the swap declares.
fn rekey(old: AccountId, new: AccountId) {
    assert!(destination_ok(old, new));
    let used = <Lending as LendingInterface<AccountId>>::on_coldkey_swap(&old, &new);
    assert!(used.all_lte(<Lending as LendingInterface<AccountId>>::coldkey_swap_weight()));
}

fn held(who: AccountId) -> Vec<(NetUid, Side)> {
    HolderPositions::<Test>::get(who).into_inner()
}

/// Subnets `1..=subnets`, each with a 5,000 TAO / 50,000 alpha pool, lending on.
fn many_subnets(subnets: u16) -> sp_io::TestExternalities {
    let mut ext = new_test_ext();
    ext.execute_with(|| {
        for netuid in 1..=subnets {
            set_pool(netuid, 5_000 * RAO, 50_000 * RAO);
            set_moving_price(netuid, px(800, 10_000));
        }
        Lending::on_runtime_upgrade();
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
    });
    ext
}

fn open_short_on(who: AccountId, netuid: u16) -> frame_support::dispatch::DispatchResult {
    Lending::open_short(
        RuntimeOrigin::signed(who),
        NetUid::from(netuid),
        200 * RAO,
        45 * RAO,
        0,
    )
}

#[test]
fn the_migration_registers_the_pallet_hotkey_once() {
    migrated_ext().execute_with(|| {
        let expected = Some((Lending::pallet_account(), Lending::pallet_hotkey_account()));
        assert_eq!(registered_pallet_hotkey(), expected);
        assert_ne!(Lending::pallet_account(), Lending::pallet_hotkey_account());
        assert_ok!(Lending::initialize_custody());
        assert_eq!(registered_pallet_hotkey(), expected);
        assert_eq!(
            <Lending as LendingInterface<AccountId>>::pallet_hotkey(),
            Some(Lending::pallet_hotkey_account())
        );
    });
}

#[test]
fn only_pallet_derived_accounts_are_lending_custody() {
    live().execute_with(|| {
        open_short(ALICE);
        let custody = Lending::custody_account(&ALICE, sn1(), Side::Short).unwrap();
        for account in [
            custody,
            Lending::pallet_account(),
            Lending::pallet_hotkey_account(),
        ] {
            assert!(<Lending as LendingInterface<AccountId>>::is_lending_custody(&account));
        }
        for account in [ALICE, BOB, 0, AccountId::MAX] {
            assert!(!<Lending as LendingInterface<AccountId>>::is_lending_custody(&account));
        }
        assert_eq!(
            <Lending as LendingInterface<AccountId>>::lending_alpha(sn1()),
            SyntheticOutstanding::<Test>::get(sn1())
        );
        assert_eq!(
            <Lending as LendingInterface<AccountId>>::locked_alpha(&ALICE, sn1()),
            0
        );
    });
}

#[test]
fn the_rekey_moves_position_schedule_retries_and_custody() {
    live().execute_with(|| {
        open_short(ALICE);
        let pos = Positions::<Test>::get(id(ALICE, Side::Short)).unwrap();
        let custody = Lending::custody_account(&ALICE, sn1(), Side::Short).unwrap();
        InterestRetry::<Test>::insert(id(ALICE, Side::Short), 3);
        let retries = InterestRetry::<Test>::get(id(ALICE, Side::Short));
        next_block();

        rekey(ALICE, CAROL);
        assert_eq!(
            lending_events(),
            vec![Event::PositionRekeyed {
                old: ALICE,
                new: CAROL,
                netuid: sn1(),
                side: Side::Short
            }]
        );
        assert!(held(ALICE).is_empty());
        assert_eq!(held(CAROL), vec![(sn1(), Side::Short)]);
        assert!(!SwappedOutAt::<Test>::contains_key(ALICE));

        assert!(!Positions::<Test>::contains_key(id(ALICE, Side::Short)));
        assert!(!Due::<Test>::contains_key(
            pos.charge_at,
            id(ALICE, Side::Short)
        ));
        assert!(!InterestRetry::<Test>::contains_key(id(ALICE, Side::Short)));
        assert!(!Custody::<Test>::contains_key(id(ALICE, Side::Short)));
        assert_eq!(Positions::<Test>::get(id(CAROL, Side::Short)), Some(pos));
        assert!(Due::<Test>::contains_key(
            pos.charge_at,
            id(CAROL, Side::Short)
        ));
        assert_eq!(InterestRetry::<Test>::get(id(CAROL, Side::Short)), retries);
        assert_eq!(
            Lending::custody_account(&CAROL, sn1(), Side::Short),
            Some(custody)
        );
        assert_ok!(Lending::do_try_state());

        // The old key may open again: its new custody is a different account.
        open_short(ALICE);
        let reopened = Lending::custody_account(&ALICE, sn1(), Side::Short).unwrap();
        assert_ne!(reopened, custody);
        assert_ok!(Lending::do_try_state());

        // Interest still charges the moved position on its schedule, and a second swap moves
        // the recorded custody on.
        run_to(pos.charge_at);
        assert!(InterestTotals::<Test>::get(sn1()).charged > 0);
        assert!(
            Positions::<Test>::get(id(CAROL, Side::Short))
                .unwrap()
                .collateral
                < pos.collateral
        );
        rekey(CAROL, BOB);
        assert_eq!(
            Lending::custody_account(&BOB, sn1(), Side::Short),
            Some(custody)
        );
        assert!(!Custody::<Test>::contains_key(id(CAROL, Side::Short)));
        assert_ok!(Lending::do_try_state());

        // Closing removes the recorded custody.
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(BOB),
            sn1(),
            200 * RAO,
            u64::MAX
        ));
        assert!(!Custody::<Test>::contains_key(id(BOB, Side::Short)));
        assert_ok!(Lending::do_try_state());
    });
}

/// A swap in the block a position was opened moves it like any other. The old key then opens
/// nothing more in that block, where it would derive the moved position's custody account.
#[test]
fn the_rekey_moves_a_position_opened_this_block() {
    live().execute_with(|| {
        open_short(ALICE);
        let custody = Lending::custody_account(&ALICE, sn1(), Side::Short).unwrap();
        rekey(ALICE, CAROL);
        assert_eq!(
            Lending::custody_account(&CAROL, sn1(), Side::Short),
            Some(custody)
        );
        assert!(held(ALICE).is_empty());
        assert_ok!(Lending::do_try_state());

        assert_noop!(
            Lending::open_short(RuntimeOrigin::signed(ALICE), sn1(), 200 * RAO, 45 * RAO, 0),
            Error::<Test>::SwappedOutThisBlock
        );
        next_block();
        open_short(ALICE);
        assert_ne!(
            Lending::custody_account(&ALICE, sn1(), Side::Short),
            Some(custody)
        );
        assert!(!SwappedOutAt::<Test>::contains_key(ALICE));
        assert_ok!(Lending::do_try_state());
    });
}

/// Admission: a holder of positions cannot swap onto a key that holds any position, on any
/// subnet or side, so a swap never has a position it cannot move. The check reads only.
#[test]
fn a_destination_with_positions_is_refused_for_a_holder_of_positions() {
    many_subnets(2).execute_with(|| {
        for who in [ALICE, BOB, CAROL] {
            set_balance(who, 100 * RAO);
        }
        assert_ok!(open_short_on(ALICE, 1));
        assert_ok!(open_short_on(BOB, 2));
        next_block();
        let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
        // Different subnets, and still refused, both ways.
        assert!(!destination_ok(ALICE, BOB));
        assert!(!destination_ok(BOB, ALICE));
        // A key without positions is a destination, and may swap onto a holder.
        assert!(destination_ok(ALICE, CAROL));
        assert!(destination_ok(CAROL, ALICE));
        assert!(destination_ok(CAROL, 99));
        assert!(destination_ok(ALICE, ALICE));
        assert_eq!(root, sp_io::storage::root(sp_runtime::StateVersion::V1));

        // Once the destination has closed, it is admitted.
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(BOB),
            NetUid::from(2),
            200 * RAO,
            u64::MAX
        ));
        assert!(destination_ok(ALICE, BOB));
        rekey(ALICE, BOB);
        assert!(held(ALICE).is_empty());
        assert_eq!(held(BOB), vec![(sn1(), Side::Short)]);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn a_rekey_of_a_key_without_positions_changes_nothing() {
    live().execute_with(|| {
        open_short(ALICE);
        next_block();
        let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
        rekey(BOB, CAROL);
        rekey(ALICE, ALICE);
        assert_eq!(root, sp_io::storage::root(sp_runtime::StateVersion::V1));
    });
}

/// A coldkey holds at most `MAX_POSITIONS_PER_HOLDER` positions: the open gate refuses the
/// next one, and a close frees a place.
#[test]
fn a_holder_opens_at_most_the_capped_number_of_positions() {
    let cap = MAX_POSITIONS_PER_HOLDER as u16;
    many_subnets(cap + 1).execute_with(|| {
        set_balance(ALICE, 10_000 * RAO);
        for netuid in 1..=cap {
            assert_ok!(open_short_on(ALICE, netuid));
        }
        assert_eq!(held(ALICE).len(), cap as usize);
        assert_noop!(
            open_short_on(ALICE, cap + 1),
            Error::<Test>::TooManyPositions
        );
        assert_ok!(Lending::do_try_state());

        next_block();
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(ALICE),
            sn1(),
            200 * RAO,
            u64::MAX
        ));
        assert_eq!(held(ALICE).len(), cap as usize - 1);
        assert_ok!(open_short_on(ALICE, cap + 1));
        assert_ok!(Lending::do_try_state());
    });
}

/// The largest re-key, every position a coldkey may hold, stays inside the declared bound
/// and leaves the old key with nothing.
#[test]
fn the_largest_rekey_moves_every_position_within_the_bound() {
    let cap = MAX_POSITIONS_PER_HOLDER as u16;
    many_subnets(cap).execute_with(|| {
        set_balance(ALICE, 10_000 * RAO);
        for netuid in 1..=cap {
            assert_ok!(open_short_on(ALICE, netuid));
        }
        let before = held(ALICE);
        let custodies: Vec<_> = before
            .iter()
            .map(|(netuid, side)| Lending::custody_account(&ALICE, *netuid, *side))
            .collect();
        next_block();
        rekey(ALICE, CAROL);
        assert_eq!(lending_events().len(), cap as usize);
        assert!(held(ALICE).is_empty());
        assert_eq!(held(CAROL), before);
        for ((netuid, side), custody) in before.iter().zip(custodies) {
            assert_eq!(Lending::custody_account(&ALICE, *netuid, *side), None);
            assert_eq!(Lending::custody_account(&CAROL, *netuid, *side), custody);
        }
        assert!(!Due::<Test>::iter_keys().any(|(_, (who, _, _))| who == ALICE));
        assert!(!Positions::<Test>::iter_keys().any(|(who, _, _)| who == ALICE));
        assert_eq!(
            Positions::<Test>::iter_keys()
                .filter(|(who, _, _)| *who == CAROL)
                .count(),
            cap as usize
        );
        assert_ok!(Lending::do_try_state());
    });
}

/// HOLD(2.6), HOLD(2.8/2.9): both hook points do nothing until the spec decides.
#[test]
fn the_hold_hooks_do_nothing() {
    live().execute_with(|| {
        open_short(ALICE);
        let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let mut meter = WeightMeter::new();
        meter.consume(Weight::from_parts(7, 3));
        <Lending as LendingInterface<AccountId>>::on_dissolve(sn1(), &mut meter);
        assert_eq!(meter.consumed(), Weight::from_parts(7, 3));
        assert_eq!(
            <Lending as LendingInterface<AccountId>>::conviction_bar_extra(sn1()),
            AlphaBalance::from(0u64)
        );
        assert_eq!(root, sp_io::storage::root(sp_runtime::StateVersion::V1));
    });
}
