use core::marker::PhantomData;

use alloc::vec::Vec;
use fp_evm::{ExitError, PrecompileFailure};
use frame_support::BoundedVec;
use pallet_evm::PrecompileHandle;
use precompile_utils::{
    EvmResult,
    prelude::{BoundedBytes, UnboundedBytes},
};
use sp_core::ConstU32;

use crate::{PrecompileExt, PrecompileHandleExt};

type BeaconConfiguration = (
    UnboundedBytes,
    u32,
    u32,
    UnboundedBytes,
    UnboundedBytes,
    UnboundedBytes,
    UnboundedBytes,
);

pub struct DrandPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for DrandPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_drand::Config,
    R::AccountId: From<[u8; 32]>,
    frame_system::pallet_prelude::BlockNumberFor<R>: TryInto<u64>,
{
    const INDEX: u64 = 2064;
}

#[precompile_utils::precompile]
impl<R> DrandPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_drand::Config,
    R::AccountId: From<[u8; 32]>,
    frame_system::pallet_prelude::BlockNumberFor<R>: TryInto<u64>,
{
    #[precompile::public("getBeaconConfig()")]
    #[precompile::view]
    fn get_beacon_config(handle: &mut impl PrecompileHandle) -> EvmResult<BeaconConfiguration> {
        handle.record_db_reads::<R>(1)?;
        let config = pallet_drand::BeaconConfig::<R>::get();
        Ok((
            config.public_key.into_inner().into(),
            config.period,
            config.genesis_time,
            config.hash.into_inner().into(),
            config.group_hash.into_inner().into(),
            config.scheme_id.into_inner().into(),
            config.metadata.beacon_id.into_inner().into(),
        ))
    }

    #[precompile::public("getPulse(uint64)")]
    #[precompile::view]
    fn get_pulse(
        handle: &mut impl PrecompileHandle,
        round: u64,
    ) -> EvmResult<(bool, u64, UnboundedBytes, UnboundedBytes)> {
        handle.record_db_reads::<R>(1)?;
        match pallet_drand::Pulses::<R>::get(round) {
            Some(pulse) => Ok((
                true,
                pulse.round,
                pulse.randomness.into_inner().into(),
                pulse.signature.into_inner().into(),
            )),
            None => Ok((
                false,
                round,
                UnboundedBytes::default(),
                UnboundedBytes::default(),
            )),
        }
    }

    #[precompile::public("getStoredRoundRange()")]
    #[precompile::view]
    fn get_stored_round_range(handle: &mut impl PrecompileHandle) -> EvmResult<(u64, u64)> {
        handle.record_db_reads::<R>(2)?;
        Ok((
            pallet_drand::OldestStoredRound::<R>::get(),
            pallet_drand::LastStoredRound::<R>::get(),
        ))
    }

    #[precompile::public("getNextUnsignedAt()")]
    #[precompile::view]
    fn get_next_unsigned_at(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        pallet_drand::Pallet::<R>::next_unsigned_at()
            .try_into()
            .map_err(|_| conversion_error("drand next unsigned block"))
    }

    #[precompile::public("hasMigrationRun(bytes)")]
    #[precompile::view]
    fn has_migration_run(
        handle: &mut impl PrecompileHandle,
        key: BoundedBytes<ConstU32<128>>,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        let key = BoundedVec::<u8, ConstU32<128>>::try_from(Vec::<u8>::from(key))
            .map_err(|_| conversion_error("drand migration key"))?;
        Ok(pallet_drand::HasMigrationRun::<R>::get(key))
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
    use crate::mock::{Runtime, addr_from_index, new_test_ext, precompiles, selector_u32};
    use precompile_utils::{
        prelude::RuntimeHelper,
        solidity::{encode_return_value, encode_with_selector},
        testing::PrecompileTesterExt,
    };

    #[test]
    fn address_selectors_and_empty_state_are_stable() {
        new_test_ext().execute_with(|| {
            assert_eq!(DrandPrecompile::<Runtime>::INDEX, 2064);
            let precompiles = precompiles::<DrandPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let address = addr_from_index(2064);
            let read_cost = RuntimeHelper::<Runtime>::db_read_gas_cost();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getPulse(uint64)"), (42u64,)),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value((
                    false,
                    42u64,
                    UnboundedBytes::default(),
                    UnboundedBytes::default(),
                )));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getStoredRoundRange()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost.saturating_mul(2))
                .execute_returns_raw(encode_return_value((0u64, 0u64)));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getNextUnsignedAt()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(0u64));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("hasMigrationRun(bytes)"),
                        (BoundedBytes::<ConstU32<128>>::from(Vec::<u8>::new()),),
                    ),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(false));
        });
    }
}
