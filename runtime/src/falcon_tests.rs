//! Falcon uses the same Executive, nonce, dispatch and composite paths as MS.
use super::*;
use frame_support::assert_ok;
use pallet_hashed_accounts::{DerivedAuthorities, EvmAliases, MlDsaAccounts};
use sp_runtime::traits::TransactionExtension;
use subtensor_hashed::{falcon, mldsa};

#[derive(Clone)]
pub(super) struct Wallet {
    pub(super) seed: [u8; 32],
    pub(super) descriptor: mldsa::Descriptor,
    pub(super) account: AccountId,
}
impl Wallet {
    pub(super) fn new(identity: u8, profile: u8) -> Self {
        for attempt in 0u32..256 {
            let mut seed = [identity; 32];
            seed[..4].copy_from_slice(&attempt.to_le_bytes());
            let public = falcon::public_key_from_seed(profile, &seed).unwrap();
            let descriptor = mldsa::Descriptor {
                version: falcon::VERSION,
                algorithm: profile,
                initial_commitment: falcon::commitment(profile, &public).unwrap(),
            };
            let account = mldsa::account_id(&descriptor);
            if !falcon::is_fixed_profile(profile) || mldsa::rejects_legacy_public_keys(&account) {
                return Self {
                    seed,
                    descriptor,
                    account: AccountId::new(account),
                };
            }
        }
        panic!("safe Falcon fixture exhausted")
    }
    fn generation_seed(&self, generation: u64) -> [u8; 32] {
        if generation == 0 {
            return self.seed;
        }
        let mut seed = self.seed;
        seed[8..16].copy_from_slice(&generation.to_le_bytes());
        seed
    }
    pub(super) fn prepare(&self) {
        setup();
        if self.descriptor.is_rotating() {
            let sponsor = AccountId::from(pair(70).public());
            fund(&sponsor);
            assert_ok!(HashedAccounts::register(
                RuntimeOrigin::signed(sponsor),
                self.descriptor.into()
            ));
        }
        fund(&self.account);
    }
    pub(super) fn sign(&self, call: RuntimeCall) -> UncheckedExtrinsic {
        self.sign_nonce(call, System::account_nonce(&self.account))
    }
    fn sign_nonce(&self, call: RuntimeCall, nonce: u32) -> UncheckedExtrinsic {
        let generation =
            MlDsaAccounts::<Runtime>::get(&self.account).map_or(0, |record| record.generation);
        self.sign_with_extra(call, extra(nonce), generation)
    }
    fn sign_with_extra(
        &self,
        call: RuntimeCall,
        extra: TxExtension,
        generation: u64,
    ) -> UncheckedExtrinsic {
        let profile = self.descriptor.algorithm;
        let seed = self.generation_seed(generation);
        let next_commitment = if self.descriptor.is_stateless() {
            self.descriptor.initial_commitment
        } else {
            falcon::commitment(
                profile,
                &falcon::public_key_from_seed(profile, &self.generation_seed(generation + 1))
                    .unwrap(),
            )
            .unwrap()
        };
        let header = falcon::Header {
            account: *self.account.as_ref(),
            version: falcon::VERSION,
            algorithm: profile,
            generation,
            public_key: falcon::public_key_from_seed(profile, &seed).unwrap(),
            next_commitment,
        };
        let payload = (&call, &extra, extra.implicit().unwrap()).encode();
        let proof = header.sign(&seed, &payload, &[1; 32]).unwrap();
        UncheckedExtrinsic::new_signed(
            call,
            Address::Raw(proof.encode()),
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
    let _ = Balances::make_free_balance_be(account, TaoBalance::new(1_000_000_000_000_000));
}
fn transfer(destination: &AccountId, amount: u64) -> RuntimeCall {
    RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
        dest: destination.clone().into(),
        value: TaoBalance::new(amount),
    })
}
fn assert_registrationless(account: &AccountId) {
    assert!(!MlDsaAccounts::<Runtime>::contains_key(account));
    assert!(!pallet_hashed_accounts::Accounts::<Runtime>::contains_key(
        account
    ));
    assert!(!DerivedAuthorities::<Runtime>::contains_key(account));
    assert!(!EvmAliases::<Runtime>::contains_key(
        HashedAccounts::evm_alias(account)
    ));
    assert_eq!(Balances::reserved_balance(account), TaoBalance::new(0));
}

