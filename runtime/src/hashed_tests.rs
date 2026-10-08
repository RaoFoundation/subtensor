#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use crate::{hashed_auth::AuthorizeAccount, hashed_extrinsic::LegacyUnchecked};
use frame_support::{dispatch::GetDispatchInfo, traits::Currency};
use sp_core::{Pair, sr25519};
use sp_runtime::{
    generic::Era,
    traits::{Applyable, Checkable, TransactionExtension},
    transaction_validity::{InvalidTransaction, TransactionSource},
};
use subtensor_hashed::{Descriptor, Proof, Scheme};
fn ext() -> sp_io::TestExternalities {
    let mut ext = sp_io::TestExternalities::default();
    ext.execute_with(|| frame_system::BlockHash::<Runtime>::insert(0, H256::zero()));
    ext
}

fn extra(nonce: u32) -> TxExtension {
    (
        (
            check_nonzero_sender::CheckNonZeroSender::new(),
            frame_system::CheckSpecVersion::new(),
            frame_system::CheckTxVersion::new(),
            frame_system::CheckGenesis::new(),
            check_mortality::CheckMortality::from(Era::Immortal),
            check_nonce::CheckNonce::from(nonce),
            frame_system::CheckWeight::new(),
        ),
        (
            transaction_payment_wrapper::ChargeTransactionPaymentWrapper::new(TaoBalance::new(0)),
            sudo_wrapper::SudoTransactionExtension::new(),
            pallet_shield::CheckShieldedTxValidity::new(),
            pallet_subtensor::SubtensorTransactionExtension::new(),
            pallet_drand::drand_priority::DrandPriority::new(),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::new(false),
    )
}

fn call() -> RuntimeCall {
    RuntimeCall::System(SystemCall::remark {
        remark: b"hashed wire vector".to_vec(),
    })
}

fn pair(generation: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[generation; 32])
}
fn descriptor() -> Descriptor {
    Descriptor {
        version: 1,
        scheme: Scheme::Sr25519,
        initial_commitment: subtensor_hashed::key_commitment(Scheme::Sr25519, &pair(10).public().0),
    }
}
fn account() -> AccountId {
    AccountId::new(subtensor_hashed::account_id(&descriptor()))
}
fn setup() {
    HashedEnabled::set(true);
    hashed_auth::TestVerificationWeight::set(Weight::zero());
    pallet_hashed_accounts::Accounts::<Runtime>::insert(
        account(),
        pallet_hashed_accounts::AccountRecord {
            descriptor: descriptor(),
            generation: 0,
            commitment: descriptor().initial_commitment,
        },
    );
    System::inc_providers(&account());
    let _ = Balances::make_free_balance_be(&account(), TaoBalance::new(1_000_000_000_000));
}
fn signed(call: RuntimeCall, generation: u64, key: &sr25519::Pair) -> UncheckedExtrinsic {
    signed_nonce(call, generation, key, System::account_nonce(account()))
}
fn signed_nonce(
    call: RuntimeCall,
    generation: u64,
    key: &sr25519::Pair,
    nonce: u32,
) -> UncheckedExtrinsic {
    signed_with_extra(call, generation, key, extra(nonce))
}

fn signed_with_extra(
    call: RuntimeCall,
    generation: u64,
    key: &sr25519::Pair,
    extra: TxExtension,
) -> UncheckedExtrinsic {
    let implication = (1u8, &call, &extra, extra.implicit().unwrap()).encode();
    let next_commitment = subtensor_hashed::key_commitment(
        Scheme::Sr25519,
        &pair(u8::try_from(generation).unwrap().checked_add(11).unwrap())
            .public()
            .0,
    );
    let payload = subtensor_hashed::transaction_payload(
        account().as_ref(),
        Scheme::Sr25519,
        generation,
        &next_commitment,
        &implication,
    );
    let proof = Proof {
        generation,
        public_key: key.public().0,
        next_commitment,
        signature: key.sign(&payload).0,
    };
    UncheckedExtrinsic::new_hashed(
        call,
        AuthorizeAccount {
            account: account(),
            proof,
        },
        extra,
    )
}

