#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use sp_keyring::Sr25519Keyring;

type LimitOrders = Pallet<Test>;

#[test]
fn order_wire_and_commitments_match_independent_golden_bytes() {
    new_test_ext().execute_with(|| {
        frame_system::BlockHash::<Test>::insert(0, H256::repeat_byte(0x44));
        let order = VersionedOrder::V1(Order {
            signer: AccountId32::new([0x11; 32]),
            hotkey: AccountId32::new([0x22; 32]),
            netuid: NetUid::from(42u16),
            order_type: OrderType::LimitBuy,
            amount: 1_234_567_890,
            limit_price: 2_000_000_000,
            expiry: 2_000_000_000_000,
            fee_rate: Perbill::from_parts(1_000_000),
            fee_recipient: AccountId32::new([0x33; 32]),
            relayer: Some(
                vec![AccountId32::new([0x55; 32]), AccountId32::new([0x66; 32])]
                    .try_into()
                    .unwrap(),
            ),
            max_slippage: Some(Perbill::from_parts(42_000_000)),
            chain_id: 42,
            partial_fills_enabled: true,
        });
        assert_eq!(
            order.encode(),
            include_bytes!("../../tests/fixtures/authorized-order.scale")
        );
        assert_eq!(
            LimitOrders::derive_authorized_order_id(42, &order).as_bytes(),
            include_bytes!("../../tests/fixtures/authorized-order-id.bin")
        );
        assert_eq!(
            &LimitOrders::order_approval_commitment(42, &order),
            include_bytes!("../../tests/fixtures/authorized-order-approval.bin")
        );
    });
}

fn order() -> VersionedOrder<AccountId32> {
    VersionedOrder::V1(Order {
        signer: alice(),
        hotkey: bob(),
        netuid: netuid(),
        order_type: OrderType::LimitBuy,
        amount: 500,
        limit_price: u64::MAX,
        expiry: FAR_FUTURE,
        fee_rate: Perbill::zero(),
        fee_recipient: fee_recipient(),
        relayer: None,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    })
}

fn approved(order: VersionedOrder<AccountId32>, nonce: u64) -> AuthorizedOrder {
    assert_ok!(LimitOrders::authorize_order(
        RuntimeOrigin::signed(order.signer().clone()),
        order.clone().with_nonce(nonce).unwrap()
    ));
    AuthorizedOrder {
        nonce,
        order,
        authorization: OrderAuthorization::AccountApproved,
        partial_fill: None,
    }
}

fn execute(signed: AuthorizedOrder) -> DispatchResult {
    LimitOrders::execute_orders(
        RuntimeOrigin::signed(charlie()),
        unified_orders(vec![signed].try_into().unwrap()),
        true,
    )
}

fn prepare() {
    MockSwap::set_buy_alpha_return(500);
    MockSwap::set_tao_balance(alice(), 5_000);
    ProtectedOrderSigner::set(Some(alice()));
}

#[test]
fn batched_stop_loss_floor_applies_to_every_authorization_and_netting_side() {
    for mode in 0..2 {
        for buy_amount in [50_000_000_000, 60_000_000_000] {
            new_test_ext().execute_with(|| {
                MockSwap::set_price(0.5);
                MockSwap::set_buy_alpha_return(20_000_000_000);
                MockSwap::set_tao_balance(bob(), buy_amount);
                MockSwap::set_alpha_balance(alice(), bob(), netuid(), 100_000_000_000);
                let mut terms = order();
                if let VersionedOrder::V1(order) = &mut terms {
                    order.order_type = OrderType::StopLoss;
                    order.amount = 100_000_000_000;
                    order.limit_price = 1_000_000_000;
                    order.max_slippage = Some(Perbill::from_percent(1));
                }
                let seller = if mode == 0 {
                    SignedOrder {
                        signature: sp_runtime::MultiSignature::Sr25519(
                            Sr25519Keyring::Alice.sign(&terms.encode()),
                        )
                        .into(),
                        order: terms.clone(),
                        partial_fill: None,
                    }
                } else {
                    unified_orders(
                        vec![AuthorizedOrder {
                            nonce: 7,
                            authorization: approved(terms.clone(), 7).authorization,
                            order: terms,
                            partial_fill: None,
                        }]
                        .try_into()
                        .unwrap(),
                    )[0]
                    .clone()
                };
                let buyer = make_signed_order(
                    Sr25519Keyring::Bob,
                    bob(),
                    netuid(),
                    OrderType::LimitBuy,
                    buy_amount,
                    u64::MAX,
                    FAR_FUTURE,
                    Perbill::zero(),
                    fee_recipient(),
                    None,
                );
                let batch = bounded(vec![buyer, seller]);
                assert_noop!(
                    LimitOrders::execute_batched_orders(
                        RuntimeOrigin::signed(charlie()),
                        netuid(),
                        batch.clone()
                    ),
                    Error::<Test>::PriceConditionNotMet
                );
                assert_eq!(MockSwap::tao_balance(&bob()), buy_amount);
                assert_eq!(
                    MockSwap::alpha_balance(&alice(), &bob(), netuid()),
                    100_000_000_000
                );
                // A rejected batch leaves the orders and authority retryable.
                MockSwap::set_price(1.0);
                MockSwap::set_sell_tao_return(100_000_000_000 - buy_amount);
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    batch
                ));
                assert_eq!(MockSwap::tao_balance(&alice()), 100_000_000_000);
            });
        }
    }
}

