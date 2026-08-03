//! Drain-scheduler and op-bound tests for [`PendingBasketDeposits`].
//!
//! Unlike `flush_baskets()` (which flushes every queued hotkey at once for economic
//! assertions), these tests exercise `flush_pending_basket_deposits_block` itself: the
//! one-hotkey-per-block cursor, round-robin wrap, root-eviction recycle, and deterministic
//! swap/quote/write counters on happy and failure paths.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::tests::claim_root::{
    escrow_alpha, fund_pool, fund_shares, register_on_root, set_root_weights_direct,
    zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{
    BasketShares, IsNetworkMember, Keys, PendingBasketDeposits, PendingBasketFlushCursor,
    RootClaimableThreshold, SubnetAlphaOut, Uids,
};
use frame_support::assert_ok;
use sp_core::U256;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{NetUid, Token};

/// Storage iteration order of distinct hotkeys currently in the pending-deposit map.
fn pending_hotkey_order() -> Vec<U256> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for (hotkey, _) in PendingBasketDeposits::<Test>::iter_keys() {
        if seen.insert(hotkey) {
            order.push(hotkey);
        }
    }
    order
}

fn pending_credit(hotkey: &U256, netuid: NetUid) -> u64 {
    PendingBasketDeposits::<Test>::get(hotkey, netuid).to_u64()
}

fn queue_credit(hotkey: &U256, netuid: NetUid, alpha: u64) {
    // Credits are assumed already counted in SubnetAlphaOut (epoch mint path).
    let existing = SubnetAlphaOut::<Test>::get(netuid);
    SubnetAlphaOut::<Test>::insert(netuid, existing.saturating_add(alpha.into()));
    SubtensorModule::enqueue_basket_deposit(hotkey, netuid, alpha.into());
}

/// Root-registered uncurated validator with stake, ready to receive queued credits.
fn setup_root_validator(hotkey: U256, coldkey: U256, uid: u16) -> NetUid {
    let owner = U256::from(u64::from(uid).saturating_add(9_000));
    let owner_hot = U256::from(u64::from(uid).saturating_add(9_100));
    let netuid = add_dynamic_network(&owner_hot, &owner);
    remove_owner_registration_stake(netuid);
    fund_pool(netuid);
    register_on_root(&hotkey, uid);
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        NetUid::ROOT,
        2_000_000u64.into(),
    );
    netuid
}

/// The per-block drain flushes exactly one hotkey and advances the cursor past it.
#[test]
fn test_flush_drain_one_hotkey_per_block_advances_cursor() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let cold_a = U256::from(2001);
        let cold_b = U256::from(2002);
        let hot_a = U256::from(3001);
        let hot_b = U256::from(3002);
        let net_a = setup_root_validator(hot_a, cold_a, 1);
        let net_b = setup_root_validator(hot_b, cold_b, 2);

        queue_credit(&hot_a, net_a, 1_000_000);
        queue_credit(&hot_b, net_b, 1_000_000);

        let order = pending_hotkey_order();
        assert_eq!(order.len(), 2, "both hotkeys must be queued");
        assert!(PendingBasketFlushCursor::<Test>::get().is_none());

        reset_basket_op_counters();
        SubtensorModule::flush_pending_basket_deposits_block();

        let first = *order.first().expect("queued hotkey order");
        let second = *order.get(1).expect("queued hotkey order");
        assert!(
            fund_shares(&first) > 0,
            "first hotkey in storage order must be flushed"
        );
        assert_eq!(
            fund_shares(&second),
            0,
            "second hotkey must wait for the next block"
        );
        assert!(
            PendingBasketFlushCursor::<Test>::get().is_some(),
            "cursor must advance past the flushed hotkey"
        );
        // One hotkey's pending rows removed + one cursor write.
        assert_eq!(basket_write_ops(), 2);
        // Uncurated accumulate: no swaps.
        assert_eq!(basket_swap_ops(), 0);

        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(fund_shares(&second) > 0, "second hotkey flushes next block");
    });
}