fn signing_state() -> (pallet_hashed_accounts::AccountRecord, u32, TaoBalance) {
    (
        HashedAccounts::accounts(account()).unwrap(),
        System::account_nonce(account()),
        Balances::free_balance(account()),
    )
}

fn legacy(key: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
    let who = AccountId::from(key.public());
    let extra = extra(System::account_nonce(&who));
    let signature = SignedPayload::new(call.clone(), extra.clone())
        .unwrap()
        .using_encoded(|m| key.sign(m));
    UncheckedExtrinsic::new_signed(call, who.into(), Signature::Sr25519(signature), extra)
}
#[test]
fn wire_roundtrip_preserves_legacy_and_rejects_unknown_pipeline() {
    ext().execute_with(|| {
        setup();
        let xt = signed(call(), 0, &pair(10));
        let bytes = xt.encode();
        assert_eq!(UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap(), xt);
        assert_eq!(
            xt.get_dispatch_info().extension_weight,
            xt.clone()
                .check(&ChainContext::default())
                .unwrap()
                .get_dispatch_info()
                .extension_weight
        );
        let old = LegacyUnchecked::new_signed(
            call(),
            AccountId::from(pair(1).public()).into(),
            Signature::Sr25519(pair(1).sign(b"x")),
            extra(0),
        );
        assert_eq!(
            UncheckedExtrinsic::Legacy(old.clone()).encode(),
            old.encode()
        );
        let general: LegacyUnchecked = sp_runtime::generic::UncheckedExtrinsic {
            preamble: sp_runtime::generic::Preamble::General(0, extra(0)),
            function: call(),
        }
        .into();
        assert_eq!(
            UncheckedExtrinsic::decode(&mut &general.encode()[..]).unwrap(),
            UncheckedExtrinsic::Legacy(general)
        );
        let mut input = &bytes[..];
        let _ = codec::Compact::<u32>::decode(&mut input).unwrap();
        let offset = bytes.len() - input.len();
        let mut changed = bytes;
        changed[offset + 1] = 9;
        assert!(UncheckedExtrinsic::decode(&mut &changed[..]).is_err());
    });
}
#[test]
fn included_failure_advances_generation_but_invalid_proof_does_not() {
    ext().execute_with(|| {
        setup();
        let failing = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: AccountId::new([25; 32]).into(),
            value: TaoBalance::new(u64::MAX),
        });
        let xt = signed(failing.clone(), 0, &pair(10));
        let invalid = signed(failing, 0, &pair(9));
        assert!(Executive::apply_extrinsic(invalid).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 0);
        assert_eq!(
            Executive::apply_extrinsic(xt.clone()).unwrap(),
            Err(sp_runtime::DispatchError::Arithmetic(
                sp_runtime::ArithmeticError::Underflow
            ))
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        assert_eq!(System::account_nonce(account()), 1);
        assert!(Executive::apply_extrinsic(xt).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(11))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 2);
        assert_eq!(System::account_nonce(account()), 2);
    });
}
#[test]
fn rejected_weight_does_not_rotate() {
    ext().execute_with(|| {
        setup();
        hashed_auth::TestVerificationWeight::set(Weight::MAX);
        assert!(Executive::apply_extrinsic(signed(call(), 0, &pair(10))).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 0);
    });
}
#[test]
fn sponsor_can_register_without_revealing_signer_and_fund_atomically() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let _ = Balances::make_free_balance_be(&sponsor_id, TaoBalance::new(1_000_000_000_000));
        let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: alloc::vec![
                RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                    descriptor: descriptor()
                }),
                RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                    dest: account().into(),
                    value: TaoBalance::new(1_000_000_000)
                }),
            ],
        });
        frame_support::assert_ok!(Executive::apply_extrinsic(legacy(&sponsor, call)).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 1);
        assert_eq!(
            Balances::free_balance(account()),
            TaoBalance::new(1_000_000_000)
        );
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(super::hashed_tests::call(), 0, &pair(10))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
    });
}
#[test]
fn registered_identity_cannot_use_legacy_signature_path() {
    ext().execute_with(|| {
        setup();
        let key = pair(20);
        let who = AccountId::from(key.public());
        pallet_hashed_accounts::Accounts::<Runtime>::insert(
            &who,
            pallet_hashed_accounts::AccountRecord {
                descriptor: descriptor(),
                generation: 0,
                commitment: descriptor().initial_commitment,
            },
        );
        assert!(Executive::apply_extrinsic(legacy(&key, call())).is_err());
    });
}

