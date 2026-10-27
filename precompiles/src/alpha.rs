use core::marker::PhantomData;

use crate::PrecompileExt;
use fp_evm::{ExitError, PrecompileFailure};
use frame_support::{
    BoundedVec,
    dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
    traits::{ConstU32, IsSubType},
};
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, BalanceConverter, PrecompileHandle, SubstrateBalance};
use precompile_utils::{EvmResult, prelude::BoundedBytes};
use sp_runtime::{
    SaturatedConversion, Vec,
    traits::{AsSystemOriginSigner, Dispatchable},
};

use crate::PrecompileHandleExt;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{NetUid, Token};
use subtensor_swap_interface::{Order, SwapHandler};
pub struct AlphaPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for AlphaPrecompile<R>
where
    R: frame_system::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_evm::Config
        + pallet_admin_utils::Config
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_admin_utils::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2056;
}

#[precompile_utils::precompile]
impl<R> AlphaPrecompile<R>
where
    R: frame_system::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_evm::Config
        + pallet_admin_utils::Config
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_admin_utils::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("getAlphaPrice(uint16)")]
    #[precompile::view]
    fn get_alpha_price(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        // SubnetMechanism + SubnetAlphaIn + SubnetTAO + SwapBalancer reads
        handle.record_db_reads::<R>(4)?;
        let current_alpha_price =
            <pallet_subtensor_swap::Pallet<R> as SwapHandler>::current_alpha_price(netuid.into());
        let price = current_alpha_price.saturating_mul(U64F64::from_num(1_000_000_000));
        let price: SubstrateBalance = price.saturating_to_num::<u64>().into();
        let price_eth = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(price)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(price_eth)
    }

    #[precompile::public("getMovingAlphaPrice(uint16)")]
    #[precompile::view]
    fn get_moving_alpha_price(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        // SubnetMechanism + SubnetMovingPrice reads
        handle.record_db_reads::<R>(2)?;
        let moving_alpha_price: U64F64 =
            pallet_subtensor::Pallet::<R>::get_moving_alpha_price(netuid.into());
        let price = moving_alpha_price.saturating_mul(U64F64::from_num(1_000_000_000));
        let price: SubstrateBalance = price.saturating_to_num::<u64>().into();
        let price_eth = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(price)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(price_eth)
    }

    #[precompile::public("getTaoInPool(uint16)")]
    #[precompile::view]
    fn get_tao_in_pool(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::SubnetTAO::<R>::get(NetUid::from(netuid)).to_u64())
    }

    #[precompile::public("getAlphaInPool(uint16)")]
    #[precompile::view]
    fn get_alpha_in_pool(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::SubnetAlphaIn::<R>::get(NetUid::from(netuid)).into())
    }

    #[precompile::public("getAlphaOutPool(uint16)")]
    #[precompile::view]
    fn get_alpha_out_pool(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::SubnetAlphaOut::<R>::get(NetUid::from(netuid)).into())
    }

    #[precompile::public("getAlphaIssuance(uint16)")]
    #[precompile::view]
    fn get_alpha_issuance(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        // SubnetAlphaIn + SubnetAlphaOut reads
        handle.record_db_reads::<R>(2)?;
        Ok(pallet_subtensor::Pallet::<R>::get_alpha_issuance(netuid.into()).into())
    }

    #[precompile::public("getTaoWeight()")]
    #[precompile::view]
    fn get_tao_weight(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        let tao_weight = pallet_subtensor::TaoWeight::<R>::get();
        Ok(U256::from(tao_weight))
    }

    #[precompile::public("getCKBurn()")]
    #[precompile::view]
    fn get_ck_burn(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        let ck_burn = pallet_subtensor::CKBurn::<R>::get();
        Ok(U256::from(ck_burn))
    }

    #[precompile::public("simSwapTaoForAlpha(uint16,uint64)")]
    #[precompile::view]
    fn sim_swap_tao_for_alpha(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        tao: u64,
    ) -> EvmResult<U256> {
        // SubnetMechanism + swap simulation reads
        handle.record_db_reads::<R>(9)?;

        let order = pallet_subtensor::GetAlphaForTao::<R>::with_amount(tao);
        let swap_result =
            <pallet_subtensor_swap::Pallet<R> as SwapHandler>::sim_swap(netuid.into(), order)
                .map_err(|e| PrecompileFailure::Error {
                    exit_status: ExitError::Other(Into::<&'static str>::into(e).into()),
                })?;
        Ok(U256::from(swap_result.amount_paid_out.to_u64()))
    }

    #[precompile::public("simSwapAlphaForTao(uint16,uint64)")]
    #[precompile::view]
    fn sim_swap_alpha_for_tao(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        alpha: u64,
    ) -> EvmResult<U256> {
        // SubnetMechanism + swap simulation reads
        handle.record_db_reads::<R>(9)?;

        let order = pallet_subtensor::GetTaoForAlpha::<R>::with_amount(alpha);
        let swap_result =
            <pallet_subtensor_swap::Pallet<R> as SwapHandler>::sim_swap(netuid.into(), order)
                .map_err(|e| PrecompileFailure::Error {
                    exit_status: ExitError::Other(Into::<&'static str>::into(e).into()),
                })?;
        Ok(U256::from(swap_result.amount_paid_out.to_u64()))
    }

    #[precompile::public("getSubnetMechanism(uint16)")]
    #[precompile::view]
    fn get_subnet_mechanism(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::SubnetMechanism::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("getRootNetuid()")]
    #[precompile::view]
    fn get_root_netuid(_handle: &mut impl PrecompileHandle) -> EvmResult<u16> {
        Ok(NetUid::ROOT.into())
    }

    #[precompile::public("getEMAPriceHalvingBlocks(uint16)")]
    #[precompile::view]
    fn get_ema_price_halving_blocks(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::EMAPriceHalvingBlocks::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("getSubnetVolume(uint16)")]
    #[precompile::view]
    fn get_subnet_volume(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(U256::from(pallet_subtensor::SubnetVolume::<R>::get(
            NetUid::from(netuid),
        )))
    }

    #[precompile::public("getTaoInEmission(uint16)")]
    #[precompile::view]
    fn get_tao_in_emission(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(U256::from(
            pallet_subtensor::SubnetTaoInEmission::<R>::get(NetUid::from(netuid)).to_u64(),
        ))
    }

    #[precompile::public("getAlphaInEmission(uint16)")]
    #[precompile::view]
    fn get_alpha_in_emission(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(U256::from(
            pallet_subtensor::SubnetAlphaInEmission::<R>::get(NetUid::from(netuid)).to_u64(),
        ))
    }

    #[precompile::public("getAlphaOutEmission(uint16)")]
    #[precompile::view]
    fn get_alpha_out_emission(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(U256::from(
            pallet_subtensor::SubnetAlphaOutEmission::<R>::get(NetUid::from(netuid)).to_u64(),
        ))
    }

    #[precompile::public("getSumAlphaPrice()")]
    #[precompile::view]
    fn get_sum_alpha_price(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        // NetworksAdded iteration + current_alpha_price reads
        handle.record_db_reads::<R>(1)?;
        let subnet_limit = pallet_subtensor::SubnetLimit::<R>::get().saturated_into::<u64>();

        handle.record_db_reads::<R>(subnet_limit)?;

        let mut sum_alpha_price: U64F64 = U64F64::from_num(0);
        let netuids = pallet_subtensor::NetworksAdded::<R>::iter()
            .filter(|(netuid, _)| *netuid != NetUid::ROOT)
            .map(|(netuid, _)| netuid)
            .collect::<Vec<_>>();

        // NetworksAdded entry + current_alpha_price reads
        handle.record_db_reads::<R>(
            netuids
                .len()
                .saturated_into::<u64>()
                .saturating_mul(5)
                .saturating_sub(subnet_limit),
        )?;

        for netuid in netuids.iter() {
            let price =
                <pallet_subtensor_swap::Pallet<R> as SwapHandler>::current_alpha_price(*netuid);

            if price < U64F64::from_num(1) {
                sum_alpha_price = sum_alpha_price.saturating_add(price);
            }
        }

        let price = sum_alpha_price.saturating_mul(U64F64::from_num(1_000_000_000));
        let price: SubstrateBalance = price.saturating_to_num::<u64>().into();
        let price_eth = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(price)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(price_eth)
    }

    #[precompile::public("setRecycleOrBurn(uint16,uint8)")]
    fn set_recycle_or_burn(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        mode: u8,
    ) -> EvmResult<()> {
        let recycle_or_burn = match mode {
            0 => pallet_subtensor::RecycleOrBurnEnum::Burn,
            1 => pallet_subtensor::RecycleOrBurnEnum::Recycle,
            _ => {
                return Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other("invalid recycle-or-burn mode".into()),
                });
            }
        };
        let caller = handle.caller_account_id::<R>();
        let call = pallet_admin_utils::Call::<R>::sudo_set_recycle_or_burn {
            netuid: NetUid::from(netuid),
            recycle_or_burn,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }

    #[precompile::public("setBurnHalfLife(uint16,uint16)")]
    fn set_burn_half_life(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        burn_half_life: u16,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_admin_utils::Call::<R>::sudo_set_burn_half_life {
            netuid: NetUid::from(netuid),
            burn_half_life,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }

    #[precompile::public("setBurnIncreaseMultiplier(uint16,uint128)")]
    fn set_burn_increase_multiplier(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        raw_multiplier: u128,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_admin_utils::Call::<R>::sudo_set_burn_increase_mult {
            netuid: NetUid::from(netuid),
            burn_increase_mult: U64F64::from_bits(raw_multiplier),
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }

    #[precompile::public("getEmissionAccounting(uint16,bytes32)")]
    #[precompile::view]
    fn get_emission_accounting(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: sp_core::H256,
    ) -> EvmResult<(u64, u64, u64, u64, u64, u64, u64, u128, u64)> {
        handle.record_db_reads::<R>(9)?;
        let netuid = NetUid::from(netuid);
        let hotkey = R::AccountId::from(hotkey.0);
        Ok((
            pallet_subtensor::AlphaDividendsPerSubnet::<R>::get(netuid, &hotkey).to_u64(),
            pallet_subtensor::RootAlphaDividendsPerSubnet::<R>::get(netuid, &hotkey).to_u64(),
            pallet_subtensor::LastHotkeyEmissionOnNetuid::<R>::get(&hotkey, netuid).to_u64(),
            pallet_subtensor::PendingServerEmission::<R>::get(netuid).to_u64(),
            pallet_subtensor::PendingValidatorEmission::<R>::get(netuid).to_u64(),
            pallet_subtensor::PendingRootAlphaDivs::<R>::get(netuid).to_u64(),
            pallet_subtensor::PendingOwnerCut::<R>::get(netuid).to_u64(),
            pallet_subtensor::MinerBurned::<R>::get(netuid).to_bits(),
            pallet_subtensor::RAORecycledForRegistration::<R>::get(netuid).to_u64(),
        ))
    }

    #[precompile::public("getSubnetEconomicState(uint16)")]
    #[precompile::view]
    fn get_subnet_economic_state(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(bool, u128, u64, u64, u64)> {
        handle.record_db_reads::<R>(5)?;
        let netuid = NetUid::from(netuid);
        Ok((
            pallet_subtensor::SubnetEmissionEnabled::<R>::get(netuid),
            pallet_subtensor::RootProp::<R>::get(netuid).to_bits(),
            pallet_subtensor::SubnetExcessTao::<R>::get(netuid).to_u64(),
            pallet_subtensor::SubnetRootSellTao::<R>::get(netuid).to_u64(),
            pallet_subtensor::SubnetProtocolAlpha::<R>::get(netuid).to_u64(),
        ))
    }

    #[precompile::public("getSubnetFlowState(uint16)")]
    #[precompile::view]
    fn get_subnet_flow_state(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(U256, bool, u64, U256, U256, bool, u64, U256)> {
        handle.record_db_reads::<R>(4)?;
        let netuid = NetUid::from(netuid);
        let tao_ema = pallet_subtensor::SubnetEmaTaoFlow::<R>::get(netuid);
        let protocol_ema = pallet_subtensor::SubnetEmaProtocolFlow::<R>::get(netuid);
        Ok((
            signed_i64_word(pallet_subtensor::SubnetTaoFlow::<R>::get(netuid)),
            tao_ema.is_some(),
            tao_ema.map(|(block, _)| block).unwrap_or(0),
            signed_i128_word(tao_ema.map(|(_, value)| value.to_bits()).unwrap_or(0)),
            signed_i64_word(pallet_subtensor::SubnetProtocolFlow::<R>::get(netuid)),
            protocol_ema.is_some(),
            protocol_ema.map(|(block, _)| block).unwrap_or(0),
            signed_i128_word(protocol_ema.map(|(_, value)| value.to_bits()).unwrap_or(0)),
        ))
    }

    #[precompile::public("getEmissionGateConfig()")]
    #[precompile::view]
    fn get_emission_gate_config(
        handle: &mut impl PrecompileHandle,
    ) -> EvmResult<(u64, U256, bool, U256, u128, u128, u128, u128, u64)> {
        handle.record_db_reads::<R>(9)?;
        let block_emission = pallet_subtensor::Pallet::<R>::calculate_block_emission()
            .map_err(|error| PrecompileFailure::Error {
                exit_status: ExitError::Other(error.into()),
            })?
            .to_u64();
        Ok((
            block_emission,
            signed_i128_word(pallet_subtensor::SubnetMovingAlpha::<R>::get().to_bits()),
            pallet_subtensor::NetTaoFlowEnabled::<R>::get(),
            signed_i128_word(pallet_subtensor::TaoFlowCutoff::<R>::get().to_bits()),
            pallet_subtensor::FlowNormExponent::<R>::get().to_bits(),
            pallet_subtensor::EmissionBarQuantile::<R>::get().to_bits(),
            pallet_subtensor::EmissionGateExponent::<R>::get().to_bits(),
            pallet_subtensor::EmissionGateBar::<R>::get().to_bits(),
            pallet_subtensor::FlowEmaSmoothingFactor::<R>::get(),
        ))
    }

    #[precompile::public("getSwapState(uint16)")]
    #[precompile::view]
    fn get_swap_state(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(u16, bool, u64, u64, u64)> {
        handle.record_db_reads::<R>(5)?;
        let netuid = NetUid::from(netuid);
        Ok((
            pallet_subtensor_swap::FeeRate::<R>::get(netuid),
            pallet_subtensor_swap::PalSwapInitialized::<R>::get(netuid),
            pallet_subtensor_swap::SwapBalancer::<R>::get(netuid)
                .get_quote_weight()
                .deconstruct(),
            pallet_subtensor_swap::BalancerTaoReservoir::<R>::get(netuid).to_u64(),
            pallet_subtensor_swap::BalancerAlphaReservoir::<R>::get(netuid).to_u64(),
        ))
    }

    #[precompile::public("hasSwapMigrationRun(bytes)")]
    #[precompile::view]
    fn has_swap_migration_run(
        handle: &mut impl PrecompileHandle,
        migration_name: BoundedBytes<ConstU32<128>>,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        let migration_name = BoundedVec::<u8, ConstU32<128>>::truncate_from(migration_name.into());
        Ok(pallet_subtensor_swap::HasMigrationRun::<R>::get(
            migration_name,
        ))
    }
}

fn signed_i64_word(value: i64) -> U256 {
    let mut encoded = [if value.is_negative() { 0xff } else { 0 }; 32];
    encoded[24..].copy_from_slice(&value.to_be_bytes());
    U256::from_big_endian(&encoded)
}

fn signed_i128_word(value: i128) -> U256 {
    let mut encoded = [if value.is_negative() { 0xff } else { 0 }; 32];
    encoded[16..].copy_from_slice(&value.to_be_bytes());
    U256::from_big_endian(&encoded)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        Runtime, addr_from_index, alpha_price_to_evm, assert_static_call, new_test_ext,
        precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use substrate_fixed::types::I96F32;
    use subtensor_runtime_common::{AlphaBalance, TaoBalance};

    const DYNAMIC_NETUID_U16: u16 = 1;
    const SUM_PRICE_NETUID_U16: u16 = 2;
    const TAO_WEIGHT: u64 = 444;
    const CK_BURN: u64 = 555;
    const EMA_HALVING_BLOCKS: u64 = 777;
    const SUBNET_VOLUME: u128 = 888;
    const TAO_IN_EMISSION: u64 = 111;
    const ALPHA_IN_EMISSION: u64 = 222;
    const ALPHA_OUT_EMISSION: u64 = 333;

    fn seed_alpha_test_state() {
        let dynamic_netuid = NetUid::from(DYNAMIC_NETUID_U16);
        let sum_price_netuid = NetUid::from(SUM_PRICE_NETUID_U16);

        pallet_subtensor::TaoWeight::<Runtime>::put(TAO_WEIGHT);
        pallet_subtensor::CKBurn::<Runtime>::put(CK_BURN);

        pallet_subtensor::NetworksAdded::<Runtime>::insert(dynamic_netuid, true);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(dynamic_netuid, 1);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(
            dynamic_netuid,
            TaoBalance::from(20_000_000_000_u64),
        );
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            dynamic_netuid,
            AlphaBalance::from(10_000_000_000_u64),
        );
        pallet_subtensor::SubnetAlphaOut::<Runtime>::insert(
            dynamic_netuid,
            AlphaBalance::from(3_000_000_000_u64),
        );
        pallet_subtensor::SubnetTaoInEmission::<Runtime>::insert(
            dynamic_netuid,
            TaoBalance::from(TAO_IN_EMISSION),
        );
        pallet_subtensor::SubnetAlphaInEmission::<Runtime>::insert(
            dynamic_netuid,
            AlphaBalance::from(ALPHA_IN_EMISSION),
        );
        pallet_subtensor::SubnetAlphaOutEmission::<Runtime>::insert(
            dynamic_netuid,
            AlphaBalance::from(ALPHA_OUT_EMISSION),
        );
        pallet_subtensor::SubnetVolume::<Runtime>::insert(dynamic_netuid, SUBNET_VOLUME);
        pallet_subtensor::EMAPriceHalvingBlocks::<Runtime>::insert(
            dynamic_netuid,
            EMA_HALVING_BLOCKS,
        );
        pallet_subtensor::SubnetMovingPrice::<Runtime>::insert(
            dynamic_netuid,
            I96F32::from_num(3.0 / 2.0),
        );

        pallet_subtensor::NetworksAdded::<Runtime>::insert(sum_price_netuid, true);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(sum_price_netuid, 1);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(
            sum_price_netuid,
            TaoBalance::from(5_000_000_000_u64),
        );
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            sum_price_netuid,
            AlphaBalance::from(10_000_000_000_u64),
        );
    }

    #[test]
    fn alpha_precompile_matches_runtime_values_for_dynamic_subnet() {
        new_test_ext().execute_with(|| {
            seed_alpha_test_state();

            let precompiles = precompiles::<AlphaPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(AlphaPrecompile::<Runtime>::INDEX);

            let dynamic_netuid = NetUid::from(DYNAMIC_NETUID_U16);
            let alpha_price =
                <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::current_alpha_price(
                    dynamic_netuid,
                );
            let moving_alpha_price =
                pallet_subtensor::Pallet::<Runtime>::get_moving_alpha_price(dynamic_netuid);

            assert!(alpha_price > U64F64::from_num(1));
            assert!(moving_alpha_price > U64F64::from_num(1));

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getAlphaPrice(uint16)"), (DYNAMIC_NETUID_U16,)),
                alpha_price_to_evm(alpha_price),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getMovingAlphaPrice(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                alpha_price_to_evm(moving_alpha_price),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getTaoInPool(uint16)"), (DYNAMIC_NETUID_U16,)),
                pallet_subtensor::SubnetTAO::<Runtime>::get(dynamic_netuid)
                    .to_u64()
                    .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAlphaInPool(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                u64::from(pallet_subtensor::SubnetAlphaIn::<Runtime>::get(
                    dynamic_netuid,
                ))
                .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAlphaOutPool(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                u64::from(pallet_subtensor::SubnetAlphaOut::<Runtime>::get(
                    dynamic_netuid,
                ))
                .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAlphaIssuance(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                u64::from(pallet_subtensor::Pallet::<Runtime>::get_alpha_issuance(
                    dynamic_netuid,
                ))
                .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getSubnetMechanism(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::SubnetMechanism::<Runtime>::get(dynamic_netuid).into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getEMAPriceHalvingBlocks(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::EMAPriceHalvingBlocks::<Runtime>::get(dynamic_netuid).into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getSubnetVolume(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::SubnetVolume::<Runtime>::get(dynamic_netuid).into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getTaoInEmission(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::SubnetTaoInEmission::<Runtime>::get(dynamic_netuid)
                    .to_u64()
                    .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAlphaInEmission(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::SubnetAlphaInEmission::<Runtime>::get(dynamic_netuid)
                    .to_u64()
                    .into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAlphaOutEmission(uint16)"),
                    (DYNAMIC_NETUID_U16,),
                ),
                pallet_subtensor::SubnetAlphaOutEmission::<Runtime>::get(dynamic_netuid)
                    .to_u64()
                    .into(),
            );
        });
    }

    #[test]
    fn alpha_precompile_matches_runtime_global_values() {
        new_test_ext().execute_with(|| {
            seed_alpha_test_state();

            let precompiles = precompiles::<AlphaPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(AlphaPrecompile::<Runtime>::INDEX);

            let mut sum_alpha_price = U64F64::from_num(0);
            for (netuid, _) in pallet_subtensor::NetworksAdded::<Runtime>::iter() {
                if netuid.is_root() {
                    continue;
                }
                let price =
                    <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::current_alpha_price(
                        netuid,
                    );
                if price < U64F64::from_num(1) {
                    sum_alpha_price += price;
                }
            }

            assert!(sum_alpha_price > U64F64::from_num(0));

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                selector_u32("getCKBurn()").to_be_bytes().to_vec(),
                pallet_subtensor::CKBurn::<Runtime>::get().into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                selector_u32("getTaoWeight()").to_be_bytes().to_vec(),
                pallet_subtensor::TaoWeight::<Runtime>::get().into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                selector_u32("getRootNetuid()").to_be_bytes().to_vec(),
                u16::from(NetUid::ROOT).into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                selector_u32("getSumAlphaPrice()").to_be_bytes().to_vec(),
                alpha_price_to_evm(sum_alpha_price),
            );
        });
    }

    #[test]
    fn alpha_precompile_matches_runtime_swap_simulations() {
        new_test_ext().execute_with(|| {
            seed_alpha_test_state();

            let precompiles = precompiles::<AlphaPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(AlphaPrecompile::<Runtime>::INDEX);

            let tao_amount = 1_000_000_000_u64;
            let alpha_amount = 1_000_000_000_u64;
            let expected_alpha = <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::sim_swap(
                NetUid::from(DYNAMIC_NETUID_U16),
                pallet_subtensor::GetAlphaForTao::<Runtime>::with_amount(tao_amount),
            )
            .expect("tao-for-alpha simulation should succeed")
            .amount_paid_out
            .to_u64();
            let expected_tao = <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::sim_swap(
                NetUid::from(DYNAMIC_NETUID_U16),
                pallet_subtensor::GetTaoForAlpha::<Runtime>::with_amount(alpha_amount),
            )
            .expect("alpha-for-tao simulation should succeed")
            .amount_paid_out
            .to_u64();

            assert!(expected_alpha > 0);
            assert!(expected_tao > 0);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("simSwapTaoForAlpha(uint16,uint64)"),
                    (DYNAMIC_NETUID_U16, tao_amount),
                ),
                expected_alpha.into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("simSwapAlphaForTao(uint16,uint64)"),
                    (DYNAMIC_NETUID_U16, alpha_amount),
                ),
                expected_tao.into(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("simSwapTaoForAlpha(uint16,uint64)"),
                    (DYNAMIC_NETUID_U16, 0_u64),
                ),
                U256::zero(),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("simSwapAlphaForTao(uint16,uint64)"),
                    (DYNAMIC_NETUID_U16, 0_u64),
                ),
                U256::zero(),
            );
        });
    }

    #[test]
    fn alpha_state_views_return_typed_runtime_state() {
        new_test_ext().execute_with(|| {
            let precompiles = precompiles::<AlphaPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let address = addr_from_index(AlphaPrecompile::<Runtime>::INDEX);
            let netuid = NetUid::from(DYNAMIC_NETUID_U16);
            let hotkey = sp_core::H256::repeat_byte(0x41);

            assert_view(
                &precompiles,
                caller,
                address,
                "getEmissionAccounting(uint16,bytes32)",
                (DYNAMIC_NETUID_U16, hotkey),
                (
                    0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u128, 0_u64,
                ),
            );

            assert_view(
                &precompiles,
                caller,
                address,
                "getSubnetEconomicState(uint16)",
                (DYNAMIC_NETUID_U16,),
                (
                    pallet_subtensor::SubnetEmissionEnabled::<Runtime>::get(netuid),
                    pallet_subtensor::RootProp::<Runtime>::get(netuid).to_bits(),
                    pallet_subtensor::SubnetExcessTao::<Runtime>::get(netuid).to_u64(),
                    pallet_subtensor::SubnetRootSellTao::<Runtime>::get(netuid).to_u64(),
                    pallet_subtensor::SubnetProtocolAlpha::<Runtime>::get(netuid).to_u64(),
                ),
            );

            let tao_ema = pallet_subtensor::SubnetEmaTaoFlow::<Runtime>::get(netuid);
            let protocol_ema = pallet_subtensor::SubnetEmaProtocolFlow::<Runtime>::get(netuid);
            assert_view(
                &precompiles,
                caller,
                address,
                "getSubnetFlowState(uint16)",
                (DYNAMIC_NETUID_U16,),
                (
                    signed_i64_word(pallet_subtensor::SubnetTaoFlow::<Runtime>::get(netuid)),
                    tao_ema.is_some(),
                    tao_ema.map(|(block, _)| block).unwrap_or(0),
                    signed_i128_word(tao_ema.map(|(_, value)| value.to_bits()).unwrap_or(0)),
                    signed_i64_word(pallet_subtensor::SubnetProtocolFlow::<Runtime>::get(netuid)),
                    protocol_ema.is_some(),
                    protocol_ema.map(|(block, _)| block).unwrap_or(0),
                    signed_i128_word(protocol_ema.map(|(_, value)| value.to_bits()).unwrap_or(0)),
                ),
            );

            let emission_gate = (
                pallet_subtensor::Pallet::<Runtime>::calculate_block_emission()
                    .expect("block emission calculation should succeed")
                    .to_u64(),
                signed_i128_word(pallet_subtensor::SubnetMovingAlpha::<Runtime>::get().to_bits()),
                pallet_subtensor::NetTaoFlowEnabled::<Runtime>::get(),
                signed_i128_word(pallet_subtensor::TaoFlowCutoff::<Runtime>::get().to_bits()),
                pallet_subtensor::FlowNormExponent::<Runtime>::get().to_bits(),
                pallet_subtensor::EmissionBarQuantile::<Runtime>::get().to_bits(),
                pallet_subtensor::EmissionGateExponent::<Runtime>::get().to_bits(),
                pallet_subtensor::EmissionGateBar::<Runtime>::get().to_bits(),
                pallet_subtensor::FlowEmaSmoothingFactor::<Runtime>::get(),
            );
            assert_view(
                &precompiles,
                caller,
                address,
                "getEmissionGateConfig()",
                (),
                emission_gate,
            );

            let balancer = pallet_subtensor_swap::SwapBalancer::<Runtime>::get(netuid);
            assert_view(
                &precompiles,
                caller,
                address,
                "getSwapState(uint16)",
                (DYNAMIC_NETUID_U16,),
                (
                    pallet_subtensor_swap::FeeRate::<Runtime>::get(netuid),
                    pallet_subtensor_swap::PalSwapInitialized::<Runtime>::get(netuid),
                    balancer.get_quote_weight().deconstruct(),
                    pallet_subtensor_swap::BalancerTaoReservoir::<Runtime>::get(netuid).to_u64(),
                    pallet_subtensor_swap::BalancerAlphaReservoir::<Runtime>::get(netuid).to_u64(),
                ),
            );

            let migration_name = b"reader-test".to_vec();
            pallet_subtensor_swap::HasMigrationRun::<Runtime>::insert(
                BoundedVec::truncate_from(migration_name.clone()),
                true,
            );
            assert_view(
                &precompiles,
                caller,
                address,
                "hasSwapMigrationRun(bytes)",
                (BoundedBytes::<ConstU32<128>>::from(migration_name),),
                true,
            );
        });
    }

    #[test]
    fn emission_gate_block_emission_matches_runtime_at_halving_boundary() {
        new_test_ext().execute_with(|| {
            const FIRST_HALVING_ISSUANCE: u64 = 10_500_000_000_000_000;

            pallet_subtensor::TotalIssuance::<Runtime>::put(TaoBalance::from(
                FIRST_HALVING_ISSUANCE,
            ));
            #[allow(deprecated)]
            pallet_subtensor::BlockEmission::<Runtime>::put(123_u64);

            let expected_block_emission =
                pallet_subtensor::Pallet::<Runtime>::calculate_block_emission()
                    .expect("halving-boundary emission calculation should succeed")
                    .to_u64();
            assert_eq!(expected_block_emission, 500_000_000);

            let precompiles = precompiles::<AlphaPrecompile<Runtime>>();
            assert_view(
                &precompiles,
                addr_from_index(1),
                addr_from_index(AlphaPrecompile::<Runtime>::INDEX),
                "getEmissionGateConfig()",
                (),
                (
                    expected_block_emission,
                    signed_i128_word(
                        pallet_subtensor::SubnetMovingAlpha::<Runtime>::get().to_bits(),
                    ),
                    pallet_subtensor::NetTaoFlowEnabled::<Runtime>::get(),
                    signed_i128_word(pallet_subtensor::TaoFlowCutoff::<Runtime>::get().to_bits()),
                    pallet_subtensor::FlowNormExponent::<Runtime>::get().to_bits(),
                    pallet_subtensor::EmissionBarQuantile::<Runtime>::get().to_bits(),
                    pallet_subtensor::EmissionGateExponent::<Runtime>::get().to_bits(),
                    pallet_subtensor::EmissionGateBar::<Runtime>::get().to_bits(),
                    pallet_subtensor::FlowEmaSmoothingFactor::<Runtime>::get(),
                ),
            );
        });
    }

    fn assert_view<Args, Output>(
        precompiles: &impl pallet_evm::PrecompileSet,
        caller: sp_core::H160,
        address: sp_core::H160,
        signature: &str,
        args: Args,
        expected: Output,
    ) where
        Args: precompile_utils::solidity::Codec,
        Output: precompile_utils::solidity::Codec,
    {
        use precompile_utils::testing::PrecompileTesterExt;

        precompiles
            .prepare_test(
                caller,
                address,
                encode_with_selector(selector_u32(signature), args),
            )
            .with_static_call(true)
            .execute_returns(expected);
    }
}
