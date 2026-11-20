//! Production-weight fee quotes for explicitly sized native signed-v4 envelopes.
//! Run with MS_FEE_QUOTE_OUTPUT to retain JSON. These are pre-dispatch quotes,
//! not successful execution proofs; destination state and order proof validity
//! are deliberately outside this deterministic pricing fixture.
#![allow(clippy::expect_used)]

use codec::{Decode, Encode};
use frame_support::dispatch::GetDispatchInfo;
use node_subtensor_runtime::{
    Address, BlockWeights, BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System,
    TxExtension, UncheckedExtrinsic, check_mortality, check_nonce, check_nonzero_sender,
    sudo_wrapper, transaction_payment_wrapper,
};
use pallet_hashed_accounts::DerivedDescriptor;
use pallet_limit_orders::{Order, OrderSignature, OrderType, SignedOrder, VersionedOrder};
use sp_runtime::{generic::Era, traits::TransactionExtension};
use subtensor_hashed::mldsa;
use subtensor_runtime_common::{AccountId, NetUid, ProxyType, Signature, TaoBalance, Token};

fn extra() -> TxExtension {
    (
        (
            check_nonzero_sender::CheckNonZeroSender::new(),
            frame_system::CheckSpecVersion::new(),
            frame_system::CheckTxVersion::new(),
            frame_system::CheckGenesis::new(),
            check_mortality::CheckMortality::from(Era::Immortal),
            check_nonce::CheckNonce::from(0),
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

fn envelope(call: RuntimeCall, ms: bool, nonce: u32) -> UncheckedExtrinsic {
    envelope_for_profile(call, ms.then_some(mldsa::ALGORITHM), nonce)
}

fn ms_key(profile: u8) -> ([u8; 32], mldsa::PublicKeyBytes, mldsa::Descriptor) {
    for attempt in 0u32..256 {
        let mut seed = [42; 32];
        if profile == mldsa::FIXED_ALGORITHM {
            seed[..4].copy_from_slice(&attempt.to_le_bytes());
        }
        let public_key = mldsa::public_key_from_seed(&seed).expect("public key");
        let descriptor = mldsa::Descriptor {
            version: mldsa::VERSION,
            algorithm: profile,
            initial_commitment: mldsa::commitment(&public_key),
        };
        if profile != mldsa::FIXED_ALGORITHM
            || mldsa::rejects_legacy_public_keys(&mldsa::account_id(&descriptor))
        {
            return (seed, public_key, descriptor);
        }
    }
    panic!("stateless fixture could not find an admissible account")
}

fn envelope_for_profile(call: RuntimeCall, profile: Option<u8>, nonce: u32) -> UncheckedExtrinsic {
    let mut extra = extra();
    extra.0.5 = check_nonce::CheckNonce::from(nonce);
    let (address, signature) = if let Some(profile) = profile {
        let (seed, public_key, descriptor) = ms_key(profile);
        let header = mldsa::Header {
            account: mldsa::account_id(&descriptor),
            version: mldsa::VERSION,
            algorithm: profile,
            generation: 0,
            public_key,
            next_commitment: if profile == mldsa::FIXED_ALGORITHM {
                descriptor.initial_commitment
            } else {
                [43; 64]
            },
        };
        let payload = (&call, &extra, extra.implicit().expect("implicit")).encode();
        let address = header.sign(&seed, &payload, &[0; 32]).expect("MS proof");
        (
            Address::Raw(address.encode()),
            Signature::Ed25519(sp_core::ed25519::Signature::from_raw(
                mldsa::CARRIER_SIGNATURE,
            )),
        )
    } else {
        (
            Address::Id(AccountId::from([1; 32])),
            Signature::Sr25519(sp_core::sr25519::Signature::from_raw([0; 64])),
        )
    };
    UncheckedExtrinsic::new_signed(call, address, signature, extra)
}

fn sample_order() -> VersionedOrder<AccountId> {
    VersionedOrder::V1(Order {
        signer: AccountId::from([1; 32]),
        hotkey: AccountId::from([1; 32]),
        netuid: NetUid::from(1u16),
        order_type: OrderType::LimitBuy,
        amount: 1,
        limit_price: 1,
        expiry: 1,
        fee_rate: sp_runtime::Perbill::from_parts(1),
        fee_recipient: AccountId::from([1; 32]),
        relayer: None,
        max_slippage: None,
        chain_id: 1,
        partial_fills_enabled: false,
    })
    .with_nonce(0)
    .expect("authorized terms")
}

#[test]
fn measured_calls_quote_finite_fees_with_real_envelope_sizes() {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("genesis")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        let descriptor = mldsa::Descriptor { version: 2, algorithm: 4, initial_commitment: [2; 64] };
        let derived = DerivedDescriptor::Multisig { threshold: 2, signatories: vec![AccountId::from([2; 32]), AccountId::from([3; 32])].try_into().expect("two members") };
        let order = sample_order();
        let mut rows = vec![
            ("register MS (SR sponsor)", false, RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register { descriptor: descriptor.into() })),
            ("check_registered MS (SR sponsor)", false, RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::check_registered { descriptor: descriptor.into() })),
            ("register_derived (2-of-2 descriptor)", false, RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register_derived { descriptor: derived.clone() })),
            ("register_derived_sponsored (inner call only)", false, RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register_derived_sponsored { descriptor: derived.clone(), deposit_payer: AccountId::from([1; 32]) })),
            ("check_derived (2-of-2 descriptor)", false, RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::check_derived { descriptor: derived })),
            ("create_protected_pure (MS)", true, RuntimeCall::Proxy(pallet_subtensor_proxy::Call::create_protected_pure { proxy_type: ProxyType::Any, delay: 0, index: 0 })),
            ("as_protected_derivative (MS; empty remark)", true, RuntimeCall::Utility(pallet_subtensor_utility::Call::as_protected_derivative { index: 0, call: Box::new(RuntimeCall::System(frame_system::Call::remark { remark: vec![] })) })),
            ("authorize_order (MS; V1 terms)", true, RuntimeCall::LimitOrders(pallet_limit_orders::Call::authorize_order { order: order.clone() })),
            ("cancel_order (MS; authorized V1 terms)", true, RuntimeCall::LimitOrders(pallet_limit_orders::Call::cancel_order { order: order.clone() })),
            ("transfer 1 TAO (MS; registered destination)", true, RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: AccountId::from([9; 32]).into(), value: TaoBalance::new(1_000_000_000) })),
            ("transfer 1 TAO (SR comparison)", false, RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: AccountId::from([9; 32]).into(), value: TaoBalance::new(1_000_000_000) })),
        ];
        for (n, label, batch_label) in [(1, "execute_orders (1 account approval; SR relayer)", "execute_batched_orders (1 account approval; SR relayer)"), (8, "execute_orders (8 account approvals; SR relayer)", "execute_batched_orders (8 account approvals; SR relayer)")] {
            let entry = SignedOrder { order: order.clone(), signature: OrderSignature::AccountApproved, partial_fill: None };
            let orders: frame_support::BoundedVec<_, <Runtime as pallet_limit_orders::Config>::MaxOrdersPerBatch> = vec![entry; n].try_into().expect("bounded orders");
            rows.push((label, false, RuntimeCall::LimitOrders(pallet_limit_orders::Call::execute_orders { orders: orders.clone(), should_fail: true })));
            rows.push((batch_label, false, RuntimeCall::LimitOrders(pallet_limit_orders::Call::execute_batched_orders { netuid: 1.into(), orders })));
        }
        for (size, label) in [(8192, "Shield carrier (8192 bytes; SR)"), (8193, "Shield carrier (8193 bytes; SR)"), (32768, "Shield carrier (32768 bytes; SR)")] {
            rows.push((label, false, RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted { ciphertext: vec![0; size].try_into().expect("carrier bound") })));
        }
        for (size, label) in [(8192, "Shield carrier (8192 bytes; MS)"), (8193, "Shield carrier (8193 bytes; MS)"), (32768, "Shield carrier (32768 bytes; MS)")] {
            rows.push((label, true, RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted { ciphertext: vec![0; size].try_into().expect("carrier bound") })));
        }
        let sponsored_call = rows.iter().find(|(name, _, _)| name.starts_with("register_derived_sponsored")).expect("sponsored fixture").2.clone();
        let max_weight = sponsored_call.get_dispatch_info().call_weight;
        rows.push(("register_derived_sponsored (2-of-2 final MS approval)", true, RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
            threshold: 2, other_signatories: vec![AccountId::from([3; 32])],
            maybe_timepoint: Some(pallet_multisig::Timepoint { height: 1, index: 0 }),
            call: Box::new(sponsored_call), max_weight,
        })));
        let mut quotes = Vec::new();
        for (name, ms, call) in rows {
            let xt = envelope(call.clone(), ms, 0);
            let encoded = xt.encode();
            let decoded = UncheckedExtrinsic::decode(&mut encoded.as_slice()).expect("signed-v4 roundtrip");
            assert_eq!(decoded.encode(), encoded);
            let info = xt.get_dispatch_info();
            assert!(info.total_weight().all_lt(BlockWeights::get().max_block), "{name}: cannot fit block");
            let details = node_subtensor_runtime::staking_fee::query_fee_details(&call, &info, encoded.len().try_into().expect("encoded length"), false);
            let fee = details.final_fee().to_u64();
            let inclusion = details.inclusion_fee.expect("quoted inclusion fee");
            quotes.push(serde_json::json!({"name":name,"ms_signer":ms,"encoded_bytes":encoded.len(),"call_bytes":call.encoded_size(),"call_ref_time":info.call_weight.ref_time(),"call_proof_size":info.call_weight.proof_size(),"extension_ref_time":info.extension_weight.ref_time(),"base_fee_rao":inclusion.base_fee.to_u64(),"length_fee_rao":inclusion.len_fee.to_u64(),"weight_fee_rao":inclusion.adjusted_weight_fee.to_u64(),"total_fee_rao":fee}));
        }
        if let Some(path) = std::env::var_os("MS_FEE_QUOTE_OUTPUT") {
            let output = serde_json::json!({
                "quotes": quotes,
                "parameters": {
                    "fee_multiplier_inner": pallet_transaction_payment::NextFeeMultiplier::<Runtime>::get().into_inner().to_string(),
                    "classical_registration_deposit_rao": node_subtensor_runtime::HashedRegistrationDeposit::get().to_u64(),
                    "ms_registration_deposit_rao": node_subtensor_runtime::MlDsaRegistrationDeposit::get().to_u64(),
                    "derived_registration_deposit_rao": node_subtensor_runtime::DerivedRegistrationDeposit::get().to_u64(),
                    "pure_proxy_deposit_rao": node_subtensor_runtime::ProxyDepositBase::get().to_u64() + node_subtensor_runtime::ProxyDepositFactor::get().to_u64(),
                }
            });
            std::fs::write(path, serde_json::to_string_pretty(&output).expect("JSON")).expect("write quotes");
        }
    });
}