#[test]
fn hashed_accounts_only_accept_hashed_proxy_delegates() {
    ext().execute_with(|| {
        setup();
        let delegate = AccountId::from(pair(30).public());
        frame_support::assert_noop!(
            Proxy::add_proxy_delegate(&account(), delegate.clone(), ProxyType::Any, 0),
            pallet_proxy::Error::<Runtime>::AccountPolicyViolation
        );
        pallet_hashed_accounts::Accounts::<Runtime>::insert(
            &delegate,
            pallet_hashed_accounts::AccountRecord {
                descriptor: descriptor(),
                generation: 0,
                commitment: descriptor().initial_commitment,
            },
        );
        frame_support::assert_ok!(Proxy::add_proxy_delegate(
            &account(),
            delegate.clone(),
            ProxyType::Any,
            0,
        ));
        assert!(Proxy::find_proxy(&account(), &delegate, None).is_ok());
        // Even a stale grant cannot authorize an unprotected delegate.
        pallet_hashed_accounts::Accounts::<Runtime>::remove(&delegate);
        assert!(Proxy::find_proxy(&account(), &delegate, None).is_err());
    });
}

#[cfg(feature = "runtime-benchmarks")]
#[test]
fn registration_benchmark_context_covers_outer_decode_and_proxy_cleanup() {
    use pallet_hashed_accounts::OnRegister;
    ext().execute_with(|| {
        HashedEnabled::set(true);
        let sponsor = AccountId::from(pair(31).public());
        let _ = Balances::make_free_balance_be(&sponsor, TaoBalance::new(1_000_000_000_000));
        frame_support::assert_ok!(hashed_auth::OnHashedRegistered::setup_benchmark(
            &account(),
            &sponsor,
            &descriptor(),
        ));
        assert_eq!(
            Proxy::proxies(account()).0.len(),
            MaxProxies::get() as usize
        );
        assert!(System::extrinsic_data(0).len() > 16_000);
        frame_support::assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor),
            descriptor()
        ));
        assert!(Proxy::proxies(account()).0.is_empty());
    });
}

#[test]
fn successive_generations_keep_the_account_and_reject_retired_keys() {
    ext().execute_with(|| {
        setup();
        let stable = account();
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 0, &pair(10))).unwrap()
        );
        let current = HashedAccounts::accounts(&stable).unwrap();
        assert_eq!(current.generation, 1);
        assert_eq!(
            current.commitment,
            subtensor_hashed::key_commitment(Scheme::Sr25519, &pair(11).public().0)
        );
        // A fresh nonce/generation cannot make a retired signing secret valid.
        let before = signing_state();
        assert_eq!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(10))),
            Err(InvalidTransaction::BadProof.into())
        );
        assert_eq!(signing_state(), before);
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(11))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(stable).unwrap().generation, 2);
    });
}
#[test]
fn future_nonce_preparation_failure_rolls_back_rotation() {
    ext().execute_with(|| {
        setup();
        let before = HashedAccounts::accounts(account()).unwrap();
        let balance = Balances::free_balance(account());
        assert!(Executive::apply_extrinsic(signed_nonce(call(), 0, &pair(10), 5)).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap(), before);
        assert_eq!(System::account_nonce(account()), 0);
        assert_eq!(Balances::free_balance(account()), balance);
    });
}
#[test]
fn setup_through_derivative_wrapper_cannot_change_authorization() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let _ = Balances::make_free_balance_be(
            &AccountId::from(sponsor.public()),
            TaoBalance::new(1_000_000_000_000),
        );
        let registration = RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
            descriptor: descriptor(),
        });
        let call = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
            index: 0,
            call: alloc::boxed::Box::new(registration),
        });
        assert!(
            Executive::apply_extrinsic(legacy(&sponsor, call))
                .unwrap()
                .is_err()
        );
        assert!(!HashedAccounts::is_registered(&account()));
    });
}

