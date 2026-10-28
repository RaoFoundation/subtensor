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
        let _ = self::approved(order.clone(), 7);
        ApprovedOrders::<Test>::mutate(alice(), |state| {
            state
                .as_mut()
                .unwrap()
                .orders
                .get_mut(&7)
                .unwrap()
                .commitment = [9u8; 64];
        });
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
        assert_eq!(
            LimitOrders::order_status(&alice(), Some(7), id),
            Some(OrderStatus::Fulfilled)
        );
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
        assert_eq!(
            LimitOrders::approved_order(&alice(), 4).map(|approval| approval.id),
            Some(id)
        );
        assert!(ApprovedOrders::<Test>::contains_key(alice()));
        MockSwap::set_swap_fail(false);
        let invalid = approved(order(), 5);
        ApprovedOrders::<Test>::mutate(alice(), |state| {
            state.as_mut().unwrap().orders.remove(&5);
        });
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
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                expired.with_nonce(0).unwrap()
            ),
            Error::<Test>::OrderExpired
        );
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
                assert_eq!(
                    LimitOrders::order_status(signed.order.signer(), Some(signed.nonce), id),
                    Some(OrderStatus::Fulfilled)
                );
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
        assert_eq!(ApprovedOrders::<Test>::iter().count(), 0);
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
            assert_eq!(Orders::<Test>::get(ids[0]), Some(OrderStatus::Fulfilled));
            assert_eq!(
                LimitOrders::order_status(&alice(), Some(1), ids[1]),
                Some(OrderStatus::Fulfilled)
            );
            assert_eq!(
                LimitOrders::order_status(&dave(), Some(2), ids[2]),
                Some(OrderStatus::Fulfilled)
            );

            assert_eq!(
                LimitOrders::approved_order(&alice(), 1).map(|approval| approval.id),
                Some(ids[1])
            );
            assert_eq!(
                LimitOrders::approved_order(&dave(), 2).map(|approval| approval.id),
                Some(ids[2])
            );
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
        LimitOrders::set_order_status(&alice(), Some(7), id, OrderStatus::PartiallyFilled(100));
        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(bob()), authorized.clone()),
            Error::<Test>::Unauthorized
        );
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            authorized.clone()
        ));
        assert_eq!(
            LimitOrders::order_status(&alice(), Some(7), id),
            Some(OrderStatus::Cancelled)
        );
        assert!(ApprovedOrders::<Test>::contains_key(alice()));
        assert_eq!(
            LimitOrders::approved_order(&alice(), 7).map(|approval| approval.id),
            Some(id)
        );
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

#[test]
fn approvals_are_bounded_and_failed_insertion_preserves_existing_authority() {
    new_test_ext().execute_with(|| {
        prepare();
        for nonce in 0..8 {
            approved(order(), nonce);
        }
        for nonce in 8..128 {
            assert_noop!(
                LimitOrders::authorize_order(
                    RuntimeOrigin::signed(alice()),
                    order().with_nonce(nonce).unwrap()
                ),
                Error::<Test>::TooManyApprovedOrders
            );
        }
        let state = ApprovedOrders::<Test>::get(alice()).unwrap();
        assert_eq!(state.orders.len(), 8);
        assert_eq!(state.highest_nonce, Some(7));
        assert!(state.encoded_size() <= AccountOrderApprovals::max_encoded_len());
        assert_eq!(ApprovedOrders::<Test>::iter().count(), 1);
        assert_eq!(Orders::<Test>::iter().count(), 0);
        // Existing approvals remain usable even with a full account record.
        assert_ok!(execute(AuthorizedOrder {
            nonce: 0,
            order: order(),
            authorization: OrderAuthorization::AccountApproved,
            partial_fill: None,
        }));
        approved(order(), 8);
        assert!(LimitOrders::approved_order(&alice(), 0).is_none());
        assert_eq!(
            ApprovedOrders::<Test>::get(alice()).unwrap().orders.len(),
            8
        );
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(0).unwrap()
            ),
            Error::<Test>::OrderNonceUsed
        );
    });
}