#[test]
fn approved_order_uses_owner_authority_and_keeps_legacy_protection() {
    new_test_ext().execute_with(|| {
        prepare();
        let order = order();
        let id = LimitOrders::derive_authorized_order_id(7, &order);
        let approved = AuthorizedOrder {
            nonce: 7,
            order: order.clone(),
            authorization: OrderAuthorization::AccountApproved,
            partial_fill: None,
        };
        assert_noop!(execute(approved.clone()), Error::<Test>::OrderNotApproved);
        // Even a matching H256 bookkeeping key cannot authorize different terms.
        ApprovedOrders::<Test>::insert(id, [9u8; 64]);
        assert_noop!(execute(approved.clone()), Error::<Test>::OrderNotApproved);
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(bob()),
                order.clone().with_nonce(7).unwrap()
            ),
            Error::<Test>::Unauthorized
        );
        assert_ok!(LimitOrders::authorize_order(
            RuntimeOrigin::signed(alice()),
            order.clone().with_nonce(7).unwrap()
        ));
        let legacy = make_signed_order(
            Sr25519Keyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            500,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![legacy]),
                true
            ),
            Error::<Test>::OrderSignerFrozen
        );
        FrozenOrderSigner::set(Some(alice()));
        assert_noop!(execute(approved.clone()), Error::<Test>::OrderSignerFrozen);
        FrozenOrderSigner::set(None);
        assert_ok!(execute(approved.clone()));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_noop!(execute(approved), Error::<Test>::OrderAlreadyProcessed);
    });
}

#[test]
fn failed_swap_preserves_approval_and_nonce_and_batch_is_atomic() {
    new_test_ext().execute_with(|| {
        prepare();

        let proof = approved(order(), 4);
        let id = LimitOrders::derive_authorized_order_id(4, &proof.order);
        MockSwap::set_swap_fail(true);
        assert!(execute(proof.clone()).is_err());

        assert!(!Orders::<Test>::contains_key(id));
        assert_eq!(OrderNonces::<Test>::get(alice(), 4), Some(id));
        assert!(ApprovedOrders::<Test>::contains_key(id));
        MockSwap::set_swap_fail(false);
        let invalid = approved(order(), 5);
        ApprovedOrders::<Test>::remove(LimitOrders::derive_authorized_order_id(5, &invalid.order));
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                unified_orders(vec![proof.clone(), invalid].try_into().unwrap()),
                true
            ),
            Error::<Test>::OrderNotApproved
        );
        assert!(!Orders::<Test>::contains_key(id));

        assert_ok!(execute(proof));
    });
}

#[test]
fn expiry_relayer_price_and_partial_overfill_rules_are_shared() {
    new_test_ext().execute_with(|| {
        prepare();

        let mut expired = order();
        if let VersionedOrder::V1(order) = &mut expired {
            order.expiry = 1;
        }
        let expired = approved(expired, 0);
        assert_noop!(execute(expired), Error::<Test>::OrderExpired);
        let mut restricted = order();
        if let VersionedOrder::V1(order) = &mut restricted {
            order.relayer = Some(vec![dave()].try_into().unwrap());
        }
        let restricted = approved(restricted, 1);
        assert_noop!(execute(restricted), Error::<Test>::RelayerMissMatch);
        let mut bad_price = order();
        if let VersionedOrder::V1(order) = &mut bad_price {
            order.limit_price = 0;
        }
        let bad_price = approved(bad_price, 2);
        assert_noop!(execute(bad_price), Error::<Test>::PriceConditionNotMet);
        let mut partial = order();
        if let VersionedOrder::V1(order) = &mut partial {
            order.partial_fills_enabled = true;
            order.relayer = Some(vec![charlie()].try_into().unwrap());
        }
        let mut signed = approved(partial, 3);
        signed.partial_fill = Some(400);
        assert_ok!(execute(signed.clone()));
        signed.partial_fill = Some(101);
        assert_noop!(
            execute(signed.clone()),
            Error::<Test>::IncorrectPartialFillAmount
        );
        signed.partial_fill = None;
        assert_noop!(execute(signed), Error::<Test>::IncorrectPartialFillAmount);
    });
}

