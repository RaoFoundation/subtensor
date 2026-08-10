#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! Tests for **linked orders** — the v2 (`OrderV2`) feature.
//!
//! A linked order is sized as a fraction of the output another order already
//! produced. Two payload fields carry the whole mechanism: `has_linked_order`
//! (record my output so linked orders can draw on it) and
//! `OrderAmount::LinkedPercentage { provider, pct }` (spend `pct` of what
//! `provider` recorded).
//!
//! Covers:
//!   A. `OrderAmount` accessors and rendering in isolation.
//!   B. Provider recording — opt-in, post-fee amount, correct output asset.
//!   C. The rotation: sell → linked buy, inside one `execute_orders` call.
//!   D. Single-use records: a provider funds exactly one linked order.
//!   E. Every rejection path a linked amount can hit.
//!   F. Partial fills — rejected on both sides of a link.
//!   G. Buy providers (the "take profit on exactly what I bought" shape) and
//!      chains longer than two legs.
//!   H. `prune_linked_output`.
//!   I. The batched (netted) execution path.
//!   J. Clear-signing: v1 messages byte-unchanged, v2's new field and amount
//!      rendering, and end-to-end acceptance of a readable-signed linked order.
//!
//! The v1 suites elsewhere in this module are the regression net for the claim
//! that v1 behaviour is unchanged: v1 projects to `OrderAmount::Fixed` with
//! `has_linked_order = false` and takes the same code path it always did.

use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_core::crypto::{Ss58AddressFormat, Ss58Codec};
use sp_core::{H256, Pair};
use sp_keyring::Sr25519Keyring as AccountKeyring;
use sp_runtime::{MultiSignature, Perbill, traits::AccountIdConversion};

use crate::{
    Error, LinkedAsset, LinkedOutput, LinkedOutputs, OrderAmount, OrderStatus, OrderType, OrderV2,
    Orders, SignedOrder, VersionedOrder, pallet::Event,
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
/// condition is satisfied at any mock price — these tests are about linking, not
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
        has_linked_order: false,
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

/// A sell that records its proceeds — the canonical provider.
fn provider_sell(amount: u64) -> OrderV2<AccountId> {
    OrderV2 {
        amount: OrderAmount::Fixed(amount),
        has_linked_order: true,
        ..base_v2_order(OrderType::TakeProfit, OrderAmount::Fixed(amount))
    }
}

/// A buy sized as `pct` of `provider`'s recorded output, on `hotkey`.
fn linked_buy(provider: H256, pct: Perbill, hotkey: AccountId) -> OrderV2<AccountId> {
    OrderV2 {
        hotkey,
        ..base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage { provider, pct },
        )
    }
}

