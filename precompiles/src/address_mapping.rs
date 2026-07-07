extern crate alloc;
use core::marker::PhantomData;
use pallet_evm::AddressMapping;

use crate::PrecompileExt;
use sp_core::{ByteArray, H256};

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use pallet_evm::PrecompileHandle;
use pallet_subtensor_proxy as pallet_proxy;
use precompile_utils::EvmResult;
use precompile_utils::prelude::Address;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};

pub struct AddressMappingPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for AddressMappingPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_evm::Config
        + pallet_proxy::Config
        + pallet_subtensor::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2060;
}

#[precompile_utils::precompile]
impl<R> AddressMappingPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_evm::Config
        + pallet_proxy::Config
        + pallet_subtensor::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("addressMapping(address)")]
    #[precompile::view]
    fn address_mapping(
        _handle: &mut impl PrecompileHandle,
        target_address: Address,
    ) -> EvmResult<H256> {
        let target_address: [u8; 32] = R::AddressMapping::into_account_id(target_address.0).into();
        Ok(target_address.into())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::mock::{
        Runtime, addr_from_index, execute_precompile, new_test_ext, precompiles, selector_u32,
    };
    use pallet_evm::AddressMapping;
    use precompile_utils::solidity::{codec::Address, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::U256;

    #[test]
    fn address_mapping_precompile_returns_runtime_address_mapping() {
        new_test_ext().execute_with(|| {
            let precompiles = precompiles::<AddressMappingPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let target_address = addr_from_index(0x1234);
            let input = encode_with_selector(
                selector_u32("addressMapping(address)"),
                (Address(target_address),),
            );
            let mapped_account =
                <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(target_address);
            let expected_output: [u8; 32] = mapped_account.into();

            precompiles
                .prepare_test(
                    caller,
                    addr_from_index(AddressMappingPrecompile::<Runtime>::INDEX),
                    input,
                )
                .with_static_call(true)
                .execute_returns_raw(expected_output.to_vec());
        });
    }

    #[test]
    fn address_mapping_precompile_maps_distinct_addresses_to_distinct_accounts() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(1);
            let first_address = addr_from_index(0x1234);
            let second_address = addr_from_index(0x5678);
            let precompile_addr = addr_from_index(AddressMappingPrecompile::<Runtime>::INDEX);

            let first_output = execute_precompile(
                &precompiles::<AddressMappingPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                encode_with_selector(
                    selector_u32("addressMapping(address)"),
                    (Address(first_address),),
                ),
                U256::zero(),
            )
            .expect("expected precompile mapping call to be routed to a precompile")
            .expect("address mapping call should succeed")
            .output;
            let second_output = execute_precompile(
                &precompiles::<AddressMappingPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                encode_with_selector(
                    selector_u32("addressMapping(address)"),
                    (Address(second_address),),
                ),
                U256::zero(),
            )
            .expect("expected precompile mapping call to be routed to a precompile")
            .expect("address mapping call should succeed")
            .output;

            assert_ne!(first_output, second_output);
        });
    }

    #[test]
    fn address_mapping_precompile_is_deterministic() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(1);
            let target_address = addr_from_index(0x1234);
            let precompile_addr = addr_from_index(AddressMappingPrecompile::<Runtime>::INDEX);
            let input = encode_with_selector(
                selector_u32("addressMapping(address)"),
                (Address(target_address),),
            );

            let first_output = execute_precompile(
                &precompiles::<AddressMappingPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                input.clone(),
                U256::zero(),
            )
            .expect("expected precompile mapping call to be routed to a precompile")
            .expect("address mapping call should succeed")
            .output;
            let second_output = execute_precompile(
                &precompiles::<AddressMappingPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                input,
                U256::zero(),
            )
            .expect("expected precompile mapping call to be routed to a precompile")
            .expect("address mapping call should succeed")
            .output;

            assert_eq!(first_output, second_output);
        });
    }
}
