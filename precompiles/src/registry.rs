use core::marker::PhantomData;

use pallet_admin_utils::{PrecompileEnable, PrecompileEnum};
use pallet_evm::PrecompileHandle;
use precompile_utils::{
    EvmResult,
    prelude::{Address, UnboundedString},
    solidity::{
        Codec,
        codec::{Reader, Writer},
    },
};
use sp_core::{H160, H256};

use crate::{PrecompileExt, PrecompileHandleExt};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bytes4([u8; 4]);

impl Codec for Bytes4 {
    fn read(reader: &mut Reader) -> precompile_utils::solidity::revert::MayRevert<Self> {
        let word = reader.read::<H256>()?;
        let [a, b, c, d, ..] = word.to_fixed_bytes();
        Ok(Self([a, b, c, d]))
    }

    fn write(writer: &mut Writer, value: Self) {
        let mut word = [0u8; 32];
        word[..4].copy_from_slice(&value.0);
        H256::write(writer, H256::from(word));
    }

    fn has_static_size() -> bool {
        true
    }

    fn signature() -> alloc::string::String {
        "bytes4".into()
    }
}

#[derive(Codec)]
struct PrecompileStatus {
    is_deprecated: bool,
    is_disabled: bool,
    new_precompile: Address,
    new_selector: Bytes4,
    message: UnboundedString,
}

pub struct PrecompileRegistry<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for PrecompileRegistry<R>
where
    R: frame_system::Config
        + pallet_admin_utils::Config
        + pallet_evm::Config
        + pallet_subtensor::Config,
    R::AccountId: From<[u8; 32]>,
{
    const INDEX: u64 = 2067;
}

#[precompile_utils::precompile]
impl<R> PrecompileRegistry<R>
where
    R: frame_system::Config
        + pallet_admin_utils::Config
        + pallet_evm::Config
        + pallet_subtensor::Config,
    R::AccountId: From<[u8; 32]>,
{
    #[precompile::public("getPrecompileStatus(address,bytes4)")]
    #[precompile::view]
    fn get_precompile_status(
        handle: &mut impl PrecompileHandle,
        precompile: Address,
        _selector: Bytes4,
    ) -> EvmResult<PrecompileStatus> {
        let is_disabled = match precompile_enum::<R>(precompile.0) {
            Some(precompile_id) => {
                handle.record_db_reads::<R>(1)?;
                !PrecompileEnable::<R>::get(precompile_id)
            }
            None => false,
        };

        Ok(PrecompileStatus {
            is_deprecated: false,
            is_disabled,
            new_precompile: Address(H160::zero()),
            new_selector: Bytes4::default(),
            message: UnboundedString::default(),
        })
    }
}

fn precompile_enum<R>(address: H160) -> Option<PrecompileEnum>
where
    R: frame_system::Config
        + pallet_admin_utils::Config
        + pallet_evm::Config
        + pallet_subtensor::Config,
    R::AccountId: From<[u8; 32]>,
{
    let _runtime = PhantomData::<R>;
    let at = |index| address == H160::from_low_u64_be(index);
    if at(2048) {
        Some(PrecompileEnum::BalanceTransfer)
    } else if at(2049) || at(2053) {
        Some(PrecompileEnum::Staking)
    } else if at(2051) {
        Some(PrecompileEnum::Subnet)
    } else if at(2050) {
        Some(PrecompileEnum::Metagraph)
    } else if at(2052) {
        Some(PrecompileEnum::Neuron)
    } else if at(2054) {
        Some(PrecompileEnum::UidLookup)
    } else if at(2056) {
        Some(PrecompileEnum::Alpha)
    } else if at(2057) {
        Some(PrecompileEnum::Crowdloan)
    } else if at(2059) {
        Some(PrecompileEnum::Proxy)
    } else if at(2058) {
        Some(PrecompileEnum::Leasing)
    } else if at(2060) {
        Some(PrecompileEnum::AddressMapping)
    } else if at(2061) {
        Some(PrecompileEnum::VotingPower)
    } else if at(2062) {
        Some(PrecompileEnum::AccountBalance)
    } else if at(2063) {
        Some(PrecompileEnum::Scheduler)
    } else if at(2064) {
        Some(PrecompileEnum::Drand)
    } else if at(2065) {
        Some(PrecompileEnum::Timestamp)
    } else if at(2066) {
        Some(PrecompileEnum::RuntimeConfiguration)
    } else if at(2067) {
        Some(PrecompileEnum::PrecompileRegistry)
    } else {
        None
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
    fn reports_reversible_disablement_at_reserved_address() {
        new_test_ext().execute_with(|| {
            assert_eq!(PrecompileRegistry::<Runtime>::INDEX, 2067);
            PrecompileEnable::<Runtime>::insert(PrecompileEnum::Scheduler, false);

            let precompiles = precompiles::<PrecompileRegistry<Runtime>>();
            let caller = addr_from_index(1);
            let registry = addr_from_index(2067);
            let scheduler = Address(addr_from_index(2063));
            let selector = Bytes4(selector_u32("getIncompleteSince()").to_be_bytes());

            precompiles
                .prepare_test(
                    caller,
                    registry,
                    encode_with_selector(
                        selector_u32("getPrecompileStatus(address,bytes4)"),
                        (scheduler, selector),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(encode_return_value(PrecompileStatus {
                    is_deprecated: false,
                    is_disabled: true,
                    new_precompile: Address(H160::zero()),
                    new_selector: Bytes4::default(),
                    message: UnboundedString::default(),
                }));
        });
    }
}
