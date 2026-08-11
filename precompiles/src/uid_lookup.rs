use core::marker::PhantomData;

use frame_support::dispatch::{GetDispatchInfo, PostDispatchInfo};
use pallet_evm::PrecompileHandle;
use precompile_utils::{EvmResult, prelude::Address};
use sp_runtime::traits::{Dispatchable, StaticLookup};
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct UidLookupPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for UidLookupPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeCall:
        GetDispatchInfo + Dispatchable<PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    const INDEX: u64 = 2054;
}

#[precompile_utils::precompile]
impl<R> UidLookupPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeCall:
        GetDispatchInfo + Dispatchable<PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    #[precompile::public("uidLookup(uint16,address,uint16)")]
    #[precompile::view]
    fn uid_lookup(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        evm_address: Address,
        limit: u16,
    ) -> EvmResult<Vec<(u16, u64)>> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::uid_lookup(
            netuid.into(),
            evm_address.0,
            limit,
        ))
    }

    #[precompile::public("getAssociatedEvmAddress(uint16,uint16)")]
    #[precompile::view]
    fn get_associated_evm_address(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        uid: u16,
    ) -> EvmResult<(bool, Address, u64)> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            match pallet_subtensor::AssociatedEvmAddress::<R>::get(NetUid::from(netuid), uid) {
                Some((address, block)) => (true, Address(address), block),
                None => (false, Address::default(), 0),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::mock::{Runtime, addr_from_index, new_test_ext, precompiles, selector_u32};
    use precompile_utils::prelude::RuntimeHelper;
    use precompile_utils::solidity::{codec::Address, encode_return_value, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use subtensor_runtime_common::NetUid;

    const TEST_NETUID_U16: u16 = 1;

    #[test]
    fn uid_lookup_precompile_returns_associated_uid_and_block() {
        new_test_ext().execute_with(|| {
            let precompiles = precompiles::<UidLookupPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(UidLookupPrecompile::<Runtime>::INDEX);

            let netuid = NetUid::from(TEST_NETUID_U16);
            let uid = 0u16;
            let evm_address = addr_from_index(0xdead_beef);
            let block_associated = 42u64;
            let limit = 1024u16;

            pallet_subtensor::Pallet::<Runtime>::set_associated_evm_address(
                netuid,
                uid,
                evm_address,
                block_associated,
            );

            let expected =
                pallet_subtensor::Pallet::<Runtime>::uid_lookup(netuid, evm_address, limit);
            assert_eq!(expected, vec![(uid, block_associated)]);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("uidLookup(uint16,address,uint16)"),
                        (TEST_NETUID_U16, Address(evm_address), limit),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(encode_return_value(expected));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getAssociatedEvmAddress(uint16,uint16)"),
                        (TEST_NETUID_U16, uid),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns((true, Address(evm_address), block_associated));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getAssociatedEvmAddress(uint16,uint16)"),
                        (TEST_NETUID_U16, uid + 1),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns((false, Address::default(), 0_u64));
        });
    }
}
