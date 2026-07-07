use core::marker::PhantomData;

use crate::PrecompileExt;
use fp_evm::{ExitError, PrecompileFailure};
use pallet_evm::{BalanceConverter, PrecompileHandle, SubstrateBalance};
use precompile_utils::EvmResult;
use sp_runtime::{SaturatedConversion, Vec};

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
        + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
{
    const INDEX: u64 = 2056;
}

#[precompile_utils::precompile]
impl<R> AlphaPrecompile<R>
where
    R: frame_system::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_evm::Config,
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
}
