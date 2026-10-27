//! Registrationless fixed-key MS uses ordinary origins and durable native nonces.
use super::*;
use pallet_hashed_accounts::{AuthenticationKind, DerivedAuthorities, EvmAliases};

pub(crate) struct Wallet {
    pub(crate) seed: [u8; 32],
    pub(crate) descriptor: mldsa::Descriptor,
    pub(crate) account: AccountId,
}

impl Wallet {
    pub(crate) fn new(identity: u8) -> Self {
        for attempt in 0u32..256 {
            let mut seed = [identity; 32];
            seed[..4].copy_from_slice(&attempt.to_le_bytes());
            let public_key = mldsa::public_key_from_seed(&seed).unwrap();
            let descriptor = mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: mldsa::FIXED_ALGORITHM,
                initial_commitment: mldsa::commitment(&public_key),
            };
            let account = mldsa::account_id(&descriptor);
            if mldsa::rejects_legacy_public_keys(&account) {
                return Self {
                    seed,
                    descriptor,
                    account: AccountId::new(account),
                };
            }
        }
        panic!("stateless fixture could not find an admissible native account")
    }

    pub(crate) fn sign(&self, call: RuntimeCall) -> UncheckedExtrinsic {
        self.sign_nonce(call, System::account_nonce(&self.account))
    }

    fn sign_nonce(&self, call: RuntimeCall, nonce: u32) -> UncheckedExtrinsic {
        let extra = extra(nonce);
        let header = mldsa::Header {
            account: *self.account.as_ref(),
            version: mldsa::VERSION,
            algorithm: mldsa::FIXED_ALGORITHM,
            generation: 0,
            public_key: mldsa::public_key_from_seed(&self.seed).unwrap(),
            next_commitment: self.descriptor.initial_commitment,
        };
        let payload = (&call, &extra, extra.implicit().unwrap()).encode();
        let address = header.sign(&self.seed, &payload, &[0; 32]).unwrap();
        UncheckedExtrinsic::new_signed(
            call,
            Address::Raw(address.encode()),
            Signature::Ed25519(sp_core::ed25519::Signature::from_raw(
                mldsa::CARRIER_SIGNATURE,
            )),
            extra,
        )
    }
}

fn setup() {
    System::set_block_number(1);
    pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
    hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
}

fn fund(account: &AccountId) {
    let _ = Balances::make_free_balance_be(account, TaoBalance::new(1_000_000_000_000));
}

fn transfer(destination: &AccountId, amount: u64) -> RuntimeCall {
    RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
        dest: destination.clone().into(),
        value: TaoBalance::new(amount),
    })
}

fn assert_registrationless(account: &AccountId) {
    assert!(!MlDsaAccounts::<Runtime>::contains_key(account));
    assert!(HashedAccounts::accounts(account).is_none());
    assert!(!DerivedAuthorities::<Runtime>::contains_key(account));
    assert!(!EvmAliases::<Runtime>::contains_key(
        HashedAccounts::evm_alias(account)
    ));
    assert_eq!(
        HashedAccounts::authentication_kind(account),
        AuthenticationKind::Ordinary
    );
    assert_eq!(Balances::reserved_balance(account), TaoBalance::new(0));
}