#[test]
fn every_falcon_profile_roundtrips_legacy_v4_and_executes_transfers_once() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(151, profile);
            wallet.prepare();
            let destination = AccountId::new([152; 32]);
            let tx = wallet.sign(transfer(&destination, 1_000_000_000));
            let wire = tx.encode();
            assert_eq!(
                LegacyUnchecked::decode(&mut wire.as_slice())
                    .unwrap()
                    .encode(),
                wire
            );
            assert_eq!(
                UncheckedExtrinsic::decode(&mut wire.as_slice()).unwrap(),
                tx
            );
            assert_ok!(Executive::apply_extrinsic(tx.clone()).unwrap());
            assert_eq!(
                Balances::free_balance(&destination),
                TaoBalance::new(1_000_000_000)
            );
            let before = System::account_nonce(&wallet.account);
            assert!(Executive::apply_extrinsic(tx).is_err());
            assert_eq!(System::account_nonce(&wallet.account), before);
            assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
            if wallet.descriptor.is_rotating() {
                assert_eq!(
                    MlDsaAccounts::<Runtime>::get(&wallet.account)
                        .unwrap()
                        .generation,
                    2
                );
            } else {
                assert_registrationless(&wallet.account);
            }
        });
    }
}

#[test]
fn falcon_pause_and_unmeasured_weight_gate_reject_before_dispatch() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(153, profile);
            wallet.prepare();
            let tx = wallet.sign(call());
            let before = System::account_nonce(&wallet.account);
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(false);
            assert!(Executive::apply_extrinsic(tx.clone()).is_err());
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
            hashed_auth::TestVerificationWeight::set(Weight::MAX);
            assert!(Executive::apply_extrinsic(tx.clone()).is_err());
            assert_eq!(System::account_nonce(&wallet.account), before);
            hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
            assert_ok!(Executive::apply_extrinsic(tx).unwrap());
        });
    }
}

#[test]
fn falcon_rejects_other_genesis_nonce_and_call_without_consuming_authority() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(155, profile);
            wallet.prepare();
            let tx = wallet.sign(call());
            let before = frame_system::Account::<Runtime>::get(&wallet.account);
            let authority = MlDsaAccounts::<Runtime>::get(&wallet.account);

            frame_system::BlockHash::<Runtime>::insert(0, H256::repeat_byte(0x42));
            assert!(Executive::apply_extrinsic(tx.clone()).is_err());
            frame_system::BlockHash::<Runtime>::insert(0, H256::zero());

            let mut wrong_nonce = tx.clone();
            let UncheckedExtrinsic::Legacy(legacy) = &mut wrong_nonce;
            let sp_runtime::generic::Preamble::Signed(_, _, extension) = &mut legacy.0.preamble
            else {
                panic!("Falcon fixture must be signed");
            };
            extension.0.0.5 = check_nonce::CheckNonce::from(before.nonce + 1);
            assert!(Executive::apply_extrinsic(wrong_nonce).is_err());

            let mut wrong_call = tx.clone();
            let UncheckedExtrinsic::Legacy(legacy) = &mut wrong_call;
            legacy.0.function = transfer(&AccountId::new([156; 32]), 1_000_000_000);
            assert!(Executive::apply_extrinsic(wrong_call).is_err());

            assert_eq!(
                frame_system::Account::<Runtime>::get(&wallet.account),
                before
            );
            assert_eq!(MlDsaAccounts::<Runtime>::get(&wallet.account), authority);
            assert_ok!(Executive::apply_extrinsic(tx).unwrap());
        });
    }
}

#[test]
fn falcon_included_failure_consumes_nonce_and_only_rotating_authority_advances() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(157, profile);
            wallet.prepare();
            let destination = AccountId::new([158; 32]);
            let nonce = System::account_nonce(&wallet.account);
            let tx = wallet.sign(transfer(&destination, u64::MAX));
            assert!(Executive::apply_extrinsic(tx.clone()).unwrap().is_err());
            assert_eq!(Balances::free_balance(&destination), TaoBalance::new(0));
            assert_eq!(System::account_nonce(&wallet.account), nonce + 1);
            if wallet.descriptor.is_rotating() {
                assert_eq!(
                    MlDsaAccounts::<Runtime>::get(&wallet.account)
                        .unwrap()
                        .generation,
                    1
                );
            } else {
                assert_registrationless(&wallet.account);
            }
            assert!(Executive::apply_extrinsic(tx).is_err());
            assert_eq!(System::account_nonce(&wallet.account), nonce + 1);
            assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
            assert_eq!(System::account_nonce(&wallet.account), nonce + 2);
        });
    }
}

