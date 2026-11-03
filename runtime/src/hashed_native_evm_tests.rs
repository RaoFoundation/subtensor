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
    MlDsa,
    FixedMlDsa,
    Falcon(u8),
}
const MODES: [Mode; 5] = [
    Mode::Sr,
    Mode::Ed,
    Mode::HashedSr,
    Mode::HashedEd,
    Mode::MlDsa,
];
struct Wallet {
    mode: Mode,
    account: AccountId,
}
impl Wallet {
    fn new(mode: Mode) -> Self {
        let account = match mode {
            Mode::MlDsa => super::mldsa::register(62),
            Mode::FixedMlDsa => super::mldsa::stateless::Wallet::new(62).account,
            Mode::Falcon(profile) => {
                let wallet = super::falcon::Wallet::new(62, profile);
                wallet.prepare();
                wallet.account
            }
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
        if matches!(self.mode, Mode::MlDsa) {
            return super::mldsa::sign(62, call);
        }
        if matches!(self.mode, Mode::FixedMlDsa) {
            return super::mldsa::stateless::Wallet::new(62).sign(call);
        }
        if let Mode::Falcon(profile) = self.mode {
            return super::falcon::Wallet::new(62, profile).sign(call);
        }
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

fn association_call(
    account: &AccountId,
    evm_pair: &sp_core::ecdsa::Pair,
    netuid: subtensor_runtime_common::NetUid,
    block_number: u64,
) -> (H160, RuntimeCall) {
    let message = [
        account.encode(),
        sp_core::keccak_256(&block_number.encode()).to_vec(),
    ]
    .concat();
    let digest = sp_core::keccak_256(
        &[
            b"\x19Ethereum Signed Message:\n64".as_slice(),
            message.as_slice(),
        ]
        .concat(),
    );
    let mut signature = evm_pair.sign_prehashed(&digest);
    let public = sp_io::crypto::secp256k1_ecdsa_recover(&signature.0, &digest)
        .unwrap_or_else(|_| panic!("valid ECDSA association proof must recover"));
    let evm_key = H160::from_slice(&sp_core::keccak_256(&public)[12..]);
    signature.0[64] += 27;
    (
        evm_key,
        RuntimeCall::SubtensorModule(pallet_subtensor::Call::associate_evm_key {
            netuid,
            evm_key,
            block_number,
            signature,
        }),
    )
}

#[test]
#[cfg(not(feature = "fast-runtime"))]
fn neuron_evm_association_honors_production_cooldown_for_every_wallet_mode() {
    use pallet_subtensor::{AssociatedEvmAddress, AssociatedUidsByEvmAddress};
    use subtensor_runtime_common::{CustomTransactionError, NetUid};

    assert_eq!(EvmKeyAssociateRateLimit::get(), 7_200);
    for mode in MODES.into_iter().chain([
        Mode::FixedMlDsa,
        Mode::Falcon(6),
        Mode::Falcon(7),
        Mode::Falcon(8),
        Mode::Falcon(9),
    ]) {
        super::mldsa::with_profiles(|| {
            let wallet = Wallet::new(mode);
            let netuid = NetUid::from(1);
            let uid = 0u16;
            pallet_subtensor::NetworksAdded::<Runtime>::insert(netuid, true);
            // Self ownership preserves each protected wallet's authority floor.
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                &wallet.account,
                &wallet.account,
            ));
            pallet_subtensor::Uids::<Runtime>::insert(netuid, &wallet.account, uid);
            pallet_subtensor::Keys::<Runtime>::insert(netuid, uid, &wallet.account);

            let evm_pair = sp_core::ecdsa::Pair::from_seed(&[73; 32]);
            let make_call = |block_number: u64| {
                association_call(&wallet.account, &evm_pair, netuid, block_number)
            };

            // Pool validation prepares the next block, so a parent at 7,198
            // must reject inclusion at 7,199. Roll its temporary header back.
            System::set_block_number(7_198);
            let (evm_key, too_early) = make_call(7_199);
            let expected_early_error =
                sp_runtime::transaction_validity::TransactionValidityError::from(
                    CustomTransactionError::EvmKeyAssociateRateLimitExceeded,
                );
            assert_eq!(
                with_transaction_unchecked(|| TransactionOutcome::Rollback(
                    Executive::validate_transaction(
                        sp_runtime::transaction_validity::TransactionSource::External,
                        wallet.sign(too_early.clone()),
                        System::parent_hash(),
                    ),
                )),
                Err(expected_early_error)
            );
            System::set_block_number(7_199);
            let before = System::account_nonce(&wallet.account);
            let result = with_transaction_unchecked(|| {
                TransactionOutcome::Rollback(Executive::apply_extrinsic(wallet.sign(too_early)))
            });
            assert_eq!(
                result,
                Err(expected_early_error),
                "{mode:?}: premature association accepted"
            );
            assert_eq!(System::account_nonce(&wallet.account), before);
            assert!(AssociatedEvmAddress::<Runtime>::get(netuid, uid).is_none());
            assert!(AssociatedUidsByEvmAddress::<Runtime>::get(netuid, evm_key).is_empty());

            // A parent at 7,199 admits the first valid inclusion at 7,200.
            let (evm_key, call) = make_call(7_200);
            assert_ok!(with_transaction_unchecked(|| TransactionOutcome::Rollback(
                Executive::validate_transaction(
                    sp_runtime::transaction_validity::TransactionSource::External,
                    wallet.sign(call.clone()),
                    System::parent_hash(),
                ),
            )));
            System::set_block_number(7_200);
            assert_ok!(Executive::apply_extrinsic(wallet.sign(call)).unwrap());
            assert_eq!(
                AssociatedEvmAddress::<Runtime>::get(netuid, uid),
                Some((evm_key, 7_200))
            );
            assert_eq!(
                AssociatedUidsByEvmAddress::<Runtime>::get(netuid, evm_key).as_slice(),
                &[(uid, 7_200)]
            );
            assert_eq!(System::account_nonce(&wallet.account), before + 1);
        });
    }
}

#[test]
fn native_evm_deposit_resolves_the_mapping_at_execution_for_every_wallet() {
    for mode in MODES.into_iter().chain([
        Mode::FixedMlDsa,
        Mode::Falcon(6),
        Mode::Falcon(7),
        Mode::Falcon(8),
        Mode::Falcon(9),
    ]) {
        super::mldsa::with_profiles(|| {
            let wallet = Wallet::new(mode);
            let target_descriptor = super::mldsa::descriptor(119);
            let target = AccountId::new(subtensor_hashed::mldsa::account_id(&target_descriptor));
            let address = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(&target)[..20]);
            let obsolete = evm_origin::HashedAddressMapping::into_account_id(address);
            assert_ne!(obsolete, target);
            let deposit = wallet.sign(RuntimeCall::EVM(pallet_evm::Call::deposit {
                address,
                value: Some(TaoBalance::new(1_000_000_000)),
            }));
            // The sender has already signed when a new alias becomes active.
            assert_eq!(super::mldsa::register(119), target);
            let before = Balances::free_balance(&target);
            assert_ok!(Executive::apply_extrinsic(deposit).unwrap());
            assert_eq!(
                Balances::free_balance(&target),
                before + TaoBalance::new(1_000_000_000)
            );
            assert_eq!(Balances::free_balance(&obsolete), TaoBalance::new(0));
        });
    }
}