/// Run `orders` through `execute_orders` in all-or-nothing mode as `bob`.
fn execute(orders: Vec<SignedOrder<AccountId>>) -> frame_support::pallet_prelude::DispatchResult {
    LimitOrders::execute_orders(RuntimeOrigin::signed(bob()), bounded(orders), true)
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
fn order_amount_fixed_accessors() {
    let amount = OrderAmount::Fixed(1_234);
    assert_eq!(amount.fixed(), Some(1_234));
    assert_eq!(amount.linked(), None);
    assert!(!amount.is_linked());
}

#[test]
fn order_amount_linked_accessors() {
    let provider = H256::repeat_byte(0x07);
    let pct = Perbill::from_percent(33);
    let amount = OrderAmount::LinkedPercentage { provider, pct };
    assert_eq!(amount.fixed(), None);
    assert_eq!(amount.linked(), Some((provider, pct)));
    assert!(amount.is_linked());
}

#[test]
fn order_amount_rendering_is_injective_across_variants() {
    let provider = H256::repeat_byte(0xab);
    let fixed = OrderAmount::Fixed(500_000_000);
    let linked = OrderAmount::LinkedPercentage {
        provider,
        pct: Perbill::from_percent(50),
    };

    assert_eq!(fixed.render(), "500000000");
    assert_eq!(
        linked.render(),
        format!("500000000 ppb of order 0x{} output", "ab".repeat(32)),
    );

    // The suffix is what separates them, and no bare-digit rendering can produce it.
    assert_ne!(fixed.render(), linked.render());
    assert!(!fixed.render().contains(" ppb of order "));
    assert!(linked.render().ends_with(" output"));
}

#[test]
fn order_amount_linked_pct_cannot_exceed_one_hundred_percent() {
    use codec::{Decode, Encode};

    // Variant index 1, then the 32-byte provider, then a Perbill above 1e9.
    let mut bytes = vec![1u8];
    bytes.extend_from_slice(H256::repeat_byte(0x01).as_bytes());
    bytes.extend_from_slice(&(1_000_000_001u32).encode());

    assert!(
        OrderAmount::decode(&mut bytes.as_slice()).is_err(),
        "Perbill::decode must reject a fraction above 100%",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// B. Provider recording
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn order_without_has_linked_order_records_nothing() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(500);
        let order = base_v2_order(OrderType::TakeProfit, OrderAmount::Fixed(1_000));
        let signed = sign_v2(AccountKeyring::Alice, order);
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));

        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert!(
            LinkedOutputs::<Test>::get(id).is_none(),
            "the flag is opt-in; without it nothing may link to the order",
        );
    });
}

#[test]
fn sell_provider_records_post_fee_tao() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        // 10% fee on the TAO output, so only 900 actually lands with the signer.
        let order = OrderV2 {
            fee_rate: Perbill::from_percent(10),
            ..provider_sell(1_000)
        };
        let signed = sign_v2(AccountKeyring::Alice, order);
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));

        let record = LinkedOutputs::<Test>::get(id).expect("provider record");
        assert_eq!(record.signer, alice());
        assert_eq!(record.asset, LinkedAsset::Tao);
        assert_eq!(
            record.total, 900,
            "recording the gross would authorise spending TAO that left the account",
        );
        // now (1_000_000) + the mock TTL (3_600_000).
        assert_eq!(record.expires_at, 4_600_000);

        assert_event(Event::LinkedOutputRecorded {
            order_id: id,
            signer: alice(),
            asset: LinkedAsset::Tao,
            total: 900,
            expires_at: 4_600_000,
        });
    });
}

#[test]
fn buy_provider_records_alpha_on_its_own_position() {
    new_test_ext().execute_with(|| {
        MockSwap::set_buy_alpha_return(700);
        let order = OrderV2 {
            has_linked_order: true,
            ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000))
        };
        let signed = sign_v2(AccountKeyring::Alice, order);
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));

        let record = LinkedOutputs::<Test>::get(id).expect("provider record");
        assert_eq!(
            record.asset,
            LinkedAsset::Alpha {
                netuid: netuid(),
                hotkey: bob(),
            },
            "alpha is only fungible within one (netuid, hotkey) position",
        );
        assert_eq!(record.total, 700);
    });
}

#[test]
fn provider_producing_zero_output_records_nothing() {
    new_test_ext().execute_with(|| {
        // The pool returns nothing, so there is no output for anyone to draw against.
        MockSwap::set_sell_tao_return(0);
        let signed = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn v1_order_can_never_be_a_provider() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(500);
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));
        assert!(
            LinkedOutputs::<Test>::get(id).is_none(),
            "v1 payloads were signed before linking existed and cannot authorise it",
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// C. The rotation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn sell_then_linked_buy_spends_exactly_the_proceeds() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(500);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);

        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );
        let consumer_id = order_id(&consumer.order);

        // Both legs in one call: each order executes to completion before the next
        // is validated, so the record is already there when the consumer resolves.
        assert_ok!(execute(vec![provider, consumer]));

        assert_eq!(sell_alpha_amounts(), vec![1_000]);
        assert_eq!(
            buy_alpha_amounts(),
            vec![500],
            "the buy is sized by the sell's proceeds, not by the signer's balance",
        );

        assert_eq!(
            Orders::<Test>::get(provider_id),
            Some(OrderStatus::Fulfilled)
        );
        assert_eq!(
            Orders::<Test>::get(consumer_id),
            Some(OrderStatus::Fulfilled)
        );

        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_none(),
            "drawing consumes the record, which is also what stops it being reused",
        );
        assert_event(Event::LinkedOutputConsumed {
            provider: provider_id,
            consumer: consumer_id,
            amount: 500,
            undrawn: 0,
        });
    });
}

