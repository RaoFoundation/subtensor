#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! Unit tests for the auxiliary helper functions in `pallet-limit-orders`.
//!
//! Extrinsics are NOT tested here. Each section focuses on one helper.

use frame_support::{BoundedVec, assert_noop, assert_ok, traits::ConstU32};
use sp_core::H256;
use sp_keyring::Sr25519Keyring as AccountKeyring;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

use sp_runtime::Perbill;

use crate::pallet::Pallet as LimitOrders;
use crate::{OrderEntry, OrderSide, OrderStatus, OrderType, Orders};

use super::mock::*;

// ─────────────────────────────────────────────────────────────────────────────
// net_amount_for_event
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn net_amount_for_event_buy_dominant() {
    new_test_ext().execute_with(|| {
        // Buys = 1000 TAO net, sells TAO-equiv = 300 TAO → net 700 TAO buy-side
        let price = U64F64::from_num(2u32); // 2 TAO/alpha
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Buy,
            1_000u128, // total_buy_net (TAO)
            150u128,   // total_sell_net (alpha)  ← not used in Buy branch
            300u128,   // total_sell_tao_equiv
            price,
        )
        .expect("conversion does not overflow");
        assert_eq!(net, 700u64);
    });
}

#[test]
fn net_amount_for_event_sell_dominant() {
    new_test_ext().execute_with(|| {
        // Sells = 500 alpha net, buys TAO = 200 TAO at price 2 → buy_alpha_equiv = 100
        // net sell = 500 - 100 = 400 alpha
        let price = U64F64::from_num(2u32); // 2 TAO/alpha → 1 alpha = 2 TAO
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Sell,
            200u128, // total_buy_net (TAO)
            500u128, // total_sell_net (alpha)
            400u128, // total_sell_tao_equiv (not used in Sell branch directly)
            price,
        )
        .expect("conversion does not overflow");
        // buy_alpha_equiv = 200 / 2 = 100; net = 500 - 100 = 400
        assert_eq!(net, 400u64);
    });
}

#[test]
fn net_amount_for_event_perfectly_offset() {
    new_test_ext().execute_with(|| {
        // Buys = 200 TAO, sells TAO-equiv = 200 → net = 0 (buy-side result = 0)
        let price = U64F64::from_num(2u32);
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Buy,
            200u128,
            100u128,
            200u128,
            price,
        )
        .expect("conversion does not overflow");
        assert_eq!(net, 0u64);
    });
}

