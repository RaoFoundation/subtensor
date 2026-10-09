//! These tests link the runtime as a dependency: its `cfg(test)` is OFF.
//! Keep this coverage outside the runtime's unit tests so production-only
//! bypasses cannot silently disable permanent account protections again.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use codec::Encode;
use fp_self_contained::SelfContainedCall;
use frame_support::{assert_ok, dispatch::GetDispatchInfo, traits::Get};
use node_subtensor_runtime::{
    Balances, BuildStorage, EVM, Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, System,
    evm_origin,
};
use pallet_evm::{AddressMapping, EnsureAddressOrigin, FeeCalculator, runner::Runner};
use sp_core::{H160, H256, Pair, U256, ecdsa};
use sp_runtime::{
    AccountId32, DispatchError, traits::BlakeTwo256, transaction_validity::InvalidTransaction,
};
use subtensor_runtime_common::TaoBalance;

type EvmRunner = <Runtime as pallet_evm::Config>::Runner;
type Mapping = <Runtime as pallet_evm::Config>::AddressMapping;

fn ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        // Registration and existing-account protection are independent. A
        // production build must protect bindings even with registration OFF.
        #[cfg(not(feature = "runtime-benchmarks"))]
        assert!(!node_subtensor_runtime::HashedEnabled::get());
    });
    ext
}

fn bind(alias: H160) -> AccountId32 {
    let mut bytes = [42; 32];
    bytes[..20].copy_from_slice(alias.as_bytes());
    let owner = AccountId32::new(bytes);
    // Model an existing permanent binding, without enabling new registrations.
    pallet_hashed_accounts::EvmAliases::<Runtime>::insert(alias.0, &owner);
    owner
}

fn fund(account: &AccountId32) {
    assert_ok!(Balances::force_set_balance(
        RuntimeOrigin::root(),
        account.clone().into(),
        TaoBalance::new(1_000_000_000_000)
    ));
}

#[test]
fn permanent_alias_mapping_and_origins_are_enforced_in_production() {
    ext().execute_with(|| {
        let alias = H160::repeat_byte(11);
        let legacy = pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(alias);
        assert_eq!(Mapping::into_account_id(alias), legacy);
        assert_ok!(evm_origin::ensure_legacy_ethereum_allowed(&alias));
        let owner = bind(alias);
        assert_eq!(Mapping::into_account_id(alias), owner);
        assert_eq!(evm_origin::hashed_owner(&alias), Some(owner.clone()));
        assert_eq!(
            evm_origin::ensure_legacy_ethereum_allowed(&alias),
            Err(InvalidTransaction::BadSigner.into())
        );

        let mut alternative = *AsRef::<[u8; 32]>::as_ref(&owner);
        alternative[31] ^= 1;
        let alternative = AccountId32::new(alternative);
        for who in [&owner, &alternative, &legacy] {
            let origin = RuntimeOrigin::signed(who.clone());
            assert_eq!(
                <Runtime as pallet_evm::Config>::CallOrigin::try_address_origin(
                    &alias,
                    origin.clone()
                )
                .is_ok(),
                who == &owner
            );
            assert_eq!(
                <Runtime as pallet_evm::Config>::WithdrawOrigin::try_address_origin(&alias, origin)
                    .is_ok(),
                who == &owner
            );
        }
        let ordinary = H160::repeat_byte(12);
        assert_eq!(
            Mapping::into_account_id(ordinary),
            pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(ordinary)
        );
    });
}

fn ethereum_call() -> RuntimeCall {
    let message = ethereum::LegacyTransactionMessage {
        nonce: U256::zero(),
        gas_price: U256::from(10_000_000_000_u64),
        gas_limit: U256::from(21_000),
        action: ethereum::TransactionAction::Call(H160::repeat_byte(2)),
        value: U256::zero(),
        input: Vec::new(),
        chain_id: None,
    };
    let signature =
        ecdsa::Pair::from_seed(&[19; 32]).sign_prehashed(message.hash().as_fixed_bytes());
    RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
        transaction: ethereum::TransactionV3::Legacy(ethereum::LegacyTransaction {
            nonce: message.nonce,
            gas_price: message.gas_price,
            gas_limit: message.gas_limit,
            action: message.action,
            value: message.value,
            input: message.input,
            signature: ethereum::legacy::TransactionSignature::new(
                u64::from(signature.0[64]).saturating_add(27),
                H256::from_slice(&signature.0[..32]),
                H256::from_slice(&signature.0[32..64]),
            )
            .unwrap(),
        }),
    })
}

