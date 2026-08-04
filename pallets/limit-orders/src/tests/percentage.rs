#![allow(clippy::indexing_slicing)]
//! Tests for percentage-denominated order amounts — the v2 (`OrderV2`) feature.
//!
//! Covers:
//!   A. `OrderAmount` arithmetic and rendering in isolation.
//!   B. `resolve_amount` on the buy side (fraction of transferable TAO).
//!   C. `resolve_amount` on the sell side (fraction of staked alpha at the order's hotkey).
//!   D. Fee interaction — the fee comes out of the *resolved* amount.
//!   E. Partial fills, which are rejected for percentage amounts but still work for v2 `Fixed`.
//!   F. The batched (netted) execution path.
//!   G. Clear-signing: the `v2` version tag, the percentage rendering, and end-to-end
//!      acceptance of a readable-signed v2 order.
//!
//! The v1 suites elsewhere in this module are the regression net for the claim that
//! v1 behaviour is unchanged: v1 projects to `OrderAmount::Fixed` and takes the same
//! code path it always did.

use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_core::Pair;
use sp_core::crypto::{Ss58AddressFormat, Ss58Codec};
use sp_keyring::Sr25519Keyring as AccountKeyring;
use sp_runtime::{MultiSignature, Perbill, traits::AccountIdConversion};
use subtensor_runtime_common::NetUid;

use crate::{
    Error, OrderAmount, OrderStatus, OrderType, OrderV2, Orders, SignedOrder, VersionedOrder,
    pallet::Event,
};

type LimitOrders = crate::pallet::Pallet<Test>;

use super::mock::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn assert_event(event: Event<Test>) {
    assert!(
        System::events()
            .iter()
            .any(|r| r.event == RuntimeEvent::LimitOrders(event.clone())),
        "expected event not found: {event:?}",
    );
}

fn canonical_ss58(acct: &AccountId) -> String {
    let prefix =
        <<Test as frame_system::Config>::SS58Prefix as frame_support::traits::Get<u16>>::get();
    acct.to_ss58check_with_version(Ss58AddressFormat::custom(prefix))
}

/// A v2 order that clears every non-amount guard under the default mock setup:
/// netuid 1, chain 945, far-future expiry, no relayer restriction, no fee.
///
/// `LimitBuy` uses a `u64::MAX` ceiling and `TakeProfit` a `0` floor so the price
/// condition is satisfied at any mock price — these tests are about the amount, not
/// the trigger.
fn base_v2_order(order_type: OrderType, amount: OrderAmount) -> OrderV2<AccountId> {
    let limit_price = if order_type.is_buy() { u64::MAX } else { 0 };
    OrderV2 {
        signer: alice(),
        hotkey: bob(),
        netuid: netuid(),
        order_type,
        amount,
        limit_price,
        expiry: FAR_FUTURE,
        fee_rate: Perbill::zero(),
        fee_recipient: fee_recipient(),
        relayer: None,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    }
}