#[test]
fn repeated_cancellation_and_execution_leave_no_per_order_tombstones() {
    for execute_order in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            MockSwap::set_tao_balance(alice(), 100_000);
            let mut first = None;
            for nonce in 0..128 {
                let proof = approved(order(), nonce);
                if nonce == 0 {
                    first = Some(proof.clone());
                }
                if execute_order {
                    assert_ok!(execute(proof));
                } else {
                    assert_ok!(LimitOrders::cancel_order(
                        RuntimeOrigin::signed(alice()),
                        proof.order.with_nonce(nonce).unwrap()
                    ));
                }
                assert_eq!(
                    ApprovedOrders::<Test>::get(alice()).unwrap().orders.len(),
                    1
                );
                assert_eq!(Orders::<Test>::iter().count(), 0);
            }
            assert_eq!(ApprovedOrders::<Test>::iter().count(), 1);
            assert_noop!(execute(first.unwrap()), Error::<Test>::OrderNotApproved);
            assert_noop!(
                LimitOrders::authorize_order(
                    RuntimeOrigin::signed(alice()),
                    order().with_nonce(0).unwrap()
                ),
                Error::<Test>::OrderNonceUsed
            );
            // Cancelling arbitrary unused nonces cannot grow state or advance its watermark.
            let before = ApprovedOrders::<Test>::get(alice());
            for nonce in 128..256 {
                assert_ok!(LimitOrders::cancel_order(
                    RuntimeOrigin::signed(alice()),
                    order().with_nonce(nonce).unwrap()
                ));
            }
            assert_eq!(ApprovedOrders::<Test>::get(alice()), before);
            assert_eq!(Orders::<Test>::iter().count(), 0);
        });
    }
}

#[test]
fn expiry_reclaims_capacity_without_restoring_old_nonces_or_partial_fills() {
    new_test_ext().execute_with(|| {
        prepare();
        MockTime::set(100);
        let mut terms = order();
        if let VersionedOrder::V1(order) = &mut terms {
            order.expiry = 100;
            order.partial_fills_enabled = true;
            order.relayer = Some(vec![charlie()].try_into().unwrap());
        }
        let mut proof = approved(terms.clone(), 0);
        proof.partial_fill = Some(100);
        assert_ok!(execute(proof.clone()));
        // Identical reapproval cannot reset partial fill accounting.
        approved(terms.clone(), 0);
        assert_eq!(
            LimitOrders::approved_order(&alice(), 0).unwrap().status,
            Some(OrderStatus::PartiallyFilled(100))
        );
        for nonce in 1..8 {
            approved(terms.clone(), nonce);
        }
        MockTime::set(101);
        approved(order(), 8);
        assert_eq!(
            ApprovedOrders::<Test>::get(alice()).unwrap().orders.len(),
            1
        );
        assert_noop!(execute(proof), Error::<Test>::OrderNotApproved);
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(0).unwrap()
            ),
            Error::<Test>::OrderNonceUsed
        );
        assert_eq!(Orders::<Test>::iter().count(), 0);
    });
}

#[test]
fn reaping_cleans_all_approval_state_and_refunding_does_not_reactivate_orders() {
    new_test_ext().execute_with(|| {
        prepare();
        let proof = approved(order(), 0);
        for nonce in 1..8 {
            approved(order(), nonce);
        }
        assert_ok!(System::dec_providers(&alice()));
        assert!(!frame_system::Account::<Test>::contains_key(alice()));
        assert_eq!(ApprovedOrders::<Test>::iter().count(), 0);
        assert_eq!(Orders::<Test>::iter().count(), 0);
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(8).unwrap()
            ),
            Error::<Test>::OrderAccountMissing
        );
        for nonce in 0..128 {
            assert_ok!(LimitOrders::cancel_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(nonce).unwrap()
            ));
        }
        assert_eq!(ApprovedOrders::<Test>::iter().count(), 0);
        assert_eq!(Orders::<Test>::iter().count(), 0);
        System::inc_providers(&alice());
        assert_noop!(execute(proof), Error::<Test>::OrderNotApproved);
        // Only a new authenticated owner approval can restore authority after reaping.
        let fresh = approved(order(), 0);
        assert_ok!(execute(fresh));
    });
}

