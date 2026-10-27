use core::marker::PhantomData;

use codec::{Decode, Encode};
use fp_evm::{ExitError, PrecompileFailure};
use pallet_evm::PrecompileHandle;
use precompile_utils::EvmResult;
use sp_core::H256;

use crate::{PrecompileExt, PrecompileHandleExt};

type ScheduledCallMetadata = (bool, bool, H256, u8, H256, bool, u32, bool, u64, u32);

pub struct SchedulerPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for SchedulerPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_scheduler::Config,
    R::AccountId: From<[u8; 32]>,
    R::Hash: AsRef<[u8]>,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64> + TryInto<u64>,
{
    const INDEX: u64 = 2063;
}

#[precompile_utils::precompile]
impl<R> SchedulerPrecompile<R>
where
    R: frame_system::Config + pallet_evm::Config + pallet_scheduler::Config,
    R::AccountId: From<[u8; 32]>,
    R::Hash: AsRef<[u8]>,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64> + TryInto<u64>,
{
    #[precompile::public("getIncompleteSince()")]
    #[precompile::view]
    fn get_incomplete_since(handle: &mut impl PrecompileHandle) -> EvmResult<(bool, u64)> {
        handle.record_db_reads::<R>(1)?;
        match pallet_scheduler::IncompleteSince::<R>::get() {
            Some(block) => Ok((true, block_number_to_u64::<R>(block)?)),
            None => Ok((false, 0)),
        }
    }

    #[precompile::public("getScheduledCallCount(uint64)")]
    #[precompile::view]
    fn get_scheduled_call_count(handle: &mut impl PrecompileHandle, when: u64) -> EvmResult<u32> {
        handle.record_db_reads::<R>(1)?;
        let agenda = pallet_scheduler::Agenda::<R>::get(block_number_from_u64::<R>(when)?);
        u32::try_from(agenda.len()).map_err(|_| conversion_error("scheduler agenda length"))
    }

    #[precompile::public("getScheduledCall(uint64,uint32)")]
    #[precompile::view]
    fn get_scheduled_call(
        handle: &mut impl PrecompileHandle,
        when: u64,
        index: u32,
    ) -> EvmResult<ScheduledCallMetadata> {
        handle.record_db_reads::<R>(1)?;
        let agenda = pallet_scheduler::Agenda::<R>::get(block_number_from_u64::<R>(when)?);
        let Some(Some(scheduled)) = agenda
            .get(usize::try_from(index).map_err(|_| conversion_error("scheduler agenda index"))?)
        else {
            return Ok((
                false,
                false,
                H256::zero(),
                0,
                H256::zero(),
                false,
                0,
                false,
                0,
                0,
            ));
        };

        let (has_task_id, task_id) = scheduled
            .maybe_id
            .map(|id| (true, H256::from(id)))
            .unwrap_or((false, H256::zero()));
        let call_hash = <[u8; 32]>::try_from(scheduled.call.hash().as_ref())
            .map(H256::from)
            .map_err(|_| conversion_error("scheduler call hash"))?;
        let (has_call_length, call_length) = scheduled
            .call
            .len()
            .map(|length| (true, length))
            .unwrap_or((false, 0));
        let (is_periodic, period, remaining) = match scheduled.maybe_periodic {
            Some((period, remaining)) => (true, block_number_to_u64::<R>(period)?, remaining),
            None => (false, 0, 0),
        };

        Ok((
            true,
            has_task_id,
            task_id,
            scheduled.priority,
            call_hash,
            has_call_length,
            call_length,
            is_periodic,
            period,
            remaining,
        ))
    }

    #[precompile::public("getRetry(uint64,uint32)")]
    #[precompile::view]
    fn get_retry(
        handle: &mut impl PrecompileHandle,
        when: u64,
        index: u32,
    ) -> EvmResult<(bool, u8, u8, u64)> {
        handle.record_db_reads::<R>(1)?;
        let address = (block_number_from_u64::<R>(when)?, index);
        match pallet_scheduler::Retries::<R>::get(address) {
            Some(retry) => {
                let encoded = retry.encode();
                let (total_retries, remaining, period) =
                    <(u8, u8, pallet_scheduler::BlockNumberFor<R>)>::decode(
                        &mut encoded.as_slice(),
                    )
                    .map_err(|_| conversion_error("scheduler retry metadata"))?;
                Ok((
                    true,
                    total_retries,
                    remaining,
                    block_number_to_u64::<R>(period)?,
                ))
            }
            None => Ok((false, 0, 0, 0)),
        }
    }

    #[precompile::public("getTaskAddress(bytes32)")]
    #[precompile::view]
    fn get_task_address(
        handle: &mut impl PrecompileHandle,
        task_id: H256,
    ) -> EvmResult<(bool, u64, u32)> {
        handle.record_db_reads::<R>(1)?;
        match pallet_scheduler::Lookup::<R>::get(task_id.0) {
            Some((when, index)) => Ok((true, block_number_to_u64::<R>(when)?, index)),
            None => Ok((false, 0, 0)),
        }
    }
}

fn block_number_from_u64<R>(block: u64) -> EvmResult<pallet_scheduler::BlockNumberFor<R>>
where
    R: pallet_scheduler::Config,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64>,
{
    block
        .try_into()
        .map_err(|_| conversion_error("scheduler block number"))
}

fn block_number_to_u64<R>(block: pallet_scheduler::BlockNumberFor<R>) -> EvmResult<u64>
where
    R: pallet_scheduler::Config,
    pallet_scheduler::BlockNumberFor<R>: TryInto<u64>,
{
    block
        .try_into()
        .map_err(|_| conversion_error("scheduler block number"))
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
    fn address_selectors_and_empty_metadata_are_stable() {
        new_test_ext().execute_with(|| {
            assert_eq!(SchedulerPrecompile::<Runtime>::INDEX, 2063);
            let precompiles = precompiles::<SchedulerPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let address = addr_from_index(2063);
            let read_cost = RuntimeHelper::<Runtime>::db_read_gas_cost();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getIncompleteSince()"), ()),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value((false, 0u64)));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getScheduledCallCount(uint64)"), (10u64,)),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value(0u32));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getScheduledCall(uint64,uint32)"),
                        (10u64, 0u32),
                    ),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value((
                    false,
                    false,
                    H256::zero(),
                    0u8,
                    H256::zero(),
                    false,
                    0u32,
                    false,
                    0u64,
                    0u32,
                )));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getRetry(uint64,uint32)"), (10u64, 0u32)),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value((false, 0u8, 0u8, 0u64)));
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getTaskAddress(bytes32)"), (H256::zero(),)),
                )
                .with_static_call(true)
                .expect_cost(read_cost)
                .execute_returns_raw(encode_return_value((false, 0u64, 0u32)));
        });
    }
}
