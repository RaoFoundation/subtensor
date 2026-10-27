#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Linked v2 orders: a consumer is sized as a fraction of a provider's
//! already-produced output.

use codec::{Decode, Encode};
use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_core::{H256, Pair};
use sp_keyring::Sr25519Keyring as AccountKeyring;
use sp_runtime::{MultiSignature, Perbill, traits::AccountIdConversion};
use subtensor_runtime_common::NetUid;

use crate::{
    Error, LEDGER_MAX_SIGN_SIZE, LimitOrdersEnabled, LinkedAsset, LinkedOutput, LinkedOutputs,
    OrderAmount, OrderStatus, OrderType, OrderV2, Orders, VersionedOrder, pallet::Event,
};

type LimitOrders = crate::pallet::Pallet<Test>;

use super::mock::*;

fn assert_event(event: Event<Test>) {
    assert!(
        System::events()
            .iter()
            .any(|r| r.event == RuntimeEvent::LimitOrders(event.clone())),
        "expected event not found: {event:?}",
    );
}

fn setup_rotation_balances() {
    MockTime::set(1_000_000);
    MockSwap::set_price(1.0);
    MockSwap::set_sell_tao_return(400);
    MockSwap::set_buy_alpha_return(350);
    MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);
    MockSwap::set_tao_balance(alice(), 0);
}

fn sign_v2(keyring: AccountKeyring, order: OrderV2<AccountId>) -> crate::SignedOrder<AccountId> {
    let versioned = VersionedOrder::V2(order);
    let sig = keyring.pair().sign(&order_signing_payload(&versioned));
    crate::SignedOrder {
        order: versioned,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

fn v2_base(order_type: OrderType, amount: OrderAmount) -> OrderV2<AccountId> {
    let limit_price = match order_type {
        OrderType::TakeProfit => 0,
        OrderType::LimitBuy | OrderType::StopLoss => u64::MAX,
    };
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

fn readable_signed_bytes(order: &VersionedOrder<AccountId>) -> Vec<u8> {
    let msg = LimitOrders::render_order(order);
    let payload = [
        b"<Bytes>".as_slice(),
        msg.as_slice(),
        b"</Bytes>".as_slice(),
    ]
    .concat();
    if payload.len() > LEDGER_MAX_SIGN_SIZE {
        sp_core::hashing::blake2_256(&payload).to_vec()
    } else {
        payload
    }
}

fn swap_buys() -> Vec<u64> {
    MockSwap::log()
        .into_iter()
        .filter_map(|c| match c {
            SwapCall::BuyAlpha { tao, .. } => Some(tao),
            _ => None,
        })
        .collect()
}

fn swap_sells() -> Vec<u64> {
    MockSwap::log()
        .into_iter()
        .filter_map(|c| match c {
            SwapCall::SellAlpha { alpha, .. } => Some(alpha),
            _ => None,
        })
        .collect()
}

#[test]
fn execute_orders_rotates_sell_then_linked_buy() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            NetUid::from(2u16),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        let consumer_id = order_id(&consumer.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider, consumer]),
            true,
        ));

        assert_eq!(
            Orders::<Test>::get(provider_id),
            Some(OrderStatus::Fulfilled)
        );
        assert_eq!(
            Orders::<Test>::get(consumer_id),
            Some(OrderStatus::Fulfilled)
        );
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
        assert_event(Event::LinkedOutputRecorded {
            order_id: provider_id,
            signer: alice(),
            asset: LinkedAsset::Tao,
            total: 400,
            expires_at: 1_000_000 + 86_400_000,
        });
        assert_event(Event::LinkedOutputConsumed {
            provider: provider_id,
            consumer: consumer_id,
            amount: 400,
            undrawn: 0,
        });

        let buys = MockSwap::log()
            .into_iter()
            .filter_map(|c| match c {
                SwapCall::BuyAlpha { tao, .. } => Some(tao),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(buys, vec![400]);
    });
}

#[test]
fn execute_orders_take_profit_on_exact_buy_output() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(250);
        MockSwap::set_sell_tao_return(200);
        MockSwap::set_tao_balance(alice(), 1_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::Fixed(500),
            u64::MAX,
            true,
        );
        let provider_id = order_id(&provider.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let record = LinkedOutputs::<Test>::get(provider_id).expect("provider recorded");
        assert_eq!(record.total, 250);
        assert_eq!(
            record.asset,
            LinkedAsset::Alpha {
                netuid: netuid(),
                hotkey: bob(),
            }
        );

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            0,
            false,
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![consumer]),
            true,
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());

        let sells = MockSwap::log()
            .into_iter()
            .filter_map(|c| match c {
                SwapCall::SellAlpha { alpha, .. } => Some(alpha),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sells, vec![250]);
    });
}

