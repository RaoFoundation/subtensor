//! Native-to-EVM origin with permanent Hashed ownership for protected aliases.
//!
//! `EnsureAddressTruncated` treats a 32-byte AccountId as the EVM address of
//! its first 20 bytes. A small-order Ed25519 encoding can have a zero prefix
//! and would otherwise be accepted as `address(0)` for `evm.call` / `evm.withdraw`.
//!
//! A registered Hashed binds its truncated alias to the complete 32-byte account.
//! A different legacy key with the same 20-byte prefix must never control it.
//! Ethereum ECDSA authorization is also disabled for every bound alias.

use frame_support::{
    dispatch::DispatchResult,
    ensure,
    traits::{Currency, ExistenceRequirement},
};
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, EnsureAddressOrigin};
use sp_core::H160;
use sp_runtime::{
    AccountId32, DispatchError,
    traits::{BlakeTwo256, Zero},
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};

/// The full account is authoritative; the truncated alias is only an EVM address.
pub fn hashed_owner(address: &H160) -> Option<AccountId32> {
    // Bindings are permanent, including when new registrations are disabled.
    // Neither build flags nor HashedEnabled may restore the legacy mapping or
    // classical signing authority over an already registered account.
    pallet_hashed_accounts::EvmAliases::<crate::Runtime>::get(address.0)
}

/// A protected alias shares its Hashed's balance, nonce and precompile identity.
/// Existing unprotected EVM addresses retain their historical hashed mapping.
pub struct HashedAddressMapping;

impl AddressMapping<AccountId32> for HashedAddressMapping {
    fn into_account_id(address: H160) -> AccountId32 {
        hashed_owner(&address).unwrap_or_else(|| legacy_backing_account(address))
    }
}

fn legacy_backing_account(address: H160) -> AccountId32 {
    pallet_evm::HashedAddressMapping::<BlakeTwo256>::into_account_id(address)
}

/// Check freshness before changing an alias's backing account. The caller must
/// run this inside the same storage transaction as the permanent alias binding.
/// Pure TAO donations are swept, so prefunding cannot prevent registration.
/// Previously used addresses must instead use a fresh Hashed key.
pub fn prepare_hashed_alias(account: &AccountId32) -> DispatchResult {
    let address = H160::from_slice(&AsRef::<[u8; 32]>::as_ref(account)[..20]);
    ensure!(
        !address.is_zero(),
        DispatchError::Other("HashedEvmZeroAlias")
    );
    let backing = legacy_backing_account(address);
    let state = frame_system::Account::<crate::Runtime>::get(&backing);
    ensure!(
        state.nonce == 0
            && state.consumers == 0
            && state.sufficients == 0
            && state.providers == u32::from(!state.data.free.is_zero())
            && state.data.reserved.is_zero()
            && state.data.frozen.is_zero()
            && pallet_balances::Locks::<crate::Runtime>::get(&backing).is_empty()
            && pallet_balances::Reserves::<crate::Runtime>::get(&backing).is_empty()
            && pallet_balances::Holds::<crate::Runtime>::get(&backing).is_empty()
            && pallet_balances::Freezes::<crate::Runtime>::get(&backing).is_empty()
            && !pallet_evm::AccountCodes::<crate::Runtime>::contains_key(address)
            && !pallet_evm::AccountCodesMetadata::<crate::Runtime>::contains_key(address)
            && pallet_evm::AccountStorages::<crate::Runtime>::iter_key_prefix(address)
                .next()
                .is_none(),
        DispatchError::Other("HashedEvmAliasAlreadyUsed")
    );
    if !state.data.free.is_zero() {
        <crate::Balances as Currency<AccountId32>>::transfer(
            &backing,
            account,
            state.data.free,
            ExistenceRequirement::AllowDeath,
        )?;
    }
    Ok(())
}

/// A protected alias cannot opt back into Ethereum's ECDSA authorization route.
pub fn ensure_legacy_ethereum_allowed(address: &H160) -> Result<(), TransactionValidityError> {
    if hashed_owner(address).is_some() {
        return Err(InvalidTransaction::BadSigner.into());
    }
    Ok(())
}

/// Match Frontier's `CheckEvmTransaction::with_eip7702_authorization_list` limit.
/// Check the length before recovering any authorization signatures, including
/// paths that run before Frontier's weight checks or skip runner validation.
pub(crate) fn ensure_authorization_list_size(
    len: usize,
) -> Result<(), fp_evm::TransactionValidationError> {
    const MAX_AUTHORIZATION_LIST_SIZE: usize = 255;
    if len > MAX_AUTHORIZATION_LIST_SIZE {
        return Err(fp_evm::TransactionValidationError::AuthorizationListTooLarge);
    }
    Ok(())
}