fn netted_outcome(account_approved: bool) -> (Vec<SwapCall>, u64, u64, u64) {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(300);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_alpha_balance(dave(), bob(), netuid(), 200);
        let buy = make_signed_order(
            Sr25519Keyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            500,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let sell = make_signed_order(
            Sr25519Keyring::Dave,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            200,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        if account_approved {
            ProtectedOrderSigner::set(Some(alice()));

            let buys = approved(buy.order, 0);
            let sells = approved(sell.order, 0);
            assert_ok!(LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                unified_orders(vec![buys.clone(), sells.clone()].try_into().unwrap())
            ));
            for signed in [buys, sells] {
                let id = LimitOrders::derive_authorized_order_id(signed.nonce, &signed.order);
                assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
            }
        } else {
            assert_ok!(LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![buy, sell])
            ));
        }
        (
            MockSwap::log(),
            MockSwap::tao_balance(&alice()),
            MockSwap::alpha_balance(&alice(), &bob(), netuid()),
            MockSwap::tao_balance(&dave()),
        )
    })
}

#[test]
fn account_approved_netted_fills_have_identical_economics_to_legacy() {
    assert_eq!(netted_outcome(true), netted_outcome(false));
}

#[test]
fn netted_approved_orders_recheck_freeze() {
    new_test_ext().execute_with(|| {
        prepare();
        let approved = AuthorizedOrder {
            nonce: 0,
            order: order(),
            authorization: OrderAuthorization::AccountApproved,
            partial_fill: None,
        };
        assert_ok!(LimitOrders::authorize_order(
            RuntimeOrigin::signed(alice()),
            approved.order.clone().with_nonce(0).unwrap()
        ));
        FrozenOrderSigner::set(Some(alice()));
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                unified_orders(vec![approved.clone()].try_into().unwrap())
            ),
            Error::<Test>::OrderSignerFrozen
        );
        FrozenOrderSigner::set(None);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            unified_orders(vec![approved].try_into().unwrap())
        ));
    });
}

fn unified_orders(
    orders: BoundedVec<AuthorizedOrder, MaxAuthorizedOrders>,
) -> BoundedVec<SignedOrder<AccountId32>, <Test as Config>::MaxOrdersPerBatch> {
    orders
        .into_iter()
        .map(|signed| signed.try_into().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

#[test]
fn original_call_indices_and_every_legacy_signature_byte_are_preserved() {
    new_test_ext().execute_with(|| {
        let order = order();
        let signatures = [
            sp_runtime::MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw([3; 64])),
            sp_runtime::MultiSignature::Sr25519(sp_core::sr25519::Signature::from_raw([4; 64])),
            sp_runtime::MultiSignature::Ecdsa(sp_core::ecdsa::Signature::from_raw([5; 65])),
        ];
        for signature in signatures {
            // Tuple encoding is the exact pre-consolidation frozen struct layout.
            let legacy = (order.clone(), signature.clone(), Some(17u64)).encode();
            let signed = SignedOrder {
                order: order.clone(),
                signature: signature.into(),
                partial_fill: Some(17),
            };
            assert_eq!(signed.encode(), legacy);
            assert_eq!(
                SignedOrder::<AccountId32>::decode(&mut legacy.as_slice()).unwrap(),
                signed
            );
            let entries = bounded(vec![signed]);
            let call = Call::<Test>::execute_orders {
                orders: entries.clone(),
                should_fail: true,
            };
            assert_eq!(call.encode(), [vec![0], entries.encode(), vec![1]].concat());
            let call = Call::<Test>::execute_batched_orders {
                netuid: netuid(),
                orders: entries.clone(),
            };
            assert_eq!(
                call.encode(),
                [vec![1], netuid().encode(), entries.encode()].concat()
            );
        }
        assert_eq!(
            Call::<Test>::cancel_order {
                order: order.clone()
            }
            .encode(),
            [vec![2], order.encode()].concat()
        );
        let authorized = order.clone().with_nonce(19).unwrap();
        assert_eq!(
            authorized.encode(),
            [vec![2], 19u64.to_le_bytes().to_vec(), order.encode()].concat()
        );
        assert!(authorized.clone().with_nonce(20).is_err());
        let mut nested = authorized.encode();
        nested[9] = 2; // OrderTerms permits only the existing V1/V2 variants.
        assert!(VersionedOrder::<AccountId32>::decode(&mut nested.as_slice()).is_err());
        assert_eq!(OrderSignature::AccountApproved.encode(), vec![3]);
        assert_eq!(
            VersionedOrder::<AccountId32>::decode(&mut authorized.encode().as_slice()).unwrap(),
            authorized
        );
        assert_eq!(
            Call::<Test>::authorize_order {
                order: authorized.clone()
            }
            .encode(),
            [vec![5], authorized.encode()].concat()
        );
    });
}

#[test]
fn unified_calls_reject_mismatched_proof_modes_and_bound_work_before_verification() {
    new_test_ext().execute_with(|| {
        prepare();
        let order = order();
        assert_noop!(
            LimitOrders::authorize_order(RuntimeOrigin::signed(alice()), order.clone()),
            Error::<Test>::InvalidAuthorizationMode
        );
        let missing_nonce = SignedOrder {
            order: order.clone(),
            signature: OrderSignature::AccountApproved,
            partial_fill: None,
        };
        let legacy_proof = SignedOrder {
            order: order.clone().with_nonce(0).unwrap(),
            signature: OrderSignature::Sr25519(sp_core::sr25519::Signature::from_raw([0; 64])),
            partial_fill: None,
        };
        for signed in [missing_nonce, legacy_proof] {
            assert_noop!(
                LimitOrders::execute_orders(
                    RuntimeOrigin::signed(charlie()),
                    bounded(vec![signed.clone()]),
                    true
                ),
                Error::<Test>::InvalidAuthorizationMode
            );
            assert_noop!(
                LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    bounded(vec![signed])
                ),
                Error::<Test>::InvalidAuthorizationMode
            );
        }
        let invalid = SignedOrder {
            order: order.with_nonce(0).unwrap(),
            signature: OrderSignature::AccountApproved,
            partial_fill: None,
        };
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![invalid.clone(); 9]),
                false
            ),
            Error::<Test>::OrderBatchTooLarge
        );
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![invalid; 9])
            ),
            Error::<Test>::OrderBatchTooLarge
        );
        assert_eq!(Orders::<Test>::iter().count(), 0);
        assert_eq!(OrderNonces::<Test>::iter().count(), 0);
    });
}