/// After the last hotkey, the next drain spends its turn clearing the cursor (wrap).
#[test]
fn test_flush_drain_wraps_and_clears_cursor_at_end() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let cold = U256::from(2001);
        let hot_a = U256::from(3001);
        let hot_b = U256::from(3002);
        let net_a = setup_root_validator(hot_a, cold, 1);
        let net_b = setup_root_validator(hot_b, cold, 2);

        queue_credit(&hot_a, net_a, 500_000);
        queue_credit(&hot_b, net_b, 500_000);

        // Flush both hotkeys.
        SubtensorModule::flush_pending_basket_deposits_block();
        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(PendingBasketDeposits::<Test>::iter().next().is_none());
        assert!(
            PendingBasketFlushCursor::<Test>::get().is_some(),
            "cursor sits at the last flushed key until the wrap pass"
        );

        // End-of-map pass: no hotkey left after the cursor → kill and restart next block.
        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(
            PendingBasketFlushCursor::<Test>::get().is_none(),
            "wrap pass must clear the cursor"
        );

        // Fresh credits are picked up from the top after the wrap.
        queue_credit(&hot_a, net_a, 250_000);
        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(fund_shares(&hot_a) > 0);
    });
}

/// An empty queue is a no-op that leaves (or clears) the cursor at None.
#[test]
fn test_flush_drain_empty_queue_kills_cursor() {
    new_test_ext(1).execute_with(|| {
        PendingBasketFlushCursor::<Test>::put(vec![1, 2, 3]);
        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(PendingBasketFlushCursor::<Test>::get().is_none());
    });
}

/// Sub-threshold dust stays queued, but the cursor still advances so the hotkey is not pinned.
#[test]
fn test_flush_drain_dust_advances_cursor_without_deposit() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        // Spot filter defers anything below this.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(1_000_000_000u64));

        let cold_a = U256::from(2001);
        let cold_b = U256::from(2002);
        let hot_a = U256::from(3001);
        let hot_b = U256::from(3002);
        let net_a = setup_root_validator(hot_a, cold_a, 1);
        let net_b = setup_root_validator(hot_b, cold_b, 2);

        queue_credit(&hot_a, net_a, 1_000);
        queue_credit(&hot_b, net_b, 1_000_000);

        let order = pending_hotkey_order();
        assert_eq!(order.len(), 2, "both hotkeys must be queued");
        let first = *order.first().expect("queued hotkey order");
        let second = *order.get(1).expect("queued hotkey order");

        // Lower threshold only for the second hotkey's turn by flushing first (dust) then
        // opening the gate before the second drain.
        SubtensorModule::flush_pending_basket_deposits_block();
        assert_eq!(fund_shares(&first), 0, "dust must not mint shares");
        assert!(
            pending_credit(&first, if first == hot_a { net_a } else { net_b }) > 0,
            "dust credit must remain queued"
        );
        assert!(
            PendingBasketFlushCursor::<Test>::get().is_some(),
            "cursor must skip past dust-only hotkey"
        );

        zero_claim_threshold();
        SubtensorModule::flush_pending_basket_deposits_block();
        assert!(
            fund_shares(&second) > 0,
            "non-dust hotkey after cursor must still flush"
        );
    });
}

/// Root replacement recycles and purges the outgoing hotkey's queued credits (no deposit).
#[test]
fn test_flush_root_eviction_recycles_pending_credits() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let cold = U256::from(2001);
        let hot = U256::from(3001);
        let replacement = U256::from(3002);
        // Origin subnet for the queued credit (pool + alpha-out accounting).
        let owner = U256::from(9001);
        let owner_hot = U256::from(9101);
        let netuid = add_dynamic_network(&owner_hot, &owner);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        // Real root membership so replace_neuron runs the churn flush hook.
        // (Burned registration is not permitted on root; seed the maps directly.)
        let uid = 1u16;
        Uids::<Test>::insert(NetUid::ROOT, hot, uid);
        Keys::<Test>::insert(NetUid::ROOT, uid, hot);
        IsNetworkMember::<Test>::insert(hot, NetUid::ROOT, true);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &cold,
            NetUid::ROOT,
            2_000_000u64.into(),
        );

        let credit = 750_000u64;
        queue_credit(&hot, netuid, credit);
        let alpha_out_before = SubnetAlphaOut::<Test>::get(netuid);

        reset_basket_op_counters();
        SubtensorModule::replace_neuron(NetUid::ROOT, uid, &replacement, 1);

        assert!(
            !PendingBasketDeposits::<Test>::contains_key(hot, netuid),
            "root churn must purge queued credits"
        );
        assert_eq!(
            fund_shares(&hot),
            0,
            "eviction recycles pending credits; it must not deposit into the basket"
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(netuid),
            alpha_out_before.saturating_sub(credit.into()),
            "recycled alpha must leave SubnetAlphaOut"
        );
        // Straggler path: recycle + row drop, no AMM work.
        assert_eq!(basket_swap_ops(), 0);
        assert_eq!(basket_quote_ops(), 0);
        assert_eq!(basket_write_ops(), 1);
    });
}