#[test]
fn linked_percentage_draws_only_pct_and_consumes_record() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(25),
            },
            u64::MAX,
            false,
        );
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![consumer]),
            true,
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());

        let buys = MockSwap::log()
            .into_iter()
            .filter_map(|c| match c {
                SwapCall::BuyAlpha { tao, .. } => Some(tao),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(buys, vec![100]);
    });
}

#[test]
fn second_consumer_of_same_provider_fails() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        let first = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(10),
            },
            u64::MAX,
            false,
        );
        let second = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(10),
            },
            u64::MAX - 1,
            false,
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider, first]),
            true,
        ));
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![second]),
                true,
            ),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn batched_provider_and_consumer_same_call_fails() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();
        MockSwap::set_tao_balance(alice(), 1_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![provider, consumer]),
            ),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn batched_consumer_of_earlier_provider_succeeds() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();
        MockSwap::set_tao_balance(alice(), 1_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![consumer]),
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn linked_rejections() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let missing = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: sp_core::H256::repeat_byte(0xab),
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![missing]),
                true,
            ),
            Error::<Test>::NoLinkedOutput
        );

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let wrong_signer = make_signed_v2_order(
            AccountKeyring::Bob,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![wrong_signer]),
                true,
            ),
            Error::<Test>::LinkedOutputSignerMismatch
        );

        let wrong_asset = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            0,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![wrong_asset]),
                true,
            ),
            Error::<Test>::LinkedOutputAssetMismatch
        );

        MockTime::set(1_000_000 + 86_400_000 + 1);
        let expired = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![expired]),
                true,
            ),
            Error::<Test>::LinkedOutputExpired
        );
    });
}

#[test]
fn linked_amount_resolved_to_zero_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(1);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_parts(1),
            },
            u64::MAX,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![consumer]),
                true,
            ),
            Error::<Test>::LinkedAmountResolvedToZero
        );
    });
}

#[test]
fn partial_fill_rejected_for_provider_and_consumer() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let mut provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        provider.partial_fill = Some(100);
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![provider]),
                true,
            ),
            Error::<Test>::PartialFillNotSupportedForProvider
        );

        LinkedOutputs::<Test>::insert(
            sp_core::H256::repeat_byte(0x11),
            crate::LinkedOutput {
                signer: alice(),
                asset: LinkedAsset::Tao,
                total: 400,
                expires_at: u64::MAX,
            },
        );
        let mut consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: sp_core::H256::repeat_byte(0x11),
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        consumer.partial_fill = Some(100);
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![consumer]),
                true,
            ),
            Error::<Test>::PartialFillNotSupportedForLinkedAmount
        );
    });
}

#[test]
fn failed_swap_does_not_consume_record() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_some());

        MockSwap::set_swap_fail(true);
        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );
        assert!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![consumer]),
                true,
            )
            .is_err()
        );
        assert!(LinkedOutputs::<Test>::get(provider_id).is_some());
    });
}

#[test]
fn prune_linked_output_rules() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        assert_noop!(
            LimitOrders::prune_linked_output(RuntimeOrigin::signed(bob()), provider_id),
            Error::<Test>::LinkedOutputNotPrunable
        );

        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(alice()),
            provider_id
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
        assert_event(Event::LinkedOutputPruned {
            order_id: provider_id,
            total: 400,
        });

        assert_noop!(
            LimitOrders::prune_linked_output(RuntimeOrigin::signed(alice()), provider_id),
            Error::<Test>::NoLinkedOutput
        );
    });
}

#[test]
fn anyone_can_prune_after_expiry() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        MockTime::set(1_000_000 + 86_400_000 + 1);
        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(bob()),
            provider_id
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn v2_fixed_without_flag_is_v1_equivalent() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(90);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::Fixed(100),
            u64::MAX,
            false,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn v2_readable_message_includes_link_fields() {
    new_test_ext().execute_with(|| {
        let provider = sp_core::H256::repeat_byte(0xab);
        let order = VersionedOrder::V2(crate::OrderV2 {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: OrderAmount::LinkedPercentage {
                provider,
                pct: Perbill::from_percent(25),
            },
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
            has_linked_order: true,
        });
        let rendered = String::from_utf8(LimitOrders::render_order(&order)).unwrap();
        assert!(rendered.starts_with("TAO.com order v2:"));
        assert!(rendered.contains(
            "250000000 ppb of order 0xabababababababababababababababababababababababababababababababab output"
        ));
        assert!(rendered.contains("has-linked-order true"));
    });
}