#[test]
fn prefunded_account_spends_without_registration_or_a_sponsor_deposit() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(80);
        let funder = pair(81);
        let funder_account = AccountId::from(funder.public());
        fund(&funder_account);
        assert_ok!(
            Executive::apply_extrinsic(legacy(&funder, transfer(&wallet.account, 10_000_000_000),))
                .unwrap()
        );
        assert_eq!(System::account_nonce(&wallet.account), 0);
        assert_registrationless(&wallet.account);
        assert_eq!(
            Balances::reserved_balance(&funder_account),
            TaoBalance::new(0)
        );

        let destination = AccountId::new([82; 32]);
        let tx = wallet.sign(transfer(&destination, 1_000_000_000));
        let encoded = tx.encode();
        assert_eq!(UncheckedExtrinsic::decode(&mut &encoded[..]).unwrap(), tx);
        let balance = Balances::free_balance(&wallet.account);
        assert!(tx.clone().check(&ChainContext::default()).is_ok());
        assert_eq!(System::account_nonce(&wallet.account), 0);
        assert_eq!(Balances::free_balance(&wallet.account), balance);
        assert_registrationless(&wallet.account);
        assert_ok!(Executive::apply_extrinsic(tx.clone()).unwrap());
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert_registrationless(&wallet.account);
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert_eq!(
            frame_system::Account::<Runtime>::get(&wallet.account).sufficients,
            1
        );
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn dispatch_failure_consumes_nonce_but_invalid_proof_does_not() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(83);
        fund(&wallet.account);
        let tx = wallet.sign(transfer(&AccountId::new([84; 32]), u64::MAX));
        let mut invalid = tx.clone();
        let UncheckedExtrinsic::Legacy(ref mut legacy) = invalid;
        legacy.0.function = call();
        let balance = Balances::free_balance(&wallet.account);
        assert!(Executive::apply_extrinsic(invalid).is_err());
        assert_eq!(System::account_nonce(&wallet.account), 0);
        assert_eq!(Balances::free_balance(&wallet.account), balance);
        assert!(Executive::apply_extrinsic(tx.clone()).unwrap().is_err());
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert!(Balances::free_balance(&wallet.account) < balance);
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn admin_pause_blocks_stateless_auth_without_consuming_the_nonce() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(100);
        fund(&wallet.account);
        let tx = wallet.sign(call());
        let balance = Balances::free_balance(&wallet.account);
        let account = frame_system::Account::<Runtime>::get(&wallet.account);
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            false
        ));
        assert!(Executive::apply_extrinsic(tx.clone()).is_err());
        assert_eq!(
            frame_system::Account::<Runtime>::get(&wallet.account),
            account
        );
        assert_eq!(Balances::free_balance(&wallet.account), balance);
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        assert_ok!(Executive::apply_extrinsic(tx).unwrap());
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn ordinary_owner_can_install_and_revoke_an_optional_order_capability() {
    use pallet_hashed_accounts::{
        ActiveCapabilities, CapabilityEpochs, CapabilityRole, CapabilityScope,
    };
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(101);
        fund(&wallet.account);
        let commitment = mldsa::commitment(&mldsa::public_key_from_seed(&[102; 32]).unwrap());
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::HashedAccounts(
                pallet_hashed_accounts::Call::install_capability {
                    expected_epoch: 1,
                    key_commitment: commitment,
                    expires_at: 100,
                    scope: CapabilityScope::UnrestrictedTrading,
                },
            )))
            .unwrap()
        );
        assert!(ActiveCapabilities::<Runtime>::contains_key(
            &wallet.account,
            CapabilityRole::Order
        ));
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::HashedAccounts(
                pallet_hashed_accounts::Call::revoke_capability {
                    role: CapabilityRole::Order
                },
            )))
            .unwrap()
        );
        assert!(!ActiveCapabilities::<Runtime>::contains_key(
            &wallet.account,
            CapabilityRole::Order
        ));
        assert_eq!(
            CapabilityEpochs::<Runtime>::get(&wallet.account, CapabilityRole::Order),
            2
        );
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn draining_and_refunding_cannot_reset_consumed_native_nonces() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(85);
        fund(&wallet.account);
        let first = wallet.sign(call());
        assert_ok!(Executive::apply_extrinsic(first.clone()).unwrap());
        let drain = wallet.sign(RuntimeCall::Balances(BalancesCall::transfer_all {
            dest: AccountId::new([86; 32]).into(),
            keep_alive: false,
        }));
        assert_ok!(Executive::apply_extrinsic(drain.clone()).unwrap());
        // Remove any fee refund too, exercising balances' complete reap path.
        assert_ok!(Balances::force_set_balance(
            RuntimeOrigin::root(),
            wallet.account.clone().into(),
            TaoBalance::new(0),
        ));
        assert_eq!(Balances::free_balance(&wallet.account), TaoBalance::new(0));
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert!(frame_system::Account::<Runtime>::contains_key(
            &wallet.account
        ));
        fund(&wallet.account);
        let refunded = Balances::free_balance(&wallet.account);
        for consumed in [first, drain] {
            assert!(Executive::apply_extrinsic(consumed).is_err());
            assert_eq!(System::account_nonce(&wallet.account), 2);
            assert_eq!(Balances::free_balance(&wallet.account), refunded);
        }
        assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
        assert_eq!(System::account_nonce(&wallet.account), 3);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn exhausted_native_nonce_is_rejected_without_wrapping_or_charging() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(103);
        fund(&wallet.account);
        frame_system::Account::<Runtime>::mutate(&wallet.account, |account| {
            account.nonce = u32::MAX
        });
        let before = frame_system::Account::<Runtime>::get(&wallet.account);
        let tx = wallet.sign_nonce(call(), u32::MAX);
        assert!(
            Executive::validate_transaction(
                sp_runtime::transaction_validity::TransactionSource::External,
                tx.clone(),
                System::parent_hash(),
            )
            .is_err()
        );
        assert_eq!(
            frame_system::Account::<Runtime>::get(&wallet.account),
            before
        );
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_eq!(
            frame_system::Account::<Runtime>::get(&wallet.account),
            before
        );
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn consecutive_fixed_transactions_have_independent_pool_nonce_tags() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(104);
        fund(&wallet.account);
        let first = wallet.sign_nonce(call(), 0);
        let second = wallet.sign_nonce(call(), 1);
        let validate = |tx| {
            Executive::validate_transaction(
                sp_runtime::transaction_validity::TransactionSource::External,
                tx,
                System::parent_hash(),
            )
            .unwrap()
        };
        let first_validity = validate(first.clone());
        let second_validity = validate(second.clone());
        let first_tag = (&wallet.account, 0u32).encode();
        let second_tag = (&wallet.account, 1u32).encode();
        let obsolete_generation_tag = (b"hashed-generation", &wallet.account, 0u64).encode();
        assert!(first_validity.provides.contains(&first_tag));
        assert!(second_validity.provides.contains(&second_tag));
        assert!(second_validity.requires.contains(&first_tag));
        assert!(!first_validity.provides.contains(&obsolete_generation_tag));
        assert!(!second_validity.provides.contains(&obsolete_generation_tag));
        assert_eq!(System::account_nonce(&wallet.account), 0);
        let before = frame_system::Account::<Runtime>::get(&wallet.account);
        assert!(Executive::apply_extrinsic(second.clone()).is_err());
        // Authorization prepares a sufficient reference before CheckNonce rejects
        // this future transaction. The failed application must roll it all back.
        assert_eq!(
            frame_system::Account::<Runtime>::get(&wallet.account),
            before
        );
        assert_registrationless(&wallet.account);
        assert_ok!(Executive::apply_extrinsic(first).unwrap());
        assert_ok!(Executive::apply_extrinsic(second).unwrap());
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn hostile_hotkey_association_does_not_block_direct_ms_spending() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(87);
        let attacker = pair(88);
        let attacker_account = AccountId::from(attacker.public());
        fund(&attacker_account);
        assert_ok!(
            Executive::apply_extrinsic(legacy(
                &attacker,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::try_associate_hotkey {
                    hotkey: wallet.account.clone(),
                }),
            ))
            .unwrap()
        );
        assert_eq!(
            pallet_subtensor::Owner::<Runtime>::get(&wallet.account),
            attacker_account
        );
        fund(&wallet.account);
        let destination = AccountId::new([89; 32]);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(transfer(&destination, 1_000_000_000))).unwrap()
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn ordinary_classical_proxy_can_be_granted_and_execute() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(90);
        fund(&wallet.account);
        let delegate = pair(91);
        let delegate_account = AccountId::from(delegate.public());
        fund(&delegate_account);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::add_proxy {
                    delegate: delegate_account.clone().into(),
                    proxy_type: ProxyType::Any,
                    delay: 0,
                },
            )))
            .unwrap()
        );
        let destination = AccountId::new([92; 32]);
        assert_ok!(
            Executive::apply_extrinsic(legacy(
                &delegate,
                RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                    real: wallet.account.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(transfer(&destination, 1_000_000_000)),
                }),
            ))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert!(!MlDsaAccounts::<Runtime>::contains_key(&wallet.account));
        assert!(!HashedAccounts::is_protected(&wallet.account));
    });
}