#[test]
fn linked_buy_resolves_against_a_provider_from_an_earlier_dispatch() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(500);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        // A separate dispatch entirely — this is the point of persisting the record.
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(40), charlie()),
        );
        let consumer_order = consumer.order.clone();
        assert_ok!(execute(vec![consumer]));

        assert_eq!(buy_alpha_amounts(), vec![200]);
        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_none(),
            "a record is single-use whatever fraction was drawn",
        );
        assert_event(Event::LinkedOutputConsumed {
            provider: provider_id,
            consumer: order_id(&consumer_order),
            amount: 200,
            undrawn: 300,
        });
    });
}

#[test]
fn consumer_fee_comes_out_of_the_drawn_amount() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);

        let consumer = OrderV2 {
            fee_rate: Perbill::from_percent(10),
            ..linked_buy(provider_id, Perbill::one(), charlie())
        };
        let consumer = sign_v2(AccountKeyring::Alice, consumer);

        assert_ok!(execute(vec![provider, consumer]));

        // Drawn: the full 1_000. Of that, 100 is fee and 900 reaches the pool.
        assert_eq!(buy_alpha_amounts(), vec![900]);
        assert_eq!(
            LimitOrders::pallet_account(),
            crate::pallet::Pallet::<Test>::pallet_account(),
        );
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// D. Single-use records and conservation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_second_linked_order_naming_the_same_provider_finds_nothing() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        let first = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(60), charlie()),
        );
        assert_ok!(execute(vec![first]));

        // Independently signed `pct` values can sum past 100%. Removing the record
        // on the first draw — rather than debiting a counter — is what makes the
        // over-draw unrepresentable instead of merely rejected.
        let second = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(60), dave()),
        );
        assert_noop!(execute(vec![second]), Error::<Test>::NoLinkedOutput);
        assert_eq!(buy_alpha_amounts(), vec![600]);
    });
}

#[test]
fn a_second_linked_order_in_the_same_call_finds_nothing() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        let first = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(50), charlie()),
        );
        let second = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(50), dave()),
        );

        // The first draw's removal is visible to the second within the same dispatch.
        assert_noop!(
            execute(vec![provider, first, second]),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn drawing_less_than_all_of_it_leaves_the_rest_with_the_signer() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);
        MockSwap::set_tao_balance(alice(), 0);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(30), charlie()),
        );
        assert_ok!(execute(vec![provider, consumer]));

        assert_eq!(buy_alpha_amounts(), vec![300]);
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
        // The record was only ever an authorisation cap; the undrawn 700 never left
        // the signer's balance in the first place.
        assert_eq!(MockSwap::tao_balance(&alice()), 700);
    });
}

#[test]
fn a_consumer_that_fails_to_swap_leaves_the_record_intact() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        MockSwap::set_swap_fail(true);
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(50), charlie()),
        );
        assert!(execute(vec![consumer]).is_err());

        let record = LinkedOutputs::<Test>::get(provider_id).expect("record untouched");
        assert_eq!(
            record.total, 1_000,
            "the record is consumed only after the trade lands",
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// E. Rejections
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn linked_order_naming_an_unknown_provider_is_rejected() {
    new_test_ext().execute_with(|| {
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(H256::repeat_byte(0x99), Perbill::one(), charlie()),
        );
        assert_noop!(execute(vec![consumer]), Error::<Test>::NoLinkedOutput);
    });
}

