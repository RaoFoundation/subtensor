use core::marker::PhantomData;

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::ConstU32;
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, PrecompileHandle};
use precompile_utils::{EvmResult, prelude::BoundedString};
use sp_core::H256;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};
use sp_std::vec;
use subtensor_runtime_common::{NetUid, Token};

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct SubnetPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for SubnetPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_admin_utils::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_admin_utils::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2051;
}

#[precompile_utils::precompile]
impl<R> SubnetPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_admin_utils::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_admin_utils::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("registerNetwork(bytes32)")]
    #[precompile::payable]
    fn register_network(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<()> {
        let hotkey = R::AccountId::from(hotkey.0);
        let call = pallet_subtensor::Call::<R>::register_network_with_identity {
            hotkey,
            identity: None,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public(
        "registerNetwork(bytes32,string,string,string,string,string,string,string)"
    )]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn register_network_with_identity(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        subnet_name: BoundedString<ConstU32<256>>,
        github_repo: BoundedString<ConstU32<1024>>,
        subnet_contact: BoundedString<ConstU32<1024>>,
        subnet_url: BoundedString<ConstU32<1024>>,
        discord: BoundedString<ConstU32<256>>,
        description: BoundedString<ConstU32<1024>>,
        additional: BoundedString<ConstU32<1024>>,
    ) -> EvmResult<()> {
        let hotkey = R::AccountId::from(hotkey.0);
        let identity = pallet_subtensor::SubnetIdentityOfV3 {
            subnet_name: subnet_name.into(),
            github_repo: github_repo.into(),
            subnet_contact: subnet_contact.into(),
            subnet_url: subnet_url.into(),
            discord: discord.into(),
            description: description.into(),
            logo_url: vec![],
            additional: additional.into(),
        };

        let call = pallet_subtensor::Call::<R>::register_network_with_identity {
            hotkey,
            identity: Some(identity),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public(
        "registerNetwork(bytes32,string,string,string,string,string,string,string,string)"
    )]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn register_network_with_identity_v2(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        subnet_name: BoundedString<ConstU32<256>>,
        github_repo: BoundedString<ConstU32<1024>>,
        subnet_contact: BoundedString<ConstU32<1024>>,
        subnet_url: BoundedString<ConstU32<1024>>,
        discord: BoundedString<ConstU32<256>>,
        description: BoundedString<ConstU32<1024>>,
        additional: BoundedString<ConstU32<1024>>,
        logo_url: BoundedString<ConstU32<1024>>,
    ) -> EvmResult<()> {
        let hotkey = R::AccountId::from(hotkey.0);
        let identity = pallet_subtensor::SubnetIdentityOfV3 {
            subnet_name: subnet_name.into(),
            github_repo: github_repo.into(),
            subnet_contact: subnet_contact.into(),
            subnet_url: subnet_url.into(),
            discord: discord.into(),
            description: description.into(),
            logo_url: logo_url.into(),
            additional: additional.into(),
        };

        let call = pallet_subtensor::Call::<R>::register_network_with_identity {
            hotkey,
            identity: Some(identity),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getNetworkRegistrationBlock(uint16)")]
    #[precompile::view]
    fn get_network_registration_block(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::NetworkRegisteredAt::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("getServingRateLimit(uint16)")]
    #[precompile::view]
    fn get_serving_rate_limit(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ServingRateLimit::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setServingRateLimit(uint16,uint64)")]
    #[precompile::payable]
    fn set_serving_rate_limit(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        serving_rate_limit: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_serving_rate_limit {
            netuid: netuid.into(),
            serving_rate_limit,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getMinDifficulty(uint16)")]
    #[precompile::view]
    fn get_min_difficulty(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::MinDifficulty::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setMinDifficulty(uint16,uint64)")]
    #[precompile::payable]
    fn set_min_difficulty(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        min_difficulty: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_min_difficulty {
            netuid: netuid.into(),
            min_difficulty,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getMaxDifficulty(uint16)")]
    #[precompile::view]
    fn get_max_difficulty(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::MaxDifficulty::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setMaxDifficulty(uint16,uint64)")]
    #[precompile::payable]
    fn set_max_difficulty(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        max_difficulty: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_max_difficulty {
            netuid: netuid.into(),
            max_difficulty,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getWeightsVersionKey(uint16)")]
    #[precompile::view]
    fn get_weights_version_key(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::WeightsVersionKey::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setWeightsVersionKey(uint16,uint64)")]
    #[precompile::payable]
    fn set_weights_version_key(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        weights_version_key: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_weights_version_key {
            netuid: netuid.into(),
            weights_version_key,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getWeightsSetRateLimit(uint16)")]
    #[precompile::view]
    fn get_weights_set_rate_limit(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::WeightsSetRateLimit::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setWeightsSetRateLimit(uint16,uint64)")]
    #[precompile::payable]
    fn set_weights_set_rate_limit(
        _handle: &mut impl PrecompileHandle,
        _netuid: u16,
        _weights_set_rate_limit: u64,
    ) -> EvmResult<()> {
        // DEPRECATED. Subnet owner cannot set weight setting rate limits
        Ok(())
    }

    #[precompile::public("getAdjustmentAlpha(uint16)")]
    #[precompile::view]
    fn get_adjustment_alpha(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::AdjustmentAlpha::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setAdjustmentAlpha(uint16,uint64)")]
    #[precompile::payable]
    fn set_adjustment_alpha(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        adjustment_alpha: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_adjustment_alpha {
            netuid: netuid.into(),
            adjustment_alpha,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getMaxWeightLimit(uint16)")]
    #[precompile::view]
    fn get_max_weight_limit(_: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        Ok(pallet_subtensor::Pallet::<R>::get_max_weight_limit(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("getImmunityPeriod(uint16)")]
    #[precompile::view]
    fn get_immunity_period(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ImmunityPeriod::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setImmunityPeriod(uint16,uint16)")]
    #[precompile::payable]
    fn set_immunity_period(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        immunity_period: u16,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_immunity_period {
            netuid: netuid.into(),
            immunity_period,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getMinAllowedWeights(uint16)")]
    #[precompile::view]
    fn get_min_allowed_weights(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::MinAllowedWeights::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setMinAllowedWeights(uint16,uint16)")]
    #[precompile::payable]
    fn set_min_allowed_weights(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        min_allowed_weights: u16,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_min_allowed_weights {
            netuid: netuid.into(),
            min_allowed_weights,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getKappa(uint16)")]
    #[precompile::view]
    fn get_kappa(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Kappa::<R>::get(NetUid::from(netuid)))
    }

    #[precompile::public("setKappa(uint16,uint16)")]
    #[precompile::payable]
    fn set_kappa(handle: &mut impl PrecompileHandle, netuid: u16, kappa: u16) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_kappa {
            netuid: netuid.into(),
            kappa,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getRho(uint16)")]
    #[precompile::view]
    fn get_rho(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Rho::<R>::get(NetUid::from(netuid)))
    }

    #[precompile::public("getAlphaSigmoidSteepness(uint16)")]
    #[precompile::view]
    fn get_alpha_sigmoid_steepness(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::AlphaSigmoidSteepness::<R>::get(NetUid::from(netuid)) as u16)
    }

    #[precompile::public("setRho(uint16,uint16)")]
    #[precompile::payable]
    fn set_rho(handle: &mut impl PrecompileHandle, netuid: u16, rho: u16) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_rho {
            netuid: netuid.into(),
            rho,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("setAlphaSigmoidSteepness(uint16,uint16)")]
    #[precompile::payable]
    fn set_alpha_sigmoid_steepness(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        steepness: u16,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_alpha_sigmoid_steepness {
            netuid: netuid.into(),
            steepness: (steepness as i16),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getActivityCutoff(uint16)")]
    #[precompile::view]
    fn get_activity_cutoff(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ActivityCutoff::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setActivityCutoff(uint16,uint16)")]
    #[precompile::payable]
    fn set_activity_cutoff(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        activity_cutoff: u16,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_activity_cutoff {
            netuid: netuid.into(),
            activity_cutoff,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getActivityCutoffFactor(uint16)")]
    #[precompile::view]
    fn get_activity_cutoff_factor(_: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u32> {
        Ok(pallet_subtensor::ActivityCutoffFactorMilli::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setActivityCutoffFactor(uint16,uint32)")]
    #[precompile::payable]
    fn set_activity_cutoff_factor(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        factor_milli: u32,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::set_activity_cutoff_factor {
            netuid: netuid.into(),
            factor_milli,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getNetworkRegistrationAllowed(uint16)")]
    #[precompile::view]
    fn get_network_registration_allowed(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::NetworkRegistrationAllowed::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setNetworkRegistrationAllowed(uint16,bool)")]
    #[precompile::payable]
    fn set_network_registration_allowed(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        registration_allowed: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_network_registration_allowed {
            netuid: netuid.into(),
            registration_allowed,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getNetworkPowRegistrationAllowed(uint16)")]
    #[precompile::view]
    fn get_network_pow_registration_allowed(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::NetworkPowRegistrationAllowed::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setNetworkPowRegistrationAllowed(uint16,bool)")]
    #[precompile::payable]
    fn set_network_pow_registration_allowed(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        registration_allowed: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_network_pow_registration_allowed {
            netuid: netuid.into(),
            registration_allowed,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getMinBurn(uint16)")]
    #[precompile::view]
    fn get_min_burn(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::MinBurn::<R>::get(NetUid::from(netuid)).to_u64())
    }

    #[precompile::public("setMinBurn(uint16,uint64)")]
    #[precompile::payable]
    fn set_min_burn(
        _handle: &mut impl PrecompileHandle,
        _netuid: u16,
        _min_burn: u64,
    ) -> EvmResult<()> {
        // DEPRECATED. The subnet owner cannot set the min burn anymore.
        Ok(())
    }

    #[precompile::public("getMaxBurn(uint16)")]
    #[precompile::view]
    fn get_max_burn(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::MaxBurn::<R>::get(NetUid::from(netuid)).to_u64())
    }

    #[precompile::public("setMaxBurn(uint16,uint64)")]
    #[precompile::payable]
    fn set_max_burn(
        _handle: &mut impl PrecompileHandle,
        _netuid: u16,
        _max_burn: u64,
    ) -> EvmResult<()> {
        // DEPRECATED. The subnet owner cannot set the max burn anymore.
        Ok(())
    }

    #[precompile::public("getDifficulty(uint16)")]
    #[precompile::view]
    fn get_difficulty(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Difficulty::<R>::get(NetUid::from(netuid)))
    }

    #[precompile::public("setDifficulty(uint16,uint64)")]
    #[precompile::payable]
    fn set_difficulty(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        difficulty: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_difficulty {
            netuid: netuid.into(),
            difficulty,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getBondsMovingAverage(uint16)")]
    #[precompile::view]
    fn get_bonds_moving_average(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::BondsMovingAverage::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setBondsMovingAverage(uint16,uint64)")]
    #[precompile::payable]
    fn set_bonds_moving_average(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        bonds_moving_average: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_bonds_moving_average {
            netuid: netuid.into(),
            bonds_moving_average,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getCommitRevealWeightsEnabled(uint16)")]
    #[precompile::view]
    fn get_commit_reveal_weights_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::CommitRevealWeightsEnabled::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setCommitRevealWeightsEnabled(uint16,bool)")]
    #[precompile::payable]
    fn set_commit_reveal_weights_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_commit_reveal_weights_enabled {
            netuid: netuid.into(),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getLiquidAlphaEnabled(uint16)")]
    #[precompile::view]
    fn get_liquid_alpha_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::LiquidAlphaOn::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setLiquidAlphaEnabled(uint16,bool)")]
    #[precompile::payable]
    fn set_liquid_alpha_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_liquid_alpha_enabled {
            netuid: netuid.into(),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getYuma3Enabled(uint16)")]
    #[precompile::view]
    fn get_yuma3_enabled(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Yuma3On::<R>::get(NetUid::from(netuid)))
    }

    #[precompile::public("getBondsResetEnabled(uint16)")]
    #[precompile::view]
    fn get_bonds_reset_enabled(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::BondsResetOn::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setYuma3Enabled(uint16,bool)")]
    #[precompile::payable]
    fn set_yuma3_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_yuma3_enabled {
            netuid: netuid.into(),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("setBondsResetEnabled(uint16,bool)")]
    #[precompile::payable]
    fn set_bonds_reset_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_bonds_reset_enabled {
            netuid: netuid.into(),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getAlphaValues(uint16)")]
    #[precompile::view]
    fn get_alpha_values(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<(u16, u16)> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::AlphaValues::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("setAlphaValues(uint16,uint16,uint16)")]
    #[precompile::payable]
    fn set_alpha_values(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        alpha_low: u16,
        alpha_high: u16,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_alpha_values {
            netuid: netuid.into(),
            alpha_low,
            alpha_high,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("getCommitRevealWeightsInterval(uint16)")]
    #[precompile::view]
    fn get_commit_reveal_weights_interval(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::RevealPeriodEpochs::<R>::get(
            NetUid::from(netuid),
        ))
    }

    #[precompile::public("setCommitRevealWeightsInterval(uint16,uint64)")]
    #[precompile::payable]
    fn set_commit_reveal_weights_interval(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        interval: u64,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_commit_reveal_weights_interval {
            netuid: netuid.into(),
            interval,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("toggleTransfers(uint16,bool)")]
    #[precompile::payable]
    fn toggle_transfers(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        toggle: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_toggle_transfer {
            netuid: netuid.into(),
            toggle,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::unwrap_used
    )]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, assert_static_call, mapped_account, new_test_ext,
        precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::{H160, H256, U256};
    use subtensor_runtime_common::TaoBalance;

    const TEST_NETUID_U16: u16 = 1;
    const TEST_TEMPO: u16 = 100;

    fn setup_owner_subnet(caller: H160) -> NetUid {
        let netuid = NetUid::from(TEST_NETUID_U16);
        let owner = mapped_account(caller);
        let owner_hotkey = AccountId::from([0x55; 32]);

        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, TEST_TEMPO);
        pallet_subtensor::SubnetOwner::<Runtime>::insert(netuid, owner);
        pallet_subtensor::SubnetOwnerHotkey::<Runtime>::insert(netuid, owner_hotkey);
        pallet_subtensor::AdminFreezeWindow::<Runtime>::set(0);
        pallet_subtensor::OwnerHyperparamRateLimit::<Runtime>::set(0);

        netuid
    }

    fn add_balance_to_coldkey_account(coldkey: &sp_core::crypto::AccountId32, tao: TaoBalance) {
        let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(tao);
        let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(coldkey, credit, tao).unwrap();
    }

    #[test]
    fn subnet_precompile_registers_network_without_identity() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5000);
            let caller_account = mapped_account(caller);
            let hotkey = AccountId::from([0x44; 32]);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            add_balance_to_coldkey_account(&caller_account, 1_000_000_000_000_u64.into());

            let total_before = pallet_subtensor::TotalNetworks::<Runtime>::get();
            let netuid = pallet_subtensor::Pallet::<Runtime>::get_next_netuid();
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("registerNetwork(bytes32)"),
                        (H256::from_slice(hotkey.as_ref()),),
                    ),
                )
                .execute_returns(());

            let total_after = pallet_subtensor::TotalNetworks::<Runtime>::get();
            assert_eq!(total_after, total_before + 1);
            assert_eq!(
                pallet_subtensor::SubnetOwner::<Runtime>::get(netuid),
                caller_account
            );
            assert!(!pallet_subtensor::SubnetIdentitiesV3::<Runtime>::contains_key(netuid));
        });
    }

    #[test]
    fn subnet_precompile_registers_network_with_identity() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5002);
            let caller_account = mapped_account(caller);
            let hotkey = AccountId::from([0x45; 32]);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            add_balance_to_coldkey_account(
                &caller_account,
                1_000_000_000_000_u64.into(),
            );

            let total_before = pallet_subtensor::TotalNetworks::<Runtime>::get();
            let netuid = pallet_subtensor::Pallet::<Runtime>::get_next_netuid();
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32(
                            "registerNetwork(bytes32,string,string,string,string,string,string,string)",
                        ),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            precompile_utils::solidity::codec::UnboundedString::from("name"),
                            precompile_utils::solidity::codec::UnboundedString::from("repo"),
                            precompile_utils::solidity::codec::UnboundedString::from("contact"),
                            precompile_utils::solidity::codec::UnboundedString::from("subnetUrl"),
                            precompile_utils::solidity::codec::UnboundedString::from("discord"),
                            precompile_utils::solidity::codec::UnboundedString::from("description"),
                            precompile_utils::solidity::codec::UnboundedString::from("additional"),
                        ),
                    ),
                )
                .execute_returns(());

            let total_after = pallet_subtensor::TotalNetworks::<Runtime>::get();
            assert_eq!(total_after, total_before + 1);
            assert_eq!(pallet_subtensor::SubnetOwner::<Runtime>::get(netuid), caller_account);
            assert!(pallet_subtensor::SubnetIdentitiesV3::<Runtime>::contains_key(netuid));
        });
    }

    #[test]
    fn subnet_precompile_sets_and_gets_owner_hyperparameters() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5001);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setServingRateLimit(uint16,uint64)"),
                        (TEST_NETUID_U16, 100_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::ServingRateLimit::<Runtime>::get(netuid),
                100
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getServingRateLimit(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(100_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setMaxDifficulty(uint16,uint64)"),
                        (TEST_NETUID_U16, 102_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(pallet_subtensor::MaxDifficulty::<Runtime>::get(netuid), 102);
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getMaxDifficulty(uint16)"), (TEST_NETUID_U16,)),
                U256::from(102_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setWeightsVersionKey(uint16,uint64)"),
                        (TEST_NETUID_U16, 103_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::WeightsVersionKey::<Runtime>::get(netuid),
                103
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getWeightsVersionKey(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(103_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setAdjustmentAlpha(uint16,uint64)"),
                        (TEST_NETUID_U16, 105_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::AdjustmentAlpha::<Runtime>::get(netuid),
                105
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getAdjustmentAlpha(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(105_u64),
            );

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getMaxWeightLimit(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(0xFFFF_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setImmunityPeriod(uint16,uint16)"),
                        (TEST_NETUID_U16, 107_u16),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::ImmunityPeriod::<Runtime>::get(netuid),
                107
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getImmunityPeriod(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(107_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setMinAllowedWeights(uint16,uint16)"),
                        (TEST_NETUID_U16, 108_u16),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::MinAllowedWeights::<Runtime>::get(netuid),
                108
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getMinAllowedWeights(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(108_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setRho(uint16,uint16)"),
                        (TEST_NETUID_U16, 110_u16),
                    ),
                )
                .execute_returns(());
            assert_eq!(pallet_subtensor::Rho::<Runtime>::get(netuid), 110);
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getRho(uint16)"), (TEST_NETUID_U16,)),
                U256::from(110_u64),
            );

            let activity_cutoff = pallet_subtensor::MinActivityCutoff::<Runtime>::get() + 1;
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setActivityCutoff(uint16,uint16)"),
                        (TEST_NETUID_U16, activity_cutoff),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::ActivityCutoff::<Runtime>::get(netuid),
                activity_cutoff
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getActivityCutoff(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(activity_cutoff),
            );

            let factor_milli: u32 = 1_500;
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setActivityCutoffFactor(uint16,uint32)"),
                        (TEST_NETUID_U16, factor_milli),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::ActivityCutoffFactorMilli::<Runtime>::get(netuid),
                factor_milli
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getActivityCutoffFactor(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(factor_milli),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setBondsMovingAverage(uint16,uint64)"),
                        (TEST_NETUID_U16, 115_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::BondsMovingAverage::<Runtime>::get(netuid),
                115
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getBondsMovingAverage(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(115_u64),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setCommitRevealWeightsEnabled(uint16,bool)"),
                        (TEST_NETUID_U16, true),
                    ),
                )
                .execute_returns(());
            assert!(pallet_subtensor::CommitRevealWeightsEnabled::<Runtime>::get(netuid));
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getCommitRevealWeightsEnabled(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::one(),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setLiquidAlphaEnabled(uint16,bool)"),
                        (TEST_NETUID_U16, true),
                    ),
                )
                .execute_returns(());
            assert!(pallet_subtensor::LiquidAlphaOn::<Runtime>::get(netuid));
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getLiquidAlphaEnabled(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::one(),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setYuma3Enabled(uint16,bool)"),
                        (TEST_NETUID_U16, true),
                    ),
                )
                .execute_returns(());
            assert!(pallet_subtensor::Yuma3On::<Runtime>::get(netuid));
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getYuma3Enabled(uint16)"), (TEST_NETUID_U16,)),
                U256::one(),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setCommitRevealWeightsInterval(uint16,uint64)"),
                        (TEST_NETUID_U16, 99_u64),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::RevealPeriodEpochs::<Runtime>::get(netuid),
                99
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getCommitRevealWeightsInterval(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(99_u64),
            );
        });
    }

    #[test]
    fn subnet_precompile_gets_network_registered_block() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5003);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            let registration_block: u64 = 42;
            pallet_subtensor::NetworkRegisteredAt::<Runtime>::insert(netuid, registration_block);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getNetworkRegistrationBlock(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(registration_block),
            );
        });
    }
}
