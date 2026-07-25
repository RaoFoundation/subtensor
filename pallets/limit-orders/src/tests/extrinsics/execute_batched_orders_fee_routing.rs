//! Extrinsic tests: execute batched orders fee routing.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// fee routing – multiple recipients
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_batched_orders_fees_routed_to_different_recipients() {
    new_test_ext().execute_with(|| {
        // Alice and Bob both buy; Alice's fee goes to charlie(), Bob's to dave().
        // fee = 1% for both orders.
        // Alice buys 1_000 TAO: fee = 10 → charlie().
        // Bob   buys 1_000 TAO: fee = 10 → dave().
        // Pool returns 900 alpha total for 1_980 TAO net.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(900);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_tao_balance(bob(), 1_000);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            charlie(),
            None,
        );
        let bob_buy = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            dave(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_buy, bob_buy]),
        ));

        // Each recipient gets exactly their order's fee.
        assert_eq!(
            MockSwap::tao_balance(&charlie()),
            10,
            "charlie gets Alice's fee"
        );
        assert_eq!(MockSwap::tao_balance(&dave()), 10, "dave gets Bob's fee");
    });
}

#[test]
fn execute_batched_orders_fees_batched_for_shared_recipient() {
    new_test_ext().execute_with(|| {
        // Both Alice and Bob's fees go to the same recipient (charlie()).
        // Expect a single combined transfer of 20 TAO to charlie().
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(900);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_tao_balance(bob(), 1_000);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            charlie(),
            None,
        );
        let bob_buy = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            charlie(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_buy, bob_buy]),
        ));

        // One combined transfer: charlie() receives 10 + 10 = 20 TAO.
        let fee_transfers: Vec<_> = MockSwap::tao_transfers()
            .into_iter()
            .filter(|(_, to, _)| to == &charlie())
            .collect();
        assert_eq!(
            fee_transfers.len(),
            1,
            "single transfer to shared recipient"
        );
        assert_eq!(fee_transfers[0].2, 20, "combined fee = 20 TAO");
    });
}

/// 4 orders split across 2 fee recipients.
///
/// Orders:
///   Alice  LimitBuy    1_000 TAO   fee_recipient = ferdie (buy-fee collector)
///   Bob    LimitBuy    1_000 TAO   fee_recipient = ferdie (buy-fee collector)
///   Charlie TakeProfit 1_000 α    fee_recipient = fee_recipient() (sell-fee collector)
///   Eve    TakeProfit  1_000 α    fee_recipient = fee_recipient() (sell-fee collector)
///
/// Neither ferdie nor fee_recipient() are order signers, so every TAO transfer
/// to those accounts is exclusively a fee transfer — making the single-transfer
/// assertion unambiguous.
///
/// At price 1.0 (1 TAO = 1 α), fee = 1%:
///   net buy TAO  = (1_000 - 10) + (1_000 - 10) = 1_980
///   sell α equiv = 2_000 TAO  →  sell-dominant, residual = 20 α → pool
///   pool returns 18 TAO for residual
///   total TAO for sellers = 18 + 1_980 = 1_998
///   each seller gross_share = 1_998 * 1_000 / 2_000 = 999
///   sell fee = mul_floor(1%, 999) = floor(9.99) = 9 TAO each
///
/// Expected:
///   ferdie          receives 10 (Alice) + 10 (Bob)   = 20 TAO (1 transfer)
///   fee_recipient() receives 9 (Charlie) + 9 (Eve)   = 18 TAO (1 transfer)
#[test]
fn execute_batched_orders_four_orders_two_fee_recipients() {
    new_test_ext().execute_with(|| {
        let ferdie = AccountKeyring::Ferdie.to_account_id();
        let eve = AccountKeyring::Eve.to_account_id();

        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(18);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_tao_balance(bob(), 1_000);
        MockSwap::set_alpha_balance(charlie(), dave(), netuid(), 1_000);
        MockSwap::set_alpha_balance(eve.clone(), dave(), netuid(), 1_000);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            ferdie.clone(),
            None,
        );
        let bob_buy = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            ferdie.clone(),
            None,
        );
        let charlie_sell = make_signed_order(
            AccountKeyring::Charlie,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );
        let eve_sell = make_signed_order(
            AccountKeyring::Eve,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(alice()),
            netuid(),
            bounded(vec![alice_buy, bob_buy, charlie_sell, eve_sell]),
        ));

        // ferdie collects Alice's and Bob's buy fees: 10 + 10 = 20 TAO in one transfer.
        let ferdie_transfers: Vec<_> = MockSwap::tao_transfers()
            .into_iter()
            .filter(|(_, to, _)| to == &ferdie)
            .collect();
        assert_eq!(ferdie_transfers.len(), 1, "single transfer to ferdie");
        assert_eq!(
            ferdie_transfers[0].2, 20,
            "ferdie receives 20 TAO in buy fees"
        );

        // fee_recipient() collects Charlie's and Eve's sell fees: 10 + 10 = 20 TAO in one transfer.
        let fp_transfers: Vec<_> = MockSwap::tao_transfers()
            .into_iter()
            .filter(|(_, to, _)| to == &fee_recipient())
            .collect();
        assert_eq!(fp_transfers.len(), 1, "single transfer to fee_recipient");
        assert_eq!(
            fp_transfers[0].2, 18,
            "fee_recipient receives 18 TAO in sell fees"
        );
    });
}

/// A mixed batch (buy + sell) must not rate-limit the pallet intermediary
/// account during asset collection, which would otherwise block the
/// subsequent alpha distribution to buyers.
///
/// Regression test: previously `transfer_staked_alpha` with a single
/// `apply_limits: true` flag set the rate-limit on `to_coldkey` (pallet)
/// during collection, then the distribution step checked `from_coldkey`
/// (pallet) and failed with `StakingOperationRateLimitExceeded`.
#[test]
fn execute_batched_orders_mixed_batch_does_not_rate_limit_pallet_intermediary() {
    new_test_ext().execute_with(|| {
        // Alice buys 1_000 TAO; Bob sells 500 alpha.
        // Buy-dominant: residual 500 TAO goes to pool, pool returns 400 alpha.
        // Total alpha = 400 (pool) + 500 (Bob passthrough) = 900 → all to Alice.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(400);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 500);

        let buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let sell = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            500,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        // Must succeed: collecting Bob's alpha must not rate-limit the pallet
        // intermediary, so distributing alpha to Alice is not blocked.
        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![buy, sell]),
        ));

        // Alice received staked alpha.
        assert!(
            MockSwap::alpha_balance(&alice(), &dave(), netuid()) > 0,
            "alice should hold staked alpha after the buy"
        );
        // Alice is rate-limited after receiving stake (set_receiver_limit=true).
        assert!(
            MockSwap::is_rate_limited(&dave(), &alice(), netuid()),
            "alice should be rate-limited after receiving stake"
        );
        // Bob's hotkey on the pallet side is NOT rate-limited (set_receiver_limit=false on collect).
        assert!(
            !MockSwap::is_rate_limited(&dave(), &bob(), netuid()),
            "bob's rate-limit should not be set by the collection step"
        );
    });
}