#[test]
fn insufficient_fee_preserves_authority_and_the_same_proof_can_be_retried() {
    ext().execute_with(|| {
        setup();
        // The signer has a permanent provider and enough ordinary balance;
        // only this self-paid tip makes the transaction unaffordable.
        let mut extensions = extra(0);
        extensions.1.0 = transaction_payment_wrapper::ChargeTransactionPaymentWrapper::new(
            TaoBalance::new(2_000_000_000_000),
        );
        let transaction = signed_with_extra(call(), 0, &pair(10), extensions);
        let before = signing_state();
        assert_eq!(
            Executive::apply_extrinsic(transaction.clone()),
            Err(InvalidTransaction::Payment.into())
        );
        assert_eq!(signing_state(), before);

        let _ = Balances::make_free_balance_be(&account(), TaoBalance::new(4_000_000_000_000));
        frame_support::assert_ok!(Executive::apply_extrinsic(transaction).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        assert_eq!(System::account_nonce(account()), 1);
    });
}

#[test]
fn competing_devices_share_a_generation_tag_even_with_different_nonces() {
    ext().execute_with(|| {
        setup();
        let first = signed_nonce(call(), 0, &pair(10), 0);
        let competitor = signed_nonce(call(), 0, &pair(10), 1);
        let before = signing_state();
        let validate = |transaction: &UncheckedExtrinsic| {
            transaction
                .clone()
                .check(&ChainContext::default())
                .unwrap()
                .validate::<Runtime>(
                    TransactionSource::External,
                    &transaction.get_dispatch_info(),
                    transaction.encoded_size(),
                )
                .unwrap()
        };
        let first_validity = validate(&first);
        let second_validity = validate(&competitor);
        let generation_tag = (b"hashed-generation", account(), 0u64).encode();
        assert!(first_validity.provides.contains(&generation_tag));
        assert!(second_validity.provides.contains(&generation_tag));
        // Ordinary nonce tags differ; the hashed tag prevents both authorities
        // being queued as if they were sequential signing generations.
        assert_ne!(first_validity.provides, second_validity.provides);
        assert_eq!(signing_state(), before);

        frame_support::assert_ok!(Executive::apply_extrinsic(first).unwrap());
        let included = signing_state();
        assert_eq!(
            Executive::apply_extrinsic(competitor),
            Err(InvalidTransaction::BadProof.into())
        );
        assert_eq!(signing_state(), included);
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(11))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 2);
    });
}