/// Happy path: a curated multi-origin flush does one batch of swaps/quotes/writes, not one
/// per origin deposit storm. Bounds match the deposit work formula.
#[test]
fn test_flush_happy_path_ops_bounded_for_curated_batch() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let owner = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let dest_owner = U256::from(1004);
        let dest_hot = U256::from(1005);

        let origin_a = add_dynamic_network(&hotkey, &owner);
        let origin_b_hot = U256::from(1006);
        let origin_b_owner = U256::from(1007);
        let origin_b = add_dynamic_network(&origin_b_hot, &origin_b_owner);
        let dest = add_dynamic_network(&dest_hot, &dest_owner);
        remove_owner_registration_stake(origin_a);
        remove_owner_registration_stake(origin_b);
        remove_owner_registration_stake(dest);
        fund_pool(origin_a);
        fund_pool(origin_b);
        fund_pool(dest);

        register_on_root(&hotkey, 0);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        set_root_weights_direct(&hotkey, 0, &[(dest, u16::MAX)]);

        let credits = 2u64;
        let credit_alpha = 1_000_000u64;
        queue_credit(&hotkey, origin_a, credit_alpha);
        queue_credit(&hotkey, origin_b, credit_alpha);

        reset_basket_op_counters();
        let (work, _) = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);

        assert!(fund_shares(&hotkey) > 0);
        assert!(escrow_alpha(&hotkey, dest) > 0);
        assert!(!PendingBasketDeposits::<Test>::contains_key(
            hotkey, origin_a
        ));
        assert!(!PendingBasketDeposits::<Test>::contains_key(
            hotkey, origin_b
        ));

        // Scan (credits) + curated deposit work with empty holdings:
        // holdings*3 + weights(1) + credits → 1 + credits; total = 2*credits + 1.
        let expected_work = credits.saturating_mul(2).saturating_add(1);
        assert_eq!(work, expected_work);

        // One sell per origin + one buy on the sole destination.
        assert_eq!(basket_swap_ops(), credits + 1);
        // Spot checks on each origin + NAV quotes during deploy (nav_before + per-origin
        // valuation on the empty fund collapses to the deploy quotes). Bound, don't pin
        // every internal quote helper call: O(credits + holdings + weights).
        assert!(
            basket_quote_ops() <= 16,
            "quotes must stay single-batch bounded, got {}",
            basket_quote_ops()
        );
        // Two pending-row removes for the batch.
        assert_eq!(basket_write_ops(), credits);
    });
}