#[test]
fn ethereum_sender_is_rechecked_in_every_executive_phase_in_production() {
    ext().execute_with(|| {
        let call = ethereum_call();
        let signer = call.check_self_contained().unwrap().unwrap();
        // A cached signature check must not bypass a newly created binding.
        let owner = bind(signer);
        fund(&owner);
        let before = System::account(&owner);
        let info = call.get_dispatch_info();
        let len = call.encoded_size();
        assert_eq!(
            call.check_self_contained().unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
        assert_eq!(
            call.validate_self_contained(&signer, &info, len).unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
        assert_eq!(
            call.pre_dispatch_self_contained(&signer, &info, len)
                .unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
        assert_eq!(
            call.apply_self_contained(signer)
                .unwrap()
                .unwrap_err()
                .error,
            DispatchError::BadOrigin
        );
        assert_eq!(System::account(&owner), before);
    });
}

#[test]
fn native_owner_can_spend_but_another_matching_prefix_cannot() {
    ext().execute_with(|| {
        let source = H160::repeat_byte(13);
        let target = H160::repeat_byte(14);
        let owner = bind(source);
        fund(&owner);
        let mut alternative = *AsRef::<[u8; 32]>::as_ref(&owner);
        alternative[31] ^= 1;
        let alternative = AccountId32::new(alternative);
        let before = System::account(&owner);
        let fee = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0;
        let call = |who| {
            EVM::call(
                RuntimeOrigin::signed(who),
                source,
                target,
                Vec::new(),
                U256::from(1_000_000_000_000_000_000_u64),
                100_000,
                fee,
                None,
                None,
                Vec::new(),
                Vec::new(),
            )
        };
        assert_eq!(
            call(alternative.clone()).unwrap_err().error,
            DispatchError::BadOrigin
        );
        assert_eq!(
            EVM::withdraw(
                RuntimeOrigin::signed(alternative),
                source,
                TaoBalance::new(1_000)
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(System::account(&owner), before);
        assert_ok!(call(owner.clone()));
        assert_eq!(
            Balances::free_balance(Mapping::into_account_id(target)),
            TaoBalance::new(1_000_000_000)
        );
        assert_eq!(
            System::account_nonce(&owner),
            before.nonce.saturating_add(1)
        );
        assert_ok!(EVM::withdraw(
            RuntimeOrigin::signed(owner),
            source,
            TaoBalance::new(1_000)
        ));
    });
}

fn authorization(seed: u8) -> ethereum::AuthorizationListItem {
    let mut item = ethereum::AuthorizationListItem {
        chain_id: 0,
        address: H160::repeat_byte(8),
        nonce: U256::zero(),
        signature: ethereum::eip2930::MalleableTransactionSignature {
            odd_y_parity: false,
            r: H256::zero(),
            s: H256::zero(),
        },
    };
    let signature = ecdsa::Pair::from_seed(&[seed; 32])
        .sign_prehashed(item.authorization_message_hash().as_fixed_bytes());
    item.signature = ethereum::eip2930::MalleableTransactionSignature {
        odd_y_parity: signature.0[64] != 0,
        r: H256::from_slice(&signature.0[..32]),
        s: H256::from_slice(&signature.0[32..64]),
    };
    item
}

fn delegated_ethereum_call(authorization_list: ethereum::AuthorizationList) -> RuntimeCall {
    let message =
        ethereum::EIP7702TransactionMessage {
            chain_id: node_subtensor_runtime::ConfigurableChainId::get(),
            nonce: U256::zero(),
            max_priority_fee_per_gas: U256::zero(),
            max_fee_per_gas: <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
            gas_limit: U256::from(200_000_u64.saturating_add(
                30_000_u64.saturating_mul(authorization_list.len().min(255) as u64),
            )),
            destination: ethereum::TransactionAction::Call(H160::repeat_byte(7)),
            value: U256::zero(),
            data: Vec::new(),
            access_list: Vec::new(),
            authorization_list,
        };
    let signature =
        ecdsa::Pair::from_seed(&[19; 32]).sign_prehashed(message.hash().as_fixed_bytes());
    RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
        transaction: ethereum::TransactionV3::EIP7702(ethereum::EIP7702Transaction {
            chain_id: message.chain_id,
            nonce: message.nonce,
            max_priority_fee_per_gas: message.max_priority_fee_per_gas,
            max_fee_per_gas: message.max_fee_per_gas,
            gas_limit: message.gas_limit,
            destination: message.destination,
            value: message.value,
            data: message.data,
            access_list: message.access_list,
            authorization_list: message.authorization_list,
            signature: ethereum::eip2930::TransactionSignature::new(
                signature.0[64] != 0,
                H256::from_slice(&signature.0[..32]),
                H256::from_slice(&signature.0[32..64]),
            )
            .unwrap(),
        }),
    })
}

#[test]
fn ethereum_authorization_limit_is_checked_before_authority_recovery() {
    ext().execute_with(|| {
        let item = authorization(33);
        let authority = item.authorizing_address().unwrap();
        let at_limit = delegated_ethereum_call(vec![item.clone(); 255]);
        let signer = at_limit.check_self_contained().unwrap().unwrap();

        // A protected first entry would produce BadSigner if recovery and
        // authority checks ran before the size guard.
        let owner = bind(authority);
        let before = System::account(&owner);
        for count in [256, 4096] {
            let call = delegated_ethereum_call(vec![item.clone(); count]);
            let info = call.get_dispatch_info();
            let len = call.encoded_size();
            let expected = InvalidTransaction::Custom(
                fp_evm::TransactionValidationError::AuthorizationListTooLarge as u8,
            );
            assert_eq!(call.check_self_contained().unwrap(), Err(expected.into()));
            assert_eq!(
                call.validate_self_contained(&signer, &info, len).unwrap(),
                Err(expected.into())
            );
            assert_eq!(
                call.pre_dispatch_self_contained(&signer, &info, len)
                    .unwrap(),
                Err(expected.into())
            );
            assert_eq!(
                call.apply_self_contained(signer)
                    .unwrap()
                    .unwrap_err()
                    .error,
                pallet_evm::Error::<Runtime>::Undefined.into()
            );
        }
        assert_eq!(System::account(&owner), before);

        // The last permitted entry must still be checked, not truncated away.
        let mut list = vec![authorization(34); 254];
        list.push(item);
        let call = delegated_ethereum_call(list);
        let signer = call.check_self_contained().unwrap().unwrap();
        assert_eq!(
            call.validate_self_contained(&signer, &call.get_dispatch_info(), call.encoded_size())
                .unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
    });
}

#[test]
fn ethereum_admission_rechecks_third_party_delegations_after_registration() {
    ext().execute_with(|| {
        let protected = authorization(33);
        let authority = protected.authorizing_address().unwrap();
        let mut invalid = authorization(34);
        invalid.signature.r = H256::zero();
        let call = delegated_ethereum_call(vec![invalid, authorization(35), protected]);
        let signer = call.check_self_contained().unwrap().unwrap();
        assert_ne!(signer, authority);
        let sender = Mapping::into_account_id(signer);
        fund(&sender);
        let info = call.get_dispatch_info();
        let len = call.encoded_size();
        assert_ok!(call.validate_self_contained(&signer, &info, len).unwrap());
        assert_ok!(
            call.pre_dispatch_self_contained(&signer, &info, len)
                .unwrap()
        );

        let owner = bind(authority);
        fund(&owner);
        let before = System::account(&owner);
        let sender_before = System::account(&sender);
        assert_eq!(call.check_self_contained().unwrap(), Ok(signer));
        assert_eq!(
            call.validate_self_contained(&signer, &info, len).unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
        assert_eq!(
            call.pre_dispatch_self_contained(&signer, &info, len)
                .unwrap(),
            Err(InvalidTransaction::BadSigner.into())
        );
        assert_eq!(
            call.apply_self_contained(signer)
                .unwrap()
                .unwrap_err()
                .error,
            pallet_evm::Error::<Runtime>::Undefined.into()
        );
        assert_eq!(System::account(&owner), before);
        assert_eq!(System::account(&sender), sender_before);
        assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(
            authority
        ));
    });
}

fn run_call(
    list: ethereum::AuthorizationList,
    validate: bool,
) -> Result<fp_evm::CallInfo, pallet_evm::runner::RunnerError<pallet_evm::Error<Runtime>>> {
    EvmRunner::call(
        H160::repeat_byte(9),
        H160::repeat_byte(7),
        Vec::new(),
        U256::zero(),
        200_000,
        Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
        None,
        None,
        Vec::new(),
        list,
        true,
        validate,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    )
}

fn assert_guarded<T>(
    result: Result<T, pallet_evm::runner::RunnerError<pallet_evm::Error<Runtime>>>,
) {
    assert!(matches!(result, Err(error) if matches!(error.error, pallet_evm::Error::Undefined)));
}

#[test]
fn third_party_7702_authorities_are_blocked_at_validation_and_all_execution_entries() {
    ext().execute_with(|| {
        let protected = authorization(33);
        let authority = protected.authorizing_address().unwrap();
        let owner = bind(authority);
        fund(&owner);
        fund(&Mapping::into_account_id(H160::repeat_byte(9)));
        let before = System::account(&owner);
        let mut invalid = authorization(34);
        invalid.signature.r = H256::zero();
        assert!(invalid.authorizing_address().is_err());
        // Neither an invalid nor an ordinary authorization may hide a later
        // protected authority, and duplicates must not bypass the check.
        let list = vec![invalid, authorization(35), protected.clone(), protected];
        let recovered = list
            .iter()
            .map(|item| {
                (
                    U256::from(item.chain_id),
                    item.address,
                    item.nonce,
                    item.authorizing_address().ok(),
                )
            })
            .collect();
        let config = <Runtime as pallet_evm::Config>::config();
        assert_guarded(EvmRunner::validate(
            H160::repeat_byte(9),
            None,
            Vec::new(),
            U256::zero(),
            200_000,
            None,
            None,
            None,
            Vec::new(),
            recovered,
            true,
            None,
            None,
            config,
        ));
        for validate in [false, true] {
            assert_guarded(run_call(list.clone(), validate));
            assert_guarded(EvmRunner::create(
                H160::repeat_byte(9),
                Vec::new(),
                U256::zero(),
                200_000,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                true,
                list.clone(),
                true,
                validate,
                None,
                None,
                config,
            ));
            assert_guarded(EvmRunner::create2(
                H160::repeat_byte(9),
                Vec::new(),
                H256::zero(),
                U256::zero(),
                200_000,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                true,
                list.clone(),
                true,
                validate,
                None,
                None,
                config,
            ));
        }
        assert_eq!(System::account(&owner), before);
        assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(
            authority
        ));
    });
}

#[test]
fn ordinary_7702_delegation_still_executes_in_production() {
    ext().execute_with(|| {
        let item = authorization(36);
        let authority = item.authorizing_address().unwrap();
        fund(&Mapping::into_account_id(H160::repeat_byte(9)));
        let result = run_call(vec![item.clone()], true).unwrap();
        assert!(result.exit_reason.is_succeed());
        let mut expected_code = vec![0xef, 0x01, 0x00];
        expected_code.extend_from_slice(item.address.as_bytes());
        assert_eq!(
            pallet_evm::AccountCodes::<Runtime>::get(authority),
            expected_code
        );
        assert_eq!(
            System::account_nonce(Mapping::into_account_id(authority)),
            1
        );
    });
}

fn metered_call(code: Vec<u8>, gas_limit: u64) -> fp_evm::CallInfo {
    let source = H160::repeat_byte(70);
    let target = H160::repeat_byte(71);
    fund(&Mapping::into_account_id(source));
    pallet_evm::AccountCodes::<Runtime>::insert(target, code);
    EvmRunner::call(
        source,
        target,
        Vec::new(),
        U256::zero(),
        gas_limit,
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    )
    .unwrap()
}

#[test]
fn warm_balance_reads_pay_mapping_gas_and_weight_with_registration_disabled() {
    for instruction in [vec![0x47, 0x50], vec![0x30, 0x31, 0x50]] {
        let run = |count| ext().execute_with(|| metered_call(instruction.repeat(count), 1_000_000));
        let one = run(1);
        let eleven = run(11);
        assert!(one.exit_reason.is_succeed());
        assert!(eleven.exit_reason.is_succeed());
        let read = Mapping::extra_read_weight();
        let gas = pallet_evm::account_cost::gas::<Runtime>(read);
        let opcode_gas = if instruction.len() == 2 { 7 } else { 104 };
        assert_eq!(
            eleven.used_gas.standard - one.used_gas.standard,
            U256::from(10 * (gas + opcode_gas))
        );
        assert_eq!(
            eleven.weight_info.unwrap().ref_time_usage.unwrap()
                - one.weight_info.unwrap().ref_time_usage.unwrap(),
            10 * read.ref_time(),
        );
    }
}

#[test]
fn mapping_gas_exhaustion_reverts_contract_storage() {
    let mut code = vec![0x60, 1, 0x60, 0, 0x55]; // SSTORE(0, 1)
    code.extend([0x47, 0x50].repeat(100)); // repeated SELFBALANCE/POP
    let full = ext().execute_with(|| {
        let result = metered_call(code.clone(), 1_000_000);
        assert!(result.exit_reason.is_succeed());
        assert_eq!(
            pallet_evm::AccountStorages::<Runtime>::get(H160::repeat_byte(71), H256::zero()),
            H256::from_low_u64_be(1)
        );
        result.used_gas.standard.as_u64()
    });
    ext().execute_with(|| {
        let result = metered_call(code, full - 1);
        assert_eq!(
            result.exit_reason,
            fp_evm::ExitReason::Error(fp_evm::ExitError::OutOfGas)
        );
        assert_eq!(
            pallet_evm::AccountStorages::<Runtime>::get(H160::repeat_byte(71), H256::zero()),
            H256::zero()
        );
    });
}

#[test]
fn create_mapping_charge_precedes_commit_for_create_and_create2() {
    for create2 in [false, true] {
        let deploy = |gas_limit| {
            let source = H160::repeat_byte(72);
            fund(&Mapping::into_account_id(source));
            // Write storage, then deploy one STOP byte from zeroed memory.
            let init = vec![0x60, 1, 0x60, 0, 0x55, 0x60, 1, 0x60, 0, 0xf3];
            let config = <Runtime as pallet_evm::Config>::config();
            if create2 {
                EvmRunner::create2(
                    source,
                    init,
                    H256::zero(),
                    U256::zero(),
                    gas_limit,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    true,
                    Vec::new(),
                    false,
                    false,
                    None,
                    None,
                    config,
                )
            } else {
                EvmRunner::create(
                    source,
                    init,
                    U256::zero(),
                    gas_limit,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    true,
                    Vec::new(),
                    false,
                    false,
                    None,
                    None,
                    config,
                )
            }
            .unwrap()
        };
        let (address, full) = ext().execute_with(|| {
            let result = deploy(1_000_000);
            assert!(result.exit_reason.is_succeed());
            assert_eq!(
                pallet_evm::AccountCodes::<Runtime>::get(result.value),
                vec![0]
            );
            assert_eq!(
                pallet_evm::AccountStorages::<Runtime>::get(result.value, H256::zero()),
                H256::from_low_u64_be(1)
            );
            (result.value, result.used_gas.standard.as_u64())
        });
        ext().execute_with(|| {
            let result = deploy(full - 1);
            assert_eq!(
                result.exit_reason,
                fp_evm::ExitReason::Error(fp_evm::ExitError::OutOfGas)
            );
            assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(address));
            assert_eq!(
                pallet_evm::AccountStorages::<Runtime>::get(address, H256::zero()),
                H256::zero()
            );
        });
    }
}

#[test]
fn insufficient_authorization_budget_fails_before_recovery_or_registry_reads() {
    // No externalities: recovering the valid authority and looking it up panics.
    // Only the cheap budget check can return this error here.
    let result = EvmRunner::call(
        H160::repeat_byte(9),
        H160::repeat_byte(7),
        Vec::new(),
        U256::zero(),
        21_000,
        None,
        None,
        None,
        Vec::new(),
        vec![authorization(33)],
        true,
        false,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    );
    assert!(
        matches!(result, Err(error) if matches!(error.error, pallet_evm::Error::GasLimitTooLow))
    );
}

#[test]
fn failed_authorization_guard_reports_recovery_and_lookup_weight() {
    ext().execute_with(|| {
        let item = authorization(33);
        bind(item.authorizing_address().unwrap());
        let error = run_call(vec![item], false).unwrap_err();
        let recovery = <<Runtime as pallet_evm::Config>::GasWeightMapping as pallet_evm::GasWeightMapping>::gas_to_weight(
            <Runtime as pallet_evm::Config>::config().gas_auth_base_cost, false,
        );
        assert!(error.weight.ref_time() >= recovery.ref_time() + Mapping::extra_read_weight().ref_time());
        assert_ne!(error.weight, frame_support::weights::Weight::MAX);
    });
}

fn call_precompile(source: H160, target: H160, input: Vec<u8>, gas_limit: u64) -> fp_evm::CallInfo {
    EvmRunner::call(
        source,
        target,
        input,
        U256::zero(),
        gas_limit,
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    )
    .unwrap()
}

#[test]
fn unchanged_mapping_precompile_returns_registered_owner_and_charges_its_lookup() {
    ext().execute_with(|| {
        let alias = H160::repeat_byte(74);
        let owner = bind(alias);
        let source = H160::repeat_byte(73);
        fund(&Mapping::into_account_id(source));
        let mut input = sp_io::hashing::keccak_256(b"addressMapping(address)")[..4].to_vec();
        input.extend([0; 12]);
        input.extend(alias.as_bytes());
        let result = call_precompile(source, H160::from_low_u64_be(2060), input, 100_000);
        assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
        assert_eq!(result.value, AsRef::<[u8; 32]>::as_ref(&owner));
        // Seven fixed reads, two value-transfer mappings, one precompile mapping.
        assert_eq!(
            result.weight_info.unwrap().ref_time_usage.unwrap(),
            10 * Mapping::extra_read_weight().ref_time()
        );
    });
}

#[test]
fn unchanged_transfer_precompile_spends_from_owner_and_reverts_on_mapping_oog() {
    let source = H160::repeat_byte(75);
    let destination = AccountId32::new([76; 32]);
    let input = || {
        let mut input =
            sp_io::hashing::keccak_256(b"transferKeepAlive(bytes32,uint256)")[..4].to_vec();
        input.extend(AsRef::<[u8; 32]>::as_ref(&destination));
        input.extend(U256::from(1_000_000).to_big_endian());
        input
    };
    let full = ext().execute_with(|| {
        let owner = bind(source);
        fund(&owner);
        let result = call_precompile(source, H160::from_low_u64_be(2048), input(), 1_000_000);
        assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
        assert_eq!(
            Balances::free_balance(&destination),
            TaoBalance::new(1_000_000)
        );
        assert_eq!(
            Balances::free_balance(&owner),
            TaoBalance::new(1_000_000_000_000 - 1_000_000)
        );
        result.used_gas.standard.as_u64()
    });
    ext().execute_with(|| {
        let owner = bind(source);
        fund(&owner);
        let result = call_precompile(source, H160::from_low_u64_be(2048), input(), full - 1);
        assert_eq!(
            result.exit_reason,
            fp_evm::ExitReason::Error(fp_evm::ExitError::OutOfGas)
        );
        assert_eq!(Balances::free_balance(&destination), TaoBalance::new(0));
        assert_eq!(
            Balances::free_balance(&owner),
            TaoBalance::new(1_000_000_000_000)
        );
    });
}

#[test]
fn rpc_estimation_can_probe_below_authorization_intrinsic_gas_without_recovery() {
    // No externalities: the estimate must fail before any registry lookup.
    let result = EvmRunner::call(
        H160::repeat_byte(9),
        H160::repeat_byte(7),
        Vec::new(),
        U256::zero(),
        21_000,
        None,
        None,
        None,
        Vec::new(),
        vec![authorization(33)],
        false,
        false,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    )
    .unwrap();
    assert_eq!(
        result.exit_reason,
        fp_evm::ExitReason::Error(fp_evm::ExitError::OutOfGas)
    );
}
