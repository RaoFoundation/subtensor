//! ML-DSA authority enters the same Executive and pallet origin pipeline.
use super::*;
use frame_support::{assert_noop, assert_ok};
use pallet_hashed_accounts::{DerivedDescriptor, MlDsaAccounts};
use sp_runtime::{generic::Preamble, traits::TransactionExtension};
use subtensor_hashed::mldsa;

#[path = "mldsa_stateless_tests.rs"]
pub(super) mod stateless;

// Registered rotation and protected authority graphs use profile 4. Fixed
// profile 5 follows ordinary authority policy and is exercised by `stateless`.
pub(super) fn with_profiles(test: impl Fn()) {
    ext().execute_with(|| {
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        test();
    });
}

pub(super) fn seed(identity: u8, generation: u64) -> [u8; 32] {
    let mut seed = [identity; 32];
    seed[..8].copy_from_slice(&generation.to_le_bytes());
    seed
}

pub(super) fn descriptor(identity: u8) -> mldsa::Descriptor {
    mldsa::Descriptor {
        version: mldsa::VERSION,
        algorithm: mldsa::ALGORITHM,
        initial_commitment: mldsa::commitment(
            &mldsa::public_key_from_seed(&seed(identity, 0)).unwrap(),
        ),
    }
}

pub(super) fn register(identity: u8) -> AccountId {
    System::set_block_number(1);
    assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
        RuntimeOrigin::root(),
        true
    ));
    hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
    let sponsor = AccountId::from(pair(70).public());
    let _ = Balances::make_free_balance_be(&sponsor, TaoBalance::new(1_000_000_000_000_000));
    let descriptor = descriptor(identity);
    let account = AccountId::new(mldsa::account_id(&descriptor));
    assert_ok!(HashedAccounts::register(
        RuntimeOrigin::signed(sponsor),
        descriptor.into()
    ));
    let _ = Balances::make_free_balance_be(&account, TaoBalance::new(1_000_000_000_000));
    account
}

pub(super) fn sign(identity: u8, call: RuntimeCall) -> UncheckedExtrinsic {
    let account = AccountId::new(mldsa::account_id(&descriptor(identity)));
    let generation = MlDsaAccounts::<Runtime>::get(&account).unwrap().generation;
    sign_with_extra(
        identity,
        generation,
        call,
        extra(System::account_nonce(account)),
    )
}

fn sign_with_extra(
    identity: u8,
    generation: u64,
    call: RuntimeCall,
    extra: TxExtension,
) -> UncheckedExtrinsic {
    let descriptor = descriptor(identity);
    let header = mldsa::Header {
        account: mldsa::account_id(&descriptor),
        version: 2,
        algorithm: descriptor.algorithm,
        generation,
        public_key: mldsa::public_key_from_seed(&seed(identity, generation)).unwrap(),
        next_commitment: mldsa::commitment(
            &mldsa::public_key_from_seed(&seed(identity, generation + 1)).unwrap(),
        ),
    };
    let payload = (&call, &extra, extra.implicit().unwrap()).encode();
    let address = header
        .sign(&seed(identity, generation), &payload, &[0; 32])
        .unwrap();
    UncheckedExtrinsic::new_signed(
        call,
        Address::Raw(address.encode()),
        Signature::Ed25519(sp_core::ed25519::Signature::from_raw(
            mldsa::CARRIER_SIGNATURE,
        )),
        extra,
    )
}

#[test]
fn full_payload_v4_roundtrip_executes_and_consumes_only_once() {
    with_profiles(|| {
        let account = register(42);
        let call = RuntimeCall::System(SystemCall::remark {
            remark: vec![91; 4096],
        });
        let tx = sign(42, call.clone());
        let wire = tx.encode();
        let mut body = wire.as_slice();
        codec::Compact::<u32>::decode(&mut body).unwrap();
        assert_eq!(&body[..4], &[0x84, 2, 0xdd, 0x53]);
        assert_eq!(UncheckedExtrinsic::decode(&mut &wire[..]).unwrap(), tx);
        assert_eq!(
            LegacyUnchecked::decode(&mut &wire[..]).unwrap().encode(),
            wire
        );
        let before = MlDsaAccounts::<Runtime>::get(&account).unwrap();
        let checked = tx.clone().check(&ChainContext::default()).unwrap();
        assert_eq!(checked.get_dispatch_info(), tx.get_dispatch_info());
        assert_eq!(MlDsaAccounts::<Runtime>::get(&account).unwrap(), before);
        assert_ok!(Executive::apply_extrinsic(tx.clone()).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            1
        );
        assert_eq!(System::account_nonce(&account), 2);
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_ok!(Executive::apply_extrinsic(sign(42, call)).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(account).unwrap().generation,
            2
        );
    });
}

