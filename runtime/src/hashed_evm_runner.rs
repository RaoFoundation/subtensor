//! Reject EIP-7702's classical account delegation for registered Hashed aliases.
//!
//! The runner is shared by native EVM calls and Ethereum transactions. Checking
//! only the outer sender would miss an authorization carried by another sender.

use alloc::vec::Vec;
use ethereum::AuthorizationList;
use frame_support::weights::Weight;
use pallet_evm::runner::{Runner, RunnerError};
use sp_core::{H160, H256, U256};

use crate::{
    Runtime,
    evm_origin::{ensure_authorization_list_size, hashed_owner},
};

type StackRunner = pallet_evm::runner::stack::Runner<Runtime>;
type Error = pallet_evm::Error<Runtime>;

pub struct HashedEvmRunner;

fn reject_protected_authorities(
    mut authorities: impl Iterator<Item = H160>,
) -> Result<(), RunnerError<Error>> {
    // Always enforce existing bindings, independently of whether this build
    // permits registering new accounts. Another sender can carry an authority's
    // delegation, and execution must recheck lists previously admitted to a pool.
    if authorities.any(|address| hashed_owner(&address).is_some()) {
        return Err(RunnerError {
            error: Error::Undefined,
            // Fail closed until reference benchmarking replaces this sentinel.
            // Registration stays disabled until EVM metering is calibrated too.
            weight: Weight::MAX,
        });
    }
    Ok(())
}

fn check_authorizations(list: &AuthorizationList) -> Result<(), RunnerError<Error>> {
    check_authorization_list_size(list.len())?;
    reject_protected_authorities(
        list.iter()
            .filter_map(|item| item.authorizing_address().ok()),
    )
}

fn check_authorization_list_size(len: usize) -> Result<(), RunnerError<Error>> {
    ensure_authorization_list_size(len).map_err(|error| RunnerError {
        error: error.into(),
        // Preserve the fail-closed sentinel until reference metering is ready.
        weight: Weight::MAX,
    })
}

impl Runner<Runtime> for HashedEvmRunner {
    type Error = Error;

    fn validate(
        source: H160,
        target: Option<H160>,
        input: Vec<u8>,
        value: U256,
        gas_limit: u64,
        max_fee_per_gas: Option<U256>,
        max_priority_fee_per_gas: Option<U256>,
        nonce: Option<U256>,
        access_list: Vec<(H160, Vec<H256>)>,
        authorization_list: Vec<(U256, H160, U256, Option<H160>)>,
        is_transactional: bool,
        weight_limit: Option<Weight>,
        proof_size_base_cost: Option<u64>,
        evm_config: &fp_evm::Config,
    ) -> Result<(), RunnerError<Self::Error>> {
        check_authorization_list_size(authorization_list.len())?;
        reject_protected_authorities(authorization_list.iter().filter_map(|item| item.3))?;
        StackRunner::validate(
            source,
            target,
            input,
            value,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            nonce,
            access_list,
            authorization_list,
            is_transactional,
            weight_limit,
            proof_size_base_cost,
            evm_config,
        )
    }

    fn call(
        source: H160,
        target: H160,
        input: Vec<u8>,
        value: U256,
        gas_limit: u64,
        max_fee_per_gas: Option<U256>,
        max_priority_fee_per_gas: Option<U256>,
        nonce: Option<U256>,
        access_list: Vec<(H160, Vec<H256>)>,
        authorization_list: AuthorizationList,
        is_transactional: bool,
        validate: bool,
        weight_limit: Option<Weight>,
        proof_size_base_cost: Option<u64>,
        config: &fp_evm::Config,
    ) -> Result<fp_evm::CallInfo, RunnerError<Self::Error>> {
        check_authorizations(&authorization_list)?;
        StackRunner::call(
            source,
            target,
            input,
            value,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            nonce,
            access_list,
            authorization_list,
            is_transactional,
            validate,
            weight_limit,
            proof_size_base_cost,
            config,
        )
    }

    fn create(
        source: H160,
        init: Vec<u8>,
        value: U256,
        gas_limit: u64,
        max_fee_per_gas: Option<U256>,
        max_priority_fee_per_gas: Option<U256>,
        nonce: Option<U256>,
        access_list: Vec<(H160, Vec<H256>)>,
        whitelist: Vec<H160>,
        disable_whitelist_check: bool,
        authorization_list: AuthorizationList,
        is_transactional: bool,
        validate: bool,
        weight_limit: Option<Weight>,
        proof_size_base_cost: Option<u64>,
        config: &fp_evm::Config,
    ) -> Result<fp_evm::CreateInfo, RunnerError<Self::Error>> {
        check_authorizations(&authorization_list)?;
        StackRunner::create(
            source,
            init,
            value,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            nonce,
            access_list,
            whitelist,
            disable_whitelist_check,
            authorization_list,
            is_transactional,
            validate,
            weight_limit,
            proof_size_base_cost,
            config,
        )
    }