/// Failure path: a multi-origin batch that cannot mint recycles the whole batch once.
/// Ops stay at one attempt — no per-credit singleton retry storm.
#[test]
fn test_flush_failure_path_ops_bounded_no_singleton_retry() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let owner = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let origin_b_hot = U256::from(1006);
        let origin_b_owner = U256::from(1007);
        let origin_c_hot = U256::from(1008);
        let origin_c_owner = U256::from(1009);

        let origin_a = add_dynamic_network(&hotkey, &owner);
        let origin_b = add_dynamic_network(&origin_b_hot, &origin_b_owner);
        let origin_c = add_dynamic_network(&origin_c_hot, &origin_c_owner);
        remove_owner_registration_stake(origin_a);
        remove_owner_registration_stake(origin_b);
        remove_owner_registration_stake(origin_c);
        fund_pool(origin_a);
        fund_pool(origin_b);
        fund_pool(origin_c);

        register_on_root(&hotkey, 0);
        // Enormous claimant base: share/rate increment rounds to zero → deposit fails.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            10_000_000_000_000_000u64.into(),
        );
        // Uncurated: accumulate-in-place (no swaps), so failure is purely the dust mint.
        let origins = [origin_a, origin_b, origin_c];
        let credits = origins.len() as u64;
        let credit_alpha = 1_000u64;
        for netuid in origins {
            queue_credit(&hotkey, netuid, credit_alpha);
        }

        let alpha_before: Vec<_> = origins
            .iter()
            .map(|n| SubnetAlphaOut::<Test>::get(*n))
            .collect();

        reset_basket_op_counters();
        let (work, _) = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);

        assert_eq!(BasketShares::<Test>::get(hotkey), 0);
        for netuid in origins {
            assert!(
                !PendingBasketDeposits::<Test>::contains_key(hotkey, netuid),
                "failed batch must not leave credits queued"
            );
        }
        // Whole batch recycled once.
        for (netuid, before) in origins.iter().zip(alpha_before.iter()) {
            assert_eq!(
                SubnetAlphaOut::<Test>::get(*netuid),
                before.saturating_sub(credit_alpha.into())
            );
        }

        // One-shot work: scan(credits) + uncurated deposit(holdings=0 + credits*2).
        // A singleton-retry fallback would charge roughly credits times that deposit term.
        let one_shot = credits + credits.saturating_mul(2);
        let retry_storm = credits + credits.saturating_mul(credits.saturating_mul(2));
        assert_eq!(work, one_shot);
        assert!(
            work < retry_storm,
            "failure must not replay per-credit deposits: work={work} retry_bound={retry_storm}"
        );
        assert_eq!(basket_swap_ops(), 0, "uncurated failure does no swaps");
        // Spot quotes per origin + NAV/origin realizable quotes inside the rolled-back attempt.
        assert!(
            basket_quote_ops() <= credits.saturating_mul(4),
            "quotes must stay one-attempt bounded, got {}",
            basket_quote_ops()
        );
        assert_eq!(
            basket_write_ops(),
            credits,
            "only the pre-deposit pending-row removes"
        );
    });
}

/// Drain path: one block's op counters stay at a single hotkey even with many queued.
#[test]
fn test_flush_drain_ops_bounded_to_one_hotkey() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let mut hotkeys = Vec::new();
        for i in 0..5u16 {
            let hot = U256::from(3_000u64 + u64::from(i));
            let cold = U256::from(2_000u64 + u64::from(i));
            let netuid = setup_root_validator(hot, cold, i + 1);
            queue_credit(&hot, netuid, 500_000);
            hotkeys.push(hot);
        }
        assert_eq!(pending_hotkey_order().len(), 5);

        reset_basket_op_counters();
        SubtensorModule::flush_pending_basket_deposits_block();

        let flushed = hotkeys.iter().filter(|h| fund_shares(h) > 0).count();
        assert_eq!(flushed, 1, "drain must flush exactly one hotkey");
        // Uncurated single-credit: 1 spot quote + accumulate quotes, 1 pending remove + cursor.
        assert_eq!(basket_swap_ops(), 0);
        assert!(
            basket_quote_ops() <= 8,
            "one-hotkey drain quotes bounded, got {}",
            basket_quote_ops()
        );
        assert_eq!(basket_write_ops(), 2);
    });
}

/// Calling the drain via block_step still only processes one hotkey.
#[test]
fn test_flush_drain_via_block_step_one_hotkey() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let hot_a = U256::from(3001);
        let hot_b = U256::from(3002);
        let net_a = setup_root_validator(hot_a, U256::from(2001), 1);
        let net_b = setup_root_validator(hot_b, U256::from(2002), 2);
        queue_credit(&hot_a, net_a, 400_000);
        queue_credit(&hot_b, net_b, 400_000);

        assert_ok!(SubtensorModule::block_step());

        let flushed = [hot_a, hot_b].iter().filter(|h| fund_shares(h) > 0).count();
        assert_eq!(flushed, 1);
        assert!(PendingBasketFlushCursor::<Test>::get().is_some());
    });
}