#[test]
fn failed_first_funding_rolls_back_registration_and_can_be_retried() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let initial_balance = TaoBalance::new(1_000_000_000_000);
        let _ = Balances::make_free_balance_be(&sponsor_id, initial_balance);
        let batch = |value| {
            RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: alloc::vec![
                    RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                        descriptor: descriptor(),
                    }),
                    RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                        dest: account().into(),
                        value,
                    }),
                ],
            })
        };
        // Balances rejects a transfer above total issuance with Underflow;
        // checking that error rules out an early registration rejection.
        assert_eq!(
            Executive::apply_extrinsic(legacy(&sponsor, batch(TaoBalance::new(u64::MAX)))).unwrap(),
            Err(sp_runtime::DispatchError::Arithmetic(
                sp_runtime::ArithmeticError::Underflow
            ))
        );
        assert!(!HashedAccounts::is_registered(&account()));
        assert!(
            !pallet_hashed_accounts::EvmAliases::<Runtime>::contains_key(
                HashedAccounts::evm_alias(&account())
            )
        );
        assert_eq!(System::providers(&account()), 0);
        assert_eq!(System::account_nonce(account()), 0);
        assert_eq!(Balances::reserved_balance(&sponsor_id), TaoBalance::new(0));
        assert_eq!(System::account_nonce(&sponsor_id), 1);
        assert!(Balances::free_balance(&sponsor_id) < initial_balance);

        frame_support::assert_ok!(
            Executive::apply_extrinsic(legacy(&sponsor, batch(TaoBalance::new(1_000_000_000))))
                .unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 0, &pair(10))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
    });
}

#[test]
fn changing_the_call_nonce_tip_or_next_key_cannot_reuse_a_signature() {
    ext().execute_with(|| {
        setup();
        let original = signed(call(), 0, &pair(10));
        let before = signing_state();
        for field in ["call", "nonce", "tip", "next_key", "signature"] {
            let mut changed = original.clone();
            let UncheckedExtrinsic::Hashed(transaction) = &mut changed else {
                panic!("expected hashed transaction");
            };
            let sp_runtime::generic::Preamble::General(_, (authorization, extensions)) =
                &mut transaction.0.preamble
            else {
                panic!("expected general transaction");
            };
            match field {
                "call" => {
                    transaction.0.function = RuntimeCall::System(SystemCall::remark {
                        remark: b"altered call".to_vec(),
                    });
                }
                "nonce" => extensions.0.5 = check_nonce::CheckNonce::from(1),
                "tip" => {
                    extensions.1.0 =
                        transaction_payment_wrapper::ChargeTransactionPaymentWrapper::new(
                            TaoBalance::new(1),
                        );
                }
                "next_key" => authorization.proof.next_commitment = [99; 32],
                "signature" => authorization.proof.signature[0] ^= 1,
                _ => unreachable!(),
            }
            assert_eq!(
                Executive::apply_extrinsic(changed),
                Err(InvalidTransaction::BadProof.into()),
                "mutated {field} must invalidate the signature"
            );
            assert_eq!(signing_state(), before);
        }
        frame_support::assert_ok!(Executive::apply_extrinsic(original).unwrap());
    });
}

#[test]
fn a_proof_signed_for_another_genesis_cannot_advance_the_account() {
    ext().execute_with(|| {
        setup();
        let transaction = signed(call(), 0, &pair(10));
        let before = signing_state();
        frame_system::BlockHash::<Runtime>::insert(0, H256::repeat_byte(7));
        assert_eq!(
            Executive::apply_extrinsic(transaction.clone()),
            Err(InvalidTransaction::BadProof.into())
        );
        assert_eq!(signing_state(), before);
        frame_system::BlockHash::<Runtime>::insert(0, H256::zero());
        frame_support::assert_ok!(Executive::apply_extrinsic(transaction).unwrap());
    });
}

#[test]
fn reverting_an_inclusion_restores_authority_and_rejects_the_orphaned_next_generation() {
    ext().execute_with(|| {
        setup();
        let before = signing_state();
        let original = signed(call(), 0, &pair(10));
        // Simulate the storage effect of discarding an included block. This
        // verifies state recovery, not consensus finality or key concealment.
        sp_io::storage::start_transaction();
        frame_support::assert_ok!(Executive::apply_extrinsic(original.clone()).unwrap());
        let orphaned_next = signed(call(), 1, &pair(11));
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        sp_io::storage::rollback_transaction();
        assert_eq!(signing_state(), before);
        assert_eq!(
            Executive::apply_extrinsic(orphaned_next),
            Err(InvalidTransaction::BadProof.into())
        );
        assert_eq!(signing_state(), before);
        frame_support::assert_ok!(Executive::apply_extrinsic(original).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
    });
}

fn first_funding(descriptor: Descriptor, value: TaoBalance) -> RuntimeCall {
    let recipient = AccountId::new(subtensor_hashed::account_id(&descriptor));
    RuntimeCall::Utility(pallet_utility::Call::batch_all {
        calls: alloc::vec![
            RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register { descriptor }),
            RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                dest: recipient.into(),
                value,
            }),
        ],
    })
}