    fn create2(
        source: H160,
        init: Vec<u8>,
        salt: H256,
        value: U256,
        gas_limit: u64,
        max_fee_per_gas: Option<U256>,
        max_priority_fee_per_gas: Option<U256>,
        nonce: Option<U256>,
        access_list: Vec<(H160, Vec<H256>)>,
        whitelist: Vec<H160>,
        disable_whitelist_check: bool,
        authorization_list: AuthorizationList,
        is_transactional: bool,
        validate: bool,
        weight_limit: Option<Weight>,
        proof_size_base_cost: Option<u64>,
        config: &fp_evm::Config,
    ) -> Result<fp_evm::CreateInfo, RunnerError<Self::Error>> {
        check_authorizations(&authorization_list)?;
        StackRunner::create2(
            source,
            init,
            salt,
            value,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            nonce,
            access_list,
            whitelist,
            disable_whitelist_check,
            authorization_list,
            is_transactional,
            validate,
            weight_limit,
            proof_size_base_cost,
            config,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use sp_core::{Pair, ecdsa};

    fn authorization() -> ethereum::AuthorizationListItem {
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
        let pair = ecdsa::Pair::from_seed(&[33; 32]);
        let signature = pair.sign_prehashed(item.authorization_message_hash().as_fixed_bytes());
        item.signature = ethereum::eip2930::MalleableTransactionSignature {
            odd_y_parity: signature.0[64] != 0,
            r: H256::from_slice(&signature.0[..32]),
            s: H256::from_slice(&signature.0[32..64]),
        };
        item
    }

    fn assert_guarded<T>(result: Result<T, RunnerError<Error>>) {
        match result {
            Err(error) => {
                assert!(matches!(error.error, Error::Undefined));
                assert_eq!(error.weight, Weight::MAX);
            }
            Ok(_) => panic!("protected EIP-7702 authorization reached EVM execution"),
        }
    }

    #[test]
    fn oversized_authorizations_are_rejected_before_recovery_and_storage_access() {
        // Deliberately omit externalities: a recovered authority reaching the
        // alias lookup would panic. Oversized lists must fail before that scan.
        let item = authorization();
        for count in [256, 4096] {
            assert_guarded(check_authorizations(&alloc::vec![item.clone(); count]));
        }
        assert!(check_authorization_list_size(255).is_ok());
    }

    #[test]
    fn unrelated_sender_cannot_install_7702_code_on_a_protected_account() {
        sp_io::TestExternalities::default().execute_with(|| {
            let item = authorization();
            let authority = item.authorizing_address().unwrap();
            let list = alloc::vec![item];
            assert!(check_authorizations(&list).is_ok());
            pallet_hashed_accounts::EvmAliases::<Runtime>::insert(
                authority.0,
                sp_runtime::AccountId32::new([44; 32]),
            );
            assert!(check_authorizations(&list).is_err());
            let source = H160::repeat_byte(9);
            let config = <Runtime as pallet_evm::Config>::config();
            assert_guarded(HashedEvmRunner::validate(
                source,
                None,
                Vec::new(),
                U256::zero(),
                100_000,
                None,
                None,
                None,
                Vec::new(),
                alloc::vec![(
                    U256::zero(),
                    H160::repeat_byte(8),
                    U256::zero(),
                    Some(authority)
                )],
                true,
                None,
                None,
                config,
            ));
            assert_guarded(HashedEvmRunner::call(
                source,
                H160::repeat_byte(7),
                Vec::new(),
                U256::zero(),
                100_000,
                None,
                None,
                None,
                Vec::new(),
                list.clone(),
                true,
                false,
                None,
                None,
                config,
            ));
            assert_guarded(HashedEvmRunner::create(
                source,
                Vec::new(),
                U256::zero(),
                100_000,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                true,
                list.clone(),
                true,
                false,
                None,
                None,
                config,
            ));
            assert_guarded(HashedEvmRunner::create2(
                source,
                Vec::new(),
                H256::zero(),
                U256::zero(),
                100_000,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                true,
                list,
                true,
                false,
                None,
                None,
                config,
            ));
            assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(
                authority
            ));
        });
    }
}