#[test]
fn native_evm_deposit_all_preserves_frozen_balance_and_batch_rollback() {
    use frame_support::traits::{LockableCurrency, WithdrawReasons};
    super::mldsa::with_profiles(|| {
        let wallet = Wallet::new(Mode::FixedMlDsa);
        let address = H160::from_low_u64_be(119);
        let destination = evm_origin::HashedAddressMapping::into_account_id(address);
        let source_before = Balances::free_balance(&wallet.account);
        let frozen = TaoBalance::new(2_000_000_000);
        Balances::set_lock(
            *b"evm-test",
            &wallet.account,
            frozen,
            WithdrawReasons::all(),
        );
        assert_ok!(EVM::deposit(
            RuntimeOrigin::signed(wallet.account.clone()),
            address,
            None
        ));
        assert_eq!(Balances::free_balance(&wallet.account), frozen);
        assert_eq!(Balances::free_balance(&destination), source_before - frozen);
        Balances::remove_lock(*b"evm-test", &wallet.account);
        let before = Balances::free_balance(&destination);
        let failing = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: vec![
                RuntimeCall::EVM(pallet_evm::Call::deposit {
                    address,
                    value: Some(TaoBalance::new(1_000_000)),
                }),
                RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                    dest: destination.clone().into(),
                    value: TaoBalance::new(u64::MAX),
                }),
            ],
        });
        assert!(
            Executive::apply_extrinsic(wallet.sign(failing))
                .unwrap()
                .is_err()
        );
        assert_eq!(Balances::free_balance(&destination), before);
        assert_eq!(
            Balances::reserved_balance(&wallet.account),
            TaoBalance::new(0)
        );
        assert!(!pallet_hashed_accounts::MlDsaAccounts::<Runtime>::contains_key(&wallet.account));
    });
}