#[test]
fn falcon_malformed_envelopes_cannot_charge_fees_or_consume_authority() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(159, profile);
            wallet.prepare();
            let tx = wallet.sign(call());
            let before = frame_system::Account::<Runtime>::get(&wallet.account);
            let authority = MlDsaAccounts::<Runtime>::get(&wallet.account);
            for mutation in 0..6 {
                let mut bad = tx.clone();
                let UncheckedExtrinsic::Legacy(legacy) = &mut bad;
                let sp_runtime::generic::Preamble::Signed(Address::Raw(bytes), signature, _) =
                    &mut legacy.0.preamble
                else {
                    panic!("Falcon fixture must carry a raw proof");
                };
                match mutation {
                    0 => {
                        bytes.pop();
                    }
                    1 => bytes.push(0),
                    2 => *bytes.last_mut().unwrap() ^= 1,
                    3 => {
                        *signature =
                            Signature::Ed25519(sp_core::ed25519::Signature::from_raw([1; 64]));
                    }
                    4 => {
                        *signature =
                            Signature::Sr25519(sp_core::sr25519::Signature::from_raw([0; 64]));
                    }
                    _ => bytes[33] = u8::MAX,
                }
                assert!(Executive::apply_extrinsic(bad).is_err());
                assert_eq!(
                    frame_system::Account::<Runtime>::get(&wallet.account),
                    before
                );
                assert_eq!(MlDsaAccounts::<Runtime>::get(&wallet.account), authority);
            }
            assert_ok!(Executive::apply_extrinsic(tx).unwrap());
        });
    }
}

#[test]
fn draining_and_refunding_cannot_reset_consumed_native_nonces() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let wallet = Wallet::new(85, profile);
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
}

#[test]
fn consecutive_fixed_transactions_have_independent_pool_nonce_tags() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let wallet = Wallet::new(104, profile);
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
}

#[test]
fn ordinary_classical_proxy_can_be_granted_and_execute() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let wallet = Wallet::new(90, profile);
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
}

#[test]
fn safe_falcon_ms_multisig_requires_the_quorum_without_registration() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let first = Wallet::new(121, profile);
            let second = super::mldsa::stateless::Wallet::new(122);
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
}

#[test]
fn safe_falcon_derivative_and_pure_proxy_use_full_width_authority() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let wallet = Wallet::new(124, profile);
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
}

#[test]
fn native_evm_keeps_ordinary_separate_backing_balance_and_nonce() {
    use pallet_evm::{AddressMapping, FeeCalculator};
    use sp_core::{H160, U256};
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        ext().execute_with(|| {
            setup();
            let wallet = Wallet::new(98, profile);
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
                Executive::apply_extrinsic(wallet.sign(transfer(&backing, 10_000_000_000)))
                    .unwrap()
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
                    max_fee_per_gas:
                        <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
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
}

#[test]
fn falcon_shield_carrier_and_inner_preserve_nonce_and_rotation_order() {
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(154, profile);
            wallet.prepare();
            frame_system::BlockHash::<Runtime>::insert(1, H256::repeat_byte(1));
            let nonce = System::account_nonce(&wallet.account);
            let mut outer_extra = extra(nonce);
            outer_extra.0.4 = check_mortality::CheckMortality::from(Era::mortal(8, 1));
            let mut inner_extra = extra(nonce + 1);
            inner_extra.0.4 = check_mortality::CheckMortality::from(Era::mortal(8, 1));
            let inner = wallet.sign_with_extra(
                call(),
                inner_extra,
                <u64 as From<bool>>::from(wallet.descriptor.is_rotating()),
            );
            let mut ciphertext = vec![0xab; 16];
            ciphertext.extend_from_slice(&1088u16.to_le_bytes());
            ciphertext.extend_from_slice(&[0xcc; 1088]);
            ciphertext.extend_from_slice(&[0xdd; 24]);
            ciphertext.extend_from_slice(&inner.encode());
            ciphertext.extend_from_slice(&[0xee; 16]);
            let carrier = RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted {
                ciphertext: ciphertext.try_into().unwrap(),
            });
            let outer = wallet.sign_with_extra(carrier, outer_extra, 0);
            assert!(hashed_extrinsic::shielded_transaction(outer.clone()).is_some());
            assert!(Executive::apply_extrinsic(inner.clone()).is_err());
            assert_ok!(Executive::apply_extrinsic(outer.clone()).unwrap());
            assert_ok!(Executive::apply_extrinsic(inner.clone()).unwrap());
            assert_eq!(System::account_nonce(&wallet.account), nonce + 2);
            assert!(Executive::apply_extrinsic(outer).is_err());
            assert!(Executive::apply_extrinsic(inner).is_err());
            if wallet.descriptor.is_rotating() {
                assert_eq!(
                    MlDsaAccounts::<Runtime>::get(&wallet.account)
                        .unwrap()
                        .generation,
                    2
                );
            } else {
                assert_registrationless(&wallet.account);
            }
        });
    }
}

