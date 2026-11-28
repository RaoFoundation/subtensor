//! Shorts of a subnet being dissolved, end to end (SPEC section 7.4 phase E): the settlement the
//! cleanup runs, the stakers' pot, the owner refund, and the books the cleanup leaves.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok};
use pallet_lending::{
    DissolutionSettlements, DissolvingSubnets, InterestTotals, PositionCount, PositionsByNetuid,
    ResidualAlpha, SettlementRetry, SyntheticOutstanding, types::Side,
};
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::lending_adapter::{RAO, alice, bob, custody, lending_subnet, short};
use super::mock::*;
use crate::subnets::dissolution::DissolveCleanupPhase;
use crate::*;

/// The owner's lock of the legacy subnets of these tests (rao): small, so that the owner's refund
/// is the lock plus rounding dust and any lending TAO that reached it would show.
const LOCK: u64 = 1;
/// Rounding dust one payee of a pro-rata payout may leave (rao); the bound on the owner's gain
/// above its lock.
const DUST: u64 = 10;

fn balance(who: &U256) -> u64 {
    SubtensorModule::get_coldkey_balance(who).to_u64()
}

/// Stakes `amount` rao from a fresh coldkey on `netuid`; returns the coldkey.
fn stake_external(netuid: NetUid, index: u64, amount: u64) -> U256 {
    let hot = U256::from(30_000 + index);
    let cold = U256::from(40_000 + index);
    let tao: TaoBalance = amount.into();
    assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
    add_balance_to_coldkey_account(&cold, tao);
    assert_ok!(SubtensorModule::stake_into_subnet(
        &hot,
        &cold,
        netuid,
        tao,
        <Test as Config>::SwapInterface::max_price(),
        false,
    ));
    cold
}

/// A legacy subnet (refund-eligible, and priced without its pool's alpha as protocol alpha, see
/// `dissolution_protocol_alpha_value`) with lending on and no owner stake: two outside stakers of
/// 1,000 TAO each, a lock of [`LOCK`], and an accrued owner cut of zero, so that the owner's
/// refund does not depend on the alpha price (which the short's sale moves).
fn legacy_lending_subnet() -> (NetUid, [U256; 2]) {
    let netuid = lending_subnet(50);
    remove_owner_registration_stake(netuid);
    let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
    NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));
    TaoInRefundDeploymentBlock::<Test>::put(reg_at);
    SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(LOCK));
    SubnetOwnerCutAccrued::<Test>::insert(netuid, AlphaBalance::ZERO);
    let stakers = [
        stake_external(netuid, 0, 1_000 * RAO),
        stake_external(netuid, 1, 1_000 * RAO),
    ];
    (netuid, stakers)
}

/// Alice opens a short of 2,000 RAO of alpha with 5,000 RAO of collateral on `netuid`.
fn open_short(netuid: NetUid) {
    assert_ok!(Lending::open_short(
        RuntimeOrigin::signed(alice()),
        netuid,
        2_000 * RAO,
        5_000 * RAO,
        0
    ));
}

/// The `ShortSettledOnDissolution` events of the current block, as `(who, price, paid)`.
fn settled_events() -> Vec<(U256, U64F64, u64)> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Lending(pallet_lending::Event::ShortSettledOnDissolution {
                who,
                price,
                paid,
                ..
            }) => Some((who, price, paid)),
            _ => None,
        })
        .collect()
}

/// Dissolves `netuid` and runs its cleanup to the end, one block at a time. Returns the
/// settlements the cleanup deposited, as `(who, price, paid)`.
fn dissolve_to_completion(netuid: NetUid) -> Vec<(U256, U64F64, u64)> {
    assert_ok!(SubtensorModule::do_dissolve_network(netuid));
    let mut settled = Vec::new();
    for _ in 0..256 {
        if CurrentDissolveCleanupStatus::<Test>::get().is_none()
            && !DissolveCleanupQueue::<Test>::get().contains(&netuid)
        {
            return settled;
        }
        System::reset_events();
        run_block_idle();
        settled.extend(settled_events());
    }
    panic!("dissolve cleanup stalled");
}

/// `q` alpha at `price`, floored: what a short of `q` owes at the dereg rate `price`.
fn owed(q: u64, price: U64F64) -> u64 {
    (U64F64::from_num(q) * price).floor().to_num::<u64>()
}

