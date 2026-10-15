//! Exercise native EVM calls through the full signed extension pipeline.
use super::*;
use frame_support::{
    assert_ok,
    dispatch::Pays,
    storage::{TransactionOutcome, with_transaction_unchecked},
};
use pallet_evm::{AddressMapping, FeeCalculator, runner::Runner};
use sp_core::{H160, U256, ed25519};

#[derive(Clone, Copy, Debug)]
enum Mode {
    Sr,
    Ed,
    HashedSr,
    HashedEd,
}
const MODES: [Mode; 4] = [Mode::Sr, Mode::Ed, Mode::HashedSr, Mode::HashedEd];
struct Wallet {
    mode: Mode,
    account: AccountId,
}
impl Wallet {
    fn new(mode: Mode) -> Self {
        let account = match mode {
            Mode::Sr => AccountId::from(pair(10).public()),
            Mode::Ed => AccountId::new(ed25519::Pair::from_seed(&[10; 32]).public().0),
            _ => composable_accounts::register(Descriptor {
                version: 1,
                scheme: Self::scheme(mode),
                initial_commitment: Self::commitment(mode, 0),
            }),
        };
        System::set_block_number(1);
        hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
        let _ = Balances::make_free_balance_be(&account, TaoBalance::new(1_000_000_000_000));
        let source = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&account)[..20]);
        let backing = evm_origin::HashedAddressMapping::into_account_id(source);
        let _ = Balances::make_free_balance_be(&backing, TaoBalance::new(1_000_000_000_000));
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);
        Self { mode, account }
    }
    fn scheme(mode: Mode) -> Scheme {
        match mode {
            Mode::Ed | Mode::HashedEd => Scheme::Ed25519,
            _ => Scheme::Sr25519,
        }
    }
    fn public(mode: Mode, generation: u8) -> Vec<u8> {
        let seed = [10 + generation; 32];
        match Self::scheme(mode) {
            Scheme::Sr25519 => sr25519::Pair::from_seed(&seed).public().0.to_vec(),
            Scheme::Ed25519 => ed25519::Pair::from_seed(&seed).public().0.to_vec(),
        }
    }
    fn commitment(mode: Mode, generation: u8) -> [u8; 32] {
        let public = Self::public(mode, generation);
        subtensor_hashed::key_commitment(
            Self::scheme(mode),
            <&[u8; 32]>::try_from(public.as_slice()).unwrap(),
        )
    }

    fn source(&self) -> H160 {
        H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&self.account)[..20])
    }
    fn sign(&self, call: RuntimeCall) -> UncheckedExtrinsic {
        let extra = extra(System::account_nonce(&self.account));
        if matches!(self.mode, Mode::Sr | Mode::Ed) {
            let payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
            let signature = if matches!(self.mode, Mode::Sr) {
                Signature::Sr25519(payload.using_encoded(|m| pair(10).sign(m)))
            } else {
                Signature::Ed25519(
                    payload.using_encoded(|m| ed25519::Pair::from_seed(&[10; 32]).sign(m)),
                )
            };
            return UncheckedExtrinsic::new_signed(
                call,
                self.account.clone().into(),
                signature,
                extra,
            );
        }
        let generation = HashedAccounts::accounts(&self.account).unwrap().generation;
        let key_generation = u8::try_from(generation).unwrap();
        let next_commitment = Self::commitment(self.mode, key_generation + 1);
        let scheme = Self::scheme(self.mode);
        let address = subtensor_hashed::SignerAddress {
            version: 1,
            account: *self.account.as_ref(),
            scheme,
            generation,
            next_commitment,
            public_key: Self::public(self.mode, key_generation).try_into().unwrap(),
        };
        let payload = hashed_extrinsic::signing_hash(&address, &call, &extra).unwrap();
        let seed = [10 + key_generation; 32];
        let signature = match scheme {
            Scheme::Sr25519 => Signature::Sr25519(sr25519::Pair::from_seed(&seed).sign(&payload)),
            Scheme::Ed25519 => Signature::Ed25519(ed25519::Pair::from_seed(&seed).sign(&payload)),
        };
        UncheckedExtrinsic::new_signed(call, Address::Raw(address.encode()), signature, extra)
    }
}