/// Sign a v2 order in the wrapped-hash form (`order_signing_payload`), the same form
/// the v1 mock helpers use.
fn sign_v2(keyring: AccountKeyring, order: OrderV2<AccountId>) -> SignedOrder<AccountId> {
    let versioned = VersionedOrder::V2(order);
    let sig = keyring.pair().sign(&order_signing_payload(&versioned));
    SignedOrder {
        order: versioned,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

/// As [`sign_v2`], but injects a `partial_fill` into the envelope.
fn sign_v2_with_partial_fill(
    keyring: AccountKeyring,
    order: OrderV2<AccountId>,
    partial_fill: u64,
) -> SignedOrder<AccountId> {
    let mut signed = sign_v2(keyring, order);
    signed.partial_fill = Some(partial_fill);
    signed
}

/// The bytes a Ledger actually signs for the readable form: the `<Bytes>`-wrapped
/// canonical message, blake2_256-hashed when it exceeds the device's raw-signing limit.
/// Mirrors `verify_readable`.
fn readable_signed_bytes(order: &VersionedOrder<AccountId>) -> Vec<u8> {
    let msg = LimitOrders::render_order(order);
    let payload = [b"<Bytes>".as_slice(), &msg, b"</Bytes>".as_slice()].concat();
    if payload.len() > crate::LEDGER_MAX_SIGN_SIZE {
        sp_core::hashing::blake2_256(&payload).to_vec()
    } else {
        payload
    }
}

/// Independent reconstruction of the canonical clear-signing message — deliberately
/// not a copy of the production `format!`, so a change to either side is caught.
#[allow(clippy::too_many_arguments)]
fn expected_message(
    version: &str,
    label: &str,
    price_word: &str,
    amount_str: &str,
    netuid_val: u16,
    limit_price: u64,
    expiry: u64,
    hotkey: &AccountId,
    fee_rate_ppb: u32,
    fee_recipient: &AccountId,
    relayer_str: &str,
    max_slippage_str: &str,
    chain_id: u64,
    partial: bool,
    signer: &AccountId,
) -> String {
    format!(
        "TAO.com order {version}: {label} {amount_str} on subnet {netuid_val}, \
{price_word} {limit_price}, expiry {expiry}, hotkey {hotkey}, \
fee {fee_rate_ppb} to {fee_recipient}, relayer {relayer_str}, \
max slippage {max_slippage_str}, chain {chain_id}, \
partial fills {partial}, signer {signer}",
        hotkey = canonical_ss58(hotkey),
        fee_recipient = canonical_ss58(fee_recipient),
        signer = canonical_ss58(signer),
    )
}

/// The `tao` argument of every `buy_alpha` call, in order.
fn buy_alpha_amounts() -> Vec<u64> {
    MockSwap::log()
        .into_iter()
        .filter_map(|c| match c {
            SwapCall::BuyAlpha { tao, .. } => Some(tao),
            _ => None,
        })
        .collect()
}

/// The `alpha` argument of every `sell_alpha` call, in order.
fn sell_alpha_amounts() -> Vec<u64> {
    MockSwap::log()
        .into_iter()
        .filter_map(|c| match c {
            SwapCall::SellAlpha { alpha, .. } => Some(alpha),
            _ => None,
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// A. OrderAmount in isolation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn order_amount_fixed_resolve_ignores_balance() {
    let amount = OrderAmount::Fixed(1_234);
    // The balance is irrelevant for a fixed amount — this is what keeps v1 orders
    // from ever depending on the signer's balance.
    assert_eq!(amount.resolve(0), 1_234);
    assert_eq!(amount.resolve(u64::MAX), 1_234);
    assert!(!amount.is_percentage());
    assert_eq!(amount.fixed(), Some(1_234));
}

#[test]
fn order_amount_percentage_resolve_floors_against_balance() {
    let half = OrderAmount::Percentage(Perbill::from_percent(50));
    assert_eq!(half.resolve(1_000), 500);
    assert_eq!(half.resolve(0), 0);
    // Floors, not rounds: 50% of 1 is 0.5 → 0.
    assert_eq!(half.resolve(1), 0);
    // 50% of 3 is 1.5 → 1.
    assert_eq!(half.resolve(3), 1);

    let full = OrderAmount::Percentage(Perbill::one());
    assert_eq!(full.resolve(1_000), 1_000);
    assert_eq!(full.resolve(u64::MAX), u64::MAX);

    assert!(half.is_percentage());
    assert_eq!(half.fixed(), None);
}

#[test]
fn order_amount_percentage_cannot_exceed_one_hundred_percent() {
    // `Perbill` is capped at 1e9 by construction, so a percentage can never resolve
    // to more than the balance. This is what makes a 100% order the true maximum.
    let over = OrderAmount::Percentage(Perbill::from_parts(1_000_000_000));
    assert_eq!(over.resolve(1_000), 1_000);
}

/// The rendered amount is part of the signed clear-signing message, so `Fixed` and
/// `Percentage` must never produce the same string — otherwise a signature over one
/// could be presented for the other.
#[test]
fn order_amount_render_is_injective_across_variants() {
    // `Fixed` renders as bare digits — this is what keeps v1 messages byte-identical.
    assert_eq!(OrderAmount::Fixed(0).render(), "0");
    assert_eq!(OrderAmount::Fixed(1_234_567).render(), "1234567");
    assert_eq!(
        OrderAmount::Fixed(u64::MAX).render(),
        "18446744073709551615"
    );

    // `Percentage` always carries the suffix, so it can never be read as a bare amount.
    assert_eq!(
        OrderAmount::Percentage(Perbill::from_percent(50)).render(),
        "500000000 ppb of balance"
    );
    assert_eq!(
        OrderAmount::Percentage(Perbill::one()).render(),
        "1000000000 ppb of balance"
    );

    // The numerically-colliding pair: same underlying integer, different renderings.
    assert_ne!(
        OrderAmount::Fixed(500_000_000).render(),
        OrderAmount::Percentage(Perbill::from_percent(50)).render()
    );

    // No `Fixed` rendering can ever parse-collide with a `Percentage` one.
    for parts in [0u32, 1, 500_000_000, 1_000_000_000] {
        let pct = OrderAmount::Percentage(Perbill::from_parts(parts)).render();
        assert!(
            pct.parse::<u64>().is_err(),
            "percentage rendering {pct:?} must not be a bare integer"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// B. Buy side: percentage of transferable TAO
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn percentage_buy_resolves_against_tao_balance() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(250);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::LimitBuy,
                OrderAmount::Percentage(Perbill::from_percent(50)),
            ),
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        // 50% of 1_000 = 500 reaches the pool (no fee).
        assert_eq!(buy_alpha_amounts(), vec![500]);
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderExecuted {
            order_id: id,
            signer: alice(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount_in: 500,
            amount_out: 250,
        });
    });
}

#[test]
fn percentage_buy_of_full_balance_spends_whole_balance() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1_000);
        MockSwap::set_tao_balance(alice(), 777);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(OrderType::LimitBuy, OrderAmount::Percentage(Perbill::one())),
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        // A 100% order resolves to the entire transferable balance. The mock has no
        // existential deposit; in the runtime this is the keep-alive figure, which is
        // exactly why a 100% order can clear `buy_alpha` without reaping the account.
        assert_eq!(buy_alpha_amounts(), vec![777]);
        assert_eq!(MockSwap::tao_balance(&alice()), 0);
    });
}