#[test]
fn fixed_ms_association_keeps_evm_funds_and_signing_authority_separate() {
    use fp_self_contained::SelfContainedCall;
    use pallet_hashed_accounts::{EvmAliases, MlDsaAccounts};
    use pallet_subtensor::{AssociatedEvmAddress, AssociatedUidsByEvmAddress};
    use subtensor_runtime_common::NetUid;

    super::mldsa::with_profiles(|| {
        let wallet = Wallet::new(Mode::FixedMlDsa);
        let netuid = NetUid::from(1);
        pallet_subtensor::NetworksAdded::<Runtime>::insert(netuid, true);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &wallet.account,
            &wallet.account,
        ));
        pallet_subtensor::Uids::<Runtime>::insert(netuid, &wallet.account, 0u16);
        pallet_subtensor::Keys::<Runtime>::insert(netuid, 0u16, &wallet.account);
        System::set_block_number(7_200);
        let evm_pair = sp_core::ecdsa::Pair::from_seed(&[74; 32]);
        let (evm_key, association) = association_call(&wallet.account, &evm_pair, netuid, 7_200);
        assert_ne!(evm_key, wallet.source());
        assert_ok!(Executive::apply_extrinsic(wallet.sign(association)).unwrap());
        assert_eq!(
            AssociatedEvmAddress::<Runtime>::get(netuid, 0u16),
            Some((evm_key, 7_200))
        );
        assert_eq!(
            AssociatedUidsByEvmAddress::<Runtime>::get(netuid, evm_key).as_slice(),
            &[(0u16, 7_200)]
        );
        let backing = evm_origin::HashedAddressMapping::into_account_id(evm_key);
        assert_ne!(backing, wallet.account);
        assert!(EvmAliases::<Runtime>::get(evm_key.0).is_none());
        assert!(MlDsaAccounts::<Runtime>::get(&wallet.account).is_none());

        // Attribution must not turn the associated ECDSA key into an alternative
        // signer for the full native account, even with a valid ECDSA signature.
        let native_before = System::account(&wallet.account);
        let call = super::call();
        let extra = extra(native_before.nonce);
        let payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
        let signature = Signature::Ecdsa(payload.using_encoded(|m| evm_pair.sign(m)));
        let forged =
            UncheckedExtrinsic::new_signed(call, wallet.account.clone().into(), signature, extra);
        assert_eq!(
            Executive::apply_extrinsic(forged),
            Err(InvalidTransaction::BadProof.into())
        );
        assert_eq!(System::account(&wallet.account), native_before);

        // Native transfers fund the conventional EVM backing account. The
        // association neither shares balances nor creates a protected alias.
        let funding = RuntimeCall::EVM(pallet_evm::Call::deposit {
            address: evm_key,
            value: Some(TaoBalance::new(20_000_000_000)),
        });
        assert_ok!(Executive::apply_extrinsic(wallet.sign(funding)).unwrap());
        assert_eq!(
            Balances::free_balance(&backing),
            TaoBalance::new(20_000_000_000)
        );
        let native_before_return = System::account(&wallet.account);
        let evm_nonce = EVM::account_basic(&evm_key).0.nonce;
        let mut input = sp_core::keccak_256(b"transfer(bytes32)")[..4].to_vec();
        input.extend_from_slice(AsRef::<[u8; 32]>::as_ref(&wallet.account));
        let destination = H160::from_low_u64_be(2048);
        let message = ethereum::LegacyTransactionMessage {
            nonce: evm_nonce,
            gas_price: gas_price(),
            gas_limit: U256::from(500_000),
            action: ethereum::TransactionAction::Call(destination),
            value: U256::from(1_000_000_000_000_000_000u64),
            input,
            chain_id: None,
        };
        let signature = evm_pair.sign_prehashed(message.hash().as_fixed_bytes());
        let transaction = ethereum::LegacyTransaction {
            nonce: message.nonce,
            gas_price: message.gas_price,
            gas_limit: message.gas_limit,
            action: message.action,
            value: message.value,
            input: message.input,
            signature: ethereum::legacy::TransactionSignature::new(
                <u64 as From<u8>>::from(signature.0[64]) + 27,
                H256::from_slice(&signature.0[..32]),
                H256::from_slice(&signature.0[32..64]),
            )
            .unwrap(),
        };
        let call = RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
            transaction: ethereum::TransactionV3::Legacy(transaction),
        });
        assert_eq!(call.check_self_contained().unwrap().unwrap(), evm_key);
        assert_ok!(Executive::apply_extrinsic(UncheckedExtrinsic::new_bare(call)).unwrap());
        assert!(System::events().iter().any(|record| matches!(
            record.event,
            RuntimeEvent::Ethereum(pallet_ethereum::Event::Executed { from, to, ref exit_reason, .. })
                if from == evm_key && to == destination && exit_reason.is_succeed()
        )));
        assert_eq!(
            Balances::free_balance(&wallet.account),
            native_before_return.data.free + TaoBalance::new(1_000_000_000)
        );
        assert_eq!(
            System::account_nonce(&wallet.account),
            native_before_return.nonce
        );
        assert_eq!(
            EVM::account_basic(&evm_key).0.nonce,
            evm_nonce + U256::one()
        );
        assert!(Balances::free_balance(&backing) < TaoBalance::new(19_000_000_000));
        assert_eq!(
            Balances::reserved_balance(&wallet.account),
            TaoBalance::ZERO
        );
        assert!(MlDsaAccounts::<Runtime>::get(&wallet.account).is_none());
        assert!(EvmAliases::<Runtime>::get(evm_key.0).is_none());
        assert!(EvmAliases::<Runtime>::get(wallet.source().0).is_none());
    });
}