fn gas_price() -> U256 {
    <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0
}
fn evm_call(wallet: &Wallet, nonce: Option<U256>) -> RuntimeCall {
    RuntimeCall::EVM(pallet_evm::Call::call {
        source: wallet.source(),
        target: H160::repeat_byte(0x42),
        input: vec![],
        value: U256::zero(),
        gas_limit: 100_000,
        max_fee_per_gas: gas_price(),
        max_priority_fee_per_gas: None,
        nonce,
        access_list: vec![],
        authorization_list: vec![],
    })
}
fn simulate_call(wallet: &Wallet, nonce: Option<U256>) {
    let info = with_transaction_unchecked(|| {
        TransactionOutcome::Rollback(hashed_evm_runner::HashedEvmRunner::call(
            wallet.source(),
            H160::repeat_byte(0x42),
            vec![],
            U256::zero(),
            100_000,
            Some(gas_price()),
            None,
            nonce,
            vec![],
            vec![],
            false,
            true,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        ))
    })
    .unwrap();
    assert!(info.exit_reason.is_succeed());
}

#[test]
fn queried_nonce_executes_and_advances_independently_for_every_wallet_mode() {
    for mode in MODES {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            let native = System::account_nonce(&wallet.account);
            let nonce = EVM::account_basic(&wallet.source()).0.nonce;
            simulate_call(&wallet, Some(nonce));
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(evm_call(&wallet, Some(nonce)))).unwrap()
            );
            assert_eq!(
                System::account_nonce(&wallet.account),
                native + 1,
                "{mode:?}"
            );
            assert_eq!(
                EVM::account_basic(&wallet.source()).0.nonce,
                nonce + U256::one(),
                "{mode:?}"
            );
        });
    }
}