/// Ethereum admission does not call the EVM runner's `validate`. Check delegated
/// authorities here as well as at execution, including after pool admission.
pub fn ensure_ethereum_transaction_allowed(
    signer: &H160,
    transaction: &ethereum::TransactionV3,
) -> Result<(), TransactionValidityError> {
    if let ethereum::TransactionV3::EIP7702(transaction) = transaction {
        ensure_authorization_list_size(transaction.authorization_list.len())
            .map_err(|error| InvalidTransaction::Custom(error as u8))?;
    }
    ensure_legacy_ethereum_allowed(signer)?;
    if let ethereum::TransactionV3::EIP7702(transaction) = transaction {
        for authorization in &transaction.authorization_list {
            // EVM execution ignores invalid signatures. They grant no authority
            // and must not prevent checking subsequent valid authorizations.
            if let Ok(authority) = authorization.authorizing_address() {
                ensure_legacy_ethereum_allowed(&authority)?;
            }
        }
    }
    Ok(())
}

/// Legacy addresses use the existing prefix rule. Protected addresses require
/// the exact registered account, and address zero is always rejected.
pub struct EnsureAddressTruncatedNonZero;

impl<OuterOrigin> EnsureAddressOrigin<OuterOrigin> for EnsureAddressTruncatedNonZero
where
    OuterOrigin: Into<Result<RawOrigin<AccountId32>, OuterOrigin>> + From<RawOrigin<AccountId32>>,
{
    type Success = AccountId32;

    fn try_address_origin(address: &H160, origin: OuterOrigin) -> Result<AccountId32, OuterOrigin> {
        if address.is_zero() {
            return Err(origin);
        }
        let owner = hashed_owner(address);
        origin.into().and_then(|o| match o {
            RawOrigin::Signed(who)
                if match &owner {
                    Some(owner) => &who == owner,
                    None => AsRef::<[u8; 32]>::as_ref(&who)[0..20] == address[0..20],
                } =>
            {
                Ok(who)
            }
            r => Err(OuterOrigin::from(r)),
        })
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, RuntimeCall, RuntimeOrigin};
    use codec::Encode;
    use fp_self_contained::SelfContainedCall;
    use frame_support::dispatch::GetDispatchInfo;
    use pallet_evm::EnsureAddressOrigin;
    use sp_core::{H256, Pair, U256, ecdsa};
    use subtensor_runtime_common::AccountId;

    fn signed(bytes: [u8; 32]) -> RuntimeOrigin {
        RuntimeOrigin::signed(AccountId::from(bytes))
    }

    #[test]
    fn ordinary_truncated_match_is_accepted() {
        sp_io::TestExternalities::default().execute_with(|| {
            let mut raw = [0u8; 32];
            raw[0] = 0xab;
            raw[19] = 0xcd;
            raw[31] = 0x11;
            let address = H160::from_slice(&raw[0..20]);
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(&address, signed(raw)).is_ok()
            );
            assert!(ensure_legacy_ethereum_allowed(&address).is_ok());
        });
    }

    #[test]
    fn zero_evm_address_is_rejected_even_when_prefix_matches() {
        let mut raw = [0u8; 32];
        raw[31] = 0x80;
        assert!(
            EnsureAddressTruncatedNonZero::try_address_origin(&H160::zero(), signed(raw)).is_err()
        );
    }

    #[test]
    fn mismatched_prefix_is_rejected() {
        sp_io::TestExternalities::default().execute_with(|| {
            let mut raw = [0u8; 32];
            raw[0] = 0xab;
            let other = H160::from_low_u64_be(1);
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(&other, signed(raw)).is_err()
            );
        });
    }

    #[test]
    fn protected_alias_requires_the_entire_registered_account() {
        sp_io::TestExternalities::default().execute_with(|| {
            let owner = [11; 32];
            let address = H160::from_slice(&owner[..20]);
            let mut alternative_key = owner;
            alternative_key[31] ^= 1;
            // The historical prefix rule accepts this distinct account.
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(
                    &address,
                    signed(alternative_key)
                )
                .is_ok()
            );
            pallet_hashed_accounts::EvmAliases::<Runtime>::insert(address.0, AccountId::new(owner));
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(&address, signed(owner)).is_ok()
            );
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(
                    &address,
                    signed(alternative_key)
                )
                .is_err()
            );
            assert!(
                EnsureAddressTruncatedNonZero::try_address_origin(&address, RuntimeOrigin::root())
                    .is_err()
            );
            assert_eq!(
                ensure_legacy_ethereum_allowed(&address),
                Err(InvalidTransaction::BadSigner.into())
            );
        });
    }

    #[test]
    fn registration_sweeps_prefunding_and_unifies_the_protected_balance_account() {
        use frame_support::assert_ok;
        use subtensor_runtime_common::TaoBalance;

        sp_io::TestExternalities::default().execute_with(|| {
            let owner = AccountId::new([17; 32]);
            let alias = H160::repeat_byte(17);
            let old_backing = legacy_backing_account(alias);
            assert_eq!(HashedAddressMapping::into_account_id(alias), old_backing);
            assert_ok!(crate::Balances::force_set_balance(
                RuntimeOrigin::root(),
                old_backing.clone().into(),
                TaoBalance::new(10_000)
            ));
            assert_ok!(crate::Balances::force_set_balance(
                RuntimeOrigin::root(),
                owner.clone().into(),
                TaoBalance::new(1_000)
            ));
            assert_ok!(prepare_hashed_alias(&owner));
            pallet_hashed_accounts::EvmAliases::<Runtime>::insert(alias.0, &owner);
            assert_eq!(HashedAddressMapping::into_account_id(alias), owner);
            assert_eq!(
                crate::Balances::free_balance(&old_backing),
                TaoBalance::new(0)
            );
            assert_eq!(
                crate::Balances::free_balance(&owner),
                TaoBalance::new(11_000)
            );
            // Signing as the old backing account no longer controls the EVM funds.
            assert_ne!(HashedAddressMapping::into_account_id(alias), old_backing);
            let ordinary = H160::repeat_byte(18);
            assert_eq!(
                HashedAddressMapping::into_account_id(ordinary),
                legacy_backing_account(ordinary)
            );
        });
    }

    #[test]
    fn registration_refuses_used_evm_state_without_moving_its_balance() {
        use frame_support::assert_ok;
        use subtensor_runtime_common::TaoBalance;

        sp_io::TestExternalities::default().execute_with(|| {
            let owner = AccountId::new([29; 32]);
            let alias = H160::repeat_byte(29);
            let backing = legacy_backing_account(alias);
            assert_ok!(crate::Balances::force_set_balance(
                RuntimeOrigin::root(),
                backing.clone().into(),
                TaoBalance::new(10_000)
            ));
            let expected = Err(DispatchError::Other("HashedEvmAliasAlreadyUsed"));
            frame_system::Account::<Runtime>::mutate(&backing, |info| info.nonce = 1);
            assert_eq!(prepare_hashed_alias(&owner), expected);
            frame_system::Account::<Runtime>::mutate(&backing, |info| info.nonce = 0);
            pallet_evm::AccountCodes::<Runtime>::insert(alias, alloc::vec![0x60, 0x00]);
            assert_eq!(prepare_hashed_alias(&owner), expected);
            pallet_evm::AccountCodes::<Runtime>::remove(alias);
            pallet_evm::AccountStorages::<Runtime>::insert(
                alias,
                H256::zero(),
                H256::repeat_byte(1),
            );
            assert_eq!(prepare_hashed_alias(&owner), expected);
            pallet_evm::AccountStorages::<Runtime>::remove(alias, H256::zero());
            frame_system::Account::<Runtime>::mutate(&backing, |info| {
                info.data.frozen = TaoBalance::new(1)
            });
            assert_eq!(prepare_hashed_alias(&owner), expected);
            assert_eq!(
                crate::Balances::free_balance(&backing),
                TaoBalance::new(10_000)
            );
            assert_eq!(crate::Balances::free_balance(&owner), TaoBalance::new(0));
            assert!(hashed_owner(&alias).is_none());
        });
    }

    fn ethereum_call() -> RuntimeCall {
        let message = ethereum::LegacyTransactionMessage {
            nonce: U256::zero(),
            gas_price: U256::from(1_000_000_000_u64),
            gas_limit: U256::from(21_000),
            action: ethereum::TransactionAction::Call(H160::repeat_byte(2)),
            value: U256::zero(),
            input: Default::default(),
            chain_id: None,
        };
        let pair = ecdsa::Pair::from_seed(&[19; 32]);
        let signature = pair.sign_prehashed(message.hash().as_fixed_bytes());
        let transaction = ethereum::LegacyTransaction {
            nonce: message.nonce,
            gas_price: message.gas_price,
            gas_limit: message.gas_limit,
            action: message.action,
            value: message.value,
            input: message.input,
            signature: ethereum::legacy::TransactionSignature::new(
                u64::from(signature.0[64]) + 27,
                H256::from_slice(&signature.0[..32]),
                H256::from_slice(&signature.0[32..64]),
            )
            .unwrap(),
        };
        RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
            transaction: ethereum::TransactionV3::Legacy(transaction),
        })
    }

    #[test]
    fn ethereum_cannot_bypass_registration_in_any_executive_phase() {
        sp_io::TestExternalities::default().execute_with(|| {
            let call = ethereum_call();
            let signer = call.check_self_contained().unwrap().unwrap();
            let mut owner = [0; 32];
            owner[..20].copy_from_slice(signer.as_bytes());
            // Registration may occur after this transaction was admitted to a pool.
            pallet_hashed_accounts::EvmAliases::<Runtime>::insert(signer.0, AccountId::new(owner));
            let expected = Err(InvalidTransaction::BadSigner.into());
            assert_eq!(call.check_self_contained().unwrap(), expected);
            let info = call.get_dispatch_info();
            let len = call.encoded_size();
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
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }
}