#[test]
fn v1_readable_message_unchanged() {
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
        let rendered = String::from_utf8(LimitOrders::render_order(&v1)).unwrap();
        assert!(rendered.starts_with("TAO.com order v1:"));
        assert!(!rendered.contains("has-linked-order"));
        assert!(rendered.contains("Limit buy 1000 on subnet 1"));
    });
}

#[test]
fn cancel_v2_order() {
    new_test_ext().execute_with(|| {
        let signed = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::Fixed(100),
            u64::MAX,
            false,
        );
        let id = order_id(&signed.order);
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            signed.order
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Cancelled));
    });
}

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
    assert_ne!(fixed.render(), linked.render());
    assert!(!fixed.render().contains(" ppb of order "));
    assert!(linked.render().ends_with(" output"));
}

#[test]
fn order_amount_linked_pct_cannot_exceed_one_hundred_percent() {
    let mut bytes = vec![1u8];
    bytes.extend_from_slice(H256::repeat_byte(0x01).as_bytes());
    bytes.extend_from_slice(&(1_000_000_001u32).encode());

    assert!(
        OrderAmount::decode(&mut bytes.as_slice()).is_err(),
        "Perbill::decode must reject a fraction above 100%",
    );
}

#[test]
fn order_without_has_linked_order_records_nothing() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let signed = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            false,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn sell_provider_records_post_fee_tao() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();
        MockSwap::set_sell_tao_return(1_000);

        let signed = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                fee_rate: Perbill::from_percent(10),
                has_linked_order: true,
                ..v2_base(OrderType::TakeProfit, OrderAmount::Fixed(500))
            },
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));

        let record = LinkedOutputs::<Test>::get(id).expect("provider record");
        assert_eq!(record.signer, alice());
        assert_eq!(record.asset, LinkedAsset::Tao);
        assert_eq!(
            record.total, 900,
            "recording the gross would authorise spending TAO that left the account",
        );
        assert_eq!(record.expires_at, 1_000_000 + 86_400_000);
    });
}

#[test]
fn provider_producing_zero_output_records_nothing() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();
        MockSwap::set_sell_tao_return(0);

        let signed = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn v1_order_can_never_be_a_provider() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));
        assert!(LinkedOutputs::<Test>::get(id).is_none());
    });
}

#[test]
fn consumer_fee_comes_out_of_the_drawn_amount() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let consumer = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                fee_rate: Perbill::from_percent(10),
                ..v2_base(
                    OrderType::LimitBuy,
                    OrderAmount::LinkedPercentage {
                        provider: provider_id,
                        pct: Perbill::one(),
                    },
                )
            },
        );
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![consumer]),
            true,
        ));

        assert_eq!(
            swap_buys(),
            vec![360],
            "fee is taken from the drawn 400 TAO before the swap",
        );
    });
}

#[test]
fn second_consumer_in_the_same_call_fails() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let first = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(10),
            },
            u64::MAX,
            false,
        );
        let second = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(10),
            },
            u64::MAX - 1,
            false,
        );

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![first, second]),
                true,
            ),
            Error::<Test>::NoLinkedOutput
        );
        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_some(),
            "the failed call must roll back the first draw",
        );
    });
}

#[test]
fn sell_from_a_different_hotkey_than_the_buy_provider_fails() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(250);
        MockSwap::set_tao_balance(alice(), 1_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::Fixed(500),
            u64::MAX,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let consumer = make_signed_v2_order(
            AccountKeyring::Alice,
            charlie(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            0,
            false,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(dave()),
                bounded(vec![consumer]),
                true,
            ),
            Error::<Test>::LinkedOutputAssetMismatch
        );
    });
}

#[test]
fn provider_with_fills_enabled_still_executes_without_a_partial_fill() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let signed = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                partial_fills_enabled: true,
                relayer: Some(BoundedVec::try_from(vec![charlie()]).unwrap()),
                has_linked_order: true,
                ..v2_base(OrderType::TakeProfit, OrderAmount::Fixed(500))
            },
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            true,
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        let record = LinkedOutputs::<Test>::get(id).expect("record");
        assert_eq!(record.total, 400);
    });
}

#[test]
fn linked_orders_chain_when_a_consumer_is_also_a_provider() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(1_000);
        MockSwap::set_buy_alpha_return(800);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 2_000);

        let a = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(2_000),
            0,
            true,
        );
        let a_id = order_id(&a.order);

        let b = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                hotkey: charlie(),
                has_linked_order: true,
                ..v2_base(
                    OrderType::LimitBuy,
                    OrderAmount::LinkedPercentage {
                        provider: a_id,
                        pct: Perbill::from_percent(50),
                    },
                )
            },
        );
        let b_id = order_id(&b.order);

        let c = sign_v2(
            AccountKeyring::Alice,
            OrderV2 {
                hotkey: charlie(),
                ..v2_base(
                    OrderType::TakeProfit,
                    OrderAmount::LinkedPercentage {
                        provider: b_id,
                        pct: Perbill::one(),
                    },
                )
            },
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(dave()),
            bounded(vec![a, b, c]),
            true,
        ));

        assert_eq!(swap_buys(), vec![500]);
        assert_eq!(swap_sells(), vec![2_000, 800]);
        assert!(LinkedOutputs::<Test>::get(a_id).is_none());
        assert!(LinkedOutputs::<Test>::get(b_id).is_none());
    });
}

