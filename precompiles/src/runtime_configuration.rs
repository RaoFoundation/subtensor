use core::marker::PhantomData;

use fp_evm::ExitError;
use frame_support::traits::{Currency, Get};
use pallet_evm::{BalanceConverter, PrecompileHandle, SubstrateBalance};
use precompile_utils::{EvmResult, prelude::UnboundedString};
use sp_core::{H256, U256};
use sp_runtime::traits::AccountIdConversion;
use subtensor_runtime_common::{TaoBalance, Token};

use crate::{PrecompileExt, PrecompileHandleExt};

type SubtensorEconomicConstants = (
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
    U256,
);
type SubtensorSubnetConstants = (
    u16,
    u16,
    u16,
    u16,
    u16,
    u16,
    u16,
    u16,
    u32,
    u32,
    u8,
    u16,
    u8,
);
type SubtensorConsensusConstants = (
    u16,
    u16,
    u16,
    U256,
    u16,
    u64,
    u16,
    bool,
    u64,
    u16,
    u16,
    u64,
    u64,
);
type SubtensorRegistrationConstants = (u64, u64, u64, u16, u64, u16, u16, u64, u64, u64, u64);
type SubtensorDelegationConstants = (u16, u16, u16, u16, u16, u16, u16, bool, bool);
type SubtensorRateLimitConstants = (u64, u64, u64, u64, u64, u64, u64, u64, u64, u64, u64);
type SubtensorProtocolConstants = (
    u32,
    u32,
    u32,
    u128,
    u64,
    u64,
    u16,
    u8,
    u64,
    u64,
    u64,
    u64,
    U256,
    u32,
    U256,
);
type BalancesConstants = (U256, u32, u32, u32);
type ProxyConstants = (U256, U256, u32, u32, U256, U256);
type SchedulerConstants = (u64, u64, u32);
type DrandConstants = (UnboundedString, u64, u64, u64, u64, u64);
type CrowdloanConstants = (U256, U256, u64, u64, u32, u32, H256);
type SwapConstants = (u16, U256, U256, H256);
pub(crate) type ProxyBalanceOf<R> = <<R as pallet_subtensor_proxy::Config>::Currency as Currency<
    <R as frame_system::Config>::AccountId,
>>::Balance;

pub struct RuntimeConfigurationPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for RuntimeConfigurationPrecompile<R>
where
    R: frame_system::Config
        + pallet_admin_utils::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_drand::Config
        + pallet_evm::Config
        + pallet_evm_chain_id::Config
        + pallet_scheduler::Config
        + pallet_subtensor::Config
        + pallet_subtensor_proxy::Config
        + pallet_subtensor_swap::Config
        + pallet_timestamp::Config,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    frame_system::pallet_prelude::BlockNumberFor<R>: TryInto<u64>,
    <R as pallet_timestamp::Config>::Moment: TryInto<u64>,
    <R as pallet_balances::Config>::Balance: Into<U256>,
    ProxyBalanceOf<R>: Into<U256>,
{
    const INDEX: u64 = 2066;
}