#[test]
fn malformed_markers_and_transcript_mutations_never_rotate_or_charge() {
    with_profiles(|| {
        let account = register(43);
        let tx = sign(43, call());
        let state = MlDsaAccounts::<Runtime>::get(&account);
        let balance = Balances::free_balance(&account);
        for mutation in 0..8 {
            let mut bad = tx.clone();
            let UncheckedExtrinsic::Legacy(ref mut legacy) = bad;
            let Preamble::Signed(Address::Raw(raw), marker, extension) = &mut legacy.0.preamble
            else {
                panic!("raw signer")
            };
            match mutation {
                0 => *marker = Signature::Ed25519(sp_core::ed25519::Signature::from_raw([0; 64])),
                1 => {
                    *marker =
                        Signature::Sr25519(sr25519::Signature::from_raw(mldsa::CARRIER_SIGNATURE))
                }
                2 => raw[0] ^= 1,
                3 => raw[34] ^= 1,
                4 => raw[100] ^= 1,
                5 => raw[2000] ^= 1,
                6 => raw[3000] ^= 1,
                _ => extension.0 = extra(8),
            }
            assert!(
                Executive::apply_extrinsic(bad).is_err(),
                "mutation {mutation}"
            );
            assert_eq!(MlDsaAccounts::<Runtime>::get(&account), state);
            assert_eq!(Balances::free_balance(&account), balance);
            assert_eq!(System::account_nonce(&account), 1);
        }
        let mut changed_call = tx;
        let UncheckedExtrinsic::Legacy(ref mut legacy) = changed_call;
        legacy.0.function = RuntimeCall::System(SystemCall::remark { remark: vec![9] });
        assert!(Executive::apply_extrinsic(changed_call).is_err());
        assert_eq!(MlDsaAccounts::<Runtime>::get(account), state);
    });
}

#[test]
fn included_dispatch_error_rotates_but_disabled_and_overweight_do_not() {
    with_profiles(|| {
        let account = register(44);
        let nonce = System::account_nonce(&account);
        System::reset_events();
        let fail = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: AccountId::new([99; 32]).into(),
            value: TaoBalance::new(u64::MAX),
        });
        let tx = sign(44, fail);
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            false
        ));
        assert!(Executive::apply_extrinsic(tx.clone()).is_err());
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        hashed_auth::TestVerificationWeight::set(Weight::MAX);
        assert!(Executive::apply_extrinsic(tx.clone()).is_err());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            0
        );
        hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
        assert!(Executive::apply_extrinsic(tx.clone()).unwrap().is_err());
        let record = MlDsaAccounts::<Runtime>::get(&account).unwrap();
        assert_eq!(record.generation, 1);
        assert_eq!(
            record.commitment,
            mldsa::commitment(&mldsa::public_key_from_seed(&seed(44, 1)).unwrap())
        );
        assert_eq!(System::account_nonce(&account), nonce + 1);
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_ok!(Executive::apply_extrinsic(sign(44, call())).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            2
        );
        assert_eq!(System::account_nonce(&account), nonce + 2);
        assert!(
            !System::events()
                .iter()
                .any(|record| matches!(record.event, RuntimeEvent::HashedAccounts(_)))
        );
    });
}