#[test]
fn linked_order_signed_by_someone_else_is_rejected() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        // Bob signs a buy against Alice's proceeds. The TAO is in Alice's account,
        // so Bob would be spending his own funds on Alice's authorisation.
        let consumer = OrderV2 {
            signer: bob(),
            ..linked_buy(provider_id, Perbill::one(), charlie())
        };
        let consumer = sign_v2(AccountKeyring::Bob, consumer);

        assert_noop!(
            execute(vec![consumer]),
            Error::<Test>::LinkedOutputSignerMismatch
        );
    });
}

#[test]
fn buy_consumer_cannot_draw_against_an_alpha_record() {
    new_test_ext().execute_with(|| {
        MockSwap::set_buy_alpha_return(700);
        // A buy provider produces alpha, which a buy cannot spend.
        let provider = OrderV2 {
            has_linked_order: true,
            ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000))
        };
        let provider = sign_v2(AccountKeyring::Alice, provider);
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );
        assert_noop!(
            execute(vec![consumer]),
            Error::<Test>::LinkedOutputAssetMismatch
        );
    });
}

#[test]
fn sell_consumer_cannot_draw_against_alpha_on_a_different_position() {
    new_test_ext().execute_with(|| {
        MockSwap::set_buy_alpha_return(700);
        MockSwap::set_sell_tao_return(400);

        // Provider buys alpha on (netuid 1, bob).
        let provider = OrderV2 {
            has_linked_order: true,
            ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000))
        };
        let provider = sign_v2(AccountKeyring::Alice, provider);
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        // Consumer tries to sell from (netuid 1, charlie) — a different position.
        let consumer = OrderV2 {
            hotkey: charlie(),
            ..base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::LinkedPercentage {
                    provider: provider_id,
                    pct: Perbill::one(),
                },
            )
        };
        let consumer = sign_v2(AccountKeyring::Alice, consumer);
        assert_noop!(
            execute(vec![consumer]),
            Error::<Test>::LinkedOutputAssetMismatch
        );
    });
}

#[test]
fn linked_order_against_an_expired_record_is_rejected() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        let expires_at = LinkedOutputs::<Test>::get(provider_id)
            .expect("record")
            .expires_at;
        MockTime::set(expires_at.saturating_add(1));

        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );
        assert_noop!(execute(vec![consumer]), Error::<Test>::LinkedOutputExpired);
        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_some(),
            "expiry is checked independently of whether anyone has pruned yet",
        );
    });
}

#[test]
fn linked_fraction_flooring_to_zero_is_rejected() {
    new_test_ext().execute_with(|| {
        LinkedOutputs::<Test>::insert(
            H256::repeat_byte(0x05),
            LinkedOutput {
                signer: alice(),
                asset: LinkedAsset::Tao,
                total: 1,
                expires_at: FAR_FUTURE,
            },
        );

        // 1 ppb of 1 raw unit floors to zero.
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(H256::repeat_byte(0x05), Perbill::from_parts(1), charlie()),
        );
        assert_noop!(
            execute(vec![consumer]),
            Error::<Test>::LinkedAmountResolvedToZero
        );
    });
}

#[test]
fn a_drawn_out_record_cannot_be_reused() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );
        assert_ok!(execute(vec![provider, consumer]));

        // A different consumer, signed against the same now-spent provider.
        let late = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(10), dave()),
        );
        assert_noop!(execute(vec![late]), Error::<Test>::NoLinkedOutput);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// F. Partial fills
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn partial_fill_against_a_linked_order_is_rejected() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        let consumer = OrderV2 {
            partial_fills_enabled: true,
            relayer: Some(BoundedVec::try_from(vec![bob()]).unwrap()),
            ..linked_buy(provider_id, Perbill::one(), charlie())
        };
        let consumer = sign_v2_with_partial_fill(AccountKeyring::Alice, consumer, 100);

        assert_noop!(
            execute(vec![consumer]),
            Error::<Test>::PartialFillNotSupportedForLinkedAmount
        );
    });
}