#[test]
fn create_prediction_matches_dispatch_with_and_without_explicit_nonce() {
    for mode in MODES {
        for explicit in [false, true] {
            ext().execute_with(|| {
                let wallet = Wallet::new(mode);
                let nonce = explicit.then(|| EVM::account_basic(&wallet.source()).0.nonce);
                let init = hex::decode("6001600c60003960016000f300").unwrap();
                let simulated = with_transaction_unchecked(|| {
                    TransactionOutcome::Rollback(hashed_evm_runner::HashedEvmRunner::create(
                        wallet.source(),
                        init.clone(),
                        U256::zero(),
                        200_000,
                        Some(gas_price()),
                        None,
                        nonce,
                        vec![],
                        vec![],
                        true,
                        vec![],
                        false,
                        true,
                        None,
                        None,
                        <Runtime as pallet_evm::Config>::config(),
                    ))
                })
                .unwrap();
                assert!(simulated.exit_reason.is_succeed());
                let call = RuntimeCall::EVM(pallet_evm::Call::create {
                    source: wallet.source(),
                    init,
                    value: U256::zero(),
                    gas_limit: 200_000,
                    max_fee_per_gas: gas_price(),
                    max_priority_fee_per_gas: None,
                    nonce,
                    access_list: vec![],
                    authorization_list: vec![],
                });
                assert_ok!(Executive::apply_extrinsic(wallet.sign(call)).unwrap());
                let created = System::events()
                    .iter()
                    .rev()
                    .find_map(|event| match event.event {
                        RuntimeEvent::EVM(pallet_evm::Event::Created { address }) => Some(address),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    created, simulated.value,
                    "{mode:?}, explicit nonce={explicit}"
                );
            });
        }
    }
}

#[test]
fn authorization_fee_is_not_refunded_when_gas_pays_for_execution() {
    for mode in MODES {
        let mut fees = Vec::new();
        let mut charges = Vec::new();
        for factor in [1, 100] {
            ext().execute_with(|| {
                let wallet = Wallet::new(mode);
                hashed_auth::TestVerificationWeight::set(Weight::from_parts(
                    10_000_000 * factor,
                    0,
                ));
                let xt = wallet.sign(evm_call(&wallet, None));
                let mut envelope = xt.get_dispatch_info();
                envelope.call_weight = Weight::zero();
                let expected = TransactionPayment::compute_fee(
                    u32::try_from(xt.encoded_size()).unwrap(),
                    &envelope,
                    TaoBalance::ZERO,
                );
                // Exercise the wire decoder as well as the local constructor.
                let decoded = UncheckedExtrinsic::decode(&mut xt.encode().as_slice()).unwrap();
                assert_eq!(decoded.encode(), xt.encode());
                let before = Balances::free_balance(&wallet.account);
                assert_ok!(Executive::apply_extrinsic(decoded).unwrap());
                charges.push(before - Balances::free_balance(&wallet.account));
                let fee = System::events()
                    .iter()
                    .rev()
                    .find_map(|event| match event.event {
                        RuntimeEvent::TransactionPayment(
                            pallet_transaction_payment::Event::TransactionFeePaid {
                                actual_fee,
                                ..
                            },
                        ) => Some(actual_fee),
                        _ => None,
                    })
                    .unwrap();
                if !matches!(mode, Mode::Sr | Mode::Ed) {
                    assert_eq!(fee, expected, "{mode:?}");
                }
                fees.push(fee);
            });
        }
        if matches!(mode, Mode::Sr | Mode::Ed) {
            assert_eq!(fees, vec![TaoBalance::ZERO; 2]);
        } else {
            assert!(
                fees[1] > fees[0] && fees[0] > TaoBalance::ZERO,
                "{mode:?}: authorization fees {fees:?}"
            );
            // Gas is unchanged: increasing authorization work must increase
            // the real balance debit by precisely the added native fee.
            assert!(charges[0] > fees[0]);
            assert_eq!(charges[1] - charges[0], fees[1] - fees[0], "{mode:?}");
        }
    }
}

#[test]
fn reverted_and_invalid_evm_calls_keep_authorization_fees_and_reject_replays() {
    for mode in MODES {
        for invalid in [false, true] {
            ext().execute_with(|| {
                let wallet = Wallet::new(mode);
                let nonce = EVM::account_basic(&wallet.source()).0.nonce;
                // REVERT(0, 0): a completed EVM execution still pays gas.
                pallet_evm::AccountCodes::<Runtime>::insert(
                    H160::repeat_byte(0x42),
                    hex::decode("60006000fd").unwrap(),
                );
                let xt = wallet.sign(evm_call(
                    &wallet,
                    Some(nonce + U256::from(<u8 as From<bool>>::from(invalid))),
                ));
                let replay = xt.clone();
                let native = System::account_nonce(&wallet.account);
                let generation = HashedAccounts::accounts(&wallet.account).map(|a| a.generation);
                let result = Executive::apply_extrinsic(xt).unwrap();
                if invalid {
                    assert_eq!(
                        result,
                        Err(pallet_evm::Error::<Runtime>::InvalidNonce.into())
                    );
                } else {
                    assert_ok!(result);
                    assert!(System::events().iter().any(|e| matches!(
                        e.event,
                        RuntimeEvent::EVM(pallet_evm::Event::ExecutedFailed { .. })
                    )));
                }
                assert_eq!(System::account_nonce(&wallet.account), native + 1);
                assert_eq!(
                    EVM::account_basic(&wallet.source()).0.nonce,
                    nonce + U256::from(<u8 as From<bool>>::from(!invalid))
                );
                if let Some(generation) = generation {
                    assert_eq!(
                        HashedAccounts::accounts(&wallet.account)
                            .unwrap()
                            .generation,
                        generation + 1
                    );
                }
                let paid = System::events()
                    .iter()
                    .rev()
                    .find_map(|e| match e.event {
                        RuntimeEvent::TransactionPayment(
                            pallet_transaction_payment::Event::TransactionFeePaid {
                                actual_fee,
                                ..
                            },
                        ) => Some(actual_fee),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    paid > TaoBalance::ZERO,
                    invalid || !matches!(mode, Mode::Sr | Mode::Ed)
                );
                assert!(Executive::apply_extrinsic(replay).is_err());
            });
        }
    }
}

#[test]
fn alias_nonce_survives_balance_reaping_and_native_transactions_when_disabled() {
    for mode in [Mode::HashedSr, Mode::HashedEd] {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(evm_call(&wallet, Some(U256::zero()))))
                    .unwrap()
            );
            let backing =
                evm_origin::HashedAddressMapping::nonce_account_id(wallet.source()).unwrap();
            assert_ne!(backing, wallet.account);
            let _ = Balances::make_free_balance_be(&backing, TaoBalance::new(1_000_000_000));
            let _ = Balances::make_free_balance_be(&backing, TaoBalance::ZERO);
            assert_eq!(System::account_nonce(&backing), 1);
            assert_eq!(System::account(&backing).sufficients, 1);
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(false);
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::one());
            simulate_call(&wallet, Some(U256::one()));
            assert!(Executive::apply_extrinsic(wallet.sign(call())).is_err());
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
            assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::one());
            simulate_call(&wallet, Some(U256::one()));
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(evm_call(&wallet, Some(U256::one()))))
                    .unwrap()
            );
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::from(2));
        });
    }
}