#[precompile_utils::precompile]
impl<R> RuntimeConfigurationPrecompile<R>
where
    R: frame_system::Config
        + pallet_admin_utils::Config
        + pallet_balances::Config
        + pallet_crowdloan::Config
        + pallet_drand::Config
        + pallet_evm::Config
        + pallet_evm_chain_id::Config
        + pallet_scheduler::Config
        + pallet_subtensor::Config
        + pallet_subtensor_proxy::Config
        + pallet_subtensor_swap::Config
        + pallet_timestamp::Config,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    frame_system::pallet_prelude::BlockNumberFor<R>: TryInto<u64>,
    <R as pallet_timestamp::Config>::Moment: TryInto<u64>,
    <R as pallet_balances::Config>::Balance: Into<U256>,
    ProxyBalanceOf<R>: Into<U256>,
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

    #[precompile::public("getSubtensorEconomicConstants()")]
    #[precompile::view]
    fn get_subtensor_economic_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorEconomicConstants> {
        Ok((
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialIssuance::get())?,
            tao_to_evm::<R>(
                <R as pallet_subtensor::Config>::InitialRAORecycledForRegistration::get(),
            )?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialBurn::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialMinBurn::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialMaxBurn::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialMinStake::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialMinTransfer::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::MinBurnUpperBound::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::MaxBurnLowerBound::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::InitialNetworkMinLockCost::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::KeySwapCost::get())?,
            tao_to_evm::<R>(<R as pallet_subtensor::Config>::KeySwapOnSubnetCost::get())?,
            tao_to_evm::<R>(pallet_subtensor::pallet::MIN_BALANCE_TO_PERFORM_COLDKEY_SWAP)?,
        ))
    }

    #[precompile::public("getSubtensorSubnetConstants()")]
    #[precompile::view]
    fn get_subtensor_subnet_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorSubnetConstants> {
        Ok((
            <R as pallet_subtensor::Config>::InitialTempo::get(),
            <R as pallet_subtensor::Config>::MinTempo::get(),
            <R as pallet_subtensor::Config>::MaxTempo::get(),
            <R as pallet_subtensor::Config>::InitialMinAllowedUids::get(),
            <R as pallet_subtensor::Config>::InitialMaxAllowedUids::get(),
            <R as pallet_subtensor::Config>::InitialMaxAllowedValidators::get(),
            <R as pallet_subtensor::Config>::InitialImmunityPeriod::get(),
            <R as pallet_subtensor::Config>::InitialActivityCutoff::get(),
            <R as pallet_subtensor::Config>::MinActivityCutoffFactorMilli::get(),
            <R as pallet_subtensor::Config>::MaxActivityCutoffFactorMilli::get(),
            <R as pallet_subtensor::Config>::MaxImmuneUidsPercentage::get().deconstruct(),
            <R as pallet_subtensor::Config>::InitialSubnetOwnerCut::get(),
            <R as pallet_subtensor::Config>::InitialMaxEpochsPerBlock::get(),
        ))
    }

    #[precompile::public("getSubtensorConsensusConstants()")]
    #[precompile::view]
    fn get_subtensor_consensus_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorConsensusConstants> {
        Ok((
            <R as pallet_subtensor::Config>::InitialMinAllowedWeights::get(),
            <R as pallet_subtensor::Config>::InitialEmissionValue::get(),
            <R as pallet_subtensor::Config>::InitialRho::get(),
            signed_i16_word(<R as pallet_subtensor::Config>::InitialAlphaSigmoidSteepness::get()),
            <R as pallet_subtensor::Config>::InitialKappa::get(),
            <R as pallet_subtensor::Config>::InitialBondsMovingAverage::get(),
            <R as pallet_subtensor::Config>::InitialBondsPenalty::get(),
            <R as pallet_subtensor::Config>::InitialBondsResetOn::get(),
            <R as pallet_subtensor::Config>::InitialValidatorPruneLen::get(),
            <R as pallet_subtensor::Config>::InitialScalingLawPower::get(),
            <R as pallet_subtensor::Config>::InitialPruningScore::get(),
            <R as pallet_subtensor::Config>::InitialWeightsVersionKey::get(),
            <R as pallet_subtensor::Config>::InitialTaoWeight::get(),
        ))
    }

    #[precompile::public("getSubtensorRegistrationConstants()")]
    #[precompile::view]
    fn get_subtensor_registration_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorRegistrationConstants> {
        Ok((
            <R as pallet_subtensor::Config>::InitialDifficulty::get(),
            <R as pallet_subtensor::Config>::InitialMinDifficulty::get(),
            <R as pallet_subtensor::Config>::InitialMaxDifficulty::get(),
            <R as pallet_subtensor::Config>::InitialAdjustmentInterval::get(),
            <R as pallet_subtensor::Config>::InitialAdjustmentAlpha::get(),
            <R as pallet_subtensor::Config>::InitialMaxRegistrationsPerBlock::get(),
            <R as pallet_subtensor::Config>::InitialTargetRegistrationsPerInterval::get(),
            <R as pallet_subtensor::Config>::InitialNetworkRateLimit::get(),
            <R as pallet_subtensor::Config>::InitialNetworkImmunityPeriod::get(),
            <R as pallet_subtensor::Config>::InitialNetworkLockReductionInterval::get(),
            <R as pallet_subtensor::Config>::InitialEmaPriceHalvingPeriod::get(),
        ))
    }

    #[precompile::public("getSubtensorDelegationConstants()")]
    #[precompile::view]
    fn get_subtensor_delegation_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorDelegationConstants> {
        Ok((
            <R as pallet_subtensor::Config>::InitialDefaultDelegateTake::get(),
            <R as pallet_subtensor::Config>::InitialMinDelegateTake::get(),
            <R as pallet_subtensor::Config>::InitialDefaultChildKeyTake::get(),
            <R as pallet_subtensor::Config>::InitialMinChildKeyTake::get(),
            <R as pallet_subtensor::Config>::InitialMaxChildKeyTake::get(),
            <R as pallet_subtensor::Config>::AlphaHigh::get(),
            <R as pallet_subtensor::Config>::AlphaLow::get(),
            <R as pallet_subtensor::Config>::LiquidAlphaOn::get(),
            <R as pallet_subtensor::Config>::Yuma3On::get(),
        ))
    }

    #[precompile::public("getSubtensorRateLimitConstants()")]
    #[precompile::view]
    fn get_subtensor_rate_limit_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorRateLimitConstants> {
        Ok((
            <R as pallet_subtensor::Config>::InitialServingRateLimit::get(),
            <R as pallet_subtensor::Config>::InitialTxRateLimit::get(),
            <R as pallet_subtensor::Config>::InitialTxDelegateTakeRateLimit::get(),
            <R as pallet_subtensor::Config>::InitialTxChildKeyTakeRateLimit::get(),
            <R as pallet_subtensor::Config>::EvmKeyAssociateRateLimit::get(),
            block_to_u64(
                <R as pallet_subtensor::Config>::InitialColdkeySwapAnnouncementDelay::get(),
            )?,
            block_to_u64(
                <R as pallet_subtensor::Config>::InitialColdkeySwapReannouncementDelay::get(),
            )?,
            block_to_u64(
                <R as pallet_subtensor::Config>::InitialDissolveNetworkScheduleDuration::get(),
            )?,
            <R as pallet_subtensor::Config>::InitialStartCallDelay::get(),
            <R as pallet_subtensor::Config>::HotkeySwapOnSubnetInterval::get(),
            block_to_u64(
                <R as pallet_subtensor::Config>::LeaseDividendsDistributionInterval::get(),
            )?,
        ))
    }

    #[precompile::public("getSubtensorProtocolConstants()")]
    #[precompile::view]
    fn get_subtensor_protocol_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SubtensorProtocolConstants> {
        Ok((
            pallet_subtensor::MAX_CRV3_COMMIT_SIZE_BYTES,
            pallet_subtensor::MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS,
            pallet_subtensor::MAX_COLDKEY_COLLATERAL_HOTKEYS,
            pallet_subtensor::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA,
            pallet_subtensor::pallet::MIN_COMMIT_REVEAL_PEROIDS,
            pallet_subtensor::pallet::MAX_COMMIT_REVEAL_PEROIDS,
            pallet_subtensor::subnets::mechanism::GLOBAL_MAX_SUBNET_COUNT,
            pallet_subtensor::subnets::mechanism::MAX_MECHANISM_COUNT_PER_SUBNET,
            pallet_subtensor::utils::voting_power::VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS,
            pallet_subtensor::utils::voting_power::MAX_VOTING_POWER_EMA_ALPHA,
            pallet_subtensor::Pallet::<R>::EMISSION_BAR_UPDATE_INTERVAL,
            pallet_subtensor::staking::lock::ONE_YEAR,
            tao_u64_to_evm::<R>(pallet_subtensor::staking::lock::LOCK_STATE_ZERO_THRESHOLD)?,
            pallet_subtensor::pallet::INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI,
            tao_u64_to_evm::<R>(pallet_subtensor::coinbase::tao::MAX_TAO_ISSUANCE)?,
        ))
    }

    #[precompile::public("getSubtensorSystemAccounts()")]
    #[precompile::view]
    fn get_subtensor_system_accounts(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<(H256, H256)> {
        Ok((
            pallet_account::<R>(<R as pallet_subtensor::Config>::SubtensorPalletId::get()),
            pallet_account::<R>(<R as pallet_subtensor::Config>::BurnAccountId::get()),
        ))
    }

    #[precompile::public("getBalancesConstants()")]
    #[precompile::view]
    fn get_balances_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<BalancesConstants> {
        Ok((
            balance_to_evm::<R, _>(<R as pallet_balances::Config>::ExistentialDeposit::get())?,
            <R as pallet_balances::Config>::MaxLocks::get(),
            <R as pallet_balances::Config>::MaxReserves::get(),
            <R as pallet_balances::Config>::MaxFreezes::get(),
        ))
    }

    #[precompile::public("getProxyConstants()")]
    #[precompile::view]
    fn get_proxy_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<ProxyConstants> {
        Ok((
            balance_to_evm::<R, _>(<R as pallet_subtensor_proxy::Config>::ProxyDepositBase::get())?,
            balance_to_evm::<R, _>(
                <R as pallet_subtensor_proxy::Config>::ProxyDepositFactor::get(),
            )?,
            <R as pallet_subtensor_proxy::Config>::MaxProxies::get(),
            <R as pallet_subtensor_proxy::Config>::MaxPending::get(),
            balance_to_evm::<R, _>(
                <R as pallet_subtensor_proxy::Config>::AnnouncementDepositBase::get(),
            )?,
            balance_to_evm::<R, _>(
                <R as pallet_subtensor_proxy::Config>::AnnouncementDepositFactor::get(),
            )?,
        ))
    }

    #[precompile::public("getSchedulerConstants()")]
    #[precompile::view]
    fn get_scheduler_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<SchedulerConstants> {
        let maximum_weight = <R as pallet_scheduler::Config>::MaximumWeight::get();
        Ok((
            maximum_weight.ref_time(),
            maximum_weight.proof_size(),
            <R as pallet_scheduler::Config>::MaxScheduledPerBlock::get(),
        ))
    }

    #[precompile::public("getDrandConstants()")]
    #[precompile::view]
    fn get_drand_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<DrandConstants> {
        Ok((
            UnboundedString::from(pallet_drand::QUICKNET_CHAIN_HASH),
            <R as pallet_drand::Config>::UnsignedPriority::get(),
            <R as pallet_drand::Config>::HttpFetchTimeout::get(),
            pallet_drand::MAX_PULSES_TO_FETCH,
            pallet_drand::MAX_KEPT_PULSES,
            pallet_drand::MAX_REMOVED_PULSES,
        ))
    }

    #[precompile::public("getCrowdloanConstants()")]
    #[precompile::view]
    fn get_crowdloan_constants(
        _handle: &mut impl PrecompileHandle,
    ) -> EvmResult<CrowdloanConstants> {
        Ok((
            tao_to_evm::<R>(<R as pallet_crowdloan::Config>::MinimumDeposit::get())?,
            tao_to_evm::<R>(<R as pallet_crowdloan::Config>::AbsoluteMinimumContribution::get())?,
            block_to_u64(<R as pallet_crowdloan::Config>::MinimumBlockDuration::get())?,
            block_to_u64(<R as pallet_crowdloan::Config>::MaximumBlockDuration::get())?,
            <R as pallet_crowdloan::Config>::RefundContributorsLimit::get(),
            <R as pallet_crowdloan::Config>::MaxContributors::get(),
            pallet_account::<R>(<R as pallet_crowdloan::Config>::PalletId::get()),
        ))
    }

    #[precompile::public("getSwapConstants()")]
    #[precompile::view]
    fn get_swap_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<SwapConstants> {
        Ok((
            <R as pallet_subtensor_swap::Config>::MaxFeeRate::get(),
            tao_u64_to_evm::<R>(<R as pallet_subtensor_swap::Config>::MinimumLiquidity::get())?,
            tao_u64_to_evm::<R>(<R as pallet_subtensor_swap::Config>::MinimumReserve::get().get())?,
            pallet_account::<R>(<R as pallet_subtensor_swap::Config>::ProtocolId::get()),
        ))
    }

    #[precompile::public("getTimestampConstants()")]
    #[precompile::view]
    fn get_timestamp_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        <R as pallet_timestamp::Config>::MinimumPeriod::get()
            .try_into()
            .map_err(|_| ExitError::InvalidRange.into())
    }

    #[precompile::public("getAdminConstants()")]
    #[precompile::view]
    fn get_admin_constants(_handle: &mut impl PrecompileHandle) -> EvmResult<u32> {
        Ok(<R as pallet_admin_utils::Config>::MaxAuthorities::get())
    }
}