#[test]
fn measured_production_weights_admit_and_execute_ms_transfer() {
    use frame_support::{assert_ok, traits::Currency};
    use node_subtensor_runtime::{AdminUtils, Balances, Executive, HashedAccounts, RuntimeOrigin};
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("genesis")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        let sponsor = AccountId::from([1; 32]);
        let _ = Balances::make_free_balance_be(&sponsor, TaoBalance::new(100_000_000_000));
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        let descriptor = mldsa::Descriptor {
            version: mldsa::VERSION,
            algorithm: mldsa::ALGORITHM,
            initial_commitment: mldsa::commitment(
                &mldsa::public_key_from_seed(&[42; 32]).expect("public key"),
            ),
        };
        let account = AccountId::from(mldsa::account_id(&descriptor));
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor),
            descriptor.into()
        ));
        let _ = Balances::make_free_balance_be(&account, TaoBalance::new(10_000_000_000));
        let nonce = System::account_nonce(&account);
        let destination = AccountId::from([9; 32]);
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
            dest: destination.clone().into(),
            value: TaoBalance::new(1_000_000_000),
        });
        let tx = envelope(call, true, nonce);
        let predicted_info = tx.get_dispatch_info();
        assert!(
            predicted_info
                .total_weight()
                .all_lt(BlockWeights::get().max_block)
        );
        assert_ok!(Executive::apply_extrinsic(tx).expect("admitted with production weights"));
        assert_eq!(Balances::free_balance(destination).to_u64(), 1_000_000_000);
        assert_eq!(System::account_nonce(&account), nonce + 1);
        assert_eq!(
            pallet_hashed_accounts::MlDsaAccounts::<Runtime>::get(&account)
                .expect("registered")
                .generation,
            1
        );
        assert!(Balances::free_balance(account).to_u64() < 9_000_000_000);
    });
}