#[test]
fn nonce_watermark_never_wraps_and_cannot_be_reset_by_cancellation() {
    new_test_ext().execute_with(|| {
        let proof = approved(order(), u64::MAX);
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            proof.order.with_nonce(u64::MAX).unwrap()
        ));
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(0).unwrap()
            ),
            Error::<Test>::OrderNonceUsed
        );
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(u64::MAX).unwrap()
            ),
            Error::<Test>::OrderAlreadyProcessed
        );
        assert_eq!(
            ApprovedOrders::<Test>::get(alice()).unwrap().highest_nonce,
            Some(u64::MAX)
        );
    });
}

#[test]
fn execution_cannot_recreate_state_after_the_swap_reaps_its_owner() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let proof = approved(order(), 0);
            ReapOnDebit::set(Some(alice()));
            if netted {
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    unified_orders(vec![proof.clone()].try_into().unwrap())
                ));
            } else {
                assert_ok!(execute(proof.clone()));
            }
            assert!(!ApprovedOrders::<Test>::contains_key(alice()));
            System::inc_providers(&alice());
            assert_noop!(execute(proof), Error::<Test>::OrderNotApproved);
            assert_eq!(Orders::<Test>::iter().count(), 0);
        });
    }
}

#[test]
fn full_approval_record_preserves_other_orders_through_partial_fill_and_cancel() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let mut terms = order();
            if let VersionedOrder::V1(order) = &mut terms {
                order.partial_fills_enabled = true;
                order.relayer = Some(vec![charlie()].try_into().unwrap());
            }
            for nonce in 0..8 {
                approved(terms.clone(), nonce);
            }
            let before = ApprovedOrders::<Test>::get(alice()).unwrap();
            let mut proof = AuthorizedOrder {
                nonce: 3,
                order: terms.clone(),
                authorization: OrderAuthorization::AccountApproved,
                partial_fill: Some(100),
            };
            if netted {
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    unified_orders(vec![proof.clone()].try_into().unwrap())
                ));
            } else {
                assert_ok!(execute(proof.clone()));
            }
            approved(terms.clone(), 3);
            let partially_filled = ApprovedOrders::<Test>::get(alice()).unwrap();
            assert_eq!(partially_filled.highest_nonce, Some(7));
            assert_eq!(partially_filled.orders.len(), 8);
            for nonce in 0..8 {
                if nonce == 3 {
                    assert_eq!(
                        partially_filled.orders[&nonce].status,
                        Some(OrderStatus::PartiallyFilled(100))
                    );
                } else {
                    assert_eq!(partially_filled.orders[&nonce], before.orders[&nonce]);
                }
            }
            assert_ok!(LimitOrders::cancel_order(
                RuntimeOrigin::signed(alice()),
                terms.clone().with_nonce(3).unwrap()
            ));
            proof.partial_fill = Some(400);
            assert_noop!(execute(proof.clone()), Error::<Test>::OrderCancelled);
            approved(terms, 8);
            let reclaimed = ApprovedOrders::<Test>::get(alice()).unwrap();
            assert_eq!(reclaimed.highest_nonce, Some(8));
            assert_eq!(reclaimed.orders.len(), 8);
            assert!(!reclaimed.orders.contains_key(&3));
            for nonce in [0, 1, 2, 4, 5, 6, 7] {
                assert_eq!(reclaimed.orders[&nonce], before.orders[&nonce]);
            }
            assert_noop!(execute(proof), Error::<Test>::OrderNotApproved);
            assert_eq!(Orders::<Test>::iter().count(), 0);
        });
    }
}