#[test]
fn ordinary_multisig_accepts_ms_approval_without_protecting_the_composite() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(93);
        fund(&wallet.account);
        let other = pair(94);
        let other_account = AccountId::from(other.public());
        fund(&other_account);
        let mut members = vec![wallet.account.clone(), other_account.clone()];
        members.sort();
        let composite = Multisig::multi_account_id(&members, 2);
        fund(&composite);
        let destination = AccountId::new([95; 32]);
        let inner = transfer(&destination, 1_000_000_000);
        let hash = sp_io::hashing::blake2_256(&inner.encode());
        let max_weight = inner.get_dispatch_info().total_weight();
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Multisig(
                pallet_multisig::Call::approve_as_multi {
                    threshold: 2,
                    other_signatories: vec![other_account.clone()],
                    maybe_timepoint: None,
                    call_hash: hash,
                    max_weight,
                },
            )))
            .unwrap()
        );
        let pending = pallet_multisig::Multisigs::<Runtime>::get(&composite, hash).unwrap();
        assert_ok!(
            Executive::apply_extrinsic(legacy(
                &other,
                RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
                    threshold: 2,
                    other_signatories: vec![wallet.account.clone()],
                    maybe_timepoint: Some(pending.when),
                    call: Box::new(inner),
                    max_weight,
                }),
            ))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert!(!pallet_multisig::Multisigs::<Runtime>::contains_key(
            &composite, hash
        ));
        assert_registrationless(&wallet.account);
        assert_registrationless(&composite);
    });
}