/// The alpha value on `netuid` of the first `count` stakers of [`stake_external`].
fn staker_alpha(netuid: NetUid, count: u64) -> u128 {
    (0..count)
        .map(|index| {
            let hot = U256::from(30_000 + index);
            let cold = U256::from(40_000 + index);
            u128::from(u64::from(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid),
            ))
        })
        .sum()
}

/// The stakers' share of a pot of `pot` rao, as the cleanup splits it: by their alpha `stakers`
/// over their alpha and the protocol's `protocol` (`SubnetProtocolAlpha`, which the emission's
/// excess buys build up over the blocks, and which the cleanup keeps back, as for a subnet with
/// no lending).
fn stakers_pot_share(pot: u64, stakers: u128, protocol: u128) -> u64 {
    (u128::from(pot) * stakers / (stakers + protocol)) as u64
}

/// Asserts the lending books of `netuid` are empty once its cleanup has finished.
fn assert_lending_books_empty(netuid: NetUid) {
    assert_eq!(PositionCount::<Test>::get(netuid), 0);
    assert_eq!(SyntheticOutstanding::<Test>::get(netuid), 0);
    assert_eq!(ResidualAlpha::<Test>::get(netuid), 0);
    assert!(!DissolvingSubnets::<Test>::contains_key(netuid));
    assert!(!DissolutionSettlements::<Test>::contains_key(netuid));
    assert!(
        SettlementRetry::<Test>::iter_prefix(netuid)
            .next()
            .is_none()
    );
    assert!(
        PositionsByNetuid::<Test>::iter_key_prefix((netuid, Side::Short))
            .next()
            .is_none()
    );
    assert!(!SubtensorModule::if_subnet_exist(netuid));
}