#[test]
fn single_admin_toggle_controls_all_hashed_schemes_and_preserves_authority() {
    use pallet_hashed_accounts::{CapabilityEpochs, CapabilityRole, CapabilityScope, EvmAliases};

    with_profiles(|| {
        // Registered rotating ML-DSA is gated through the same AdminUtils flag.
        let account = register(44);
        let tx = sign(44, call());
        let before = MlDsaAccounts::<Runtime>::get(&account);
        let nonce = System::account_nonce(&account);
        let balance = Balances::free_balance(&account);
        assert_ok!(HashedAccounts::revoke_capability(
            RuntimeOrigin::signed(account.clone()),
            CapabilityRole::Order
        ));
        let epoch = CapabilityEpochs::<Runtime>::get(&account, CapabilityRole::Order);
        assert_eq!(epoch, 1);
        let sponsor = pair(70);
        let sponsor_id = AccountId::from(sponsor.public());
        let mut classical = Vec::new();
        for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
            let sr = pair(92);
            let ed = sp_core::ed25519::Pair::from_seed(&[93; 32]);
            let (public_key, next_public) = match scheme {
                Scheme::Sr25519 => (sr.public().0, pair(94).public().0),
                Scheme::Ed25519 => (
                    ed.public().0,
                    sp_core::ed25519::Pair::from_seed(&[95; 32]).public().0,
                ),
            };
            let descriptor = Descriptor {
                version: 1,
                scheme,
                initial_commitment: subtensor_hashed::key_commitment(scheme, &public_key),
            };
            let who = AccountId::new(subtensor_hashed::account_id(&descriptor));
            assert_ok!(
                Executive::apply_extrinsic(legacy(
                    &sponsor,
                    RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                        descriptor: descriptor.into(),
                    })
                ))
                .unwrap()
            );
            let _ = Balances::make_free_balance_be(&who, TaoBalance::new(1_000_000_000_000));
            let extra = extra(System::account_nonce(&who));
            let call = call();
            let address = subtensor_hashed::SignerAddress {
                version: 1,
                account: *who.as_ref(),
                scheme,
                generation: 0,
                public_key,
                next_commitment: subtensor_hashed::key_commitment(scheme, &next_public),
            };
            let message = hashed_extrinsic::signing_hash(&address, &call, &extra).unwrap();
            let signature = match scheme {
                Scheme::Sr25519 => Signature::Sr25519(sr.sign(&message)),
                Scheme::Ed25519 => Signature::Ed25519(ed.sign(&message)),
            };
            let signed = UncheckedExtrinsic::new_signed(
                call,
                Address::Raw(address.encode()),
                signature,
                extra,
            );
            let state = (
                HashedAccounts::accounts(&who),
                System::account_nonce(&who),
                Balances::free_balance(&who),
            );
            classical.push((who, descriptor, signed, state));
        }
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            false
        ));
        assert!(!HashedEnabled::get());
        assert!(Executive::apply_extrinsic(tx.clone()).is_err());
        assert_eq!(MlDsaAccounts::<Runtime>::get(&account), before);
        assert_eq!(System::account_nonce(&account), nonce);
        assert_eq!(Balances::free_balance(&account), balance);
        assert!(HashedAccounts::is_protected(&account));
        assert_eq!(
            EvmAliases::<Runtime>::get(HashedAccounts::evm_alias(&account)),
            Some(account.clone())
        );
        assert_eq!(
            CapabilityEpochs::<Runtime>::get(&account, CapabilityRole::Order),
            epoch
        );
        let fresh_ms = descriptor(45);
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor_id.clone()), fresh_ms.into()),
            pallet_hashed_accounts::Error::<Runtime>::Disabled
        );
        for (who, descriptor, signed, state) in &classical {
            assert!(Executive::apply_extrinsic(signed.clone()).is_err());
            assert_eq!(
                (
                    HashedAccounts::accounts(who),
                    System::account_nonce(who),
                    Balances::free_balance(who)
                ),
                *state
            );
            assert!(HashedAccounts::is_protected(who));
            assert_eq!(
                EvmAliases::<Runtime>::get(HashedAccounts::evm_alias(who)),
                Some(who.clone())
            );
            let mut fresh = *descriptor;
            fresh.initial_commitment[0] ^= 1;
            assert_noop!(
                HashedAccounts::register(RuntimeOrigin::signed(sponsor_id.clone()), fresh.into()),
                pallet_hashed_accounts::Error::<Runtime>::Disabled
            );
        }

        // Ordinary keys keep transferring while every registered hashed scheme is paused.
        let recipient = AccountId::new([98; 32]);
        let transfer = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: recipient.clone().into(),
            value: TaoBalance::new(1_000_000_000),
        });
        let sr = pair(90);
        let _ = Balances::make_free_balance_be(
            &AccountId::from(sr.public()),
            TaoBalance::new(1_000_000_000_000),
        );
        assert_ok!(Executive::apply_extrinsic(legacy(&sr, transfer.clone())).unwrap());
        let ed = sp_core::ed25519::Pair::from_seed(&[91; 32]);
        let who = AccountId::from(ed.public());
        let _ = Balances::make_free_balance_be(&who, TaoBalance::new(1_000_000_000_000));
        let extra = extra(System::account_nonce(&who));
        let signature = SignedPayload::new(transfer.clone(), extra.clone())
            .unwrap()
            .using_encoded(|m| ed.sign(m));
        let ordinary = UncheckedExtrinsic::new_signed(
            transfer,
            who.into(),
            Signature::Ed25519(signature),
            extra,
        );
        assert_ok!(Executive::apply_extrinsic(ordinary).unwrap());
        assert_eq!(
            Balances::free_balance(recipient),
            TaoBalance::new(2_000_000_000)
        );

        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        assert_ok!(Executive::apply_extrinsic(tx.clone()).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            1
        );
        assert_eq!(
            CapabilityEpochs::<Runtime>::get(&account, CapabilityRole::Order),
            epoch
        );
        assert_noop!(
            HashedAccounts::install_capability(
                RuntimeOrigin::signed(account.clone()),
                epoch,
                fresh_ms.initial_commitment,
                100,
                CapabilityScope::UnrestrictedTrading
            ),
            pallet_hashed_accounts::Error::<Runtime>::CapabilityEpochMismatch
        );
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor_id),
            fresh_ms.into()
        ));
        for (who, _, signed, _) in &classical {
            assert_ok!(Executive::apply_extrinsic(signed.clone()).unwrap());
            assert_eq!(HashedAccounts::accounts(who).unwrap().generation, 1);
        }
        // A later pause/resume must not reset consumed generations or capability tombstones.
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            false
        ));
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            1
        );
        assert_eq!(
            CapabilityEpochs::<Runtime>::get(&account, CapabilityRole::Order),
            epoch
        );
        for (who, _, signed, _) in classical {
            assert!(Executive::apply_extrinsic(signed).is_err());
            assert_eq!(HashedAccounts::accounts(who).unwrap().generation, 1);
        }
    });
}