#[test]
fn partial_fill_against_a_provider_is_rejected() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(300);

        let order = OrderV2 {
            partial_fills_enabled: true,
            relayer: Some(BoundedVec::try_from(vec![bob()]).unwrap()),
            ..provider_sell(1_000)
        };
        let signed = sign_v2_with_partial_fill(AccountKeyring::Alice, order, 400);
        let id = order_id(&signed.order);

        // Filling in instalments would make the recorded total depend on how the
        // relayer sliced the fills, so a provider must execute in one shot.
        assert_noop!(
            execute(vec![signed]),
            Error::<Test>::PartialFillNotSupportedForProvider
        );
        assert!(Orders::<Test>::get(id).is_none());
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn a_provider_without_a_partial_fill_still_executes_with_fills_enabled() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(300);

        // The rejection is on the submitted fill, not on the signed flag: a full
        // execution of the same payload is fine and records normally.
        let order = OrderV2 {
            partial_fills_enabled: true,
            relayer: Some(BoundedVec::try_from(vec![bob()]).unwrap()),
            ..provider_sell(1_000)
        };
        let signed = sign_v2(AccountKeyring::Alice, order);
        let id = order_id(&signed.order);

        assert_ok!(execute(vec![signed]));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        let record = LinkedOutputs::<Test>::get(id).expect("record");
        assert_eq!(record.total, 300);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// G. Buy providers and longer chains
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn buy_then_linked_sell_takes_profit_on_exactly_what_was_bought() {
    new_test_ext().execute_with(|| {
        MockSwap::set_buy_alpha_return(700);
        MockSwap::set_sell_tao_return(1_500);

        let provider = OrderV2 {
            has_linked_order: true,
            ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000))
        };
        let provider = sign_v2(AccountKeyring::Alice, provider);
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        // The take-profit sells 100% of the alpha that buy produced, from the very
        // same (netuid, hotkey) position — and no more.
        let consumer = base_v2_order(
            OrderType::TakeProfit,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
        );
        let consumer = sign_v2(AccountKeyring::Alice, consumer);
        assert_ok!(execute(vec![consumer]));

        assert_eq!(sell_alpha_amounts(), vec![700]);
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn linked_orders_chain_when_a_consumer_is_also_a_provider() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(800);

        // A: sell 2_000 alpha → 1_000 TAO recorded.
        let a = sign_v2(AccountKeyring::Alice, provider_sell(2_000));
        let a_id = order_id(&a.order);

        // B: buy with 50% of A's proceeds, and record its own alpha output.
        let b = OrderV2 {
            has_linked_order: true,
            ..linked_buy(a_id, Perbill::from_percent(50), charlie())
        };
        let b = sign_v2(AccountKeyring::Alice, b);
        let b_id = order_id(&b.order);

        // C: sell 100% of the alpha B produced.
        let c = OrderV2 {
            hotkey: charlie(),
            ..base_v2_order(
                OrderType::TakeProfit,
                OrderAmount::LinkedPercentage {
                    provider: b_id,
                    pct: Perbill::one(),
                },
            )
        };
        let c = sign_v2(AccountKeyring::Alice, c);

        assert_ok!(execute(vec![a, b, c]));

        assert_eq!(buy_alpha_amounts(), vec![500]);
        assert_eq!(sell_alpha_amounts(), vec![2_000, 800]);

        // Both records were drawn from once and are gone. A's undrawn 500 TAO
        // stays with Alice as ordinary balance.
        assert!(LinkedOutputs::<Test>::get(a_id).is_none());
        assert!(LinkedOutputs::<Test>::get(b_id).is_none());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// H. Pruning
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn signer_can_prune_their_own_record_at_any_time() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(alice()),
            provider_id
        ));

        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
        assert_event(Event::LinkedOutputPruned {
            order_id: provider_id,
            total: 1_000,
        });

        // Revoking the authorisation is exactly what makes the consumer unexecutable.
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );
        assert_noop!(execute(vec![consumer]), Error::<Test>::NoLinkedOutput);
    });
}