fn assert_no_received_account(recipient: &AccountId) {
    assert!(!HashedAccounts::is_registered(recipient));
    assert!(
        !pallet_hashed_accounts::EvmAliases::<Runtime>::contains_key(HashedAccounts::evm_alias(
            recipient
        ))
    );
    assert_eq!(System::providers(recipient), 0);
    assert_eq!(System::account_nonce(recipient), 0);
    assert_eq!(Balances::free_balance(recipient), TaoBalance::new(0));
}

#[test]
fn competing_first_funding_sponsors_preserve_the_recipients_rotated_authority() {
    ext().execute_with(|| {
        System::set_block_number(1);
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let first = pair(22);
        let second = pair(23);
        let first_id = AccountId::from(first.public());
        let second_id = AccountId::from(second.public());
        for sponsor in [&first_id, &second_id] {
            let _ = Balances::make_free_balance_be(sponsor, TaoBalance::new(1_000_000_000_000));
        }
        // Both senders build against the same unregistered receiving address.
        let first_transaction = legacy(
            &first,
            first_funding(descriptor(), TaoBalance::new(1_000_000_000)),
        );
        let second_transaction = legacy(
            &second,
            first_funding(descriptor(), TaoBalance::new(2_000_000_000)),
        );
        assert_no_received_account(&account());
        frame_support::assert_ok!(Executive::apply_extrinsic(first_transaction).unwrap());
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 0, &pair(10))).unwrap()
        );
        let (rotated, nonce, balance) = signing_state();
        let providers = System::providers(&account());
        frame_support::assert_ok!(Executive::apply_extrinsic(second_transaction).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap(), rotated);
        assert_eq!(System::account_nonce(account()), nonce);
        assert_eq!(System::providers(&account()), providers);
        assert_eq!(
            Balances::free_balance(account()),
            balance + TaoBalance::new(2_000_000_000)
        );
        assert_eq!(
            Balances::reserved_balance(first_id),
            HashedRegistrationDeposit::get()
        );
        assert_eq!(Balances::reserved_balance(second_id), TaoBalance::new(0));
        assert_eq!(
            System::events()
                .iter()
                .filter(|event| matches!(
                    &event.event,
                    RuntimeEvent::HashedAccounts(pallet_hashed_accounts::Event::Registered { .. })
                ))
                .count(),
            1
        );
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(11))).unwrap()
        );
    });
}