#[test]
fn net_amount_for_event_sell_overflow_returns_error() {
    new_test_ext().execute_with(|| {
        let tiny_price = U64F64::from_bits(1);
        assert_eq!(
            LimitOrders::<Test>::net_amount_for_event(
                &OrderSide::Sell,
                u128::MAX,
                500u128,
                0u128,
                tiny_price,
            ),
            Err(Error::<Test>::ArithmeticOverflow.into()),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_separates_buys_and_sells() {
    new_test_ext().execute_with(|| {
        // Current time = 1_000_000 ms; expiry = 2_000_000 ms (well in the future).
        MockTime::set(1_000_000);
        // Price = 1.0 TAO/alpha.
        MockSwap::set_price(1.0);

        let buy_order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,         // amount in TAO
            2_000_000_000u64, // limit_price: willing to pay up to 2 TAO/alpha in ×10⁹ scale (scaled=1_000_000_000 ≤ 2_000_000_000 ✓)
            2_000_000u64,     // expiry ms
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let sell_order = make_signed_order(
            AccountKeyring::Bob,
            alice(),
            netuid(),
            OrderType::TakeProfit,
            500u64,           // amount in alpha
            1_000_000_000u64, // limit_price: sell if price >= 1 TAO/alpha in ×10⁹ scale (scaled=1_000_000_000 >= 1_000_000_000 ✓)
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![buy_order, sell_order]);
        let (buys, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1, "expected 1 valid buy");
        assert_eq!(sells.len(), 1, "expected 1 valid sell");

        // Buy entry: gross=1000, net=1000 (0% fee_rate)
        let buy = &buys[0];
        assert_eq!(buy.signer, alice());
        assert_eq!(buy.gross, 1_000u64);
        assert_eq!(buy.net, 1_000u64);
        assert_eq!(buy.fee_rate, Perbill::zero());

        // Sell entry: gross=500, net=500 (fee applied on TAO output, not alpha input)
        let sell = &sells[0];
        assert_eq!(sell.signer, bob());
        assert_eq!(sell.gross, 500u64);
        assert_eq!(sell.net, 500u64);
    });
}

#[test]
fn validate_and_classify_fails_for_wrong_netuid() {
    new_test_ext().execute_with(|| {
        // An order whose netuid does not match the batch netuid must cause a hard failure.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let wrong_netuid_order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            NetUid::from(99u16), // different netuid
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![wrong_netuid_order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(), // batch is for netuid 1
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderNetUidMismatch
        );
    });
}

#[test]
fn validate_and_classify_fails_for_expired_order() {
    new_test_ext().execute_with(|| {
        // now_ms = 2_000_001, expiry = 2_000_000 → expired → hard failure.
        MockTime::set(2_000_001);
        MockSwap::set_price(1.0);

        let expired = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,     // expiry already past
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![expired]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                2_000_001u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderExpired
        );
    });
}

#[test]
fn validate_and_classify_fails_for_price_condition_not_met_for_buy() {
    new_test_ext().execute_with(|| {
        // Price = 3.0 TAO/alpha, scaled = 3_000_000_000, buyer's limit = 2_000_000_000 (2.0 in ×10⁹) → scaled > limit → hard failure.
        MockTime::set(1_000_000);
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(3u32), // current price = 3 > limit 2 → fails
                bob()
            ),
            crate::Error::<Test>::PriceConditionNotMet
        );
    });
}

#[test]
fn validate_and_classify_fails_for_already_processed_order() {
    new_test_ext().execute_with(|| {
        // An order already marked Fulfilled must cause a hard failure.
        MockTime::set(1_000_000);
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        // Pre-mark as fulfilled on-chain.
        let oid = LimitOrders::<Test>::derive_order_id(&order.order);
        Orders::<Test>::insert(oid, OrderStatus::Fulfilled);

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn validate_and_classify_applies_buy_fee_to_net() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // 1_000_000 ppb = 0.1%
        // amount = 1_000_000_000, fee = 1_000_000, net = 999_000_000

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000_000_000u64,
            u64::MAX, // limit price: accept any price
            2_000_000u64,
            Perbill::from_parts(1_000_000), // 0.1% fee
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![order]);
        let (buys, _) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1);
        let entry = &buys[0];
        assert_eq!(entry.gross, 1_000_000_000u64);
        assert_eq!(entry.fee_rate, Perbill::from_parts(1_000_000));
        assert_eq!(entry.net, 999_000_000u64);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// compute_effective_swap_limit
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compute_effective_swap_limit_buy_no_slippage() {
    new_test_ext().execute_with(|| {
        // No slippage → u64::MAX (no ceiling).
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(true, 1_000, None);
        assert_eq!(limit, u64::MAX);
    });
}

#[test]
fn compute_effective_swap_limit_sell_no_slippage() {
    new_test_ext().execute_with(|| {
        // No slippage → 0 (no floor).
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(false, 1_000, None);
        assert_eq!(limit, 0);
    });
}

#[test]
fn compute_effective_swap_limit_buy_one_percent() {
    new_test_ext().execute_with(|| {
        // 1% slippage on a buy with limit_price=1000 → ceiling = 1010.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            true,
            1_000,
            Some(Perbill::from_percent(1)),
        );
        assert_eq!(limit, 1_010);
    });
}

#[test]
fn compute_effective_swap_limit_sell_one_percent() {
    new_test_ext().execute_with(|| {
        // 1% slippage on a sell with limit_price=1000 → floor = 990.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            false,
            1_000,
            Some(Perbill::from_percent(1)),
        );
        assert_eq!(limit, 990);
    });
}

#[test]
fn compute_effective_swap_limit_sell_saturates_at_zero() {
    new_test_ext().execute_with(|| {
        // 100% slippage on a sell with limit_price=500 → floor saturates at 0.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            false,
            500,
            Some(Perbill::from_percent(100)),
        );
        assert_eq!(limit, 0);
    });
}