#[test]
fn failure_after_reaping_rolls_back_full_approval_record_and_system_account() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let mut terms = order();
            if let VersionedOrder::V1(order) = &mut terms {
                order.fee_rate = Perbill::from_percent(1);
            }
            let proof = approved(terms.clone(), 0);
            for nonce in 1..8 {
                approved(terms.clone(), nonce);
            }
            let before = ApprovedOrders::<Test>::get(alice());
            let account_before = frame_system::Account::<Test>::get(alice());
            ReapOnDebit::set(Some(alice()));
            if netted {
                // Asset collection reaps the owner before the pool rejects the swap.
                MockSwap::set_swap_fail(true);
                assert_noop!(
                    LimitOrders::execute_batched_orders(
                        RuntimeOrigin::signed(charlie()),
                        netuid(),
                        unified_orders(vec![proof.clone()].try_into().unwrap())
                    ),
                    sp_runtime::DispatchError::Other("pool error")
                );
                assert!(MockSwap::log().iter().any(|call| matches!(
                    call,
                    SwapCall::TransferTao { from, .. } if from == &alice()
                )));
                MockSwap::set_swap_fail(false);
            } else {
                // The swap reaps the owner before forwarding the order fee fails.
                FAIL_FEE_TRANSFER.with(|fail| *fail.borrow_mut() = true);
                assert_noop!(
                    execute(proof.clone()),
                    sp_runtime::DispatchError::CannotLookup
                );
                assert!(MockSwap::log().iter().any(|call| matches!(
                    call,
                    SwapCall::BuyAlpha { coldkey, .. } if coldkey == &alice()
                )));
                FAIL_FEE_TRANSFER.with(|fail| *fail.borrow_mut() = false);
            }
            assert_eq!(ApprovedOrders::<Test>::get(alice()), before);
            assert_eq!(frame_system::Account::<Test>::get(alice()), account_before);
            assert_eq!(Orders::<Test>::iter().count(), 0);
            // Mock balances are not transactional; only assert the real storage
            // rollback here and restore the balance before checking retry authority.
            MockSwap::set_tao_balance(alice(), 5_000);
            ReapOnDebit::set(None);
            assert_ok!(execute(proof));
        });
    }
}

#[test]
fn reaping_one_owner_preserves_another_owners_same_nonce_and_full_record() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let mut other_terms = order();
            if let VersionedOrder::V1(order) = &mut other_terms {
                order.signer = dave();
            }
            MockSwap::set_tao_balance(dave(), 5_000);
            for nonce in 0..8 {
                approved(order(), nonce);
                approved(other_terms.clone(), nonce);
            }
            let other_before = ApprovedOrders::<Test>::get(dave());
            assert_ok!(System::dec_providers(&alice()));
            assert!(!ApprovedOrders::<Test>::contains_key(alice()));
            assert_eq!(ApprovedOrders::<Test>::get(dave()), other_before);
            let proof = AuthorizedOrder {
                nonce: 0,
                order: other_terms,
                authorization: OrderAuthorization::AccountApproved,
                partial_fill: None,
            };
            if netted {
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(charlie()),
                    netuid(),
                    unified_orders(vec![proof.clone()].try_into().unwrap())
                ));
            } else {
                assert_ok!(execute(proof.clone()));
            }
            assert_noop!(execute(proof), Error::<Test>::OrderAlreadyProcessed);
            assert_eq!(ApprovedOrders::<Test>::iter().count(), 1);
            assert_eq!(Orders::<Test>::iter().count(), 0);
        });
    }
}

#[test]
fn sufficient_only_owner_keeps_approvals_until_its_last_reference_is_removed() {
    new_test_ext().execute_with(|| {
        prepare();
        System::inc_sufficients(&alice());
        assert_ok!(System::dec_providers(&alice()));
        assert_eq!(System::providers(&alice()), 0);
        let proof = approved(order(), 0);
        for nonce in 1..8 {
            approved(order(), nonce);
        }
        assert_ok!(execute(proof.clone()));
        assert_eq!(
            ApprovedOrders::<Test>::get(alice()).unwrap().orders.len(),
            8
        );
        System::dec_sufficients(&alice());
        assert!(!frame_system::Account::<Test>::contains_key(alice()));
        assert!(!ApprovedOrders::<Test>::contains_key(alice()));
        System::inc_sufficients(&alice());
        assert_noop!(execute(proof), Error::<Test>::OrderNotApproved);
        assert_eq!(Orders::<Test>::iter().count(), 0);
    });
}

#[test]
fn full_approval_record_is_not_reclaimed_at_the_inclusive_expiry_boundary() {
    new_test_ext().execute_with(|| {
        let mut terms = order();
        if let VersionedOrder::V1(order) = &mut terms {
            order.expiry = 100;
        }
        MockTime::set(100);
        for nonce in 0..8 {
            approved(terms.clone(), nonce);
        }
        assert_noop!(
            LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                order().with_nonce(8).unwrap()
            ),
            Error::<Test>::TooManyApprovedOrders
        );
        MockTime::set(101);
        approved(order(), 8);
        let state = ApprovedOrders::<Test>::get(alice()).unwrap();
        assert_eq!(state.highest_nonce, Some(8));
        assert_eq!(state.orders.len(), 1);
        assert!(state.orders.contains_key(&8));
    });
}
