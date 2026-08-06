use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::pallet_prelude::{StorageValue, ValueQuery};
use frame_support::traits::StorageInstance;
use pallet_evm::PrecompileHandle;
use precompile_utils::EvmResult;

use crate::{PrecompileExt, PrecompileHandleExt};

struct DidUpdateStorage;

impl StorageInstance for DidUpdateStorage {
    const STORAGE_PREFIX: &'static str = "DidUpdate";

    fn pallet_prefix() -> &'static str {
        "Timestamp"
    }
}

type DidUpdate = StorageValue<DidUpdateStorage, bool, ValueQuery>;

pub struct TimestampPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for TimestampPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_timestamp::Config,
    R::AccountId: From<[u8; 32]>,
    R::Moment: TryInto<u64>,
{
    const INDEX: u64 = 2065;
}

#[precompile_utils::precompile]
impl<R> TimestampPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_timestamp::Config,
    R::AccountId: From<[u8; 32]>,
    R::Moment: TryInto<u64>,
{
    #[precompile::public("getTimestamp()")]
    #[precompile::view]
    fn get_timestamp(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        pallet_timestamp::Pallet::<R>::get()
            .try_into()
            .map_err(|_| conversion_error("timestamp moment"))
    }

    #[precompile::public("wasUpdatedThisBlock()")]
    #[precompile::view]
    fn was_updated_this_block(handle: &mut impl PrecompileHandle) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(DidUpdate::get())
    }
}

fn conversion_error(field: &'static str) -> PrecompileFailure {
    PrecompileFailure::Error {
        exit_status: ExitError::Other(field.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{
        Runtime, Timestamp, addr_from_index, new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::{
        prelude::RuntimeHelper,
        solidity::{encode_return_value, encode_with_selector},
        testing::PrecompileTesterExt,
    };

    #[test]
    fn address_selectors_and_values_are_stable() {
        new_test_ext().execute_with(|| {
            assert_eq!(TimestampPrecompile::<Runtime>::INDEX, 2065);
            Timestamp::set_timestamp(1_234);

            let precompiles = precompiles::<TimestampPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let address = addr_from_index(2065);
            let read_cost = RuntimeHelper::<Runtime>::db_read_gas_cost();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getTimestamp()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(1_234u64));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("wasUpdatedThisBlock()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(true));
        });
    }
}