#[test]
fn sponsor_registration_and_funding_are_atomic_under_normal_signed_v4() {
    with_profiles(|| {
        System::set_block_number(1);
        hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
        let sponsor = pair(71);
        let sponsor_id = AccountId::from(sponsor.public());
        let _ = Balances::make_free_balance_be(&sponsor_id, TaoBalance::new(1_000_000_000_000));
        let descriptor = descriptor(45);
        let account = AccountId::new(mldsa::account_id(&descriptor));
        let register_and_fund = |amount| {
            RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: vec![
                    RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                        descriptor: descriptor.into(),
                    }),
                    RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                        dest: account.clone().into(),
                        value: TaoBalance::new(amount),
                    }),
                ],
            })
        };
        assert!(
            Executive::apply_extrinsic(legacy(&sponsor, register_and_fund(u64::MAX)))
                .unwrap()
                .is_err()
        );
        assert!(MlDsaAccounts::<Runtime>::get(&account).is_none());
        assert_eq!(System::account_nonce(&account), 0);
        assert_eq!(Balances::reserved_balance(&sponsor_id), TaoBalance::new(0));
        assert_ok!(
            Executive::apply_extrinsic(legacy(&sponsor, register_and_fund(10_000_000_000)))
                .unwrap()
        );
        assert_eq!(
            Balances::reserved_balance(&sponsor_id),
            TaoBalance::new(200_000_000)
        );
        assert_ok!(Executive::apply_extrinsic(sign(45, call())).unwrap());
    });
}

#[test]
fn mixed_and_all_pq_multisig_execute_under_the_unchanged_account_and_call_hash() {
    for all_pq in [false, true] {
        with_profiles(|| {
            let first = register(46);
            let ordinary = pair(72);
            let second = if all_pq {
                register(47)
            } else {
                AccountId::from(ordinary.public())
            };
            let _ = Balances::make_free_balance_be(&second, TaoBalance::new(1_000_000_000_000));
            let mut members = vec![first.clone(), second.clone()];
            members.sort();
            let descriptor = DerivedDescriptor::Multisig {
                threshold: 2,
                signatories: members.clone().try_into().unwrap(),
            };
            let composite = Multisig::multi_account_id(&members, 2);
            assert_eq!(descriptor.account_id(), composite);
            assert_noop!(
                HashedAccounts::register_derived(
                    RuntimeOrigin::signed(first.clone()),
                    descriptor.clone()
                ),
                sp_runtime::DispatchError::Other("DerivedProtectionRequiresOwner")
            );
            let protect = RuntimeCall::HashedAccounts(
                pallet_hashed_accounts::Call::register_derived_sponsored {
                    descriptor,
                    deposit_payer: second.clone(),
                },
            );
            let protection_hash = sp_io::hashing::blake2_256(&protect.encode());
            let protection_weight = protect.get_dispatch_info().total_weight();
            assert_ok!(
                Executive::apply_extrinsic(sign(
                    46,
                    RuntimeCall::Multisig(pallet_multisig::Call::approve_as_multi {
                        threshold: 2,
                        other_signatories: vec![second.clone()],
                        maybe_timepoint: None,
                        call_hash: protection_hash,
                        max_weight: protection_weight,
                    })
                ))
                .unwrap()
            );
            let pending =
                pallet_multisig::Multisigs::<Runtime>::get(&composite, protection_hash).unwrap();
            let execute_protection = RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
                threshold: 2,
                other_signatories: vec![first.clone()],
                maybe_timepoint: Some(pending.when),
                call: Box::new(protect),
                max_weight: protection_weight,
            });
            let registration = if all_pq {
                sign(47, execute_protection)
            } else {
                legacy(&ordinary, execute_protection)
            };
            let sponsor_reserve = Balances::reserved_balance(&second);
            assert_ok!(Executive::apply_extrinsic(registration).unwrap());
            assert!(HashedAccounts::is_protected(&composite));
            assert_eq!(
                Balances::reserved_balance(&second),
                sponsor_reserve + TaoBalance::new(200_000_000)
            );
            let _ = Balances::make_free_balance_be(&composite, TaoBalance::new(10_000_000_000));
            let recipient = AccountId::new([98; 32]);
            let transfer = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                dest: recipient.clone().into(),
                value: TaoBalance::new(1_000_000_000),
            });
            let hash = sp_io::hashing::blake2_256(&transfer.encode());
            let weight = transfer.get_dispatch_info().total_weight();
            let approve = RuntimeCall::Multisig(pallet_multisig::Call::approve_as_multi {
                threshold: 2,
                other_signatories: vec![second.clone()],
                maybe_timepoint: None,
                call_hash: hash,
                max_weight: weight,
            });
            assert_ok!(Executive::apply_extrinsic(sign(46, approve)).unwrap());
            let pending = pallet_multisig::Multisigs::<Runtime>::get(&composite, hash).unwrap();
            let execute = RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
                threshold: 2,
                other_signatories: vec![first.clone()],
                maybe_timepoint: Some(pending.when),
                call: Box::new(transfer),
                max_weight: weight,
            });
            let tx = if all_pq {
                sign(47, execute)
            } else {
                legacy(&ordinary, execute)
            };
            assert_ok!(Executive::apply_extrinsic(tx).unwrap());
            assert_eq!(
                Balances::free_balance(recipient),
                TaoBalance::new(1_000_000_000)
            );
            assert!(pallet_multisig::Multisigs::<Runtime>::get(&composite, hash).is_none());
            assert!(HashedAccounts::is_protected(&composite));
            assert_eq!(MlDsaAccounts::<Runtime>::get(first).unwrap().generation, 2);
            assert_eq!(MlDsaAccounts::<Runtime>::get(composite), None);
        });
    }
}