fn tao_to_evm<R>(value: TaoBalance) -> EvmResult<U256>
where
    R: pallet_evm::Config,
{
    tao_u64_to_evm::<R>(value.to_u64())
}

fn tao_u64_to_evm<R>(value: u64) -> EvmResult<U256>
where
    R: pallet_evm::Config,
{
    let value: SubstrateBalance = value.into();
    R::BalanceConverter::into_evm_balance(value)
        .map(|amount| amount.into_u256())
        .ok_or_else(|| ExitError::InvalidRange.into())
}

fn balance_to_evm<R, Balance>(value: Balance) -> EvmResult<U256>
where
    R: pallet_evm::Config,
    Balance: Into<U256>,
{
    let value = SubstrateBalance::new(value.into());
    R::BalanceConverter::into_evm_balance(value)
        .map(|amount| amount.into_u256())
        .ok_or_else(|| ExitError::InvalidRange.into())
}

fn block_to_u64<Block: TryInto<u64>>(block: Block) -> EvmResult<u64> {
    block.try_into().map_err(|_| ExitError::InvalidRange.into())
}

fn pallet_account<R>(pallet_id: frame_support::PalletId) -> H256
where
    R: frame_system::Config,
    R::AccountId: Into<[u8; 32]>,
{
    let account: R::AccountId = pallet_id.into_account_truncating();
    H256::from(<R::AccountId as Into<[u8; 32]>>::into(account))
}