#[test]
fn ordinary_derivative_and_pure_proxy_creation_need_no_protected_registration() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(96);
        fund(&wallet.account);
        let child = Utility::derivative_account_id(wallet.account.clone(), 7).unwrap();
        fund(&child);
        let destination = AccountId::new([97; 32]);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Utility(
                pallet_utility::Call::as_derivative {
                    index: 7,
                    call: Box::new(transfer(&destination, 1_000_000_000)),
                },
            )))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert_registrationless(&child);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::create_pure {
                    proxy_type: ProxyType::Any,
                    delay: 0,
                    index: 8,
                },
            )))
            .unwrap()
        );
        let pure = System::events()
            .iter()
            .find_map(|record| match &record.event {
                RuntimeEvent::Proxy(pallet_proxy::Event::PureCreated { pure, who, .. })
                    if who == &wallet.account =>
                {
                    Some(pure.clone())
                }
                _ => None,
            })
            .unwrap();
        assert!(!DerivedAuthorities::<Runtime>::contains_key(&pure));
        assert!(!EvmAliases::<Runtime>::contains_key(
            HashedAccounts::evm_alias(&pure)
        ));
        assert_eq!(Proxy::proxies(pure).0[0].delegate, wallet.account);
        assert!(!MlDsaAccounts::<Runtime>::contains_key(&wallet.account));
    });
}