#[test]
fn pq_delegates_preserve_identity_and_classical_delegates_are_rejected() {
    with_profiles(|| {
        let account = register(48);
        let delegate = register(49);
        let ordinary = AccountId::from(pair(73).public());
        assert_noop!(
            Proxy::add_proxy(
                RuntimeOrigin::signed(account.clone()),
                ordinary.into(),
                ProxyType::Any,
                0
            ),
            pallet_proxy::Error::<Runtime>::AccountPolicyViolation
        );
        assert_ok!(
            Executive::apply_extrinsic(sign(
                48,
                RuntimeCall::Proxy(pallet_proxy::Call::add_proxy {
                    delegate: delegate.clone().into(),
                    proxy_type: ProxyType::Any,
                    delay: 0,
                })
            ))
            .unwrap()
        );
        let before = MlDsaAccounts::<Runtime>::get(&account);
        let recipient = AccountId::new([97; 32]);
        let inner = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: recipient.clone().into(),
            value: TaoBalance::new(1_000_000_000),
        });
        assert_ok!(
            Executive::apply_extrinsic(sign(
                49,
                RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                    real: account.clone().into(),
                    force_proxy_type: Some(ProxyType::Any),
                    call: Box::new(inner),
                })
            ))
            .unwrap()
        );
        assert_eq!(
            Balances::free_balance(recipient),
            TaoBalance::new(1_000_000_000)
        );
        assert_eq!(MlDsaAccounts::<Runtime>::get(account), before);
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(delegate).unwrap().generation,
            1
        );
    });
}