#[test]
fn queried_nonce_executes_and_advances_independently_for_every_wallet_mode() {
    for mode in MODES {
        super::mldsa::with_profiles(|| {
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
            super::mldsa::with_profiles(|| {
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
    super::mldsa::with_profiles(|| {
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
    });
}

#[test]
fn reverted_and_invalid_evm_calls_keep_authorization_fees_and_reject_replays() {
    for mode in MODES {
        for invalid in [false, true] {
            super::mldsa::with_profiles(|| {
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
    for mode in [Mode::HashedSr, Mode::HashedEd, Mode::MlDsa] {
        super::mldsa::with_profiles(|| {
            let wallet = Wallet::new(mode);
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::one());
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(evm_call(&wallet, Some(U256::one()))))
                    .unwrap()
            );
            let backing =
                evm_origin::HashedAddressMapping::nonce_account_id(wallet.source()).unwrap();
            assert_ne!(backing, wallet.account);
            let _ = Balances::make_free_balance_be(&backing, TaoBalance::new(1_000_000_000));
            let _ = Balances::make_free_balance_be(&backing, TaoBalance::ZERO);
            assert_eq!(System::account_nonce(&backing), 2);
            assert_eq!(System::account(&backing).sufficients, 1);
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(false);
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::from(2));
            simulate_call(&wallet, Some(U256::from(2)));
            assert!(Executive::apply_extrinsic(wallet.sign(call())).is_err());
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
            assert_ok!(Executive::apply_extrinsic(wallet.sign(call())).unwrap());
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::from(2));
            simulate_call(&wallet, Some(U256::from(2)));
            assert_ok!(
                Executive::apply_extrinsic(wallet.sign(evm_call(&wallet, Some(U256::from(2)))))
                    .unwrap()
            );
            assert_eq!(EVM::account_basic(&wallet.source()).0.nonce, U256::from(3));
        });
    }
}

#[test]
fn protected_alias_rejects_create2_collision_before_any_native_evm_transaction() {
    ext().execute_with(|| {
        System::set_block_number(1);
        pallet_evm::DisableWhitelistCheck::<Runtime>::put(true);
        let source = H160::repeat_byte(0x42);
        let backing = evm_origin::HashedAddressMapping::into_account_id(source);
        let _ = Balances::make_free_balance_be(&backing, TaoBalance::new(1_000_000_000_000));
        let salt = H256::repeat_byte(0x17);
        let init = hex::decode("6001600c60003960016000f300").unwrap();
        let mut preimage = vec![0xff];
        preimage.extend_from_slice(source.as_bytes());
        preimage.extend_from_slice(salt.as_bytes());
        preimage.extend_from_slice(&sp_io::hashing::keccak_256(&init));
        let expected = H160::from_slice(&sp_io::hashing::keccak_256(&preimage)[12..]);
        let create = || {
            hashed_evm_runner::HashedEvmRunner::create2(
                source,
                init.clone(),
                salt,
                U256::zero(),
                200_000,
                Some(gas_price()),
                None,
                None,
                vec![],
                vec![],
                true,
                vec![],
                false,
                true,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            )
        };
        let before = with_transaction_unchecked(|| TransactionOutcome::Rollback(create())).unwrap();
        assert!(before.exit_reason.is_succeed());
        assert_eq!(before.value, expected);

        // Construct the collision directly: discovering one cryptographically
        // is outside this regression. The alias must remain creation-ineligible
        // regardless of the destination derivation or the owner's balance.
        let mut owner_bytes = [0x73; 32];
        owner_bytes[..20].copy_from_slice(expected.as_bytes());
        let owner = AccountId::new(owner_bytes);
        assert_ok!(evm_origin::prepare_hashed_alias(&owner));
        pallet_hashed_accounts::EvmAliases::<Runtime>::insert(expected.0, &owner);
        assert_eq!(EVM::account_basic(&expected).0.nonce, U256::one());
        assert_eq!(System::account_nonce(&owner), 0);
        let after = create().unwrap();
        assert_eq!(
            after.exit_reason,
            pallet_evm::ExitReason::Error(pallet_evm::ExitError::CreateCollision)
        );
        assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(expected));
        assert_eq!(EVM::account_basic(&expected).0.nonce, U256::one());
    });
}

#[test]
fn create2_prediction_and_envelope_fee_match_dispatch() {
    for mode in MODES {
        super::mldsa::with_profiles(|| {
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
        super::mldsa::with_profiles(|| {
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

            let delegate = Wallet::new(if matches!(mode, Mode::MlDsa) {
                Mode::MlDsa
            } else if matches!(mode, Mode::HashedEd) {
                Mode::HashedSr
            } else {
                Mode::HashedEd
            });
            let delegate_account = if matches!(mode, Mode::MlDsa) {
                super::mldsa::register(63)
            } else {
                delegate.account.clone()
            };
            let grant = RuntimeCall::Proxy(pallet_proxy::Call::add_proxy {
                delegate: delegate_account.into(),
                proxy_type: ProxyType::Any,
                delay: 0,
            });
            assert_ok!(Executive::apply_extrinsic(wallet.sign(grant)).unwrap());
            let proxy = RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                real: wallet.account.clone().into(),
                force_proxy_type: Some(ProxyType::Any),
                call: Box::new(transfer),
            });
            assert_ok!(
                Executive::apply_extrinsic(if matches!(mode, Mode::MlDsa) {
                    super::mldsa::sign(63, proxy)
                } else {
                    delegate.sign(proxy)
                })
                .unwrap()
            );
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
        super::mldsa::with_profiles(|| {
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
            assert!(hashed_extrinsic::shielded_transaction(invalid.clone()).is_some());
            assert!(invalid.check(&ChainContext::default()).is_err());
        });
    }
}

#[test]
fn staking_fee_quotes_and_charges_preserve_account_wrapper_costs() {
    use crate::transaction_payment_wrapper::{
        ChargeTransactionPaymentWrapper, FeeWeightDiscount, fee_dispatch_info,
    };
    use sp_runtime::traits::DispatchTransaction;
    for mode in [Mode::HashedSr, Mode::HashedEd, Mode::MlDsa] {
        super::mldsa::with_profiles(|| {
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