#[test]
fn falcon_order_capability_preserves_scope_epoch_budget_and_revocation() {
    use pallet_hashed_accounts::{
        ActiveCapabilities, CapabilityEpochs, CapabilityRole, CapabilityScope, TradingScope,
    };
    use pallet_limit_orders::{Order, OrderCapabilities, OrderType, VersionedOrder};
    for profile in [6, 7, 8, 9] {
        ext().execute_with(|| {
            let wallet = Wallet::new(151, profile);
            wallet.prepare();
            let initial_nonce = System::account_nonce(&wallet.account);
            let algorithm = if profile < 8 {
                falcon::ALGORITHM_512
            } else {
                falcon::ALGORITHM_1024
            };
            let public = falcon::public_key_from_seed(algorithm, &[152; 32]).unwrap();
            let recipient = AccountId::from(pair(153).public());
            let order = VersionedOrder::V1(Order {
                signer: wallet.account.clone(),
                hotkey: wallet.account.clone(),
                netuid: NetUid::from(42u16),
                order_type: OrderType::LimitBuy,
                amount: 100,
                limit_price: u64::MAX,
                expiry: u64::MAX,
                fee_rate: sp_runtime::Perbill::zero(),
                fee_recipient: recipient.clone(),
                relayer: None,
                max_slippage: None,
                chain_id: ConfigurableChainId::get(),
                partial_fills_enabled: false,
            });
            let view = order.view();
            let check = |epoch, view: &pallet_limit_orders::OrderView<AccountId>| {
                MlDsaOrderCapabilities::check_falcon(
                    &wallet.account,
                    epoch,
                    algorithm,
                    &public,
                    view,
                    100,
                    100,
                )
            };
            assert!(check(1, &view).is_err());
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(RuntimeCall::HashedAccounts(
                    pallet_hashed_accounts::Call::install_capability {
                        expected_epoch: 1,
                        key_commitment: falcon::commitment(algorithm, &public).unwrap(),
                        expires_at: 100,
                        scope: CapabilityScope::Trading(TradingScope {
                            netuid: 42,
                            hotkey: wallet.account.clone(),
                            fee_recipient: recipient,
                            max_fee_perbill: 0,
                            max_tao_per_order: 100,
                            max_alpha_per_order: 0,
                            total_tao: 150,
                            total_alpha: 0,
                        }),
                    },
                )))
                .unwrap()
            );
            assert_ok!(check(1, &view));
            assert!(check(2, &view).is_err());
            let mut wrong_subnet = view.clone();
            wrong_subnet.netuid = NetUid::from(43u16);
            assert!(check(1, &wrong_subnet).is_err());
            assert!(
                MlDsaOrderCapabilities::check_falcon(
                    &wallet.account,
                    1,
                    algorithm + 1,
                    &public,
                    &view,
                    100,
                    100,
                )
                .is_err()
            );
            assert_ok!(MlDsaOrderCapabilities::consume(
                &wallet.account,
                1,
                true,
                100
            ));
            assert!(check(1, &view).is_err());
            assert_eq!(
                ActiveCapabilities::<Runtime>::get(&wallet.account, CapabilityRole::Order)
                    .unwrap()
                    .spent_tao,
                100
            );
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(RuntimeCall::HashedAccounts(
                    pallet_hashed_accounts::Call::revoke_capability {
                        role: CapabilityRole::Order,
                    },
                )))
                .unwrap()
            );
            assert!(check(1, &view).is_err());
            assert_eq!(
                CapabilityEpochs::<Runtime>::get(&wallet.account, CapabilityRole::Order),
                2
            );
            assert_eq!(System::account_nonce(&wallet.account), initial_nonce + 2);
            if wallet.descriptor.is_stateless() {
                assert_registrationless(&wallet.account);
            }
        });
    }
}