#[test]
fn compute_effective_swap_limit_buy_saturates_at_u64_max() {
    new_test_ext().execute_with(|| {
        // 100% slippage on a buy with limit_price=u64::MAX → ceiling saturates at u64::MAX.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            true,
            u64::MAX,
            Some(Perbill::from_percent(100)),
        );
        assert_eq!(limit, u64::MAX);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify — effective_swap_limit propagation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_stores_effective_swap_limit_for_buy() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // 1% slippage on limit_price=2_000_000_000 (2.0 in ×10⁹) → ceiling = 2_020_000_000.
        // price=1.0, scaled=1_000_000_000 <= 2_000_000_000 ✓.
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            500u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        // Override max_slippage on the inner order after signing — we need to rebuild
        // the signed order so the signature covers the updated payload.
        let new_inner = {
            let mut o = order.order.inner().clone();
            o.max_slippage = Some(Perbill::from_percent(1));
            o
        };
        let versioned = crate::VersionedOrder::V1(new_inner.clone());
        let sig = AccountKeyring::Alice.pair().sign(&versioned.encode());
        let signed_with_slippage = crate::SignedOrder {
            order: versioned,
            signature: sp_runtime::MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        let orders = bounded(vec![signed_with_slippage]);
        let (buys, _) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("should succeed");

        assert_eq!(buys[0].effective_swap_limit, 2_020_000_000);
    });
}

#[test]
fn validate_and_classify_stores_effective_swap_limit_for_sell() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Price must be >= limit_price (in ×10⁹ scale) for TakeProfit to trigger.
        // limit_price=1_000_000_000 (1.0 in ×10⁹), 1% slippage → floor = 990_000_000.
        let new_inner = crate::Order {
            signer: AccountKeyring::Alice.to_account_id(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::TakeProfit,
            amount: 500u64,
            limit_price: 1_000_000_000u64, // 1.0 in ×10⁹ scale
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: Some(Perbill::from_percent(1)),
            chain_id: 945,
            partial_fills_enabled: false,
        };
        let versioned = crate::VersionedOrder::V1(new_inner);
        let sig = AccountKeyring::Alice.pair().sign(&versioned.encode());
        let signed = crate::SignedOrder {
            order: versioned,
            signature: sp_runtime::MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        let orders = bounded(vec![signed]);
        let (_, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(2u32), // current_price=2.0, scaled=2_000_000_000 >= limit_price=1_000_000_000 ✓
            bob(),
        )
        .expect("should succeed");

        assert_eq!(sells[0].effective_swap_limit, 990_000_000);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify — relayer enforcement
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_fails_for_wrong_relayer() {
    new_test_ext().execute_with(|| {
        // Order explicitly locks execution to charlie(); submitting as bob() must fail.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            u64::MAX,
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::try_from(vec![charlie()]).unwrap()), // only charlie may relay this order
        );

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob() // wrong relayer
            ),
            crate::Error::<Test>::RelayerMissMatch
        );
    });
}