#[test]
fn percentage_buy_resolving_to_zero_is_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        // 50% of 1 floors to 0.
        MockSwap::set_tao_balance(alice(), 1);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::LimitBuy,
                OrderAmount::Percentage(Perbill::from_percent(50)),
            ),
        );

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PercentageAmountResolvedToZero
        );
        // No swap attempted — the order stays retryable once the balance grows.
        assert!(buy_alpha_amounts().is_empty());
    });
}

#[test]
fn percentage_buy_with_zero_balance_is_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        // No balance seeded for alice at all.

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(OrderType::LimitBuy, OrderAmount::Percentage(Perbill::one())),
        );
        let id = order_id(&signed.order);

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PercentageAmountResolvedToZero
        );
        // Critically, the order is NOT marked terminal — it must remain executable.
        assert_eq!(Orders::<Test>::get(id), None);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// C. Sell side: percentage of staked alpha at the order's hotkey
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn percentage_sell_resolves_against_staked_alpha() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        MockSwap::set_sell_tao_return(400);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 800);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::Percentage(Perbill::from_percent(25)),
            ),
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        // 25% of 800 = 200 alpha sold.
        assert_eq!(sell_alpha_amounts(), vec![200]);
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

/// The sell-side balance must be the stake at the order's *own* hotkey, not the
/// signer's stake anywhere on the subnet. Seeding a second, larger position under a
/// different hotkey would silently inflate the amount if the wrong key were read.
#[test]
fn percentage_sell_reads_stake_at_the_orders_hotkey_only() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        MockSwap::set_sell_tao_return(100);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 400);
        // A much larger position under a different hotkey that must be ignored.
        MockSwap::set_alpha_balance(alice(), dave(), netuid(), 10_000);

        // `base_v2_order` uses bob() as the hotkey.
        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::Percentage(Perbill::one()),
            ),
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        // 100% of the bob() position (400), not of the dave() position or their sum.
        assert_eq!(sell_alpha_amounts(), vec![400]);
        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 10_000);
    });
}