#[test]
fn prune_works_while_the_pallet_is_disabled() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        LimitOrdersEnabled::<Test>::set(false);

        assert_ok!(LimitOrders::prune_linked_output(
            RuntimeOrigin::signed(alice()),
            provider_id
        ));
        assert!(LinkedOutputs::<Test>::get(provider_id).is_none());
    });
}

#[test]
fn batched_provider_records_its_pro_rata_output() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();
        MockSwap::set_sell_tao_return(900);

        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        MockSwap::set_tao_balance(pallet_acct, 10_000);

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![provider]),
        ));

        let record = LinkedOutputs::<Test>::get(provider_id).expect("provider record");
        assert_eq!(record.asset, LinkedAsset::Tao);
        assert_eq!(record.total, 900);
    });
}

#[test]
fn second_batched_linked_order_finds_nothing() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
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

        let first = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(60),
            },
            u64::MAX,
            false,
        );
        let second = make_signed_v2_order(
            AccountKeyring::Alice,
            charlie(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(60),
            },
            u64::MAX - 1,
            false,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(dave()),
                netuid(),
                bounded(vec![first, second]),
            ),
            Error::<Test>::NoLinkedOutput
        );
        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_some(),
            "a rejected second consumer must not take the record",
        );
    });
}

/// Consume happens after distribute. A batch that fails there (`ZeroShareInBatch`)
/// must leave the provider record in place without relying on FRAME rollback of
/// an earlier `take`.
#[test]
fn batched_zero_share_does_not_consume_record() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1000);
        MockSwap::set_tao_balance(alice(), 1_000_000);
        MockSwap::set_tao_balance(bob(), 1);

        let provider_id = H256::repeat_byte(0x33);
        LinkedOutputs::<Test>::insert(
            provider_id,
            LinkedOutput {
                signer: bob(),
                asset: LinkedAsset::Tao,
                total: 1,
                expires_at: FAR_FUTURE,
            },
        );

        let big_buyer = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let consumer = make_signed_v2_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::one(),
            },
            u64::MAX,
            false,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![big_buyer, consumer]),
            ),
            Error::<Test>::ZeroShareInBatch
        );
        assert!(
            LinkedOutputs::<Test>::get(provider_id).is_some(),
            "a failed batch must not consume the provider record",
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
        let v2 = VersionedOrder::V2(v2_base(OrderType::LimitBuy, OrderAmount::Fixed(1_000)));

        assert_ne!(
            LimitOrders::render_order(&v1.order),
            LimitOrders::render_order(&v2),
            "the version tag alone must keep a v1 signature from being replayed as v2",
        );
    });
}

#[test]
fn readable_signed_linked_order_executes() {
    new_test_ext().execute_with(|| {
        setup_rotation_balances();

        let provider = make_signed_v2_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            OrderAmount::Fixed(500),
            0,
            true,
        );
        let provider_id = order_id(&provider.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![provider]),
            true,
        ));

        let versioned = VersionedOrder::V2(v2_base(
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: provider_id,
                pct: Perbill::from_percent(50),
            },
        ));
        let sig = AccountKeyring::Alice
            .pair()
            .sign(&readable_signed_bytes(&versioned));
        let consumer = crate::SignedOrder {
            order: versioned,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![consumer]),
            true,
        ));
        assert_eq!(swap_buys(), vec![200]);
    });
}

#[test]
fn readable_payload_for_a_linked_order_is_hashed() {
    new_test_ext().execute_with(|| {
        let versioned = VersionedOrder::V2(v2_base(
            OrderType::LimitBuy,
            OrderAmount::LinkedPercentage {
                provider: H256::repeat_byte(0x11),
                pct: Perbill::one(),
            },
        ));
        let msg = LimitOrders::render_order(&versioned);
        let payload = [
            b"<Bytes>".as_slice(),
            msg.as_slice(),
            b"</Bytes>".as_slice(),
        ]
        .concat();
        assert!(
            payload.len() > LEDGER_MAX_SIGN_SIZE,
            "must be hashed on-device, not signed bare",
        );
    });
}