#[test]
fn validate_and_classify_succeeds_for_correct_relayer() {
    new_test_ext().execute_with(|| {
        // Same setup as above but now the correct relayer (charlie) is used.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            u64::MAX,
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::try_from(vec![charlie()]).unwrap()), // only charlie may relay this order
        );

        let orders = bounded(vec![order]);
        let (buys, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            charlie(), // correct relayer
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1, "expected 1 valid buy");
        assert_eq!(sells.len(), 0);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// distribute_alpha_pro_rata
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario A – buy-dominant, pool rate = 1:1
// ───────────────────────────────────────────
// Both buyers and sellers are present, but buys exceed sells in TAO terms.
// Sellers are settled first (they receive TAO in distribute_tao_pro_rata).
// Their alpha (200 total) stays in the pallet account as passthrough for buyers.
// The residual buy TAO hits the pool and returns 800 alpha (at 1:1 rate).
//
// 3 buyers: Alice 300 TAO net, Bob 200 TAO net, Charlie 500 TAO net (total 1000)
// Sellers contributed 200 alpha (passthrough, no pool interaction).
// Net residual TAO to pool = 1000 - 200 = 800 TAO → pool returns 800 alpha (1:1).
// Total alpha available to buyers = 800 (pool) + 200 (seller passthrough) = 1000.
//
// Pro-rata shares (proportional to each buyer's net TAO):
//   Alice:   1000 * 300 / 1000 = 300 alpha
//   Bob:     1000 * 200 / 1000 = 200 alpha
//   Charlie: 1000 * 500 / 1000 = 500 alpha
//
// Scenario B – sell-dominant
// ───────────────────────────
// Both buyers and sellers are present, but sells exceed buys in TAO terms.
// Buyers are settled from the sellers' alpha directly (no pool for them).
// The residual sell alpha hits the pool; sellers receive TAO in distribute_tao_pro_rata.
//
// 2 buyers: Alice 400 TAO net, Bob 600 TAO net (total 1000)
// Price = 2.0 TAO/alpha → total alpha for buyers = 1000 / 2 = 500 alpha.
//
// Pro-rata shares:
//   Alice:  500 * 400 / 1000 = 200 alpha
//   Bob:    500 * 600 / 1000 = 300 alpha
//
// Scenario C – buy-dominant, pool rate != 1:1
// ────────────────────────────────────────────────────────
// Same structure as Scenario A but the pool returns fewer alpha than the TAO
// sent in, simulating realistic AMM. Pro-rata is computed over
// whatever the pool actually returned — the distribution logic is rate-agnostic.
//
// 3 buyers: Alice 300 TAO net, Bob 200 TAO net, Charlie 500 TAO net (total 1000)
// Sellers contributed 200 alpha (passthrough).
// Net residual TAO to pool = 800 TAO → pool returns 750 alpha (slippage).
// Total alpha available to buyers = 750 (pool) + 200 (seller passthrough) = 950.
//
// Pro-rata shares:
//   Alice:   950 * 300 / 1000 = 285 alpha
//   Bob:     950 * 200 / 1000 = 190 alpha
//   Charlie: 950 * 500 / 1000 = 475 alpha
//
// Scenario D – buy-dominant, indivisible remainder (dust)
// ─────────────────────────────────────────────────────────
// Integer division floors every share. The sum of floors is strictly less than
// total_alpha when total_alpha is not divisible by total_buy_net.
// The leftover alpha stays in the pallet intermediary account (never transferred).
//
// 3 buyers: Alice 1 TAO net, Bob 1 TAO net, Charlie 1 TAO net (total 3)
// Pool returns 10 alpha; no sellers → total_alpha = 10.
//
// Pro-rata shares (floor):
//   Alice:   floor(10 * 1 / 3) = 3 alpha
//   Bob:     floor(10 * 1 / 3) = 3 alpha
//   Charlie: floor(10 * 1 / 3) = 3 alpha
//   Total distributed: 9 alpha
//   Dust remaining in pallet account: 10 - 9 = 1 alpha (never transferred)

fn make_buy_entry(
    order_id: H256,
    signer: AccountId,
    hotkey: AccountId,
    gross: u64,
    net: u64,
    fee_rate: Perbill,
    fee_recipient: AccountId,
) -> OrderEntry<AccountId> {
    OrderEntry {
        order_id,
        signer,
        hotkey,
        side: OrderType::LimitBuy,
        gross,
        order_amount: gross,
        net,
        fee_rate,
        fee_recipient,
        effective_swap_limit: u64::MAX, // no slippage constraint
        partial_fill: None,
    }
}

fn bounded_buy_entries(
    v: Vec<OrderEntry<AccountId>>,
) -> BoundedVec<OrderEntry<AccountId>, ConstU32<64>> {
    BoundedVec::try_from(v).unwrap()
}

fn bounded_sell_entries(
    v: Vec<OrderEntry<AccountId>>,
) -> BoundedVec<OrderEntry<AccountId>, ConstU32<64>> {
    BoundedVec::try_from(v).unwrap()
}

#[test]
fn distribute_alpha_pro_rata_buy_dominant_scenario_a() {
    new_test_ext().execute_with(|| {
        // Pool returned 800 alpha; sell-side passthrough = 200 alpha.
        // Total = 1000 alpha distributed across 3 buyers (300, 200, 500 TAO net).
        // Expected shares: Alice 300, Bob 200, Charlie 500.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(1),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(2),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(3),
                charlie(),
                hotkey.clone(),
                500,
                500,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get(); // reuse as coldkey for brevity
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            800u128,   // actual_out from pool (alpha)
            1_000u128, // total_buy_net (TAO)
            200u128,   // total_sell_net (alpha passthrough)
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        // 3 transfers expected (one per buyer)
        assert_eq!(transfers.len(), 3);

        // Check each recipient's amount (signer is to_coldkey).
        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 300u64, "Alice should receive 300 alpha");
        assert_eq!(bob_amt, 200u64, "Bob should receive 200 alpha");
        assert_eq!(charlie_amt, 500u64, "Charlie should receive 500 alpha");
    });
}