/// The `ShortSettlementFailed` events of the current block: the holders.
fn failed_events() -> Vec<U256> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Lending(pallet_lending::Event::ShortSettlementFailed { who, .. }) => {
                Some(who)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_profitable_short_settles_at_the_dereg_rate_and_its_holdings_go_back() {
    new_test_ext(1).execute_with(|| {
        let (netuid, stakers) = legacy_lending_subnet();
        open_short(netuid);
        // Blocks pass, so the short accrues interest before the dissolution.
        for _ in 0..50 {
            next_block_no_epoch(netuid);
        }
        let custody_account = custody(alice(), netuid);
        let holdings = balance(&custody_account);
        let alice_before = balance(&alice());
        let stakers_before: u64 = stakers.iter().map(balance).sum();
        let pool_at_flag = SubnetTAO::<Test>::get(netuid).to_u64();
        let charged_before = InterestTotals::<Test>::get(netuid).charged;
        let stakers_alpha = staker_alpha(netuid, 2);
        let protocol_alpha = SubnetProtocolAlpha::<Test>::get(netuid).to_u64() as u128;

        let settled = dissolve_to_completion(netuid);
        let interest =
            u64::try_from(InterestTotals::<Test>::get(netuid).charged - charged_before).unwrap();

        assert_eq!(settled.len(), 1);
        let (who, r, paid) = settled[0];
        assert_eq!(who, alice());
        // Profitable: the short pays what it owes at the rate, not its holdings.
        assert_eq!(paid, owed(2_000 * RAO, r));
        assert!(
            paid < holdings - interest,
            "the holdings cover what is owed"
        );
        // The custody's rest, the deposit included, goes back to the holder; the interest and the
        // payment leave it.
        assert_eq!(balance(&alice()), alice_before + holdings - interest - paid);
        assert_eq!(balance(&custody_account), 0);
        assert!(short(alice(), netuid).is_none());
        assert_lending_books_empty(netuid);

        // The stakers get their share of the pot, which is the pool's TAO once the interest is in,
        // and the whole settlement. Rounding of each payee's share is the only difference.
        let stakers_gain = stakers.iter().map(balance).sum::<u64>() - stakers_before;
        let expected =
            stakers_pot_share(pool_at_flag + interest, stakers_alpha, protocol_alpha) + paid;
        assert!(
            stakers_gain <= expected && expected - stakers_gain <= 2 * DUST,
            "stakers gain {stakers_gain}, expected {expected}"
        );
        assert_ok!(pallet_lending::Pallet::<Test>::do_try_state());
    });
}

#[test]
fn a_losing_short_is_capped_at_its_holdings_and_the_stakers_get_them() {
    new_test_ext(1).execute_with(|| {
        let (netuid, stakers) = legacy_lending_subnet();
        open_short(netuid);
        // A large stake after the open raises the pool's price, so the short owes more than it holds
        // (the rate is the pool TAO over the alpha, about 10 at this stake; the short owes about 2.6
        // times its holdings at a rate of 1).
        let big = stake_external(netuid, 2, 10_000_000 * RAO);
        let stakers = [stakers[0], stakers[1], big];
        // The emission EMA would bind the rate below the flag's pool; the loss needs the rate of the
        // pool itself, so the EMA does not bind here.
        pallet_lending::PoolTaoEma::<Test>::insert(netuid, u64::MAX);
        let pos = short(alice(), netuid).expect("the short is open");
        let custody_account = custody(alice(), netuid);
        let holdings = balance(&custody_account);
        let alice_before = balance(&alice());
        let stakers_before: u64 = stakers.iter().map(balance).sum();
        let pool_at_flag = SubnetTAO::<Test>::get(netuid).to_u64();
        let stakers_alpha = staker_alpha(netuid, 3);
        let protocol_alpha = SubnetProtocolAlpha::<Test>::get(netuid).to_u64() as u128;

        let settled = dissolve_to_completion(netuid);

        assert_eq!(settled.len(), 1);
        let (_, r, paid) = settled[0];
        // No block passed after the open, so the holdings are collateral, proceeds and deposit.
        assert!(
            owed(2_000 * RAO, r) > holdings - pos.deposit,
            "the short owes more than it holds"
        );
        assert_eq!(
            paid,
            holdings - pos.deposit,
            "the payment is the whole of the holdings"
        );
        // The holder keeps only the deposit back.
        assert_eq!(balance(&alice()), alice_before + pos.deposit);
        assert_eq!(balance(&custody_account), 0);
        assert_lending_books_empty(netuid);

        let stakers_gain = stakers.iter().map(balance).sum::<u64>() - stakers_before;
        let expected = stakers_pot_share(pool_at_flag, stakers_alpha, protocol_alpha) + paid;
        assert!(
            stakers_gain <= expected && expected - stakers_gain <= 2 * DUST,
            "stakers gain {stakers_gain}, expected {expected}"
        );
    });
}

#[test]
fn the_owner_refund_is_the_refund_of_the_same_subnet_without_a_short() {
    // The twin is the same subnet with lending on and no short: the flag records nothing for it.
    let refund = |with_short: bool| -> u64 {
        new_test_ext(1).execute_with(|| {
            let (netuid, _) = legacy_lending_subnet();
            if with_short {
                open_short(netuid);
            }
            let owner = SubnetOwner::<Test>::get(netuid);
            let before = balance(&owner);
            dissolve_to_completion(netuid);
            balance(&owner) - before
        })
    };
    let with_lending = refund(true);
    let twin = refund(false);
    // Lending TAO (the settlements, the interest) reaches the stakers; the owner gets its lock and
    // the rounding dust of the pot, as in the twin.
    for gain in [with_lending, twin] {
        assert!((LOCK..=LOCK + DUST).contains(&gain), "owner gain {gain}");
    }
}

/// The owner's gain from dissolving the legacy subnet of these tests after 50 blocks (which build
/// up the protocol's alpha and the owner's accrued cut), with or without a profitable short.
fn owner_gain_after_50_blocks(with_short: bool) -> u64 {
    let mut gain = 0;
    new_test_ext(1).execute_with(|| {
        let (netuid, _) = legacy_lending_subnet();
        if with_short {
            open_short(netuid);
        }
        for _ in 0..50 {
            next_block_no_epoch(netuid);
        }
        let owner = SubnetOwner::<Test>::get(netuid);
        let before = balance(&owner);
        dissolve_to_completion(netuid);
        gain = balance(&owner).saturating_sub(before);
    });
    gain
}

/// The owner's refund does not depend on the lending: after the same blocks, the owner gets the
/// same from the dissolution with a short of the subnet and without one.
#[test]
fn the_owner_refund_after_blocks_is_the_refund_without_lending() {
    let with_short = owner_gain_after_50_blocks(true);
    let without = owner_gain_after_50_blocks(false);
    assert!(without >= LOCK, "the owner gets its lock back");
    assert_eq!(with_short, without, "the short changes the owner's refund");
}

#[test]
fn the_residual_alpha_is_cleared_by_the_cleanup() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _) = legacy_lending_subnet();
        open_short(netuid);
        // A defaulted short's alpha still in circulation.
        ResidualAlpha::<Test>::insert(netuid, 7 * RAO);
        dissolve_to_completion(netuid);
        assert_lending_books_empty(netuid);
        assert_ok!(pallet_lending::Pallet::<Test>::do_try_state());
    });
}

