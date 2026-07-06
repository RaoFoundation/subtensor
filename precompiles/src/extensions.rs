extern crate alloc;

use alloc::format;

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, Pays, PostDispatchInfo};
use frame_system::RawOrigin;
use pallet_admin_utils::{PrecompileEnable, PrecompileEnum};
use pallet_evm::{
    AddressMapping, BalanceConverter, EvmBalance, ExitError, GasWeightMapping, Precompile,
    PrecompileFailure, PrecompileHandle, PrecompileResult,
};
use precompile_utils::EvmResult;
use precompile_utils::prelude::RuntimeHelper;
use sp_core::{H160, U256, blake2_256};
use sp_runtime::traits::{Dispatchable, ExtensionPostDispatchWeightHandler};
use sp_std::vec::Vec;
use subtensor_runtime_common::with_evm_context;

pub(crate) trait PrecompileHandleExt: PrecompileHandle {
    fn caller_account_id<R>(&self) -> R::AccountId
    where
        R: frame_system::Config + pallet_evm::Config,
        <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    {
        <R as pallet_evm::Config>::AddressMapping::into_account_id(self.context().caller)
    }

    fn record_db_reads<R>(&mut self, reads: u64) -> EvmResult<()>
    where
        R: frame_system::Config + pallet_evm::Config,
    {
        self.record_cost(RuntimeHelper::<R>::db_read_gas_cost().saturating_mul(reads))?;
        Ok(())
    }

    fn record_db_writes<R>(&mut self, writes: u64) -> EvmResult<()>
    where
        R: frame_system::Config + pallet_evm::Config,
    {
        self.record_cost(RuntimeHelper::<R>::db_write_gas_cost().saturating_mul(writes))?;

        Ok(())
    }

    fn try_convert_apparent_value<R>(&self) -> EvmResult<U256>
    where
        R: pallet_evm::Config,
    {
        let amount = EvmBalance::new(self.context().apparent_value);
        let result = <R as pallet_evm::Config>::BalanceConverter::into_substrate_balance(amount)
            .ok_or(PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    "error converting balance from ETH to subtensor".into(),
                ),
            })?;
        Ok(result.into())
    }

    /// Dispatches a runtime call, but also checks and records the gas costs.
    fn try_dispatch_runtime_call<R, Call>(
        &mut self,
        call: Call,
        origin: RawOrigin<R::AccountId>,
    ) -> EvmResult<()>
    where
        R: frame_system::Config
            + pallet_evm::Config
            + pallet_subtensor::Config
            + pallet_shield::Config
            + pallet_subtensor_proxy::Config,
        <R as frame_system::Config>::RuntimeCall: From<Call>,
        <R as frame_system::Config>::RuntimeCall:
            GetDispatchInfo + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
        <R as frame_system::Config>::RuntimeOrigin: From<RawOrigin<R::AccountId>> + Clone,
    {
        let call = <R as frame_system::Config>::RuntimeCall::from(call);
        let info = GetDispatchInfo::get_dispatch_info(&call);

        let target_gas = self.gas_limit();
        if let Some(gas) = target_gas {
            let valid_weight =
                <R as pallet_evm::Config>::GasWeightMapping::gas_to_weight(gas, false).ref_time();
            if info.total_weight().ref_time() > valid_weight {
                return Err(PrecompileFailure::Error {
                    exit_status: ExitError::OutOfGas,
                });
            }
        }

        self.record_external_cost(
            Some(info.total_weight().ref_time()),
            Some(info.total_weight().proof_size()),
            None,
        )?;

        let origin = <R as frame_system::Config>::RuntimeOrigin::from(origin);

        match with_evm_context(|| call.dispatch(origin)) {
            Ok(mut post_info) => {
                post_info.set_extension_weight(&info);
                log::debug!("Dispatch succeeded. Post info: {post_info:?}");
                self.charge_and_refund_after_dispatch::<R, Call>(&info, &post_info)?;

                Ok(())
            }
            Err(e) => {
                let err_str: &'static str = e.into();
                let mut post_info = e.post_info;
                post_info.set_extension_weight(&info);
                log::info!("Precompile dispatch failed. message as: {e:?}");
                self.charge_and_refund_after_dispatch::<R, Call>(&info, &post_info)?;

                Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other(
                        format!("dispatch execution failed: {err_str}").into(),
                    ),
                })
            }
        }
    }

    fn charge_and_refund_after_dispatch<R, Call>(
        &mut self,
        info: &DispatchInfo,
        post_info: &PostDispatchInfo,
    ) -> EvmResult<()>
    where
        R: frame_system::Config + pallet_evm::Config,
    {
        if post_info.pays_fee(info) == Pays::Yes {
            let actual_weight = post_info.calc_actual_weight(info);
            let cost = <R as pallet_evm::Config>::GasWeightMapping::weight_to_gas(actual_weight);
            self.record_cost(cost)?;

            self.refund_external_cost(
                Some(
                    info.total_weight()
                        .ref_time()
                        .saturating_sub(actual_weight.ref_time()),
                ),
                Some(
                    info.total_weight()
                        .proof_size()
                        .saturating_sub(actual_weight.proof_size()),
                ),
            );
        }

        Ok(())
    }
}

impl<T> PrecompileHandleExt for T where T: PrecompileHandle {}

pub trait PrecompileExt<AccountId: From<[u8; 32]>>: Precompile {
    const INDEX: u64;

    // ss58 public key i.e., the contract sends funds it received to the destination address from
    // the method parameter.
    fn account_id() -> AccountId {
        let hash = H160::from_low_u64_be(Self::INDEX);
        let prefix = b"evm:";

        // Concatenate prefix and ethereum address
        let mut combined = Vec::new();
        combined.extend_from_slice(prefix);
        combined.extend_from_slice(hash.as_bytes());

        let hash = blake2_256(&combined);

        hash.into()
    }

    fn try_execute<R>(
        handle: &mut impl PrecompileHandle,
        precompile_enum: PrecompileEnum,
    ) -> Option<PrecompileResult>
    where
        R: frame_system::Config + pallet_admin_utils::Config,
    {
        if PrecompileEnable::<R>::get(&precompile_enum) {
            Some(Self::execute(handle))
        } else {
            Some(Err(PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    format!("Precompile {precompile_enum:?} is disabled").into(),
                ),
            }))
        }
    }
}

// allowing unreachable for the whole module fixes clippy reports about precompile macro
// implementation for `TestPrecompile`, that couldn't be fixed granularly
#[allow(unreachable_code)]
#[cfg(test)]
mod test {
    use super::*;

    use sp_core::crypto::AccountId32;

    #[test]
    fn ss58_address_from_index_works() {
        assert_eq!(
            TestPrecompile::account_id(),
            AccountId32::from([
                0x3a, 0x86, 0x18, 0xfb, 0xbb, 0x1b, 0xbc, 0x47, 0x86, 0x64, 0xff, 0x53, 0x46, 0x18,
                0x0c, 0x35, 0xd0, 0x9f, 0xac, 0x26, 0xf2, 0x02, 0x70, 0x85, 0xb3, 0x1c, 0x56, 0xc1,
                0x06, 0x3c, 0x1c, 0xd3,
            ])
        );
    }

    struct TestPrecompile;

    impl PrecompileExt<AccountId32> for TestPrecompile {
        const INDEX: u64 = 2051;
    }

    #[precompile_utils::precompile]
    impl TestPrecompile {}
}