#[test]
fn configured_runtime_admits_fixed_ms_transfer_without_registration() {
    use frame_support::{assert_ok, traits::Currency};
    use node_subtensor_runtime::{AdminUtils, Balances, Executive, HashedAccounts, RuntimeOrigin};
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("genesis")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        let (_, _, descriptor) = ms_key(mldsa::FIXED_ALGORITHM);
        let account = AccountId::new(mldsa::account_id(&descriptor));
        let _ = Balances::make_free_balance_be(&account, TaoBalance::new(10_000_000_000));
        let destination = AccountId::new([9; 32]);
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
            dest: destination.clone().into(),
            value: TaoBalance::new(1_000_000_000),
        });
        let tx = envelope_for_profile(call, Some(mldsa::FIXED_ALGORITHM), 0);
        assert!(
            tx.get_dispatch_info()
                .total_weight()
                .all_lt(BlockWeights::get().max_block)
        );
        assert_ok!(
            Executive::apply_extrinsic(tx.clone()).expect("admitted with production weights")
        );
        assert_eq!(Balances::free_balance(destination).to_u64(), 1_000_000_000);
        assert_eq!(System::account_nonce(&account), 1);
        assert_eq!(Balances::reserved_balance(&account).to_u64(), 0);
        assert!(!pallet_hashed_accounts::MlDsaAccounts::<Runtime>::contains_key(&account));
        assert!(
            !pallet_hashed_accounts::EvmAliases::<Runtime>::contains_key(
                HashedAccounts::evm_alias(&account)
            )
        );
        assert!(Balances::free_balance(&account).to_u64() < 9_000_000_000);
        assert!(Executive::apply_extrinsic(tx).is_err());
    });
}