#[test]
fn a_stranger_can_only_prune_after_expiry() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        assert_noop!(
            LimitOrders::prune_linked_output(RuntimeOrigin::signed(dave()), provider_id),
            Error::<Test>::LinkedOutputNotPrunable
        );

        let expires_at = LinkedOutputs::<Test>::get(provider_id)
            .expect("record")
            .expires_at;
        MockTime::set(expires_at.saturating_add(1));

        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(dave()),
            provider_id
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn pruning_an_absent_record_fails() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            LimitOrders::prune_linked_output(
                RuntimeOrigin::signed(alice()),
                H256::repeat_byte(0x42)
            ),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn pruning_works_while_the_pallet_is_disabled() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        crate::LimitOrdersEnabled::<Test>::set(false);

        // Disabling the pallet must not freeze cleanup of state it already wrote.
        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(alice()),
            provider_id
        ));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// I. Batched (netted) execution
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn batched_provider_records_its_pro_rata_output() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(900);

        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);
        MockSwap::set_tao_balance(pallet_acct, 10_000);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(bob()),
            netuid(),
            bounded(vec![provider]),
        ));

        let record = LinkedOutputs::<Test>::get(provider_id).expect("provider record");
        assert_eq!(record.asset, LinkedAsset::Tao);
        assert_eq!(
            record.total, 900,
            "the pro-rata payout is what actually reached the seller",
        );
    });
}

#[test]
fn batched_consumer_draws_against_an_earlier_dispatchs_record() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(700);

        let provider_id = H256::repeat_byte(0x33);
        LinkedOutputs::<Test>::insert(
            provider_id,
            LinkedOutput {
                signer: alice(),
                asset: LinkedAsset::Tao,
                total: 1_000,
                expires_at: FAR_FUTURE,
            },
        );

        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        MockSwap::set_tao_balance(alice(), 10_000);
        MockSwap::set_tao_balance(pallet_acct, 10_000);

        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(50), charlie()),
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(bob()),
            netuid(),
            bounded(vec![consumer]),
        ));

        assert_eq!(buy_alpha_amounts(), vec![500]);
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn batched_provider_and_its_consumer_in_one_call_is_rejected() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(900);
        MockSwap::set_buy_alpha_return(700);

        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        let consumer = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::one(), charlie()),
        );

        // Structural, not incidental: the batched path resolves every amount before
        // the netted swap that would produce the provider's output even runs.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(bob()),
                netuid(),
                bounded(vec![provider, consumer]),
            ),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn a_second_batched_linked_order_finds_nothing() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(700);

        let provider_id = H256::repeat_byte(0x33);
        LinkedOutputs::<Test>::insert(
            provider_id,
            LinkedOutput {
                signer: alice(),
                asset: LinkedAsset::Tao,
                total: 1_000,
                expires_at: FAR_FUTURE,
            },
        );

        let first = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(60), charlie()),
        );
        let second = sign_v2(
            AccountKeyring::Alice,
            linked_buy(provider_id, Perbill::from_percent(60), dave()),
        );

        // The first draw's removal is visible to the second within the same batch.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(bob()),
                netuid(),
                bounded(vec![first, second]),
            ),
            Error::<Test>::NoLinkedOutput
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// J. Clear-signing
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn v1_rendering_is_byte_identical_to_before_v2() {
    new_test_ext().execute_with(|| {
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            2_000_000_000,
            1_700_000_000_000,
            Perbill::from_perthousand(5),
            fee_recipient(),
            None,
        );
        let rendered = String::from_utf8(LimitOrders::render_order(&signed.order)).unwrap();

        assert_eq!(
            rendered,
            format!(
                "TAO.com order v1: Limit buy 1000 on subnet 1, \
limit price 2000000000, expiry 1700000000000, hotkey {hotkey}, \
fee 5000000 to {fee_recipient}, relayer none, \
max slippage none, chain 945, partial fills false, signer {signer}",
                hotkey = canonical_ss58(&bob()),
                fee_recipient = canonical_ss58(&fee_recipient()),
                signer = canonical_ss58(&alice()),
            ),
        );
        assert!(
            !rendered.contains("has-linked-order"),
            "v1 has no linking concept and must render no trace of one",
        );
    });
}