#[test]
fn percentage_sell_resolving_to_zero_is_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        // 10% of 9 floors to 0.
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 9);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::Percentage(Perbill::from_percent(10)),
            ),
        );

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PercentageAmountResolvedToZero
        );
        assert!(sell_alpha_amounts().is_empty());
    });
}

#[test]
fn percentage_stop_loss_also_resolves_against_staked_alpha() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(0.5);
        MockSwap::set_sell_tao_return(50);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 600);

        // StopLoss is a sell, so it must read the alpha side too.
        let mut order = base_v2_order(
            OrderType::StopLoss,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        );
        order.limit_price = u64::MAX; // StopLoss triggers when price <= limit.
        let signed = sign_v2(AccountKeyring::Alice, order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        assert_eq!(sell_alpha_amounts(), vec![300]);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// D. Fee interaction
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn percentage_buy_fee_is_taken_out_of_the_resolved_amount() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(100);
        MockSwap::set_tao_balance(alice(), 1_000);

        let mut order = base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        );
        order.fee_rate = Perbill::from_percent(1);
        let signed = sign_v2(AccountKeyring::Alice, order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        // Resolved amount = 50% of 1_000 = 500. Fee = 1% of 500 = 5.
        // The fee comes out of the resolved amount, so total spend stays 500 — a 100%
        // order can never need more TAO than the signer holds.
        assert_eq!(buy_alpha_amounts(), vec![495]);
        assert!(
            MockSwap::tao_transfers().contains(&(alice(), fee_recipient(), 5)),
            "fee of 5 must be forwarded to the fee recipient",
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// E. Partial fills
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn percentage_order_rejects_partial_fill() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Everything a partial fill would otherwise need: relayer set and the flag on.
        let mut order = base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        );
        order.relayer = Some(BoundedVec::try_from(vec![charlie()]).unwrap());
        order.partial_fills_enabled = true;
        let signed = sign_v2_with_partial_fill(AccountKeyring::Alice, order, 100);

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PartialFillNotSupportedForPercentageAmount
        );
    });
}

/// The percentage check must fire *before* the relayer / flag checks, so the caller
/// gets told the real reason rather than being sent down a dead end: satisfying
/// `RelayerRequiredForPartialFill` and `PartialFillsNotEnabled` would not make a
/// percentage order partially fillable.
#[test]
fn percentage_partial_fill_rejection_takes_precedence_over_relayer_and_flag_checks() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // No relayer and the flag off — both of which would also be errors.
        let order = base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        );
        assert!(order.relayer.is_none());
        assert!(!order.partial_fills_enabled);
        let signed = sign_v2_with_partial_fill(AccountKeyring::Alice, order, 100);

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PartialFillNotSupportedForPercentageAmount
        );
    });
}

/// The rejection is scoped to percentage amounts only: a v2 order with a `Fixed`
/// amount partially fills exactly like its v1 equivalent.
#[test]
fn v2_fixed_amount_order_still_supports_partial_fills() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(40);
        MockSwap::set_tao_balance(alice(), 1_000);

        let mut order = base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000));
        order.relayer = Some(BoundedVec::try_from(vec![charlie()]).unwrap());
        order.partial_fills_enabled = true;
        let signed = sign_v2_with_partial_fill(AccountKeyring::Alice, order, 400);
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        assert_eq!(buy_alpha_amounts(), vec![400]);
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(400)),
        );
    });
}

/// A percentage order is all-or-nothing, so a full execution is terminal in one shot.
#[test]
fn percentage_order_without_partial_fill_is_fulfilled_in_one_shot() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(100);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::LimitBuy,
                OrderAmount::Percentage(Perbill::from_percent(50)),
            ),
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed.clone()]),
            true,
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));

        // Replay is refused by the terminal status, even though the signer still has
        // a balance a percentage could be taken from.
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::OrderAlreadyProcessed
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// F. Batched (netted) execution
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn batched_percentage_buy_collects_the_resolved_amount() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_tao_balance(bob(), 400);

        // Alice: 60% of 1_000 = 600. Bob: fixed 400. Total net 1_000.
        let alice_order = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                hotkey: dave(),
                ..base_v2_order(
                    OrderType::LimitBuy,
                    OrderAmount::Percentage(Perbill::from_percent(60)),
                )
            },
        );
        let bob_order = sign_v2(
            AccountKeyring::Bob,
            OrderV2 {
                signer: bob(),
                hotkey: dave(),
                ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(400))
            },
        );
        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order]),
        ));

        // `collect_assets` pulls the resolved 600 from alice, not some other figure.
        let transfers = MockSwap::tao_transfers();
        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        assert!(
            transfers.contains(&(alice(), pallet_acct.clone(), 600)),
            "expected 600 TAO collected from alice, got {transfers:?}",
        );
        assert!(transfers.contains(&(bob(), pallet_acct, 400)));

        // Pro-rata on 500 alpha over a 1_000 net: alice 300, bob 200.
        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));
        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 300);
        assert_eq!(MockSwap::alpha_balance(&bob(), &dave(), netuid()), 200);
    });
}