#[test]
fn create2_prediction_and_envelope_fee_match_dispatch() {
    for mode in MODES {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            let nonce = Some(EVM::account_basic(&wallet.source()).0.nonce);
            let init = hex::decode("6001600c60003960016000f300").unwrap();
            let salt = H256::repeat_byte(0x17);
            let simulated = with_transaction_unchecked(|| {
                TransactionOutcome::Rollback(hashed_evm_runner::HashedEvmRunner::create2(
                    wallet.source(),
                    init.clone(),
                    salt,
                    U256::zero(),
                    200_000,
                    Some(gas_price()),
                    None,
                    nonce,
                    vec![],
                    vec![],
                    true,
                    vec![],
                    false,
                    true,
                    None,
                    None,
                    <Runtime as pallet_evm::Config>::config(),
                ))
            })
            .unwrap();
            assert!(simulated.exit_reason.is_succeed());
            let call = RuntimeCall::EVM(pallet_evm::Call::create2 {
                source: wallet.source(),
                init,
                salt,
                value: U256::zero(),
                gas_limit: 200_000,
                max_fee_per_gas: gas_price(),
                max_priority_fee_per_gas: None,
                nonce,
                access_list: vec![],
                authorization_list: vec![],
            });
            let xt = wallet.sign(call);
            let mut envelope = xt.get_dispatch_info();
            envelope.call_weight = Weight::zero();
            envelope.pays_fee = Pays::Yes;
            let expected = TransactionPayment::compute_fee(
                u32::try_from(xt.encoded_size()).unwrap(),
                &envelope,
                TaoBalance::ZERO,
            );
            assert_ok!(Executive::apply_extrinsic(xt).unwrap());
            let events = System::events();
            let created = events
                .iter()
                .rev()
                .find_map(|event| match event.event {
                    RuntimeEvent::EVM(pallet_evm::Event::Created { address }) => Some(address),
                    _ => None,
                })
                .unwrap();
            assert_eq!(created, simulated.value, "{mode:?}");
            let fee = events
                .iter()
                .rev()
                .find_map(|event| match event.event {
                    RuntimeEvent::TransactionPayment(
                        pallet_transaction_payment::Event::TransactionFeePaid {
                            actual_fee, ..
                        },
                    ) => Some(actual_fee),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                fee,
                if matches!(mode, Mode::Sr | Mode::Ed) {
                    TaoBalance::ZERO
                } else {
                    expected
                }
            );
        });
    }
}

#[test]
fn every_wallet_mode_executes_mixed_multisig_and_proxy_transfers() {
    for mode in MODES {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            let other = pair(24);
            let other_id = AccountId::from(other.public());
            let _ = Balances::make_free_balance_be(&other_id, TaoBalance::new(100_000_000_000));
            let mut members = vec![wallet.account.clone(), other_id.clone()];
            members.sort();
            let multisig = Multisig::multi_account_id(&members, 2);
            let _ = Balances::make_free_balance_be(&multisig, TaoBalance::new(10_000_000_000));
            let dest = AccountId::new([97; 32]);
            let transfer = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                dest: dest.clone().into(),
                value: TaoBalance::new(1_000_000_000),
            });
            let hash = sp_io::hashing::blake2_256(&transfer.encode());
            let weight = transfer.get_dispatch_info().total_weight();
            let approval = RuntimeCall::Multisig(pallet_multisig::Call::approve_as_multi {
                threshold: 2,
                other_signatories: vec![other_id],
                maybe_timepoint: None,
                call_hash: hash,
                max_weight: weight,
            });
            assert_ok!(Executive::apply_extrinsic(wallet.sign(approval)).unwrap());
            let pending = pallet_multisig::Multisigs::<Runtime>::get(&multisig, hash).unwrap();
            assert_eq!(pending.depositor, wallet.account);
            let execute = RuntimeCall::Multisig(pallet_multisig::Call::as_multi {
                threshold: 2,
                other_signatories: vec![wallet.account.clone()],
                maybe_timepoint: Some(pending.when),
                call: Box::new(transfer.clone()),
                max_weight: weight,
            });
            assert_ok!(Executive::apply_extrinsic(legacy(&other, execute)).unwrap());
            assert_eq!(
                Balances::free_balance(&dest),
                TaoBalance::new(1_000_000_000)
            );
            assert!(pallet_multisig::Multisigs::<Runtime>::get(&multisig, hash).is_none());

            let delegate = Wallet::new(if matches!(mode, Mode::HashedEd) {
                Mode::HashedSr
            } else {
                Mode::HashedEd
            });
            let grant = RuntimeCall::Proxy(pallet_proxy::Call::add_proxy {
                delegate: delegate.account.clone().into(),
                proxy_type: ProxyType::Any,
                delay: 0,
            });
            assert_ok!(Executive::apply_extrinsic(wallet.sign(grant)).unwrap());
            let proxy = RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                real: wallet.account.clone().into(),
                force_proxy_type: Some(ProxyType::Any),
                call: Box::new(transfer),
            });
            assert_ok!(Executive::apply_extrinsic(delegate.sign(proxy)).unwrap());
            assert_eq!(
                Balances::free_balance(&dest),
                TaoBalance::new(2_000_000_000)
            );
        });
    }
}