#[test]
fn derived_protection_cannot_elevate_restricted_proxies_through_evm() {
    use pallet_evm::{AddressMapping, FeeCalculator};
    use sp_core::{H160, U256};
    for kind in 0..3 {
        with_profiles(|| {
            let parent = register(54);
            let member = register(55);
            let delegate = register(56);
            let attacker = register(57);
            let descriptor = match kind {
                0 => {
                    let mut members = vec![parent.clone(), member];
                    members.sort();
                    DerivedDescriptor::Multisig {
                        threshold: 2,
                        signatories: members.try_into().unwrap(),
                    }
                }
                1 => DerivedDescriptor::Derivative {
                    parent: parent.clone(),
                    index: 42,
                },
                _ => {
                    assert_ok!(Proxy::create_protected_pure(
                        RuntimeOrigin::signed(parent.clone()),
                        ProxyType::Any,
                        0,
                        42
                    ));
                    DerivedDescriptor::PureProxy {
                        spawner: parent.clone(),
                        proxy_type: ProxyType::Any.encode().try_into().unwrap(),
                        index: 42,
                        height: System::block_number(),
                        extrinsic_index: System::extrinsic_index().unwrap_or_default(),
                    }
                }
            };
            let account = descriptor.account_id();
            let _ =
                Balances::make_free_balance_be(&account, TaoBalance::new(1_000_000_000_000_000));
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(account.clone()),
                delegate.clone().into(),
                ProxyType::NonCritical,
                0
            ));
            assert_ok!(HashedAccounts::register_derived(
                RuntimeOrigin::signed(account.clone()),
                descriptor
            ));
            let source = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&account)[..20]);
            assert_eq!(
                crate::evm_origin::HashedAddressMapping::into_account_id(source),
                account
            );
            assert!(
                Proxy::proxies(account.clone())
                    .0
                    .iter()
                    .any(|p| p.delegate == delegate && p.proxy_type == ProxyType::NonCritical)
            );
            let escalation = RuntimeCall::Proxy(pallet_proxy::Call::add_proxy {
                delegate: attacker.clone().into(),
                proxy_type: ProxyType::Any,
                delay: 0,
            });
            let evm = RuntimeCall::EVM(pallet_evm::Call::call {
                source,
                target: H160::from_low_u64_be(6),
                input: escalation.encode(),
                value: U256::zero(),
                gas_limit: 5_000_000,
                max_fee_per_gas: <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
                max_priority_fee_per_gas: None,
                nonce: None,
                access_list: vec![],
                authorization_list: Default::default(),
            });
            let nested = RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: vec![evm.clone()],
            });
            for inner in [evm.clone(), nested] {
                System::reset_events();
                assert_ok!(Proxy::proxy(
                    RuntimeOrigin::signed(delegate.clone()),
                    account.clone().into(),
                    Some(ProxyType::NonCritical),
                    Box::new(inner)
                ));
                assert!(System::events().iter().any(|record| matches!(&record.event,
                    RuntimeEvent::Proxy(pallet_proxy::Event::ProxyExecuted { result: Err(error) })
                    if *error == frame_system::Error::<Runtime>::CallFiltered.into()
                )));
                assert!(
                    !Proxy::proxies(account.clone())
                        .0
                        .iter()
                        .any(|p| p.delegate == attacker)
                );
            }
            let call_hash = sp_io::hashing::blake2_256(&evm.encode()).into();
            assert_ok!(Proxy::announce(
                RuntimeOrigin::signed(delegate.clone()),
                account.clone().into(),
                call_hash
            ));
            System::reset_events();
            assert_ok!(Proxy::proxy_announced(
                RuntimeOrigin::signed(parent),
                delegate.clone().into(),
                account.clone().into(),
                Some(ProxyType::NonCritical),
                Box::new(evm.clone())
            ));
            assert!(System::events().iter().any(|record| matches!(&record.event,
                RuntimeEvent::Proxy(pallet_proxy::Event::ProxyExecuted { result: Err(error) })
                if *error == frame_system::Error::<Runtime>::CallFiltered.into()
            )));
            assert!(
                !Proxy::proxies(account.clone())
                    .0
                    .iter()
                    .any(|p| p.delegate == attacker)
            );
            // A grant of full authority may use precisely the same EVM call.
            // This proves the negative cases are filter enforcement, not an
            // unusable payload, gas shortage or unrelated precompile failure.
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(account.clone()),
                delegate.clone().into(),
                ProxyType::Any,
                0
            ));
            assert_ok!(Proxy::proxy(
                RuntimeOrigin::signed(delegate),
                account.clone().into(),
                Some(ProxyType::Any),
                Box::new(evm)
            ));
            assert!(
                Proxy::proxies(account)
                    .0
                    .iter()
                    .any(|p| p.delegate == attacker && p.proxy_type == ProxyType::Any)
            );
        });
    }
}