fn signed_i16_word(value: i16) -> U256 {
    let mut encoded = [if value.is_negative() { 0xff } else { 0 }; 32];
    encoded[30..].copy_from_slice(&value.to_be_bytes());
    U256::from_big_endian(&encoded)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::mock::{Runtime, addr_from_index, new_test_ext, precompiles, selector_u32};
    use precompile_utils::{
        prelude::RuntimeHelper,
        solidity::{Codec, encode_return_value, encode_with_selector},
        testing::PrecompileTesterExt,
    };

    fn assert_view<Output: Codec>(signature: &str, expected: Output) {
        let precompiles = precompiles::<RuntimeConfigurationPrecompile<Runtime>>();
        precompiles
            .prepare_test(
                addr_from_index(1),
                addr_from_index(RuntimeConfigurationPrecompile::<Runtime>::INDEX),
                encode_with_selector(selector_u32(signature), ()),
            )
            .with_static_call(true)
            .execute_returns_raw(encode_return_value(expected));
    }

    #[test]
    fn address_storage_selectors_and_values_are_stable() {
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

    #[test]
    fn subtensor_constant_views_return_authoritative_runtime_values() {
        new_test_ext().execute_with(|| {
            assert_view(
                "getSubtensorEconomicConstants()",
                (
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialIssuance::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialRAORecycledForRegistration::get())
                        .unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialBurn::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialMinBurn::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialMaxBurn::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialMinStake::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialMinTransfer::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::MinBurnUpperBound::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::MaxBurnLowerBound::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::InitialNetworkMinLockCost::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::KeySwapCost::get()).unwrap(),
                    tao_to_evm::<Runtime>(<Runtime as pallet_subtensor::Config>::KeySwapOnSubnetCost::get()).unwrap(),
                    tao_to_evm::<Runtime>(
                        pallet_subtensor::pallet::MIN_BALANCE_TO_PERFORM_COLDKEY_SWAP,
                    )
                    .unwrap(),
                ),
            );
            assert_view(
                "getSubtensorSubnetConstants()",
                (
                    <Runtime as pallet_subtensor::Config>::InitialTempo::get(),
                    <Runtime as pallet_subtensor::Config>::MinTempo::get(),
                    <Runtime as pallet_subtensor::Config>::MaxTempo::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMinAllowedUids::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxAllowedUids::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxAllowedValidators::get(),
                    <Runtime as pallet_subtensor::Config>::InitialImmunityPeriod::get(),
                    <Runtime as pallet_subtensor::Config>::InitialActivityCutoff::get(),
                    <Runtime as pallet_subtensor::Config>::MinActivityCutoffFactorMilli::get(),
                    <Runtime as pallet_subtensor::Config>::MaxActivityCutoffFactorMilli::get(),
                    <Runtime as pallet_subtensor::Config>::MaxImmuneUidsPercentage::get().deconstruct(),
                    <Runtime as pallet_subtensor::Config>::InitialSubnetOwnerCut::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxEpochsPerBlock::get(),
                ),
            );
            assert_view(
                "getSubtensorConsensusConstants()",
                (
                    <Runtime as pallet_subtensor::Config>::InitialMinAllowedWeights::get(),
                    <Runtime as pallet_subtensor::Config>::InitialEmissionValue::get(),
                    <Runtime as pallet_subtensor::Config>::InitialRho::get(),
                    signed_i16_word(<Runtime as pallet_subtensor::Config>::InitialAlphaSigmoidSteepness::get()),
                    <Runtime as pallet_subtensor::Config>::InitialKappa::get(),
                    <Runtime as pallet_subtensor::Config>::InitialBondsMovingAverage::get(),
                    <Runtime as pallet_subtensor::Config>::InitialBondsPenalty::get(),
                    <Runtime as pallet_subtensor::Config>::InitialBondsResetOn::get(),
                    <Runtime as pallet_subtensor::Config>::InitialValidatorPruneLen::get(),
                    <Runtime as pallet_subtensor::Config>::InitialScalingLawPower::get(),
                    <Runtime as pallet_subtensor::Config>::InitialPruningScore::get(),
                    <Runtime as pallet_subtensor::Config>::InitialWeightsVersionKey::get(),
                    <Runtime as pallet_subtensor::Config>::InitialTaoWeight::get(),
                ),
            );
            assert_view(
                "getSubtensorRegistrationConstants()",
                (
                    <Runtime as pallet_subtensor::Config>::InitialDifficulty::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMinDifficulty::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxDifficulty::get(),
                    <Runtime as pallet_subtensor::Config>::InitialAdjustmentInterval::get(),
                    <Runtime as pallet_subtensor::Config>::InitialAdjustmentAlpha::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxRegistrationsPerBlock::get(),
                    <Runtime as pallet_subtensor::Config>::InitialTargetRegistrationsPerInterval::get(),
                    <Runtime as pallet_subtensor::Config>::InitialNetworkRateLimit::get(),
                    <Runtime as pallet_subtensor::Config>::InitialNetworkImmunityPeriod::get(),
                    <Runtime as pallet_subtensor::Config>::InitialNetworkLockReductionInterval::get(),
                    <Runtime as pallet_subtensor::Config>::InitialEmaPriceHalvingPeriod::get(),
                ),
            );
            assert_view(
                "getSubtensorDelegationConstants()",
                (
                    <Runtime as pallet_subtensor::Config>::InitialDefaultDelegateTake::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMinDelegateTake::get(),
                    <Runtime as pallet_subtensor::Config>::InitialDefaultChildKeyTake::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMinChildKeyTake::get(),
                    <Runtime as pallet_subtensor::Config>::InitialMaxChildKeyTake::get(),
                    <Runtime as pallet_subtensor::Config>::AlphaHigh::get(),
                    <Runtime as pallet_subtensor::Config>::AlphaLow::get(),
                    <Runtime as pallet_subtensor::Config>::LiquidAlphaOn::get(),
                    <Runtime as pallet_subtensor::Config>::Yuma3On::get(),
                ),
            );
            assert_view(
                "getSubtensorRateLimitConstants()",
                (
                    <Runtime as pallet_subtensor::Config>::InitialServingRateLimit::get(),
                    <Runtime as pallet_subtensor::Config>::InitialTxRateLimit::get(),
                    <Runtime as pallet_subtensor::Config>::InitialTxDelegateTakeRateLimit::get(),
                    <Runtime as pallet_subtensor::Config>::InitialTxChildKeyTakeRateLimit::get(),
                    <Runtime as pallet_subtensor::Config>::EvmKeyAssociateRateLimit::get(),
                    block_to_u64(<Runtime as pallet_subtensor::Config>::InitialColdkeySwapAnnouncementDelay::get()).unwrap(),
                    block_to_u64(<Runtime as pallet_subtensor::Config>::InitialColdkeySwapReannouncementDelay::get()).unwrap(),
                    block_to_u64(<Runtime as pallet_subtensor::Config>::InitialDissolveNetworkScheduleDuration::get()).unwrap(),
                    <Runtime as pallet_subtensor::Config>::InitialStartCallDelay::get(),
                    <Runtime as pallet_subtensor::Config>::HotkeySwapOnSubnetInterval::get(),
                    block_to_u64(<Runtime as pallet_subtensor::Config>::LeaseDividendsDistributionInterval::get()).unwrap(),
                ),
            );
            assert_view(
                "getSubtensorProtocolConstants()",
                (
                    pallet_subtensor::MAX_CRV3_COMMIT_SIZE_BYTES,
                    pallet_subtensor::MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS,
                    pallet_subtensor::MAX_COLDKEY_COLLATERAL_HOTKEYS,
                    pallet_subtensor::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA,
                    pallet_subtensor::pallet::MIN_COMMIT_REVEAL_PEROIDS,
                    pallet_subtensor::pallet::MAX_COMMIT_REVEAL_PEROIDS,
                    pallet_subtensor::subnets::mechanism::GLOBAL_MAX_SUBNET_COUNT,
                    pallet_subtensor::subnets::mechanism::MAX_MECHANISM_COUNT_PER_SUBNET,
                    pallet_subtensor::utils::voting_power::VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS,
                    pallet_subtensor::utils::voting_power::MAX_VOTING_POWER_EMA_ALPHA,
                    pallet_subtensor::Pallet::<Runtime>::EMISSION_BAR_UPDATE_INTERVAL,
                    pallet_subtensor::staking::lock::ONE_YEAR,
                    tao_u64_to_evm::<Runtime>(
                        pallet_subtensor::staking::lock::LOCK_STATE_ZERO_THRESHOLD,
                    )
                    .unwrap(),
                    pallet_subtensor::pallet::INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI,
                    tao_u64_to_evm::<Runtime>(pallet_subtensor::coinbase::tao::MAX_TAO_ISSUANCE)
                        .unwrap(),
                ),
            );
            assert_view(
                "getSubtensorSystemAccounts()",
                (
                    pallet_account::<Runtime>(<Runtime as pallet_subtensor::Config>::SubtensorPalletId::get()),
                    pallet_account::<Runtime>(<Runtime as pallet_subtensor::Config>::BurnAccountId::get()),
                ),
            );
        });
    }

    #[test]
    fn other_pallet_constant_views_return_authoritative_runtime_values() {
        new_test_ext().execute_with(|| {
            assert_view(
                "getBalancesConstants()",
                (
                    balance_to_evm::<Runtime, _>(
                        <Runtime as pallet_balances::Config>::ExistentialDeposit::get(),
                    )
                    .unwrap(),
                    <<Runtime as pallet_balances::Config>::MaxLocks as Get<u32>>::get(),
                    <<Runtime as pallet_balances::Config>::MaxReserves as Get<u32>>::get(),
                    <Runtime as pallet_balances::Config>::MaxFreezes::get(),
                ),
            );
            assert_view(
                "getProxyConstants()",
                (
                    balance_to_evm::<Runtime, _>(
                        <Runtime as pallet_subtensor_proxy::Config>::ProxyDepositBase::get(),
                    )
                    .unwrap(),
                    balance_to_evm::<Runtime, _>(
                        <Runtime as pallet_subtensor_proxy::Config>::ProxyDepositFactor::get(),
                    )
                    .unwrap(),
                    <Runtime as pallet_subtensor_proxy::Config>::MaxProxies::get(),
                    <Runtime as pallet_subtensor_proxy::Config>::MaxPending::get(),
                    balance_to_evm::<Runtime, _>(
                        <Runtime as pallet_subtensor_proxy::Config>::AnnouncementDepositBase::get(),
                    )
                    .unwrap(),
                    balance_to_evm::<Runtime, _>(
                        <Runtime as pallet_subtensor_proxy::Config>::AnnouncementDepositFactor::get(
                        ),
                    )
                    .unwrap(),
                ),
            );
            let maximum_weight = <Runtime as pallet_scheduler::Config>::MaximumWeight::get();
            assert_view(
                "getSchedulerConstants()",
                (
                    maximum_weight.ref_time(),
                    maximum_weight.proof_size(),
                    <Runtime as pallet_scheduler::Config>::MaxScheduledPerBlock::get(),
                ),
            );
            assert_view(
                "getDrandConstants()",
                (
                    UnboundedString::from(pallet_drand::QUICKNET_CHAIN_HASH),
                    <<Runtime as pallet_drand::Config>::UnsignedPriority as Get<u64>>::get(),
                    <<Runtime as pallet_drand::Config>::HttpFetchTimeout as Get<u64>>::get(),
                    pallet_drand::MAX_PULSES_TO_FETCH,
                    pallet_drand::MAX_KEPT_PULSES,
                    pallet_drand::MAX_REMOVED_PULSES,
                ),
            );
            assert_view(
                "getCrowdloanConstants()",
                (
                    tao_to_evm::<Runtime>(
                        <Runtime as pallet_crowdloan::Config>::MinimumDeposit::get(),
                    )
                    .unwrap(),
                    tao_to_evm::<Runtime>(
                        <Runtime as pallet_crowdloan::Config>::AbsoluteMinimumContribution::get(),
                    )
                    .unwrap(),
                    block_to_u64(
                        <Runtime as pallet_crowdloan::Config>::MinimumBlockDuration::get(),
                    )
                    .unwrap(),
                    block_to_u64(
                        <Runtime as pallet_crowdloan::Config>::MaximumBlockDuration::get(),
                    )
                    .unwrap(),
                    <Runtime as pallet_crowdloan::Config>::RefundContributorsLimit::get(),
                    <Runtime as pallet_crowdloan::Config>::MaxContributors::get(),
                    pallet_account::<Runtime>(
                        <Runtime as pallet_crowdloan::Config>::PalletId::get(),
                    ),
                ),
            );
            assert_view(
                "getSwapConstants()",
                (
                    <Runtime as pallet_subtensor_swap::Config>::MaxFeeRate::get(),
                    tao_u64_to_evm::<Runtime>(
                        <Runtime as pallet_subtensor_swap::Config>::MinimumLiquidity::get(),
                    )
                    .unwrap(),
                    tao_u64_to_evm::<Runtime>(
                        <Runtime as pallet_subtensor_swap::Config>::MinimumReserve::get().get(),
                    )
                    .unwrap(),
                    pallet_account::<Runtime>(
                        <Runtime as pallet_subtensor_swap::Config>::ProtocolId::get(),
                    ),
                ),
            );
            assert_view(
                "getTimestampConstants()",
                <Runtime as pallet_timestamp::Config>::MinimumPeriod::get(),
            );
            assert_view(
                "getAdminConstants()",
                <Runtime as pallet_admin_utils::Config>::MaxAuthorities::get(),
            );
        });
    }

    #[test]
    fn signed_i16_constants_use_solidity_sign_extension() {
        assert_eq!(signed_i16_word(1), U256::one());
        assert_eq!(signed_i16_word(-1), U256::from_big_endian(&[0xff; 32]));
        assert_eq!(
            signed_i16_word(i16::MIN),
            U256::from_big_endian(&{
                let mut word = [0xff; 32];
                word[30..].copy_from_slice(&i16::MIN.to_be_bytes());
                word
            })
        );
    }
}