#[test]
fn first_funding_send_all_reserves_registration_before_computing_transferable_funds() {
    use transaction_payment_wrapper::FeeWeightDiscount;
    ext().execute_with(|| {
        System::set_block_number(1);
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let initial = TaoBalance::new(1_000_000_000_000);
        let _ = Balances::make_free_balance_be(&sponsor_id, initial);
        let batch = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: alloc::vec![
                RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                    descriptor: descriptor()
                }),
                RuntimeCall::Balances(BalancesCall::transfer_all {
                    dest: account().into(),
                    keep_alive: false,
                }),
            ],
        });
        let transaction = legacy(&sponsor, batch.clone());
        let info = transaction.get_dispatch_info();
        let fee_info = transaction_payment_wrapper::fee_dispatch_info(
            &info,
            Runtime::fee_weight_discount(&batch, &info),
        );
        let prepaid = TransactionPayment::compute_fee(
            u32::try_from(transaction.encoded_size()).unwrap(),
            &fee_info,
            TaoBalance::new(0),
        );
        frame_support::assert_ok!(Executive::apply_extrinsic(transaction).unwrap());
        let actual_fee = System::events()
            .iter()
            .find_map(|event| match &event.event {
                RuntimeEvent::TransactionPayment(
                    pallet_transaction_payment::Event::TransactionFeePaid {
                        who, actual_fee, ..
                    },
                ) if who == &sponsor_id => Some(*actual_fee),
                _ => None,
            })
            .unwrap();
        let deposit = HashedRegistrationDeposit::get();
        assert_eq!(Balances::reserved_balance(&sponsor_id), deposit);
        // The permanent reserve also needs the sponsor's ordinary balances
        // provider, so transfer_all must retain its existential deposit.
        let retained = ExistentialDeposit::get();
        assert_eq!(
            Balances::free_balance(account()),
            initial - deposit - prepaid - retained
        );
        assert_eq!(
            Balances::free_balance(&sponsor_id),
            retained + prepaid - actual_fee
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert!(System::providers(&account()) > 0);
    });
}

#[test]
fn disabled_or_unsupported_first_funding_leaves_no_recipient_or_deposit() {
    let mut bad_version = descriptor();
    bad_version.version = 2;
    let mut zero_commitment = descriptor();
    zero_commitment.initial_commitment = [0; 32];
    for (enabled, receiving_descriptor, error) in [
        (
            false,
            descriptor(),
            pallet_hashed_accounts::Error::<Runtime>::Disabled,
        ),
        (
            true,
            bad_version,
            pallet_hashed_accounts::Error::<Runtime>::UnsupportedDescriptor,
        ),
        (
            true,
            zero_commitment,
            pallet_hashed_accounts::Error::<Runtime>::UnsupportedDescriptor,
        ),
    ] {
        ext().execute_with(|| {
            HashedEnabled::set(enabled);
            hashed_auth::TestVerificationWeight::set(Weight::zero());
            let sponsor = pair(22);
            let sponsor_id = AccountId::from(sponsor.public());
            let initial = TaoBalance::new(1_000_000_000_000);
            let _ = Balances::make_free_balance_be(&sponsor_id, initial);
            let recipient = AccountId::new(subtensor_hashed::account_id(&receiving_descriptor));
            assert_eq!(
                Executive::apply_extrinsic(legacy(
                    &sponsor,
                    first_funding(receiving_descriptor, TaoBalance::new(1_000_000_000))
                ))
                .unwrap(),
                Err(error.into()),
            );
            assert_no_received_account(&recipient);
            assert_eq!(Balances::reserved_balance(&sponsor_id), TaoBalance::new(0));
            assert_eq!(System::account_nonce(&sponsor_id), 1);
            assert!(Balances::free_balance(&sponsor_id) < initial);
        });
    }
}

#[test]
fn nested_first_funding_batch_is_filtered_without_registration_or_transfer() {
    ext().execute_with(|| {
        System::set_block_number(1);
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let _ = Balances::make_free_balance_be(&sponsor_id, TaoBalance::new(1_000_000_000_000));
        let nested = RuntimeCall::Utility(pallet_utility::Call::batch {
            calls: alloc::vec![first_funding(descriptor(), TaoBalance::new(1_000_000_000))],
        });
        // The runtime's nesting filter rejects this before registration dispatch.
        assert_eq!(
            Executive::apply_extrinsic(legacy(&sponsor, nested)).unwrap(),
            Err(frame_system::Error::<Runtime>::CallFiltered.into())
        );
        assert_no_received_account(&account());
        assert_eq!(Balances::reserved_balance(sponsor_id), TaoBalance::new(0));
    });
}

