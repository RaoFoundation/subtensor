//! Use Frontier's bounded, metered authorization checks and address mapping costs.
//! Recovery is shared with execution; no runtime adapter repeats signatures.

pub type HashedEvmRunner = pallet_evm::runner::stack::Runner<crate::Runtime>;

#[cfg(test)]
use {
    crate::Runtime,
    alloc::vec::Vec,
    ethereum::AuthorizationList,
    frame_support::weights::Weight,
    pallet_evm::runner::{Runner, RunnerError},
    sp_core::{H160, H256, U256},
};
#[cfg(test)]
type Error = pallet_evm::Error<Runtime>;
#[cfg(test)]
fn check_authorizations(list: &AuthorizationList) -> Result<(), RunnerError<Error>> {
    HashedEvmRunner::recover_authorizations(list).map(|_| ())
}
#[cfg(test)]
fn check_authorization_list_size(len: usize) -> Result<(), RunnerError<Error>> {
    HashedEvmRunner::check_authorization_size(len)
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
                assert_ne!(error.weight, Weight::MAX);
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
