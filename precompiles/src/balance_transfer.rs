use alloc::format;
use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, PrecompileHandle};
use precompile_utils::EvmResult;
use sp_core::{H256, U256};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, StaticLookup, UniqueSaturatedInto};

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct BalanceTransferPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for BalanceTransferPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_balances::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
    <R as pallet_balances::Config>::Balance: TryFrom<U256>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2048;
}

#[precompile_utils::precompile]
impl<R> BalanceTransferPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_balances::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
    <R as pallet_balances::Config>::Balance: TryFrom<U256>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("transfer(bytes32)")]
    #[precompile::payable]
    fn transfer(handle: &mut impl PrecompileHandle, address: H256) -> EvmResult<()> {
        let amount_sub = handle.try_convert_apparent_value::<R>()?;

        if amount_sub.is_zero() {
            return Ok(());
        }

        let caller = handle.caller_account_id::<R>();
        let dest = R::AccountId::from(address.0).into();

        let call = <R as frame_system::Config>::RuntimeCall::from(
            pallet_balances::Call::<R>::transfer_allow_death {
                dest,
                value: amount_sub.unique_saturated_into(),
            },
        );

        // Frontier has already moved msg.value from the mapped caller into this precompile's
        // account, so the balance dispatch must continue to spend from the precompile account.
        // Apply the existing centralized swap guard to the true caller before that dispatch.
        handle.record_db_reads::<R>(2)?;
        pallet_subtensor::CheckColdkeySwap::<R>::check(&caller, &call).map_err(|error| {
            PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    format!("dispatch execution failed: {error:?}").into(),
                ),
            }
        })?;

        handle.try_dispatch_runtime_call::<R, <R as frame_system::Config>::RuntimeCall>(
            call,
            RawOrigin::Signed(Self::account_id()),
        )
    }

    #[precompile::public("transferKeepAlive(bytes32,uint256)")]
    fn transfer_keep_alive(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount: U256,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_balances::Call::<R>::transfer_keep_alive {
            dest: R::AccountId::from(address.0).into(),
            value: amount
                .try_into()
                .map_err(|_| fp_evm::PrecompileFailure::Error {
                    exit_status: fp_evm::ExitError::Other(
                        "balance amount does not fit runtime".into(),
                    ),
                })?,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }

    #[precompile::public("transferAll(bytes32,bool)")]
    fn transfer_all(
        handle: &mut impl PrecompileHandle,
        address: H256,
        keep_alive: bool,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_balances::Call::<R>::transfer_all {
            dest: R::AccountId::from(address.0).into(),
            keep_alive,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, fund_account, mapped_account, new_test_ext,
        precompiles, selector_u32,
    };
    use precompile_utils::{solidity::encode_with_selector, testing::PrecompileTesterExt};

    #[test]
    fn transfer_keep_alive_dispatches_as_mapped_caller() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x8100);
            let caller_account = mapped_account(caller);
            let destination = RUNTIME_DESTINATION;
            fund_account(&caller_account, 1_000);

            precompiles::<BalanceTransferPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(BalanceTransferPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("transferKeepAlive(bytes32,uint256)"),
                        (destination, U256::from(100u64)),
                    ),
                )
                .execute_returns(());

            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(AccountId::from(destination.0)),
                100u64.into()
            );
        });
    }

    const RUNTIME_DESTINATION: H256 = H256([0x44; 32]);
}
