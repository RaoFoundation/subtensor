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
    let message = ethereum::EIP7702TransactionMessage {
        chain_id: node_subtensor_runtime::ConfigurableChainId::get(),
        nonce: U256::zero(),
        max_priority_fee_per_gas: U256::zero(),
        max_fee_per_gas: <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0,
        gas_limit: U256::from(200_000),
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