#[test]
fn protected_derivatives_and_pure_proxies_preserve_their_floor_through_lifecycle() {
    with_profiles(|| {
        let parent = register(50);
        let derivative = Utility::derivative_account_id(parent.clone(), 7).unwrap();
        assert_ok!(
            Executive::apply_extrinsic(sign(
                50,
                RuntimeCall::Utility(pallet_utility::Call::as_protected_derivative {
                    index: 7,
                    call: Box::new(call()),
                })
            ))
            .unwrap()
        );
        assert!(HashedAccounts::authority_meets(
            &derivative,
            pallet_hashed_accounts::SecurityFloor::PqRequired
        ));
        assert_eq!(
            Balances::reserved_balance(&parent),
            TaoBalance::new(200_000_000)
        );
        let failed_child = Utility::derivative_account_id(parent.clone(), 8).unwrap();
        let before = MlDsaAccounts::<Runtime>::get(&parent).unwrap().generation;
        let fail = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: AccountId::new([98; 32]).into(),
            value: TaoBalance::new(u64::MAX),
        });
        assert!(
            Executive::apply_extrinsic(sign(
                50,
                RuntimeCall::Utility(pallet_utility::Call::as_protected_derivative {
                    index: 8,
                    call: Box::new(fail),
                })
            ))
            .unwrap()
            .is_err()
        );
        assert!(!HashedAccounts::is_protected(&failed_child));
        assert_eq!(
            Balances::reserved_balance(&parent),
            TaoBalance::new(200_000_000)
        );
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&parent).unwrap().generation,
            before + 1
        );
        let ext_index = System::extrinsic_index().unwrap_or_default();
        let height = System::block_number();
        let pure =
            Proxy::pure_account(&parent, &ProxyType::Any, 9, Some((height, ext_index))).unwrap();
        assert_ok!(
            Executive::apply_extrinsic(sign(
                50,
                RuntimeCall::Proxy(pallet_proxy::Call::create_protected_pure {
                    proxy_type: ProxyType::Any,
                    delay: 0,
                    index: 9,
                })
            ))
            .unwrap()
        );
        assert!(HashedAccounts::authority_meets(
            &pure,
            pallet_hashed_accounts::SecurityFloor::PqRequired
        ));
        let kill = RuntimeCall::Proxy(pallet_proxy::Call::kill_pure {
            spawner: parent.clone().into(),
            proxy_type: ProxyType::Any,
            index: 9,
            height,
            ext_index,
        });
        assert_ok!(
            Executive::apply_extrinsic(sign(
                50,
                RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                    real: pure.clone().into(),
                    force_proxy_type: None,
                    call: Box::new(kill),
                })
            ))
            .unwrap()
        );
        assert!(Proxy::proxies(pure.clone()).0.is_empty());
        assert!(HashedAccounts::is_protected(&pure));
        assert!(HashedAccounts::is_protected(&derivative));
        // Removing a pure proxy releases its proxy deposit, but both account
        // registration reserves remain permanent.
        assert_eq!(
            Balances::reserved_balance(&parent),
            TaoBalance::new(400_000_000)
        );
    });
}

#[test]
fn restricted_proxies_cannot_reserve_permanent_protected_account_deposits() {
    use frame_support::traits::InstanceFilter;
    use pallet_hashed_accounts::DerivedAuthorities;

    for proxy_type in [
        ProxyType::NonTransfer,
        ProxyType::NonFungible,
        ProxyType::NonCritical,
    ] {
        for pure in [false, true] {
            with_profiles(|| {
                let [owner, delegate, bridge] = [61, 62, 63].map(register);
                assert_ok!(Proxy::add_proxy_delegate(
                    &owner,
                    delegate.clone(),
                    proxy_type,
                    0
                ));
                assert_ok!(Proxy::add_proxy_delegate(
                    &bridge,
                    delegate.clone(),
                    proxy_type,
                    0
                ));
                assert_ok!(Proxy::add_proxy_delegate(
                    &owner,
                    bridge.clone(),
                    ProxyType::Any,
                    0
                ));
                let index = 23;
                let (child, creation) = if pure {
                    (
                        Proxy::pure_account(&owner, &ProxyType::Any, index, None).unwrap(),
                        RuntimeCall::Proxy(pallet_proxy::Call::create_protected_pure {
                            proxy_type: ProxyType::Any,
                            delay: 0,
                            index,
                        }),
                    )
                } else {
                    (
                        Utility::derivative_account_id(owner.clone(), index).unwrap(),
                        RuntimeCall::Utility(pallet_utility::Call::as_protected_derivative {
                            index,
                            call: Box::new(call()),
                        }),
                    )
                };
                for grant in (0..=u8::MAX).filter_map(|i| ProxyType::try_from(i).ok()) {
                    assert_eq!(grant.filter(&creation), grant == ProxyType::Any);
                }
                let balances = [&owner, &delegate, &bridge].map(|account| {
                    (
                        Balances::free_balance(account),
                        Balances::reserved_balance(account),
                    )
                });
                let routes = [
                    (owner.clone(), creation.clone()),
                    (
                        owner.clone(),
                        RuntimeCall::Utility(pallet_utility::Call::batch_all {
                            calls: vec![creation.clone()],
                        }),
                    ),
                    (
                        bridge.clone(),
                        RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                            real: owner.clone().into(),
                            force_proxy_type: Some(ProxyType::Any),
                            call: Box::new(creation.clone()),
                        }),
                    ),
                ];
                for (real, call) in routes {
                    assert_ok!(Proxy::proxy(
                        RuntimeOrigin::signed(delegate.clone()),
                        real.into(),
                        Some(proxy_type),
                        Box::new(call)
                    ));
                    assert_eq!(
                        pallet_proxy::LastCallResult::<Runtime>::get(&owner),
                        Some(Err(frame_system::Error::<Runtime>::CallFiltered.into()))
                    );
                    assert_eq!(
                        [&owner, &delegate, &bridge].map(|account| {
                            (
                                Balances::free_balance(account),
                                Balances::reserved_balance(account),
                            )
                        }),
                        balances
                    );
                    assert!(!DerivedAuthorities::<Runtime>::contains_key(&child));
                    assert!(
                        !pallet_hashed_accounts::EvmAliases::<Runtime>::contains_key(
                            HashedAccounts::evm_alias(&child)
                        )
                    );
                    assert!(Proxy::proxies(child.clone()).0.is_empty());
                    assert_eq!(System::account_nonce(&child), 0);
                }
                // The identical call succeeds with full authority. This proves
                // the failed routes were denied before registration, rather
                // than relying on an invalid fixture or insufficient funds.
                assert_ok!(Proxy::proxy(
                    RuntimeOrigin::signed(bridge),
                    owner.clone().into(),
                    Some(ProxyType::Any),
                    Box::new(creation)
                ));
                assert_eq!(
                    pallet_proxy::LastCallResult::<Runtime>::get(&owner),
                    Some(Ok(()))
                );
                assert!(DerivedAuthorities::<Runtime>::contains_key(&child));
                assert!(
                    Balances::reserved_balance(&owner)
                        >= balances[0]
                            .1
                            .saturating_add(DerivedRegistrationDeposit::get())
                );
                assert!(HashedAccounts::is_protected(&child));
            });
        }
    }
}