#[test]
fn v2_rendering_carries_the_provider_flag_and_the_linked_amount() {
    new_test_ext().execute_with(|| {
        let provider = H256::repeat_byte(0x2c);

        let plain = base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000));
        let rendered =
            String::from_utf8(LimitOrders::render_order(&VersionedOrder::V2(plain))).unwrap();
        assert!(rendered.starts_with("TAO.com order v2: Limit buy 1000 on subnet 1, "));
        assert!(rendered.ends_with(", has-linked-order false"));

        let recorded = OrderV2 {
            has_linked_order: true,
            ..base_v2_order(OrderType::LimitBuy, OrderAmount::Fixed(1_000))
        };
        let rendered =
            String::from_utf8(LimitOrders::render_order(&VersionedOrder::V2(recorded))).unwrap();
        assert!(rendered.ends_with(", has-linked-order true"));

        let linked = base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider,
                pct: Perbill::from_percent(25),
            },
        );
        let rendered =
            String::from_utf8(LimitOrders::render_order(&VersionedOrder::V2(linked))).unwrap();
        assert!(
            rendered.contains(&format!(
                "Limit buy 250000000 ppb of order 0x{} output on subnet 1,",
                "2c".repeat(32),
            )),
            "unexpected rendering: {rendered}",
        );
    });
}

#[test]
fn v1_and_v2_renderings_of_the_same_fields_differ() {
    new_test_ext().execute_with(|| {
        let v1 = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let v2 = VersionedOrder::V2(base_v2_order(
            OrderType::LimitBuy,
            OrderAmount::Fixed(1_000),
        ));

        assert_ne!(
            LimitOrders::render_order(&v1.order),
            LimitOrders::render_order(&v2),
            "the version tag alone must keep a v1 signature from being replayed as v2",
        );
    });
}

#[test]
fn readable_signed_linked_order_executes_end_to_end() {
    new_test_ext().execute_with(|| {
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(700);

        let provider = sign_v2(AccountKeyring::Alice, provider_sell(1_000));
        let provider_id = order_id(&provider.order);
        assert_ok!(execute(vec![provider]));

        // Sign the clear-signing form rather than the wrapped hash — the Ledger path.
        let versioned = VersionedOrder::V2(linked_buy(
            provider_id,
            Perbill::from_percent(50),
            charlie(),
        ));
        let sig = AccountKeyring::Alice
            .pair()
            .sign(&readable_signed_bytes(&versioned));
        let consumer = SignedOrder {
            order: versioned,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        assert_ok!(execute(vec![consumer]));
        assert_eq!(buy_alpha_amounts(), vec![500]);
    });
}

#[test]
fn readable_payload_for_a_linked_order_still_lands_in_the_hashed_branch() {
    new_test_ext().execute_with(|| {
        // The 64-hex provider id lengthens the message; it must not push it out of
        // the branch `verify_readable` already relies on.
        let versioned = VersionedOrder::V2(linked_buy(
            H256::repeat_byte(0x11),
            Perbill::one(),
            charlie(),
        ));
        let msg = LimitOrders::render_order(&versioned);
        let payload = [b"<Bytes>".as_slice(), &msg, b"</Bytes>".as_slice()].concat();
        assert!(
            payload.len() > crate::LEDGER_MAX_SIGN_SIZE,
            "must be hashed on-device, not signed bare",
        );
    });
}