#[test]
fn safe_ms_multisig_requires_the_quorum_without_registration() {
    ext().execute_with(|| {
        setup();
        let first = Wallet::new(121);
        let second = Wallet::new(122);
        fund(&first.account);
        fund(&second.account);
        let mut members = vec![first.account.clone(), second.account.clone()];
        members.sort();
        let composite = Multisig::safe_multi_account_id(&members, 2).unwrap();
        assert!(mldsa::rejects_legacy_public_keys(composite.as_ref()));
        assert_ne!(composite, Multisig::multi_account_id(&members, 2));
        fund(&composite);
        let destination = AccountId::new([123; 32]);
        let inner = transfer(&destination, 1_000_000_000);
        let hash = sp_io::hashing::blake2_256(&inner.encode());
        let max_weight = inner.get_dispatch_info().total_weight();
        assert_ok!(
            Executive::apply_extrinsic(first.sign(RuntimeCall::Multisig(
                pallet_multisig::Call::approve_as_multi_safe {
                    threshold: 2,
                    other_signatories: vec![second.account.clone()],
                    maybe_timepoint: None,
                    call_hash: hash,
                    max_weight,
                },
            )))
            .unwrap()
        );
        let pending = pallet_multisig::Multisigs::<Runtime>::get(&composite, hash).unwrap();
        assert_eq!(
            pending.approvals.as_slice(),
            core::slice::from_ref(&first.account)
        );
        assert_eq!(Balances::free_balance(&destination), TaoBalance::new(0));
        assert_eq!(Balances::reserved_balance(&first.account), pending.deposit);
        assert_ok!(
            Executive::apply_extrinsic(second.sign(RuntimeCall::Multisig(
                pallet_multisig::Call::as_multi_safe {
                    threshold: 2,
                    other_signatories: vec![first.account.clone()],
                    maybe_timepoint: Some(pending.when),
                    call: Box::new(inner),
                    max_weight,
                },
            )))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(&destination),
            TaoBalance::new(1_000_000_000)
        );
        assert!(!pallet_multisig::Multisigs::<Runtime>::contains_key(
            &composite, hash
        ));
        for account in [&first.account, &second.account, &composite] {
            assert_registrationless(account);
        }
    });
}

#[test]
fn safe_ms_derivative_and_pure_proxy_use_full_width_authority() {
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(124);
        fund(&wallet.account);
        let child = Utility::safe_derivative_account_id(wallet.account.clone(), 7).unwrap();
        assert!(mldsa::rejects_legacy_public_keys(child.as_ref()));
        fund(&child);
        let destination = AccountId::new([125; 32]);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Utility(
                pallet_utility::Call::as_safe_derivative {
                    index: 7,
                    call: Box::new(transfer(&destination, 1_000_000_000)),
                },
            )))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(&destination),
            TaoBalance::new(1_000_000_000)
        );
        assert_registrationless(&child);
        let height = System::block_number();
        let ext_index = System::extrinsic_index().unwrap_or_default();
        let pure = Proxy::safe_pure_account(
            &wallet.account,
            &ProxyType::Any,
            8,
            Some((height, ext_index)),
        )
        .unwrap();
        assert!(mldsa::rejects_legacy_public_keys(pure.as_ref()));
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::create_safe_pure {
                    proxy_type: ProxyType::Any,
                    delay: 0,
                    index: 8,
                },
            )))
            .unwrap()
        );
        let (delegates, deposit) = Proxy::proxies(pure.clone());
        assert_eq!(delegates[0].delegate, wallet.account);
        assert_eq!(Balances::reserved_balance(&wallet.account), deposit);
        assert!(!DerivedAuthorities::<Runtime>::contains_key(&pure));
        assert!(!EvmAliases::<Runtime>::contains_key(
            HashedAccounts::evm_alias(&pure)
        ));
        fund(&pure);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::proxy {
                    real: pure.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(transfer(&destination, 1_000_000_000)),
                },
            )))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(&destination),
            TaoBalance::new(2_000_000_000)
        );
        // Move funds before killing the pure, just as for the legacy lifecycle.
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::proxy {
                    real: pure.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(RuntimeCall::Balances(BalancesCall::transfer_all {
                        dest: wallet.account.clone().into(),
                        keep_alive: false,
                    })),
                },
            )))
            .unwrap()
        );
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::Proxy(
                pallet_proxy::Call::proxy {
                    real: pure.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(RuntimeCall::Proxy(pallet_proxy::Call::kill_safe_pure {
                        spawner: wallet.account.clone().into(),
                        proxy_type: ProxyType::Any,
                        index: 8,
                        height,
                        ext_index,
                    })),
                },
            )))
            .unwrap()
        );
        assert!(Proxy::proxies(pure.clone()).0.is_empty());
        assert_registrationless(&wallet.account);
        assert_registrationless(&pure);
    });
}

