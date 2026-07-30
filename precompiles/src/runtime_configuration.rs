use core::marker::PhantomData;

use pallet_evm::PrecompileHandle;
use precompile_utils::EvmResult;

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct RuntimeConfigurationPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for RuntimeConfigurationPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_evm_chain_id::Config
        + pallet_subtensor::Config,
    R::AccountId: From<[u8; 32]>,
{
    const INDEX: u64 = 2066;
}

#[precompile_utils::precompile]
impl<R> RuntimeConfigurationPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_evm_chain_id::Config
        + pallet_subtensor::Config,
    R::AccountId: From<[u8; 32]>,
{
    #[precompile::public("getEvmChainId()")]
    #[precompile::view]
    fn get_evm_chain_id(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_evm_chain_id::ChainId::<R>::get())
    }

    #[precompile::public("getTransactionRateLimit()")]
    #[precompile::view]
    fn get_transaction_rate_limit(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_tx_rate_limit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Runtime, addr_from_index, new_test_ext, precompiles, selector_u32};
    use precompile_utils::{
        prelude::RuntimeHelper,
        solidity::{encode_return_value, encode_with_selector},
        testing::PrecompileTesterExt,
    };

    #[test]
    fn address_selectors_and_values_are_stable() {
        new_test_ext().execute_with(|| {
            assert_eq!(RuntimeConfigurationPrecompile::<Runtime>::INDEX, 2066);
            pallet_evm_chain_id::ChainId::<Runtime>::put(9_999u64);
            pallet_subtensor::TxRateLimit::<Runtime>::put(77u64);

            let precompiles = precompiles::<RuntimeConfigurationPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let address = addr_from_index(2066);
            let read_cost = RuntimeHelper::<Runtime>::db_read_gas_cost();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getEvmChainId()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(9_999u64));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getTransactionRateLimit()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(77u64));
        });
    }
}