#[test]
fn no_short_call_runs_from_the_flag_to_the_end_of_the_cleanup() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _) = legacy_lending_subnet();
        open_short(netuid);
        let custody_account = custody(alice(), netuid);
        let holdings = balance(&custody_account);
        let alice_before = balance(&alice());
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        // From the flag on, the short cannot be closed, grown or topped up, and none opens: the
        // dissolution alone settles it.
        let closed = || pallet_lending::Error::<Test>::DissolutionInProgress;
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(alice()), netuid, u64::MAX, u64::MAX),
            closed()
        );
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(alice()), netuid, RAO, RAO, 0),
            closed()
        );
        assert_noop!(
            Lending::add_collateral(RuntimeOrigin::signed(alice()), netuid, RAO),
            closed()
        );
        assert_noop!(
            Lending::open_short(
                RuntimeOrigin::signed(bob()),
                netuid,
                2_000 * RAO,
                5_000 * RAO,
                0
            ),
            closed()
        );
        assert_eq!(balance(&alice()), alice_before);
        assert_eq!(balance(&custody_account), holdings);

        // The cleanup settles the short once, and that is the only payment the holder gets.
        let mut settled = Vec::new();
        for _ in 0..256 {
            if CurrentDissolveCleanupStatus::<Test>::get().is_none()
                && !DissolveCleanupQueue::<Test>::get().contains(&netuid)
            {
                break;
            }
            System::reset_events();
            run_block_idle();
            settled.extend(settled_events());
        }
        assert_eq!(settled.len(), 1);
        let (who, _, paid) = settled[0];
        assert_eq!(who, alice());
        assert_eq!(balance(&alice()), alice_before + holdings - paid);
        assert_eq!(balance(&custody_account), 0);
        assert_lending_books_empty(netuid);
        assert_ok!(pallet_lending::Pallet::<Test>::do_try_state());
    });
}

#[test]
fn a_short_that_fails_to_settle_holds_the_cleanup_until_it_settles() {
    new_test_ext(1).execute_with(|| {
        let (netuid, stakers) = legacy_lending_subnet();
        open_short(netuid);
        let custody_account = custody(alice(), netuid);
        let holdings = balance(&custody_account);
        let alice_before = balance(&alice());
        let stakers_before: u64 = stakers.iter().map(balance).sum();
        // The custody cannot pay what the position records.
        remove_balance_from_coldkey_account(&custody_account, holdings.into());
        assert_eq!(balance(&custody_account), 0);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        // Every cleanup step tries the short, reports the failure and keeps it queued. The
        // cleanup does not go past the lending phase: the pot is not paid without the short.
        for round in 1..=3u32 {
            System::reset_events();
            run_block_idle();
            assert_eq!(failed_events(), vec![alice()], "round {round}");
            assert!(settled_events().is_empty());
            assert_eq!(SettlementRetry::<Test>::get(netuid, alice()), Some(round));
            assert!(short(alice(), netuid).is_some());
            let status = CurrentDissolveCleanupStatus::<Test>::get().expect("cleanup is held");
            assert_eq!(status.netuid, netuid);
            assert_eq!(
                status.phase,
                DissolveCleanupPhase::SubnetRootDividendsRootClaimed
            );
            assert_eq!(stakers.iter().map(balance).sum::<u64>(), stakers_before);
            assert_eq!(
                DissolvingSubnets::<Test>::get(netuid).map(|record| record.settled_tao),
                Some(0)
            );
        }

        // With the custody whole again the next step settles the short and the cleanup ends.
        add_balance_to_coldkey_account(&custody_account, holdings.into());
        let mut settled = Vec::new();
        let mut failed = Vec::new();
        for _ in 0..256 {
            if CurrentDissolveCleanupStatus::<Test>::get().is_none()
                && !DissolveCleanupQueue::<Test>::get().contains(&netuid)
            {
                break;
            }
            System::reset_events();
            run_block_idle();
            settled.extend(settled_events());
            failed.extend(failed_events());
        }
        assert!(failed.is_empty());
        assert_eq!(settled.len(), 1);
        let (who, _, paid) = settled[0];
        assert_eq!(who, alice());
        assert_eq!(balance(&alice()), alice_before + holdings - paid);
        assert_eq!(balance(&custody_account), 0);
        assert!(stakers.iter().map(balance).sum::<u64>() > stakers_before + paid - 2 * DUST);
        assert_lending_books_empty(netuid);
        assert_ok!(pallet_lending::Pallet::<Test>::do_try_state());
    });
}