#[test]
fn original_calls_execute_mixed_classical_and_approved_orders_atomically() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let approved_terms = order().with_nonce(1).unwrap();
            assert_ok!(LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                approved_terms.clone()
            ));
            let legacy = make_signed_order(
                Sr25519Keyring::Bob,
                bob(),
                netuid(),
                OrderType::LimitBuy,
                500,
                u64::MAX,
                FAR_FUTURE,
                Perbill::zero(),
                fee_recipient(),
                None,
            );
            let mut other_terms = order();
            if let VersionedOrder::V1(terms) = &mut other_terms {
                terms.signer = dave();
            }

            MockSwap::set_tao_balance(bob(), 5_000);
            MockSwap::set_tao_balance(dave(), 5_000);
            let other_approved = SignedOrder::try_from(approved(other_terms, 2)).unwrap();
            let entries = bounded(vec![
                legacy,
                SignedOrder {
                    order: approved_terms.clone(),
                    signature: OrderSignature::AccountApproved,
                    partial_fill: None,
                },
                other_approved,
            ]);
            let ids: Vec<_> = entries
                .iter()
                .map(|signed| LimitOrders::derive_order_id(&signed.order))
                .collect();
            if netted {
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    entries
                ));
            } else {
                assert_ok!(LimitOrders::execute_orders(
                    RuntimeOrigin::signed(charlie()),
                    entries,
                    true
                ));
            }
            assert!(
                ids.iter()
                    .all(|id| Orders::<Test>::get(id) == Some(OrderStatus::Fulfilled))
            );

            assert_eq!(OrderNonces::<Test>::get(alice(), 1), Some(ids[1]));
            assert_eq!(OrderNonces::<Test>::get(dave(), 2), Some(ids[2]));
            assert_noop!(
                LimitOrders::cancel_order(RuntimeOrigin::signed(alice()), approved_terms),
                Error::<Test>::OrderAlreadyProcessed
            );
        });
    }
}

#[test]
fn original_cancel_clears_approved_remainder_without_changing_legacy_cancellation() {
    new_test_ext().execute_with(|| {
        prepare();
        let mut terms = order();
        if let VersionedOrder::V1(order) = &mut terms {
            order.partial_fills_enabled = true;
        }
        let authorized = terms.with_nonce(7).unwrap();
        let id = LimitOrders::derive_order_id(&authorized);
        assert_ok!(LimitOrders::authorize_order(
            RuntimeOrigin::signed(alice()),
            authorized.clone()
        ));
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(100));
        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(bob()), authorized.clone()),
            Error::<Test>::Unauthorized
        );
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            authorized.clone()
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Cancelled));
        assert!(!ApprovedOrders::<Test>::contains_key(id));
        assert_eq!(OrderNonces::<Test>::get(alice(), 7), Some(id));
        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(alice()), authorized),
            Error::<Test>::OrderAlreadyProcessed
        );
        let legacy = order();
        let legacy_id = LimitOrders::derive_order_id(&legacy);
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            legacy
        ));
        assert_eq!(Orders::<Test>::get(legacy_id), Some(OrderStatus::Cancelled));
    });
}