#[test]
fn configured_runtime_admits_every_safe_composite_call_with_fixed_ms_authority() {
    use frame_support::{
        assert_ok,
        traits::{Currency, Get},
    };
    use node_subtensor_runtime::{
        AdminUtils, Balances, Executive, Multisig, Proxy, RuntimeOrigin, SafeDerivationWeight,
    };

    // Integration tests link the production runtime, including its real weights.
    // Unit mocks alone would not catch a MAX gate or an oversized MS envelope.
    for operation in 0..8 {
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
            .build_storage()
            .expect("genesis")
            .into();
        ext.execute_with(|| {
            System::set_block_number(1);
            System::set_extrinsic_index(0);
            assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
                RuntimeOrigin::root(),
                true
            ));
            let (_, _, descriptor) = ms_key(mldsa::FIXED_ALGORITHM);
            let account = AccountId::new(mldsa::account_id(&descriptor));
            let _ = Balances::make_free_balance_be(&account, TaoBalance::new(100_000_000_000));
            let others = vec![AccountId::new([7; 32])];
            let inner = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
            let call_hash = sp_io::hashing::blake2_256(&inner.encode());
            let max_weight = inner.get_dispatch_info().call_weight;
            let when = Multisig::timepoint();
            if matches!(operation, 3 | 4) {
                assert_ok!(Multisig::as_multi_safe(
                    RuntimeOrigin::signed(account.clone()),
                    2,
                    others.clone(),
                    None,
                    Box::new(inner.clone()),
                    max_weight
                ));
            }
            let pure = Proxy::safe_pure_account(&account, &ProxyType::Any, 0, Some((1, 0)))
                .expect("safe pure account");
            if operation == 6 {
                assert_ok!(Proxy::create_safe_pure(
                    RuntimeOrigin::signed(account.clone()),
                    ProxyType::Any,
                    0,
                    0
                ));
            }
            let call = match operation {
                0 => RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1_safe {
                    other_signatories: others,
                    call: Box::new(inner),
                }),
                1 => RuntimeCall::Multisig(pallet_multisig::Call::as_multi_safe {
                    threshold: 2,
                    other_signatories: others,
                    maybe_timepoint: None,
                    call: Box::new(inner),
                    max_weight,
                }),
                2 => RuntimeCall::Multisig(pallet_multisig::Call::approve_as_multi_safe {
                    threshold: 2,
                    other_signatories: others,
                    maybe_timepoint: None,
                    call_hash,
                    max_weight,
                }),
                3 => RuntimeCall::Multisig(pallet_multisig::Call::cancel_as_multi_safe {
                    threshold: 2,
                    other_signatories: others,
                    timepoint: when,
                    call_hash,
                }),
                4 => RuntimeCall::Multisig(pallet_multisig::Call::poke_deposit_safe {
                    threshold: 2,
                    other_signatories: others,
                    call_hash,
                }),
                5 => RuntimeCall::Proxy(pallet_subtensor_proxy::Call::create_safe_pure {
                    proxy_type: ProxyType::Any,
                    delay: 0,
                    index: 0,
                }),
                6 => RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
                    real: pure.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(RuntimeCall::Proxy(
                        pallet_subtensor_proxy::Call::kill_safe_pure {
                            spawner: account.clone().into(),
                            proxy_type: ProxyType::Any,
                            index: 0,
                            height: 1,
                            ext_index: 0,
                        },
                    )),
                }),
                7 => RuntimeCall::Utility(pallet_subtensor_utility::Call::as_safe_derivative {
                    index: 0,
                    call: Box::new(inner),
                }),
                _ => unreachable!(),
            };
            let tx = envelope_for_profile(call.clone(), Some(mldsa::FIXED_ALGORITHM), 0);
            let info = tx.get_dispatch_info();
            assert!(info.call_weight.all_gte(SafeDerivationWeight::get()));
            assert!(info.total_weight().all_lt(BlockWeights::get().max_block));
            let quote = node_subtensor_runtime::staking_fee::query_fee_details(
                &call,
                &info,
                tx.encoded_size().try_into().expect("encoded length"),
                false,
            );
            assert!(quote.final_fee().to_u64() > 0);
            assert_ok!(Executive::apply_extrinsic(tx).expect("production admission"));
            assert_eq!(System::account_nonce(&account), 1);
            assert!(!pallet_hashed_accounts::MlDsaAccounts::<Runtime>::contains_key(&account));
            assert!(
                pallet_hashed_accounts::DerivedAuthorities::<Runtime>::iter()
                    .next()
                    .is_none()
            );
            if operation == 6 {
                // proxy() reports dispatch success even if its inner call fails.
                assert!(Proxy::proxies(pure).0.is_empty());
                assert_eq!(Balances::reserved_balance(&account).to_u64(), 0);
            }
        });
    }
}