#[test]
fn batched_percentage_sell_resolves_against_staked_alpha() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(800);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 800);

        let signed = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::Percentage(Perbill::one()),
            ),
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![signed]),
        ));

        // The whole 800 alpha position is collected into the intermediary.
        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        let pallet_hotkey = PalletHotkeyAccount::get();
        assert!(
            MockSwap::alpha_transfers().contains(&(
                alice(),
                bob(),
                pallet_acct,
                pallet_hotkey,
                netuid(),
                800,
            )),
            "expected the full 800 alpha position to be collected",
        );
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

/// Documents the same-signer aliasing case: percentage amounts in one batch are all
/// resolved up front, before `collect_assets` moves anything, so two 100% orders from
/// the same signer each resolve to the *full* balance rather than splitting it.
///
/// The mock's ledger saturates instead of erroring, so the over-collection is visible
/// here as two full-size debits. In the runtime `transfer_staked_alpha` rejects the
/// second one (`NotEnoughStakeToWithdraw`), which hard-fails and rolls back the whole
/// batch — safe, but the batch builder is responsible for not composing such a batch.
#[test]
fn batched_same_signer_percentage_orders_each_resolve_against_the_full_balance() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(1_600);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 800);

        // Two distinct 100% sell orders from the same signer (different expiry keeps
        // their order_ids distinct, so this is not the duplicate-in-batch case).
        let first = sign_v2(
            AccountKeyring::Alice,
            base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::Percentage(Perbill::one()),
            ),
        );
        let second = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                expiry: FAR_FUTURE - 1,
                ..base_v2_order(
                    OrderType::TakeProfit,
                    OrderAmount::Percentage(Perbill::one()),
                )
            },
        );
        assert_ne!(order_id(&first.order), order_id(&second.order));

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![first, second]),
        ));

        // Each resolved to the full 800 — they did not split the position.
        let collected: Vec<u64> = MockSwap::alpha_transfers()
            .into_iter()
            .filter(|(from_coldkey, _, _, _, _, _)| from_coldkey == &alice())
            .map(|(_, _, _, _, _, amount)| amount)
            .collect();
        assert_eq!(collected, vec![800, 800]);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// G. Clear-signing (readable form) for v2
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn render_order_v2_percentage_golden() {
    new_test_ext().execute_with(|| {
        let order = OrderV2 {
            signer: alice(),
            hotkey: bob(),
            netuid: NetUid::from(7u16),
            order_type: OrderType::LimitBuy,
            amount: OrderAmount::Percentage(Perbill::from_percent(25)),
            limit_price: 2_000_000_000,
            expiry: 9_999_999,
            fee_rate: Perbill::from_parts(5_000_000),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        };
        let rendered = LimitOrders::render_order(&VersionedOrder::V2(order));
        let expected = expected_message(
            "v2",
            "Limit buy",
            "limit price",
            "250000000 ppb of balance",
            7,
            2_000_000_000,
            9_999_999,
            &bob(),
            5_000_000,
            &fee_recipient(),
            "none",
            "none",
            945,
            false,
            &alice(),
        );
        assert_eq!(String::from_utf8(rendered.clone()).unwrap(), expected);
        // Ledger-renderability invariant: every byte must be printable ASCII.
        for (i, b) in rendered.iter().enumerate() {
            assert!(
                (0x20..=0x7e).contains(b),
                "byte {i} = {b:#x} is not printable ASCII"
            );
        }
    });
}

