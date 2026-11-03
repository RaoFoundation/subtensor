#![allow(clippy::expect_used, clippy::unwrap_used)]

use ethereum::{
    AuthorizationListItem, EIP7702Transaction, TransactionAction, TransactionV3,
    eip2930::{MalleableTransactionSignature, TransactionSignature},
};
use node_subtensor_runtime::{Balances, BuildStorage, Runtime, RuntimeGenesisConfig, System};
use pallet_evm::{AccountProvider, AddressMapping, FeeCalculator, GasWeightMapping, Runner};
use precompile_utils::solidity::encode_with_selector;
use sp_core::{H160, H256, Pair, U256, ecdsa};
use subtensor_runtime_common::TaoBalance;

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn fund(address: H160) {
    let account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address);
    let amount = TaoBalance::new(10_000_000_000);
    let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(amount);
    let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(&account, credit, amount).unwrap();
}

fn authorization() -> AuthorizationListItem {
    AuthorizationListItem {
        chain_id: 0,
        address: H160::from_low_u64_be(0x9000),
        nonce: U256::zero(),
        signature: MalleableTransactionSignature {
            odd_y_parity: false,
            r: H256::repeat_byte(1),
            s: H256::repeat_byte(2),
        },
    }
}

#[test]
fn transfer_all_preserves_evm_nonce_after_reaping_and_refunding() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        let account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(source);
        // Exercise an existing account, whose nonce predates the upgrade.
        frame_system::Account::<Runtime>::mutate(&account, |info| info.nonce = 7);
        let destination = H256::repeat_byte(0x44);
        let hash = sp_io::hashing::keccak_256(b"transferAll(bytes32,bool)");
        let selector = u32::from_be_bytes(hash[..4].try_into().unwrap());
        let fee = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0;
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            H160::from_low_u64_be(2048),
            encode_with_selector(selector, (destination, false)),
            U256::zero(),
            500_000,
            Some(fee),
            None,
            Some(U256::from(7)),
            vec![],
            vec![],
            true,
            true,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
        assert!(
            Balances::free_balance(sp_runtime::AccountId32::from(destination.0))
                > TaoBalance::new(0)
        );
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&source)
                .0
                .nonce,
            U256::from(8)
        );

        fund(source);
        let replay = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            H160::from_low_u64_be(0x8200),
            vec![],
            U256::zero(),
            pallet_evm::account_cost::config::<Runtime>(<Runtime as pallet_evm::Config>::config())
                .gas_transaction_call,
            Some(fee),
            None,
            Some(U256::from(7)),
            vec![],
            vec![],
            true,
            true,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap_err();
        assert!(matches!(
            replay.error,
            pallet_evm::Error::<Runtime>::InvalidNonce
        ));
    });
}

#[test]
fn oversized_authorizations_are_rejected_even_without_runner_validation() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        for validate in [true, false] {
            let result = <Runtime as pallet_evm::Config>::Runner::call(
                source,
                H160::from_low_u64_be(0x8200),
                vec![],
                U256::zero(),
                21_000,
                Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
                None,
                None,
                vec![],
                vec![authorization(); 256],
                true,
                validate,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            );
            assert!(matches!(
                result.unwrap_err().error,
                pallet_evm::Error::<Runtime>::AuthorizationListTooLarge
            ));
            assert_eq!(
                System::account_nonce(
                    <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(source)
                ),
                0
            );
        }
    });
}

#[test]
fn create_and_create2_reject_oversized_authorizations_before_execution() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        for validate in [true, false] {
            let fee = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0;
            let result = <Runtime as pallet_evm::Config>::Runner::create(
                source,
                vec![],
                U256::zero(),
                21_000,
                Some(fee),
                None,
                None,
                vec![],
                vec![],
                true,
                vec![authorization(); 256],
                true,
                validate,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            )
            .unwrap_err();
            assert!(matches!(
                result.error,
                pallet_evm::Error::<Runtime>::AuthorizationListTooLarge
            ));
            let result = <Runtime as pallet_evm::Config>::Runner::create2(
                source,
                vec![],
                H256::zero(),
                U256::zero(),
                21_000,
                Some(fee),
                None,
                None,
                vec![],
                vec![],
                true,
                vec![authorization(); 256],
                true,
                validate,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            )
            .unwrap_err();
            assert!(matches!(
                result.error,
                pallet_evm::Error::<Runtime>::AuthorizationListTooLarge
            ));
        }
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&source)
                .0
                .nonce,
            U256::zero()
        );
    });
}

#[test]
fn ethereum_rejects_oversized_authorizations_before_signature_validation() {
    new_test_ext().execute_with(|| {
        let transaction = TransactionV3::EIP7702(EIP7702Transaction {
            chain_id: 0,
            nonce: U256::zero(),
            max_priority_fee_per_gas: U256::zero(),
            max_fee_per_gas: U256::zero(),
            gas_limit: U256::from(21_000),
            destination: TransactionAction::Call(H160::from_low_u64_be(0x8200)),
            value: U256::zero(),
            data: vec![],
            access_list: vec![],
            authorization_list: vec![authorization(); 256],
            signature: TransactionSignature::new(false, H256::repeat_byte(1), H256::repeat_byte(2))
                .unwrap(),
        });
        let expected = sp_runtime::transaction_validity::TransactionValidityError::Invalid(
            sp_runtime::transaction_validity::InvalidTransaction::Custom(
                fp_evm::TransactionValidationError::AuthorizationListTooLarge as u8,
            ),
        );
        assert_eq!(
            pallet_ethereum::Call::<Runtime>::transact {
                transaction: transaction.clone()
            }
            .check_self_contained(),
            Some(Err(expected)),
        );
        assert_eq!(
            pallet_ethereum::Pallet::<Runtime>::validate_transaction_in_block(
                H160::zero(),
                &transaction
            ),
            Err(expected)
        );
    });
}