#[test]
fn proxied_first_funding_cannot_leave_a_partial_registration_or_transfer() {
    ext().execute_with(|| {
        System::set_block_number(1);
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let delegate = pair(22);
        let delegate_id = AccountId::from(delegate.public());
        let real = AccountId::from(pair(23).public());
        for who in [&delegate_id, &real] {
            let _ = Balances::make_free_balance_be(who, TaoBalance::new(1_000_000_000_000));
        }
        frame_support::assert_ok!(Proxy::add_proxy_delegate(
            &real,
            delegate_id.clone(),
            ProxyType::Any,
            0
        ));
        let real_free = Balances::free_balance(&real);
        let real_reserved = Balances::reserved_balance(&real);
        let proxied = RuntimeCall::Proxy(pallet_proxy::Call::proxy {
            real: real.clone().into(),
            force_proxy_type: None,
            call: alloc::boxed::Box::new(first_funding(
                descriptor(),
                TaoBalance::new(1_000_000_000),
            )),
        });
        // Proxy also reports inner failure in an event while returning success.
        frame_support::assert_ok!(Executive::apply_extrinsic(legacy(&delegate, proxied)).unwrap());
        // DispatchError::Other omits its text when encoded into event storage.
        assert!(System::events().iter().any(|event| matches!(
            &event.event,
            RuntimeEvent::Proxy(pallet_proxy::Event::ProxyExecuted {
                result: Err(sp_runtime::DispatchError::Other(_)),
            })
        )));
        assert_no_received_account(&account());
        assert_eq!(Balances::free_balance(&real), real_free);
        assert_eq!(Balances::reserved_balance(&real), real_reserved);
        assert_eq!(Balances::reserved_balance(delegate_id), TaoBalance::new(0));
    });
}

#[test]
fn multisig_first_funding_cannot_leave_a_partial_registration_or_transfer() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let signer = pair(22);
        let signer_id = AccountId::from(signer.public());
        let other = AccountId::from(pair(23).public());
        let mut signatories = alloc::vec![signer_id.clone(), other.clone()];
        signatories.sort();
        let multisig = Multisig::multi_account_id(&signatories, 1);
        let initial = TaoBalance::new(1_000_000_000_000);
        for who in [&signer_id, &multisig] {
            let _ = Balances::make_free_balance_be(who, initial);
        }
        let wrapped = RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
            other_signatories: alloc::vec![other],
            call: alloc::boxed::Box::new(first_funding(
                descriptor(),
                TaoBalance::new(1_000_000_000),
            )),
        });
        assert_eq!(
            Executive::apply_extrinsic(legacy(&signer, wrapped)).unwrap(),
            Err(sp_runtime::DispatchError::Other(
                "InvalidHashedRegistrationContext"
            ))
        );
        assert_no_received_account(&account());
        assert_eq!(Balances::free_balance(&multisig), initial);
        assert_eq!(Balances::reserved_balance(multisig), TaoBalance::new(0));
        assert_eq!(Balances::reserved_balance(signer_id), TaoBalance::new(0));
    });
}

#[test]
fn preassociated_classical_owner_blocks_activation_without_funding_the_account() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let classical_owner = AccountId::from(pair(23).public());
        let _ = Balances::make_free_balance_be(&sponsor_id, TaoBalance::new(1_000_000_000_000));
        pallet_subtensor::Owner::<Runtime>::insert(account(), &classical_owner);
        assert_eq!(
            Executive::apply_extrinsic(legacy(
                &sponsor,
                first_funding(descriptor(), TaoBalance::new(1_000_000_000))
            ))
            .unwrap(),
            Err(sp_runtime::DispatchError::Other(
                "HashedHotkeyRequiresHashedOwner"
            )),
        );
        assert_no_received_account(&account());
        assert_eq!(Balances::reserved_balance(sponsor_id), TaoBalance::new(0));
        assert_eq!(
            pallet_subtensor::Owner::<Runtime>::get(account()),
            classical_owner
        );
        // This fails closed against alternative authority. It does not resolve
        // the activation denial caused by an earlier classical owner assignment.
    });
}