#[test]
fn render_order_v2_fixed_golden() {
    new_test_ext().execute_with(|| {
        let order = base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_234_567));
        let rendered = LimitOrders::render_order(&VersionedOrder::V2(order));
        let expected = expected_message(
            "v2",
            "Limit buy",
            "limit price",
            // A fixed amount renders as bare digits under v2 too — only the version
            // tag distinguishes it from the v1 message.
            "1234567",
            1,
            u64::MAX,
            FAR_FUTURE,
            &bob(),
            0,
            &fee_recipient(),
            "none",
            "none",
            945,
            false,
            &alice(),
        );
        assert_eq!(String::from_utf8(rendered).unwrap(), expected);
    });
}

/// A v1 order and a v2 `Fixed` order with otherwise identical fields must not share a
/// signable identity: neither the rendered message nor the order id may collide, or a
/// signature for one could be replayed as the other.
#[test]
fn v1_and_v2_fixed_orders_are_not_interchangeable() {
    new_test_ext().execute_with(|| {
        let v1 = VersionedOrder::V1(crate::Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let v2 = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Fixed(1_000),
        ));

        assert_ne!(
            LimitOrders::render_order(&v1),
            LimitOrders::render_order(&v2),
            "the version tag must distinguish the two messages",
        );
        assert_ne!(
            order_id(&v1),
            order_id(&v2),
            "the SCALE variant index must distinguish the two order ids",
        );
        assert_eq!(v1.version_tag(), "v1");
        assert_eq!(v2.version_tag(), "v2");
    });
}

/// A `Fixed(500_000_000)` and a `Percentage(50%)` order share the same underlying
/// integer. They must still render — and hash — differently.
#[test]
fn v2_fixed_and_percentage_with_the_same_integer_are_not_interchangeable() {
    new_test_ext().execute_with(|| {
        let fixed = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Fixed(500_000_000),
        ));
        let pct = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        ));

        assert_ne!(
            LimitOrders::render_order(&fixed),
            LimitOrders::render_order(&pct),
        );
        assert_ne!(order_id(&fixed), order_id(&pct));
    });
}

/// End-to-end: a v2 percentage order signed in the human-readable ("clear-signing")
/// form — the only form a Ledger can produce — is accepted and executed.
#[test]
fn readable_signed_v2_percentage_order_executes() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(300);
        MockSwap::set_tao_balance(alice(), 1_000);

        let versioned = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(30)),
        ));
        let sig = AccountKeyring::Alice
            .pair()
            .sign(&readable_signed_bytes(&versioned));
        let signed = SignedOrder {
            order: versioned,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let id = order_id(&signed.order);

        assert!(
            LimitOrders::verify_readable(&signed),
            "readable-signed v2 order must pass verify_readable",
        );
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        assert_eq!(buy_alpha_amounts(), vec![300]);
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

/// A signature over the v1 rendering of an order must not validate the v2 order with
/// the same fields — the concrete replay the version tag exists to prevent.
#[test]
fn v1_readable_signature_does_not_validate_the_v2_order() {
    new_test_ext().execute_with(|| {
        let v1 = VersionedOrder::V1(crate::Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        // Sign the v1 message, then transplant the signature onto the v2 payload.
        let sig = AccountKeyring::Alice
            .pair()
            .sign(&readable_signed_bytes(&v1));
        let transplanted = SignedOrder {
            order: VersionedOrder::V2(base_v2_order(
                OrderType::LimitBuy,
                OrderAmount::Fixed(1_000),
            )),
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        assert!(!LimitOrders::verify_readable(&transplanted));
        assert!(!LimitOrders::verify_order(&transplanted));
        let id = order_id(&transplanted.order);
        assert!(!LimitOrders::verify_wrapped(&transplanted, id));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// H. cancel_order works for v2
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cancel_order_works_for_v2_percentage_order() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Percentage(Perbill::from_percent(50)),
        ));
        let id = order_id(&order);

        // Cancellation reads the signer through `VersionedOrder::signer()`, which must
        // work for v2 — and must not require resolving the amount (no balance seeded).
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            order.clone()
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Cancelled));

        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(bob()), order),
            Error::<Test>::Unauthorized
        );
    });
}