#[test]
fn valid_authorization_is_applied_after_validation() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        let mut authorization = authorization();
        let pair = ecdsa::Pair::from_seed(&[0x41; 32]);
        let signature = pair.sign_prehashed(&authorization.authorization_message_hash().0);
        authorization.signature = MalleableTransactionSignature {
            odd_y_parity: signature.0[64] != 0,
            r: H256::from_slice(&signature.0[..32]),
            s: H256::from_slice(&signature.0[32..64]),
        };
        let authority = authorization.authorizing_address().unwrap();
        let destination = authorization.address;
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            H160::from_low_u64_be(0x8200),
            vec![],
            U256::zero(),
            100_000,
            Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
            None,
            None,
            vec![],
            vec![authorization],
            true,
            true,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed());
        let mut code = vec![0xef, 0x01, 0x00];
        code.extend_from_slice(destination.as_bytes());
        assert_eq!(pallet_evm::AccountCodes::<Runtime>::get(authority), code);
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&authority)
                .0
                .nonce,
            U256::one()
        );
    });
}

#[test]
fn maximum_authorization_count_is_accepted_and_charged() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        let mut invalid = authorization();
        invalid.signature.r = H256::zero();
        invalid.signature.s = H256::zero();
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            H160::from_low_u64_be(0x8200),
            vec![],
            U256::zero(),
            8_000_000,
            Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
            None,
            None,
            vec![],
            vec![invalid; 255],
            true,
            true,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed());
        assert!(result.used_gas.standard >= U256::from(21_000 + 255 * 25_000));
    });
}

#[test]
fn insufficient_authorization_gas_is_rejected_without_validation() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            H160::from_low_u64_be(0x8200),
            vec![],
            U256::zero(),
            21_000,
            Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
            None,
            None,
            vec![],
            vec![authorization(); 255],
            true,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap_err();
        assert!(matches!(
            result.error,
            pallet_evm::Error::<Runtime>::GasLimitTooLow
        ));
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&source)
                .0
                .nonce,
            U256::zero()
        );
    });
}

#[test]
fn retained_nonce_survives_native_reaping_and_tracks_native_increments() {
    new_test_ext().execute_with(|| {
        type Provider = <Runtime as pallet_evm::Config>::AccountProvider;
        let source = H160::from_low_u64_be(0x8100);
        fund(source);
        let account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(source);
        Provider::inc_account_nonce(&account);
        Balances::transfer_all(
            node_subtensor_runtime::RuntimeOrigin::signed(account.clone()),
            sp_runtime::MultiAddress::Id(sp_runtime::AccountId32::from([0x44; 32])),
            false,
        )
        .unwrap();
        assert!(!frame_system::Account::<Runtime>::contains_key(&account));
        assert_eq!(Provider::account_nonce(&account), 1);
        fund(source);
        Provider::inc_account_nonce(&account);
        assert_eq!(System::account_nonce(&account), 2);
        // Native transactions can advance the shared nonce further.
        frame_system::Account::<Runtime>::mutate(&account, |info| info.nonce = 10);
        assert_eq!(Provider::account_nonce(&account), 10);
        Provider::inc_account_nonce(&account);
        assert_eq!(Provider::account_nonce(&account), 11);
        assert_eq!(System::account_nonce(&account), 11);
    });
}

#[test]
fn explicit_contract_deletion_preserves_existing_nonce_reset_semantics() {
    new_test_ext().execute_with(|| {
        type Provider = <Runtime as pallet_evm::Config>::AccountProvider;
        let address = H160::from_low_u64_be(0x8300);
        let account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address);
        pallet_evm::Pallet::<Runtime>::create_account(address, vec![0x00], None).unwrap();
        Provider::inc_account_nonce(&account);
        assert_eq!(Provider::account_nonce(&account), 1);
        pallet_evm::Pallet::<Runtime>::remove_account(&address);
        assert!(!pallet_evm::AccountNonces::<Runtime>::contains_key(
            &account
        ));
        assert_eq!(Provider::account_nonce(&account), 0);
    });
}

#[test]
fn retained_nonce_does_not_wrap_at_the_native_nonce_limit() {
    new_test_ext().execute_with(|| {
        type Provider = <Runtime as pallet_evm::Config>::AccountProvider;
        let address = H160::from_low_u64_be(0x8100);
        fund(address);
        let account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address);
        frame_system::Account::<Runtime>::mutate(&account, |info| info.nonce = u32::MAX);
        assert_eq!(
            Provider::try_inc_account_nonce(&account),
            Err(fp_evm::ExitError::MaxNonce)
        );
        assert_eq!(Provider::account_nonce(&account), u32::MAX);
        assert_eq!(System::account_nonce(&account), u32::MAX);
    });
}

#[test]
fn ordinary_transfer_fits_metered_intrinsic_gas_with_external_weight_metering() {
    new_test_ext().execute_with(|| {
        let source = H160::from_low_u64_be(0x8100);
        let destination = H160::from_low_u64_be(0x8200);
        fund(source);
        fund(destination);
        let gas_limit =
            pallet_evm::account_cost::config::<Runtime>(<Runtime as pallet_evm::Config>::config())
                .gas_transaction_call;
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            source,
            destination,
            vec![],
            U256::from(1_000_000_000u64),
            gas_limit,
            Some(<Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price().0),
            None,
            None,
            vec![],
            vec![],
            true,
            true,
            Some(<Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(gas_limit, true)),
            Some(0),
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
        assert_eq!(result.used_gas.standard, U256::from(gas_limit));
        assert!(result.used_gas.effective <= U256::from(gas_limit));
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&source)
                .0
                .nonce,
            U256::one()
        );
    });
}