#[test]
fn distribute_alpha_pro_rata_sell_dominant_scenario_b() {
    new_test_ext().execute_with(|| {
        // Price = 2.0 TAO/alpha; buyers have 400 + 600 = 1000 TAO net.
        // Total alpha = 1000 / 2 = 500.
        // Expected: Alice 200 alpha, Bob 300 alpha.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(4),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(5),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            0u128,     // actual_out unused in sell-dominant branch
            1_000u128, // total_buy_net (TAO)
            999u128,   // total_sell_net — doesn't matter for sell-dominant logic
            &OrderSide::Sell,
            U64F64::from_num(2u32), // price = 2 TAO/alpha
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 2);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 200u64, "Alice should receive 200 alpha");
        assert_eq!(bob_amt, 300u64, "Bob should receive 300 alpha");
    });
}

#[test]
fn distribute_alpha_pro_rata_buy_dominant_scenario_c() {
    new_test_ext().execute_with(|| {
        // Scenario C: same buyer setup as A but pool returns 750 alpha (slippage)
        // instead of 800. Proves pro-rata is computed over actual pool output and
        // is therefore rate-agnostic — the distribution logic doesn't assume 1:1.
        //
        // Net residual TAO to pool = 800 TAO → pool returns 750 alpha (not 800).
        // Total alpha = 750 (pool) + 200 (seller passthrough) = 950.
        //
        // Expected shares:
        //   Alice:   950 * 300 / 1000 = 285 alpha
        //   Bob:     950 * 200 / 1000 = 190 alpha
        //   Charlie: 950 * 500 / 1000 = 475 alpha

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(6),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(7),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(8),
                charlie(),
                hotkey.clone(),
                500,
                500,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            750u128,   // actual_out from pool (750, not 800 — slippage)
            1_000u128, // total_buy_net (TAO)
            200u128,   // total_sell_net (alpha passthrough)
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(
            alice_amt, 285u64,
            "Alice receives 950 * 300/1000 = 285 alpha"
        );
        assert_eq!(bob_amt, 190u64, "Bob receives 950 * 200/1000 = 190 alpha");
        assert_eq!(
            charlie_amt, 475u64,
            "Charlie receives 950 * 500/1000 = 475 alpha"
        );
    });
}

