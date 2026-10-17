#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok, dispatch::DispatchResult};
use sp_keyring::Sr25519Keyring;
use sp_runtime::DispatchError;

type LimitOrders = Pallet<Test>;
const SEED: [u8; 32] = [77; 32];

#[test]
fn order_wire_transcript_and_commitments_match_independent_golden_bytes() {
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
            LimitOrders::order_signing_payload(3, 42, &order),
            include_bytes!("../../tests/fixtures/authorized-order-transcript.bin")
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

/// Storage-backed adapter so the order transaction also rolls budget state back.
/// Real registry epoch/scope behavior is tested by pallet-hashed-accounts.
pub struct MockCapabilities;

#[derive(Encode, Decode)]
struct Grant {
    epoch: u64,
    commitment: [u8; 64],
    remaining: u64,
}

fn grant_key(owner: &AccountId32) -> Vec<u8> {
    (b"test/order-capability", owner).encode()
}

fn read_grant(owner: &AccountId32) -> Result<Grant, DispatchError> {
    let raw = sp_io::storage::get(&grant_key(owner)).ok_or(DispatchError::Other("NoGrant"))?;
    Grant::decode(&mut raw.as_ref()).map_err(|_| DispatchError::Other("BadGrant"))
}

fn grant(owner: &AccountId32, epoch: u64, remaining: u64) {
    let pk = subtensor_hashed::mldsa::public_key_from_seed(&SEED).unwrap();
    sp_io::storage::set(
        &grant_key(owner),
        &Grant {
            epoch,
            commitment: subtensor_hashed::mldsa::commitment(&pk),
            remaining,
        }
        .encode(),
    );
}

impl OrderCapabilities for MockCapabilities {
    fn check(
        owner: &AccountId32,
        epoch: u64,
        pk: &subtensor_hashed::mldsa::PublicKeyBytes,
        _: &OrderView<AccountId32>,
        _: u64,
        fill: u64,
    ) -> DispatchResult {
        let grant = read_grant(owner)?;
        if grant.epoch != epoch || grant.commitment != subtensor_hashed::mldsa::commitment(pk) {
            return Err(DispatchError::Other("WrongGrant"));
        }
        if fill > grant.remaining {
            return Err(DispatchError::Other("BudgetExceeded"));
        }
        Ok(())
    }
    fn consume(owner: &AccountId32, epoch: u64, _: bool, fill: u64) -> DispatchResult {
        let mut grant = read_grant(owner)?;
        if grant.epoch != epoch {
            return Err(DispatchError::Other("WrongGrant"));
        }
        grant.remaining = grant
            .remaining
            .checked_sub(fill)
            .ok_or(DispatchError::Other("BudgetExceeded"))?;
        sp_io::storage::set(&grant_key(owner), &grant.encode());
        Ok(())
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark(
        owner: &AccountId32,
        epoch: u64,
        pk: &subtensor_hashed::mldsa::PublicKeyBytes,
        _: &OrderView<AccountId32>,
    ) -> DispatchResult {
        sp_io::storage::set(
            &grant_key(owner),
            &Grant {
                epoch,
                commitment: subtensor_hashed::mldsa::commitment(pk),
                remaining: u64::MAX,
            }
            .encode(),
        );
        Ok(())
    }
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

fn signed(order: VersionedOrder<AccountId32>, nonce: u64, epoch: u64) -> AuthorizedOrder {
    let public_key = subtensor_hashed::mldsa::public_key_from_seed(&SEED).unwrap();
    let payload = LimitOrders::order_signing_payload(epoch, nonce, &order);
    let signature =
        subtensor_hashed::mldsa::sign_message(&SEED, &payload, ORDER_CONTEXT, &[0; 32]).unwrap();
    AuthorizedOrder {
        nonce,
        order,
        authorization: OrderAuthorization::MlDsa(MlDsaOrderProof {
            epoch,
            public_key,
            signature: signature.to_vec().try_into().unwrap(),
        }),
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
fn detached_order_checks_epoch_commitment_nonce_genesis_and_every_term() {
    new_test_ext().execute_with(|| {
        prepare();
        grant(&alice(), 3, 2_000);
        let good = signed(order(), 11, 3);
        let mut mutated = good.clone();
        mutated.nonce = 12;
        assert_noop!(execute(mutated), Error::<Test>::InvalidSignature);
        let mut mutated = good.clone();
        if let VersionedOrder::V1(order) = &mut mutated.order {
            order.fee_recipient = dave();
        }
        assert_noop!(execute(mutated), Error::<Test>::InvalidSignature);
        let mut mutated = good.clone();
        if let OrderAuthorization::MlDsa(proof) = &mut mutated.authorization {
            proof.epoch = 4;
        }
        assert_noop!(execute(mutated), DispatchError::Other("WrongGrant"));
        let mut mutated = good.clone();
        if let OrderAuthorization::MlDsa(proof) = &mut mutated.authorization {
            proof.public_key[1] ^= 1;
        }
        assert_noop!(execute(mutated), DispatchError::Other("WrongGrant"));
        let genesis = frame_system::BlockHash::<Test>::get(0);
        frame_system::BlockHash::<Test>::insert(0, H256::repeat_byte(9));
        assert_noop!(execute(good.clone()), Error::<Test>::InvalidSignature);
        frame_system::BlockHash::<Test>::insert(0, genesis);
        assert_ok!(execute(good));
        assert_eq!(read_grant(&alice()).unwrap().remaining, 1_500);
        let mut changed = order();
        if let VersionedOrder::V1(order) = &mut changed {
            order.amount = 501;
        }
        assert_noop!(
            execute(signed(changed, 11, 3)),
            Error::<Test>::OrderNonceUsed
        );
    });
}

#[test]
fn detached_partial_fill_cancellation_and_revocation_recheck_at_execution() {
    new_test_ext().execute_with(|| {
        prepare();
        grant(&alice(), 1, 1_000);
        let mut order = order();
        if let VersionedOrder::V1(order) = &mut order {
            order.partial_fills_enabled = true;
            order.relayer = Some(vec![charlie()].try_into().unwrap());
        }
        let mut proof = signed(order.clone(), 2, 1);
        proof.partial_fill = Some(200);
        assert_ok!(execute(proof.clone()));
        assert_eq!(read_grant(&alice()).unwrap().remaining, 800);
        assert_eq!(
            Orders::<Test>::get(LimitOrders::derive_authorized_order_id(2, &order)),
            Some(OrderStatus::PartiallyFilled(200))
        );
        grant(&alice(), 3, 1_000);
        assert_noop!(execute(proof.clone()), DispatchError::Other("WrongGrant"));
        grant(&alice(), 1, 800);
        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            order.with_nonce(2).unwrap()
        ));
        assert_noop!(execute(proof), Error::<Test>::OrderCancelled);
    });
}

#[test]
fn failed_swap_does_not_consume_capability_or_nonce_and_batch_is_atomic() {
    new_test_ext().execute_with(|| {
        prepare();
        grant(&alice(), 1, 1_000);
        let proof = signed(order(), 4, 1);
        let id = LimitOrders::derive_authorized_order_id(4, &proof.order);
        MockSwap::set_swap_fail(true);
        assert!(execute(proof.clone()).is_err());
        assert_eq!(read_grant(&alice()).unwrap().remaining, 1_000);
        assert!(!Orders::<Test>::contains_key(id));
        assert!(!OrderNonces::<Test>::contains_key(alice(), 4));
        MockSwap::set_swap_fail(false);
        let mut invalid = signed(order(), 5, 1);
        if let OrderAuthorization::MlDsa(ref mut proof) = invalid.authorization {
            proof.signature[0] ^= 1;
        }
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                unified_orders(vec![proof.clone(), invalid].try_into().unwrap()),
                true
            ),
            Error::<Test>::InvalidSignature
        );
        assert!(!Orders::<Test>::contains_key(id));
        assert_eq!(read_grant(&alice()).unwrap().remaining, 1_000);
        assert_ok!(execute(proof));
    });
}

#[test]
fn expiry_relayer_price_and_partial_overfill_rules_are_shared() {
    new_test_ext().execute_with(|| {
        prepare();
        grant(&alice(), 1, 1_000);
        let mut expired = order();
        if let VersionedOrder::V1(order) = &mut expired {
            order.expiry = 1;
        }
        assert_noop!(execute(signed(expired, 0, 1)), Error::<Test>::OrderExpired);
        let mut restricted = order();
        if let VersionedOrder::V1(order) = &mut restricted {
            order.relayer = Some(vec![dave()].try_into().unwrap());
        }
        assert_noop!(
            execute(signed(restricted, 0, 1)),
            Error::<Test>::RelayerMissMatch
        );
        let mut bad_price = order();
        if let VersionedOrder::V1(order) = &mut bad_price {
            order.limit_price = 0;
        }
        assert_noop!(
            execute(signed(bad_price, 0, 1)),
            Error::<Test>::PriceConditionNotMet
        );
        let mut partial = order();
        if let VersionedOrder::V1(order) = &mut partial {
            order.partial_fills_enabled = true;
            order.relayer = Some(vec![charlie()].try_into().unwrap());
        }
        let mut signed = signed(partial, 0, 1);
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

fn netted_outcome(mldsa: bool) -> (Vec<SwapCall>, u64, u64, u64) {
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
        if mldsa {
            ProtectedOrderSigner::set(Some(alice()));
            grant(&alice(), 1, 1_000);
            grant(&dave(), 1, 1_000);
            let buys = signed(buy.order, 0, 1);
            let sells = signed(sell.order, 0, 1);
            assert_ok!(LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                unified_orders(vec![buys.clone(), sells.clone()].try_into().unwrap())
            ));
            for signed in [buys, sells] {
                let id = LimitOrders::derive_authorized_order_id(signed.nonce, &signed.order);
                assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
            }
            assert_eq!(read_grant(&alice()).unwrap().remaining, 500);
            assert_eq!(read_grant(&dave()).unwrap().remaining, 800);
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
fn mldsa_netted_fills_have_identical_economics_to_legacy() {
    assert_eq!(netted_outcome(true), netted_outcome(false));
}

#[test]
fn netted_budget_exhaustion_reverts_all_fill_nonce_and_capability_state() {
    new_test_ext().execute_with(|| {
        prepare();
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1_000);
        grant(&alice(), 1, 750);
        let first = signed(order(), 1, 1);
        let second = signed(order(), 2, 1);
        // A classical owner shares the same atomic economic batch. A later
        // aggregate capability failure must roll its fills back as well.
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
        MockSwap::set_tao_balance(bob(), 5_000);
        let legacy_id = LimitOrders::derive_order_id(&legacy.order);
        let mut entries = unified_orders(vec![first.clone(), second.clone()].try_into().unwrap());
        entries.try_insert(0, legacy).unwrap();
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                entries
            ),
            DispatchError::Other("BudgetExceeded")
        );
        assert_eq!(read_grant(&alice()).unwrap().remaining, 750);
        assert!(!Orders::<Test>::contains_key(legacy_id));
        for signed in [first, second] {
            assert!(!Orders::<Test>::contains_key(
                LimitOrders::derive_authorized_order_id(signed.nonce, &signed.order)
            ));
            assert!(!OrderNonces::<Test>::contains_key(alice(), signed.nonce));
        }
    });
}

#[test]
fn netted_approved_orders_recheck_freeze_and_detached_orders_recheck_revocation() {
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
        grant(&alice(), 1, 1_000);
        let detached = signed(order(), 1, 1);
        grant(&alice(), 2, 1_000);
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                unified_orders(vec![detached].try_into().unwrap())
            ),
            DispatchError::Other("WrongGrant")
        );
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
fn original_calls_execute_mixed_classical_approved_and_mldsa_orders_atomically() {
    for netted in [false, true] {
        new_test_ext().execute_with(|| {
            prepare();
            let approved = order().with_nonce(1).unwrap();
            assert_ok!(LimitOrders::authorize_order(
                RuntimeOrigin::signed(alice()),
                approved.clone()
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
            let mut detached_terms = order();
            if let VersionedOrder::V1(terms) = &mut detached_terms {
                terms.signer = dave();
            }
            grant(&dave(), 3, 500);
            MockSwap::set_tao_balance(bob(), 5_000);
            MockSwap::set_tao_balance(dave(), 5_000);
            let detached = SignedOrder::try_from(signed(detached_terms, 2, 3)).unwrap();
            let entries = bounded(vec![
                legacy,
                SignedOrder {
                    order: approved.clone(),
                    signature: OrderSignature::AccountApproved,
                    partial_fill: None,
                },
                detached,
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
            assert_eq!(read_grant(&dave()).unwrap().remaining, 0);
            assert_eq!(OrderNonces::<Test>::get(alice(), 1), Some(ids[1]));
            assert_eq!(OrderNonces::<Test>::get(dave(), 2), Some(ids[2]));
            assert_noop!(
                LimitOrders::cancel_order(RuntimeOrigin::signed(alice()), approved),
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

#[test]
fn mldsa_order_proof_decodes_bounded_bytes_and_requires_exact_signature_length() {
    new_test_ext().execute_with(|| {
        prepare();
        grant(&alice(), 1, 1_000);
        let valid = signed(order(), 1, 1);
        let OrderAuthorization::MlDsa(proof) = &valid.authorization else {
            panic!("ML-DSA fixture")
        };
        assert_eq!(proof.signature.len(), 3309);
        let encoded = proof.encode();
        assert_eq!(
            &encoded[8 + 1952..8 + 1952 + 2],
            &codec::Compact(3309u32).encode()
        );
        assert_eq!(
            MlDsaOrderProof::decode(&mut encoded.as_slice()).unwrap(),
            *proof
        );
        let oversized = [
            proof.epoch.encode(),
            proof.public_key.encode(),
            codec::Compact(3310u32).encode(),
            vec![0; 3310],
        ]
        .concat();
        assert!(MlDsaOrderProof::decode(&mut oversized.as_slice()).is_err());
        for length in [0, 3308] {
            let mut malformed = valid.clone();
            if let OrderAuthorization::MlDsa(proof) = &mut malformed.authorization {
                proof.signature.truncate(length);
            }
            assert_noop!(execute(malformed), Error::<Test>::InvalidSignature);
            assert_eq!(read_grant(&alice()).unwrap().remaining, 1_000);
        }
        assert_ok!(execute(valid));
        assert_eq!(read_grant(&alice()).unwrap().remaining, 500);
    });
}