#[test]
fn shield_recognizes_every_wallet_mode_without_rewriting_or_rotating_it() {
    for mode in MODES {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            let mut wire = vec![0xAB; 16];
            wire.extend_from_slice(&1088u16.to_le_bytes());
            wire.extend_from_slice(&[0xCC; 1088]);
            wire.extend_from_slice(&[0xDD; 24]);
            wire.extend_from_slice(&[0xEE; 64]);
            let call = RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted {
                ciphertext: wire.try_into().unwrap(),
            });
            let xt = wallet.sign(call);
            let original = xt.encode();
            let state = HashedAccounts::accounts(&wallet.account);
            let parsed = hashed_extrinsic::shielded_transaction(xt.clone()).unwrap();
            assert_eq!(parsed.key_hash, [0xAB; 16]);
            assert_eq!(parsed.kem_ct, vec![0xCC; 1088]);
            assert_eq!(parsed.nonce, [0xDD; 24]);
            assert_eq!(parsed.aead_ct, vec![0xEE; 64]);
            assert_eq!(HashedAccounts::accounts(&wallet.account), state);
            assert_eq!(xt.encode(), original);
            assert!(hashed_extrinsic::shielded_transaction(wallet.sign(super::call())).is_none());
            let mut invalid = xt;
            let UncheckedExtrinsic::Legacy(ref mut tx) = invalid;
            let sp_runtime::generic::Preamble::Signed(_, signature, _) = &mut tx.0.preamble else {
                unreachable!()
            };
            match signature {
                Signature::Sr25519(sig) => sig.0[0] ^= 1,
                Signature::Ed25519(sig) => sig.0[0] ^= 1,
                _ => unreachable!(),
            }
            assert!(hashed_extrinsic::shielded_transaction(invalid).is_none());
        });
    }
}

#[test]
fn staking_fee_quotes_and_charges_preserve_account_wrapper_costs() {
    use crate::transaction_payment_wrapper::{
        ChargeTransactionPaymentWrapper, FeeWeightDiscount, fee_dispatch_info,
    };
    use sp_runtime::traits::DispatchTransaction;
    for mode in [Mode::HashedSr, Mode::HashedEd] {
        ext().execute_with(|| {
            let wallet = Wallet::new(mode);
            let inner = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
                hotkey: AccountId::new([98; 32]),
                netuid: 1.into(),
                amount_unstaked: subtensor_runtime_common::AlphaBalance::new(1_000_000),
            });
            let tx = wallet.sign(inner.clone());
            let info = tx.get_dispatch_info();
            let len = u32::try_from(tx.encoded_size()).unwrap();
            let discount = Runtime::fee_weight_discount(&inner, &inner.get_dispatch_info());
            assert!(discount.ref_time() > 0);
            let expected = TransactionPayment::compute_fee(
                len,
                &fee_dispatch_info(&info, discount),
                TaoBalance::ZERO,
            );
            let quote = staking_fee::query_info(
                sp_runtime::traits::ExtrinsicCall::call(&tx),
                &info,
                len,
                false,
            );
            assert_eq!(quote.partial_fee, expected);
            let before = Balances::free_balance(&wallet.account);
            let result = ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::ZERO)
                .with_account_authorization()
                .test_run(
                    RuntimeOrigin::signed(wallet.account.clone()),
                    &inner,
                    &info,
                    len as usize,
                    0,
                    |_| Err(sp_runtime::DispatchError::Other("failed inner call").into()),
                );
            assert!(result.unwrap().is_err());
            assert_eq!(before - Balances::free_balance(&wallet.account), expected);
        });
    }
}