#[test]
fn shield_mldsa_outer_and_inner_consume_exactly_the_two_prepared_generations() {
    with_profiles(|| {
        let account = register(51);
        frame_system::BlockHash::<Runtime>::insert(1, H256::repeat_byte(1));
        let mut outer_extra = extra(1);
        outer_extra.0.4 = check_mortality::CheckMortality::from(Era::mortal(8, 1));
        let mut inner_extra = extra(2);
        inner_extra.0.4 = check_mortality::CheckMortality::from(Era::mortal(8, 1));
        let inner = sign_with_extra(51, 1, call(), inner_extra);
        // Runtime admission validates the carrier's framing. Proposer crypto
        // decryption is tested by the Shield crate; the inner bytes stay exact.
        let mut ciphertext = vec![0xab; 16];
        ciphertext.extend_from_slice(&1088u16.to_le_bytes());
        ciphertext.extend_from_slice(&[0xcc; 1088]);
        ciphertext.extend_from_slice(&[0xdd; 24]);
        ciphertext.extend_from_slice(&inner.encode());
        ciphertext.extend_from_slice(&[0xee; 16]);
        let carrier = RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted {
            ciphertext: ciphertext.try_into().unwrap(),
        });
        let immortal = sign(51, carrier.clone());
        assert!(Executive::apply_extrinsic(immortal).is_err());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            0
        );
        // An inner generation cannot execute before its own carrier.
        assert!(Executive::apply_extrinsic(inner.clone()).is_err());
        let outer = sign_with_extra(51, 0, carrier, outer_extra);
        assert!(hashed_extrinsic::shielded_transaction(outer.clone()).is_some());
        // Recognition is deliberately syntactic. A forged proof remains a
        // carrier but cannot pass the outer apply step that precedes unshielding.
        let mut forged = outer.clone();
        let UncheckedExtrinsic::Legacy(ref mut raw) = forged;
        if let Preamble::Signed(Address::Raw(proof), _, _) = &mut raw.0.preamble {
            *proof.last_mut().unwrap() ^= 1;
        } else {
            panic!("expected signed ML-DSA carrier");
        }
        assert!(hashed_extrinsic::shielded_transaction(forged.clone()).is_some());
        assert!(Executive::apply_extrinsic(forged).is_err());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            0
        );
        assert_eq!(System::account_nonce(&account), 1);
        assert!(
            hashed_extrinsic::shielded_transaction(UncheckedExtrinsic::new_bare(
                outer.clone().into_call()
            ))
            .is_none()
        );
        assert_ok!(Executive::apply_extrinsic(outer.clone()).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            1
        );
        assert_ok!(Executive::apply_extrinsic(inner.clone()).unwrap());
        assert_eq!(
            MlDsaAccounts::<Runtime>::get(&account).unwrap().generation,
            2
        );
        assert_eq!(System::account_nonce(account), 3);
        assert!(Executive::apply_extrinsic(outer).is_err());
        assert!(Executive::apply_extrinsic(inner).is_err());
    });
}