#[test]
fn distribute_alpha_pro_rata_dust_remains_in_pallet_scenario_d() {
    new_test_ext().execute_with(|| {
        // Scenario D: total_alpha = 10, three equal buyers (total_buy_net = 3).
        // floor(10 * 1/3) = 3 each → 9 distributed → 1 alpha dust stays in pallet.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        // Seed the pallet account with the 10 alpha it would hold after collect_assets
        // and the pool swap (actual_out=10, no sellers).
        MockSwap::set_alpha_balance(pallet_acct.clone(), pallet_hk.clone(), netuid(), 10);

        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(9),
                alice(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(10),
                bob(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(11),
                charlie(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            10u128, // actual_out from pool
            3u128,  // total_buy_net (TAO) — not divisible into 10 evenly
            0u128,  // total_sell_net — no sellers
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(bob_amt, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(charlie_amt, 3u64, "floor(10 * 1/3) = 3");

        // The pallet account started with 10 and sent out 9 — 1 alpha dust remains
        // in the pallet account, not burnt, not distributed.
        let pallet_remaining = MockSwap::alpha_balance(&pallet_acct, &pallet_hk, netuid());
        assert_eq!(
            pallet_remaining, 1u64,
            "1 alpha dust stays in pallet account, not burnt"
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// distribute_tao_pro_rata
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario A – sell-dominant, fee = 0
// ─────────────────────────────────────
// Both buyers and sellers are present, but sells exceed buys in TAO terms.
// Buyers are settled first (they receive alpha in distribute_alpha_pro_rata).
// The residual sell alpha hits the pool; pool returns TAO.
// Buy-side TAO also stays in pallet as passthrough for sellers.
//
// 2 sellers: Alice 400 alpha, Bob 600 alpha (total 1000 alpha)
// Price = 2.0 TAO/alpha → sell_tao_equiv: Alice 800, Bob 1200, total 2000.
// Pool returned 1200 TAO for the residual alpha; buy passthrough = 800 TAO.
// Total TAO available to sellers = 1200 (pool) + 800 (buy passthrough) = 2000.
//
// Pro-rata shares (proportional to each seller's TAO-equiv):
//   Alice:  2000 * 800 / 2000 = 800 TAO
//   Bob:    2000 * 1200 / 2000 = 1200 TAO
//
// Scenario B – sell-dominant, fee = 1% (10_000_000 ppb)
// ────────────────────────────────────────────────────────
// Same structure as Scenario A. Fee is deducted from each seller's gross TAO
// payout; the withheld TAO stays in the pallet account for collect_fees.
//
// Alice gross=800, fee=8 (1% of 800), net=792 TAO
// Bob   gross=1200, fee=12, net=1188 TAO
// Total sell fee returned: 20 TAO
//
// Scenario C – buy-dominant
// ──────────────────────────
// Both buyers and sellers are present, but buys exceed sells in TAO terms.
// Sellers receive their alpha valued at current_price — no pool interaction
// for them. The TAO they receive comes from the buyers' collected TAO directly.
//
// 2 sellers: Alice 300 alpha, Bob 200 alpha (total 500 alpha)
// Price = 2.0 TAO/alpha → sell_tao_equiv: Alice 600, Bob 400, total 1000.
// Buy-dominant branch: total_tao = total_sell_tao_equiv = 1000 TAO.
//
// Shares:
//   Alice:  1000 * 600 / 1000 = 600 TAO
//   Bob:    1000 * 400 / 1000 = 400 TAO
//
// Scenario D – sell-dominant, indivisible remainder (dust)
// ─────────────────────────────────────────────────────────
// Integer division floors every gross share. The leftover TAO stays in the
// pallet intermediary account (never transferred, not burnt).
//
// 3 sellers: Alice 1 alpha, Bob 1 alpha, Charlie 1 alpha (total 3 alpha)
// Price = 1.0 TAO/alpha → sell_tao_equiv = 1 each, total_sell_tao_equiv = 3.
// No buyers; actual_out from pool = 10 TAO, buy passthrough = 0.
// total_tao = 10 + 0 = 10.
//
// Pro-rata shares (floor):
//   Alice:   floor(10 * 1 / 3) = 3 TAO
//   Bob:     floor(10 * 1 / 3) = 3 TAO
//   Charlie: floor(10 * 1 / 3) = 3 TAO
//   Total distributed: 9 TAO
//   Dust remaining in pallet account: 10 - 9 = 1 TAO (never transferred)

#[test]
fn distribute_tao_pro_rata_sell_dominant_no_fee_scenario_a() {
    new_test_ext().execute_with(|| {
        // Price = 2, total_tao = 1200 (pool) + 800 (buy passthrough) = 2000
        // Alice alpha=400 → tao_equiv=800; Bob alpha=600 → tao_equiv=1200.
        // total_sell_tao_equiv = 2000.
        // Shares: Alice 800, Bob 1200.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(6),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(7),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            1_200u128, // actual_out (pool TAO)
            800u128,   // total_buy_net (buy passthrough TAO)
            2_000u128, // total_sell_tao_equiv (Alice 800 + Bob 1200)
            &OrderSide::Sell,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 800u64, "Alice should receive 800 TAO");
        assert_eq!(bob_tao, 1_200u64, "Bob should receive 1200 TAO");
        assert_eq!(
            sell_fees,
            vec![] as Vec<(AccountId, u64)>,
            "No fees at 0 ppb"
        );
    });
}

#[test]
fn distribute_tao_pro_rata_sell_dominant_with_fee_scenario_b() {
    new_test_ext().execute_with(|| {
        // Same setup as above but fee = 10_000_000 ppb = 1%.
        // Alice gross=800, fee=8, net=792; Bob gross=1200, fee=12, net=1188.
        // Total sell fee = 20.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(8),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::from_parts(10_000_000),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(9),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::from_parts(10_000_000),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            1_200u128,
            800u128,
            2_000u128,
            &OrderSide::Sell,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 792u64, "Alice net after 1% fee on 800");
        assert_eq!(bob_tao, 1_188u64, "Bob net after 1% fee on 1200");
        assert_eq!(
            sell_fees,
            vec![(fee_recipient(), 20u64)],
            "total sell fee = 8 + 12"
        );
    });
}

#[test]
fn distribute_tao_pro_rata_buy_dominant_scenario_c() {
    new_test_ext().execute_with(|| {
        // Buy-dominant: total_tao = total_sell_tao_equiv = 1000.
        // Alice alpha=300 → tao_equiv=600; Bob alpha=200 → tao_equiv=400.
        // Shares: Alice 600, Bob 400.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(10),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(11),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            0u128,     // actual_out unused in Buy-dominant branch
            0u128,     // total_buy_net unused in Buy-dominant branch
            1_000u128, // total_sell_tao_equiv (total_tao = this in Buy branch)
            &OrderSide::Buy,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 600u64, "Alice should receive 600 TAO");
        assert_eq!(bob_tao, 400u64, "Bob should receive 400 TAO");
        assert_eq!(sell_fees, vec![] as Vec<(AccountId, u64)>);
    });
}

#[test]
fn distribute_tao_pro_rata_dust_remains_in_pallet_scenario_d() {
    new_test_ext().execute_with(|| {
        // Scenario D: total_tao = 10, three equal sellers (total_sell_tao_equiv = 3).
        // floor(10 * 1/3) = 3 each → 9 distributed → 1 TAO dust stays in pallet.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let pallet_acct = PalletHotkeyAccount::get();

        // Seed the pallet account with the 10 TAO it would hold after collect_assets
        // and the pool swap (actual_out=10, no buyers).
        MockSwap::set_tao_balance(pallet_acct.clone(), 10);

        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(12),
                alice(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(13),
                bob(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(14),
                charlie(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            10u128, // actual_out from pool (TAO)
            0u128,  // total_buy_net — no buyers
            3u128,  // total_sell_tao_equiv — not divisible into 10 evenly
            &OrderSide::Sell,
            U64F64::from_num(1u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;
        let charlie_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &charlie())
            .unwrap()
            .2;

        assert_eq!(alice_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(bob_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(charlie_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(sell_fees, vec![] as Vec<(AccountId, u64)>);

        // The pallet account started with 10 TAO and sent out 9 — 1 TAO dust remains,
        // not burnt, not distributed.
        let pallet_remaining = MockSwap::tao_balance(&pallet_acct);
        assert_eq!(
            pallet_remaining, 1u64,
            "1 TAO dust stays in pallet account, not burnt"
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// collect_fees
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario:
// 2 buy orders with fees 50 and 150 TAO → total_buy_fee = 200 TAO.
// sell_fee_tao passed in = 80 TAO.
// Total fee = 280 TAO forwarded to FeeCollector in one transfer.

#[test]
fn collect_fees_forwards_combined_fees_to_collector() {
    new_test_ext().execute_with(|| {
        let hotkey = AccountKeyring::Dave.to_account_id();
        // Buy entries carry fee in field index 5.
        let buys = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(20),
                alice(),
                hotkey.clone(),
                1_000,
                950,
                Perbill::from_parts(50_000_000), // 5% of 1000 = 50
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(21),
                bob(),
                hotkey.clone(),
                1_500,
                1_350,
                Perbill::from_parts(100_000_000), // 10% of 1500 = 150
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        assert_ok!(LimitOrders::<Test>::collect_fees(
            &buys,
            vec![(fee_recipient(), 80u64)],
            &pallet_acct
        ));

        let tao_transfers = MockSwap::tao_transfers();
        assert_eq!(tao_transfers.len(), 1, "single transfer to fee_recipient");
        let (from, to, amount) = &tao_transfers[0];
        assert_eq!(from, &pallet_acct, "fee comes from pallet account");
        assert_eq!(to, &fee_recipient(), "fee goes to fee_recipient");
        assert_eq!(*amount, 280u64, "total fee = 200 (buy) + 80 (sell)");
    });
}

#[test]
fn collect_fees_no_transfer_when_zero_fees() {
    new_test_ext().execute_with(|| {
        // No buy fees, no sell fee.
        let hotkey = AccountKeyring::Dave.to_account_id();
        let buys = bounded_buy_entries(vec![make_buy_entry(
            H256::repeat_byte(22),
            alice(),
            hotkey,
            1_000,
            1_000,
            Perbill::zero(),
            fee_recipient(),
        )]);
        let pallet_acct = PalletHotkeyAccount::get();

        assert_ok!(LimitOrders::<Test>::collect_fees(
            &buys,
            vec![],
            &pallet_acct
        ));

        let tao_transfers = MockSwap::tao_transfers();
        assert_eq!(tao_transfers.len(), 0, "no transfer when total fee is zero");
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// is_order_valid
// ─────────────────────────────────────────────────────────────────────────────

use crate::Error;
use codec::Encode;
use sp_core::Pair;
use sp_runtime::MultiSignature;
use subtensor_swap_interface::OrderSwapInterface;

fn make_valid_signed_order() -> (crate::SignedOrder<AccountId>, sp_core::H256) {
    let keyring = AccountKeyring::Alice;
    let order = crate::VersionedOrder::V1(crate::Order {
        signer: keyring.to_account_id(),
        hotkey: AccountKeyring::Bob.to_account_id(),
        netuid: netuid(),
        order_type: OrderType::LimitBuy,
        amount: 1_000,
        limit_price: u64::MAX,
        expiry: u64::MAX,
        fee_rate: Perbill::zero(),
        fee_recipient: fee_recipient(),
        relayer: None,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    });
    let id = H256(sp_io::hashing::blake2_256(&order.encode()));
    let sig = keyring.pair().sign(&order.encode());
    let signed = crate::SignedOrder {
        order,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    };
    (signed, id)
}

#[test]
fn is_order_valid_returns_ok_for_well_formed_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, id) = make_valid_signed_order();
        let price = MockSwap::current_alpha_price(netuid());
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            price,
            &bob()
        ));
    });
}

#[test]
fn is_order_valid_invalid_signature_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (mut signed, id) = make_valid_signed_order();
        // Replace with a signature from a different key.
        let wrong_sig = AccountKeyring::Bob.pair().sign(&signed.order.encode());
        signed.signature = MultiSignature::Sr25519(wrong_sig);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

#[test]
fn is_order_valid_non_sr25519_signature_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (mut signed, id) = make_valid_signed_order();
        let ed_pair = sp_core::ed25519::Pair::from_legacy_string("//Alice", None);
        let ed_sig = ed_pair.sign(&signed.order.encode());
        signed.signature = MultiSignature::Ed25519(ed_sig);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

#[test]
fn is_order_valid_already_processed_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, id) = make_valid_signed_order();
        Orders::<Test>::insert(id, crate::OrderStatus::Fulfilled);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn is_order_valid_expired_order_returns_error() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        let (signed, _id) = make_valid_signed_order();
        // now_ms (2_000_001) > expiry (u64::MAX is fine, so use a low expiry order).
        // Re-build a signed order with a past expiry.
        let keyring = AccountKeyring::Alice;
        let order = crate::VersionedOrder::V1(crate::Order {
            expiry: 500_000,
            ..signed.order.inner().clone()
        });
        let id2 = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed2 = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed2, id2, 1_000_000, price, &bob()),
            Error::<Test>::OrderExpired
        );
    });
}

#[test]
fn is_order_valid_price_condition_not_met_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Price 5.0, scaled = 5_000_000_000 > limit_price 2_000_000_000 (2.0 in ×10⁹) → LimitBuy condition (scaled ≤ limit) not met.
        MockSwap::set_price(5.0);
        let keyring = AccountKeyring::Alice;
        let order = crate::VersionedOrder::V1(crate::Order {
            signer: keyring.to_account_id(),
            hotkey: AccountKeyring::Bob.to_account_id(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: 2_000_000_000, // 2.0 in ×10⁹ scale
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::PriceConditionNotMet
        );
    });
}

#[test]
fn is_order_valid_wrong_chain_id_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let keyring = AccountKeyring::Alice;
        // Build an order with a chain_id that doesn't match the mock config (945).
        let order = crate::VersionedOrder::V1(crate::Order {
            chain_id: 9999,
            ..make_valid_signed_order().0.order.inner().clone()
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::ChainIdMismatch
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// compute_order_status
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compute_order_status_no_partial_fill_returns_fulfilled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(1);
        // No existing state, no partial fill → Fulfilled immediately.
        let status = LimitOrders::<Test>::compute_order_status(id, None, 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_partial_fill_below_total_returns_partially_filled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(2);
        // First partial fill of 400 on a 1000-unit order → PartiallyFilled(400).
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::PartiallyFilled(400));
    });
}

#[test]
fn compute_order_status_partial_fill_exact_total_returns_fulfilled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(3);
        // Single partial fill that equals the full order amount → Fulfilled.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(1_000), 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_accumulates_previous_partial_fill() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(4);
        // Pre-seed storage as if a prior partial fill of 300 already happened.
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(300));

        // Second fill of 400 → 300 + 400 = 700, still below 1000.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::PartiallyFilled(700));
    });
}

#[test]
fn compute_order_status_completes_order_when_accumulated_total_reaches_amount() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(5);
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(600));

        // Fill the remaining 400 → 600 + 400 = 1000 = order_amount → Fulfilled.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_ignores_fulfilled_storage_when_no_partial_fill() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(6);
        // If somehow called with no partial_fill regardless of what's in storage
        // (should not happen in practice) it still returns Fulfilled.
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(500));
        let status = LimitOrders::<Test>::compute_order_status(id, None, 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}