#[test]
fn native_evm_keeps_ordinary_separate_backing_balance_and_nonce() {
    use pallet_evm::{AddressMapping, FeeCalculator};
    use sp_core::{H160, U256};
    ext().execute_with(|| {
        setup();
        let wallet = Wallet::new(98);
        fund(&wallet.account);
        let source = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&wallet.account)[..20]);
        let backing = evm_origin::HashedAddressMapping::into_account_id(source);
        assert_eq!(
            backing,
            pallet_evm::HashedAddressMapping::<sp_runtime::traits::BlakeTwo256>::into_account_id(
                source
            )
        );
        assert_ne!(backing, wallet.account);
        // Ordinary native funding of the historical EVM backing address.
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(transfer(&backing, 10_000_000_000))).unwrap()
        );
        assert_eq!(System::account_nonce(&wallet.account), 1);
        assert_eq!(EVM::account_basic(&source).0.nonce, U256::zero());
        let target = H160::repeat_byte(0x42);
        let destination = evm_origin::HashedAddressMapping::into_account_id(target);
        let before = Balances::free_balance(&backing);
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);
        assert_ok!(
            Executive::apply_extrinsic(wallet.sign(RuntimeCall::EVM(pallet_evm::Call::call {
                source,
                target,
                input: vec![],
                value: U256::from(1_000_000_000_u64) * U256::from(EVM_TO_SUBSTRATE_DECIMALS),
                gas_limit: 100_000,
                max_fee_per_gas: <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
                max_priority_fee_per_gas: None,
                nonce: Some(U256::zero()),
                access_list: vec![],
                authorization_list: vec![],
            },)))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(destination),
            TaoBalance::new(1_000_000_000)
        );
        assert!(Balances::free_balance(&backing) < before - TaoBalance::new(1_000_000_000));
        assert_eq!(System::account_nonce(&wallet.account), 2);
        assert_eq!(EVM::account_basic(&source).0.nonce, U256::one());
        assert!(evm_origin::HashedAddressMapping::is_ethereum_authority_allowed(source));
        assert_registrationless(&wallet.account);
    });
}

#[test]
fn rotating_profile_still_requires_registration_and_keeps_generation_state() {
    with_profiles(|| {
        setup();
        let descriptor = descriptor(99);
        let account = AccountId::new(mldsa::account_id(&descriptor));
        fund(&account);
        let before = Balances::free_balance(&account);
        let unregistered = sign_with_extra(99, 0, call(), extra(0));
        assert!(Executive::apply_extrinsic(unregistered).is_err());
        assert_eq!(Balances::free_balance(&account), before);
        assert_eq!(System::account_nonce(&account), 0);
        let account = register(99);
        let alias = HashedAccounts::evm_alias(&account);
        assert_eq!(EvmAliases::<Runtime>::get(alias), Some(account.clone()));
        assert_ok!(Executive::apply_extrinsic(sign(99, call())).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            1
        );
        assert_eq!(System::account_nonce(&account), 2);
        assert_eq!(EvmAliases::<Runtime>::get(alias), Some(account.clone()));
        assert!(HashedAccounts::is_protected(&account));
    });
}
