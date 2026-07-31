use core::marker::PhantomData;

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::ConstU32;
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, PrecompileHandle};
use precompile_utils::{
    EvmResult,
    prelude::{BoundedString, BoundedVec, UnboundedBytes},
};
use sp_core::H256;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, UniqueSaturatedInto};
use sp_std::{vec, vec::Vec};
use subtensor_runtime_common::{NetUid, TaoBalance, Token};

use crate::{PrecompileExt, PrecompileHandleExt};
use pallet_subtensor::subnets::dissolution::DissolveCleanupPhase;

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
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
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
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
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

    #[precompile::public("getRegisteredSubnetCounter(uint16)")]
    #[precompile::view]
    fn get_registered_subnet_counter(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::RegisteredSubnetCounter::<R>::get(
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
        let call = pallet_admin_utils::Call::<R>::sudo_set_activity_cutoff_factor {
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

    /// Return whether subnet owner-cut emission is automatically stake-locked.
    #[precompile::public("getOwnerCutAutoLockEnabled(uint16)")]
    #[precompile::view]
    fn get_owner_cut_auto_lock_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::OwnerCutAutoLockEnabled::<R>::get(
            NetUid::from(netuid),
        ))
    }

    /// Set whether subnet owner-cut emission is automatically stake-locked.
    #[precompile::public("setOwnerCutAutoLockEnabled(uint16,bool)")]
    #[precompile::payable]
    fn set_owner_cut_auto_lock_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_admin_utils::Call::<R>::sudo_set_owner_cut_auto_lock_enabled {
            netuid: NetUid::from(netuid),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
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

    #[precompile::public("isSubnetDissolving(uint16)")]
    #[precompile::view]
    fn is_subnet_dissolving(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::DissolveCleanupQueue::<R>::get().contains(&NetUid::from(netuid)))
    }

    #[precompile::public("getSubnetDissolutionStatus(uint16)")]
    #[precompile::view]
    fn get_subnet_dissolution_status(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(bool, bool, u8)> {
        handle.record_db_reads::<R>(2)?;
        let netuid = NetUid::from(netuid);
        let is_queued = pallet_subtensor::DissolveCleanupQueue::<R>::get().contains(&netuid);

        match pallet_subtensor::CurrentDissolveCleanupStatus::<R>::get() {
            Some(status) if status.netuid == netuid => {
                Ok((true, true, dissolution_cleanup_phase_code(&status.phase)))
            }
            _ => Ok((is_queued, false, 0)),
        }
    }

    #[precompile::public(
        "setSubnetIdentity(uint16,string,string,string,string,string,string,string,string)"
    )]
    #[allow(clippy::too_many_arguments)]
    fn set_subnet_identity(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        subnet_name: BoundedString<ConstU32<256>>,
        github_repo: BoundedString<ConstU32<1024>>,
        subnet_contact: BoundedString<ConstU32<1024>>,
        subnet_url: BoundedString<ConstU32<1024>>,
        discord: BoundedString<ConstU32<256>>,
        description: BoundedString<ConstU32<1024>>,
        logo_url: BoundedString<ConstU32<1024>>,
        additional: BoundedString<ConstU32<1024>>,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::set_subnet_identity {
            netuid: NetUid::from(netuid),
            subnet_name: subnet_name.into(),
            github_repo: github_repo.into(),
            subnet_contact: subnet_contact.into(),
            subnet_url: subnet_url.into(),
            discord: discord.into(),
            description: description.into(),
            logo_url: logo_url.into(),
            additional: additional.into(),
        };
        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("updateSubnetSymbol(uint16,string)")]
    fn update_subnet_symbol(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        symbol: BoundedString<ConstU32<16>>,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::update_symbol {
            netuid: NetUid::from(netuid),
            symbol: symbol.into(),
        };
        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("triggerEpoch(uint16)")]
    fn trigger_epoch(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::trigger_epoch {
            netuid: NetUid::from(netuid),
        };
        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("setBondsPenalty(uint16,uint16)")]
    fn set_bonds_penalty(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        bonds_penalty: u16,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_bonds_penalty {
                netuid: netuid.into(),
                bonds_penalty,
            },
        )
    }

    #[precompile::public("setMaxAllowedUids(uint16,uint16)")]
    fn set_max_allowed_uids(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        max_allowed_uids: u16,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_max_allowed_uids {
                netuid: netuid.into(),
                max_allowed_uids,
            },
        )
    }

    #[precompile::public("setMaxBurnV2(uint16,uint64)")]
    fn set_max_burn_v2(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        max_burn: u64,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_max_burn {
                netuid: netuid.into(),
                max_burn: TaoBalance::from(max_burn),
            },
        )
    }

    #[precompile::public("setMechanismCount(uint16,uint8)")]
    fn set_mechanism_count(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        mechanism_count: u8,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_mechanism_count {
                netuid: netuid.into(),
                mechanism_count: mechanism_count.into(),
            },
        )
    }

    #[precompile::public("setMechanismEmissionSplit(uint16,bool,uint16[])")]
    fn set_mechanism_emission_split(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        has_split: bool,
        split: BoundedVec<u16, ConstU32<256>>,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_mechanism_emission_split {
                netuid: netuid.into(),
                maybe_split: has_split.then(|| Vec::<u16>::from(split)),
            },
        )
    }

    #[precompile::public("setMinBurnV2(uint16,uint64)")]
    fn set_min_burn_v2(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        min_burn: u64,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_min_burn {
                netuid: netuid.into(),
                min_burn: TaoBalance::from(min_burn),
            },
        )
    }

    #[precompile::public("setOwnerCutEnabled(uint16,bool)")]
    fn set_owner_cut_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        enabled: bool,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_owner_cut_enabled {
                netuid: netuid.into(),
                enabled,
            },
        )
    }

    #[precompile::public("setOwnerImmuneNeuronLimit(uint16,uint16)")]
    fn set_owner_immune_neuron_limit(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        immune_neurons: u16,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_owner_immune_neuron_limit {
                netuid: netuid.into(),
                immune_neurons,
            },
        )
    }

    #[precompile::public("setTempo(uint16,uint16)")]
    fn set_tempo(handle: &mut impl PrecompileHandle, netuid: u16, tempo: u16) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_tempo {
                netuid: netuid.into(),
                tempo,
            },
        )
    }

    #[precompile::public("trimToMaxAllowedUids(uint16,uint16)")]
    fn trim_to_max_allowed_uids(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        max_n: u16,
    ) -> EvmResult<()> {
        dispatch_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_trim_to_max_allowed_uids {
                netuid: netuid.into(),
                max_n,
            },
        )
    }

    #[precompile::public("getSubnetMetadata(uint16)")]
    #[precompile::view]
    fn get_subnet_metadata(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(UnboundedBytes, H256, H256, u16, u8)> {
        handle.record_db_reads::<R>(5)?;
        let netuid = NetUid::from(netuid);
        let recycle_or_burn = match pallet_subtensor::RecycleOrBurn::<R>::get(netuid) {
            pallet_subtensor::RecycleOrBurnEnum::Burn => 0,
            pallet_subtensor::RecycleOrBurnEnum::Recycle => 1,
        };
        Ok((
            UnboundedBytes::from(pallet_subtensor::TokenSymbol::<R>::get(netuid)),
            account_to_h256(pallet_subtensor::SubnetOwner::<R>::get(netuid)),
            account_to_h256(pallet_subtensor::SubnetOwnerHotkey::<R>::get(netuid)),
            pallet_subtensor::Tempo::<R>::get(netuid),
            recycle_or_burn,
        ))
    }

    #[precompile::public("getSubnetCapacityConfig(uint16)")]
    #[precompile::view]
    fn get_subnet_capacity_config(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(u16, u16, u16, u16, u16, u16, u16, u16, bool, bool, u16, u8)> {
        handle.record_db_reads::<R>(12)?;
        let netuid = NetUid::from(netuid);
        Ok((
            pallet_subtensor::MinAllowedUids::<R>::get(netuid),
            pallet_subtensor::MaxAllowedUids::<R>::get(netuid),
            pallet_subtensor::MaxAllowedValidators::<R>::get(netuid),
            pallet_subtensor::AdjustmentInterval::<R>::get(netuid),
            pallet_subtensor::TargetRegistrationsPerInterval::<R>::get(netuid),
            pallet_subtensor::MinNonImmuneUids::<R>::get(netuid),
            pallet_subtensor::ImmuneOwnerUidsLimit::<R>::get(netuid),
            pallet_subtensor::BondsPenalty::<R>::get(netuid),
            pallet_subtensor::OwnerCutEnabled::<R>::get(netuid),
            pallet_subtensor::TransferToggle::<R>::get(netuid),
            pallet_subtensor::MaxRegistrationsPerBlock::<R>::get(netuid),
            pallet_subtensor::MechanismCountCurrent::<R>::get(netuid).into(),
        ))
    }

    #[precompile::public("getMechanismEmissionSplit(uint16)")]
    #[precompile::view]
    fn get_mechanism_emission_split(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(bool, Vec<u16>)> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            match pallet_subtensor::MechanismEmissionSplit::<R>::get(NetUid::from(netuid)) {
                Some(split) => (true, split),
                None => (false, Vec::new()),
            },
        )
    }

    #[precompile::public("getBurnConfig(uint16)")]
    #[precompile::view]
    fn get_burn_config(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<(u16, u128)> {
        handle.record_db_reads::<R>(2)?;
        let netuid = NetUid::from(netuid);
        Ok((
            pallet_subtensor::BurnHalfLife::<R>::get(netuid),
            pallet_subtensor::BurnIncreaseMult::<R>::get(netuid).to_bits(),
        ))
    }

    #[precompile::public("getGlobalNetworkLimits()")]
    #[precompile::view]
    fn get_global_network_limits(
        handle: &mut impl PrecompileHandle,
    ) -> EvmResult<(u16, u16, u16, u64, u16, u16, u64, u64, u64, u64, u64, u16)> {
        handle.record_db_reads::<R>(12)?;
        Ok((
            pallet_subtensor::MinActivityCutoff::<R>::get(),
            pallet_subtensor::AdminFreezeWindow::<R>::get(),
            pallet_subtensor::OwnerHyperparamRateLimit::<R>::get(),
            pallet_subtensor::DissolveNetworkScheduleDuration::<R>::get().unique_saturated_into(),
            pallet_subtensor::SubnetLimit::<R>::get(),
            pallet_subtensor::TotalNetworks::<R>::get(),
            pallet_subtensor::NetworkImmunityPeriod::<R>::get(),
            pallet_subtensor::StartCallDelay::<R>::get(),
            pallet_subtensor::NetworkMinLockCost::<R>::get().to_u64(),
            pallet_subtensor::NetworkLastLockCost::<R>::get().to_u64(),
            pallet_subtensor::NetworkLockReductionInterval::<R>::get(),
            pallet_subtensor::SubnetOwnerCut::<R>::get(),
        ))
    }

    #[precompile::public("getGlobalRateLimits()")]
    #[precompile::view]
    fn get_global_rate_limits(
        handle: &mut impl PrecompileHandle,
    ) -> EvmResult<(u64, u64, u64, u64, u64, u8)> {
        handle.record_db_reads::<R>(6)?;
        Ok((
            pallet_subtensor::NetworkRateLimit::<R>::get(),
            pallet_subtensor::WeightsVersionKeyRateLimit::<R>::get(),
            pallet_subtensor::TxRateLimit::<R>::get(),
            pallet_subtensor::TxDelegateTakeRateLimit::<R>::get(),
            pallet_subtensor::TxChildkeyTakeRateLimit::<R>::get(),
            pallet_subtensor::MaxEpochsPerBlock::<R>::get(),
        ))
    }

    #[precompile::public("getGlobalProtocolConfig()")]
    #[precompile::view]
    fn get_global_protocol_config(
        handle: &mut impl PrecompileHandle,
    ) -> EvmResult<(u8, u16, u64, u64)> {
        handle.record_db_reads::<R>(4)?;
        Ok((
            pallet_subtensor::MaxMechanismCount::<R>::get().into(),
            pallet_subtensor::CommitRevealWeightsVersion::<R>::get(),
            pallet_subtensor::NetworkRegistrationStartBlock::<R>::get(),
            pallet_subtensor::TaoInRefundDeploymentBlock::<R>::get(),
        ))
    }
}

fn account_to_h256<AccountId: Into<[u8; 32]>>(account: AccountId) -> H256 {
    H256::from(account.into())
}

fn dispatch_admin<R>(
    handle: &mut impl PrecompileHandle,
    call: pallet_admin_utils::Call<R>,
) -> EvmResult<()>
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
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
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
    let caller = handle.caller_account_id::<R>();
    handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
}

/// Stable, append-only EVM codes for the runtime's detailed cleanup phases.
///
/// These values intentionally do not use the Rust enum discriminant. Runtime
/// phases may be reordered internally without changing the Solidity contract.
fn dissolution_cleanup_phase_code(phase: &DissolveCleanupPhase) -> u8 {
    match phase {
        DissolveCleanupPhase::SubnetRootDividendsRootClaimable => 1,
        DissolveCleanupPhase::SubnetRootDividendsRootClaimed => 2,
        DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue => 3,
        DissolveCleanupPhase::AlphaInOutStakesSettleStakes => 4,
        DissolveCleanupPhase::AlphaInOutStakesAlpha => 5,
        DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals => 6,
        DissolveCleanupPhase::AlphaInOutStakesLocks => 7,
        DissolveCleanupPhase::AlphaInOutStakesDecayingLocks => 8,
        DissolveCleanupPhase::AlphaInOutStakes => 9,
        DissolveCleanupPhase::ProtocolLiquidity => 10,
        DissolveCleanupPhase::PurgeNetuid => 11,
        DissolveCleanupPhase::NetworkIsNetworkMember => 12,
        DissolveCleanupPhase::NetworkParameters => 13,
        DissolveCleanupPhase::NetworkMapParameters => 14,
        DissolveCleanupPhase::NetworkUpdateWeightsOnRoot => 15,
        DissolveCleanupPhase::NetworkChildkeyTake => 16,
        DissolveCleanupPhase::NetworkChildkeys => 17,
        DissolveCleanupPhase::NetworkParentkeys => 18,
        DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid => 19,
        DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch => 20,
        DissolveCleanupPhase::NetworkTransactionKeyLastBlock => 21,
        DissolveCleanupPhase::NetworkLock => 22,
        DissolveCleanupPhase::NetworkDecayingLock => 23,
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
        AccountId, Runtime, addr_from_index, assert_static_call, execute_precompile,
        mapped_account, new_test_ext, precompiles, selector_u32,
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
    fn subnet_precompile_sets_and_gets_owner_cut_auto_lock() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5003);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getOwnerCutAutoLockEnabled(uint16)"),
                        (TEST_NETUID_U16,),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    precompile_utils::prelude::RuntimeHelper::<Runtime>::db_read_gas_cost(),
                )
                .execute_returns(false);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setOwnerCutAutoLockEnabled(uint16,bool)"),
                        (TEST_NETUID_U16, true),
                    ),
                )
                .execute_returns(());
            assert!(pallet_subtensor::OwnerCutAutoLockEnabled::<Runtime>::get(
                netuid
            ));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getOwnerCutAutoLockEnabled(uint16)"),
                        (TEST_NETUID_U16,),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    precompile_utils::prelude::RuntimeHelper::<Runtime>::db_read_gas_cost(),
                )
                .execute_returns(true);
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

            pallet_subtensor::NetworkRegisteredAt::<Runtime>::remove(netuid);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getNetworkRegistrationBlock(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::zero(),
            );
        });
    }

    #[test]
    fn subnet_precompile_gets_registered_subnet_counter() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5003);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            pallet_subtensor::RegisteredSubnetCounter::<Runtime>::insert(netuid, 7);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getRegisteredSubnetCounter(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::from(7_u64),
            );

            pallet_subtensor::RegisteredSubnetCounter::<Runtime>::remove(netuid);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getRegisteredSubnetCounter(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::zero(),
            );
        });
    }

    #[test]
    fn subnet_precompile_is_subnet_dissolving() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5003);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("isSubnetDissolving(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::zero(),
            );

            pallet_subtensor::DissolveCleanupQueue::<Runtime>::set(vec![netuid]);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("isSubnetDissolving(uint16)"),
                    (TEST_NETUID_U16,),
                ),
                U256::one(),
            );
        });
    }

    #[test]
    fn subnet_precompile_reports_stable_dissolution_cleanup_status() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5003);
            let netuid = setup_owner_subnet(caller);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);
            let input = || {
                encode_with_selector(
                    selector_u32("getSubnetDissolutionStatus(uint16)"),
                    (TEST_NETUID_U16,),
                )
            };

            precompiles
                .prepare_test(caller, precompile_addr, input())
                .with_static_call(true)
                .execute_returns((false, false, 0_u8));

            pallet_subtensor::DissolveCleanupQueue::<Runtime>::set(vec![netuid]);

            precompiles
                .prepare_test(caller, precompile_addr, input())
                .with_static_call(true)
                .execute_returns((true, false, 0_u8));

            let mut status =
                pallet_subtensor::subnets::dissolution::DissolveCleanupStatus::new(netuid);
            status.set_phase(DissolveCleanupPhase::AlphaInOutStakesSettleStakes);
            pallet_subtensor::CurrentDissolveCleanupStatus::<Runtime>::set(Some(status));

            precompiles
                .prepare_test(caller, precompile_addr, input())
                .with_static_call(true)
                .execute_returns((true, true, 4_u8));
        });
    }

    #[test]
    fn dissolution_cleanup_phase_codes_are_stable() {
        let phases = [
            (DissolveCleanupPhase::SubnetRootDividendsRootClaimable, 1),
            (DissolveCleanupPhase::SubnetRootDividendsRootClaimed, 2),
            (DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue, 3),
            (DissolveCleanupPhase::AlphaInOutStakesSettleStakes, 4),
            (DissolveCleanupPhase::AlphaInOutStakesAlpha, 5),
            (DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals, 6),
            (DissolveCleanupPhase::AlphaInOutStakesLocks, 7),
            (DissolveCleanupPhase::AlphaInOutStakesDecayingLocks, 8),
            (DissolveCleanupPhase::AlphaInOutStakes, 9),
            (DissolveCleanupPhase::ProtocolLiquidity, 10),
            (DissolveCleanupPhase::PurgeNetuid, 11),
            (DissolveCleanupPhase::NetworkIsNetworkMember, 12),
            (DissolveCleanupPhase::NetworkParameters, 13),
            (DissolveCleanupPhase::NetworkMapParameters, 14),
            (DissolveCleanupPhase::NetworkUpdateWeightsOnRoot, 15),
            (DissolveCleanupPhase::NetworkChildkeyTake, 16),
            (DissolveCleanupPhase::NetworkChildkeys, 17),
            (DissolveCleanupPhase::NetworkParentkeys, 18),
            (DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid, 19),
            (DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch, 20),
            (DissolveCleanupPhase::NetworkTransactionKeyLastBlock, 21),
            (DissolveCleanupPhase::NetworkLock, 22),
            (DissolveCleanupPhase::NetworkDecayingLock, 23),
        ];

        for (phase, expected) in phases {
            assert_eq!(dissolution_cleanup_phase_code(&phase), expected);
        }
    }

    #[test]
    fn subnet_state_views_return_grouped_runtime_configuration() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5020);
            let netuid = setup_owner_subnet(caller);
            let address = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);
            let precompiles = precompiles::<SubnetPrecompile<Runtime>>();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getSubnetMetadata(uint16)"),
                        (TEST_NETUID_U16,),
                    ),
                )
                .with_static_call(true)
                .execute_returns((
                    UnboundedBytes::from(pallet_subtensor::TokenSymbol::<Runtime>::get(netuid)),
                    account_to_h256(pallet_subtensor::SubnetOwner::<Runtime>::get(netuid)),
                    account_to_h256(pallet_subtensor::SubnetOwnerHotkey::<Runtime>::get(netuid)),
                    pallet_subtensor::Tempo::<Runtime>::get(netuid),
                    0_u8,
                ));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getSubnetCapacityConfig(uint16)"),
                        (TEST_NETUID_U16,),
                    ),
                )
                .with_static_call(true)
                .execute_returns((
                    pallet_subtensor::MinAllowedUids::<Runtime>::get(netuid),
                    pallet_subtensor::MaxAllowedUids::<Runtime>::get(netuid),
                    pallet_subtensor::MaxAllowedValidators::<Runtime>::get(netuid),
                    pallet_subtensor::AdjustmentInterval::<Runtime>::get(netuid),
                    pallet_subtensor::TargetRegistrationsPerInterval::<Runtime>::get(netuid),
                    pallet_subtensor::MinNonImmuneUids::<Runtime>::get(netuid),
                    pallet_subtensor::ImmuneOwnerUidsLimit::<Runtime>::get(netuid),
                    pallet_subtensor::BondsPenalty::<Runtime>::get(netuid),
                    pallet_subtensor::OwnerCutEnabled::<Runtime>::get(netuid),
                    pallet_subtensor::TransferToggle::<Runtime>::get(netuid),
                    pallet_subtensor::MaxRegistrationsPerBlock::<Runtime>::get(netuid),
                    u8::from(pallet_subtensor::MechanismCountCurrent::<Runtime>::get(
                        netuid,
                    )),
                ));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getMechanismEmissionSplit(uint16)"),
                        (TEST_NETUID_U16,),
                    ),
                )
                .with_static_call(true)
                .execute_returns((false, Vec::<u16>::new()));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getBurnConfig(uint16)"), (TEST_NETUID_U16,)),
                )
                .with_static_call(true)
                .execute_returns((
                    pallet_subtensor::BurnHalfLife::<Runtime>::get(netuid),
                    pallet_subtensor::BurnIncreaseMult::<Runtime>::get(netuid).to_bits(),
                ));

            let dissolve_schedule_duration: u64 =
                pallet_subtensor::DissolveNetworkScheduleDuration::<Runtime>::get()
                    .unique_saturated_into();
            precompiles
                .prepare_test(
                    caller,
                    address,
                    selector_u32("getGlobalNetworkLimits()")
                        .to_be_bytes()
                        .to_vec(),
                )
                .with_static_call(true)
                .execute_returns((
                    pallet_subtensor::MinActivityCutoff::<Runtime>::get(),
                    pallet_subtensor::AdminFreezeWindow::<Runtime>::get(),
                    pallet_subtensor::OwnerHyperparamRateLimit::<Runtime>::get(),
                    dissolve_schedule_duration,
                    pallet_subtensor::SubnetLimit::<Runtime>::get(),
                    pallet_subtensor::TotalNetworks::<Runtime>::get(),
                    pallet_subtensor::NetworkImmunityPeriod::<Runtime>::get(),
                    pallet_subtensor::StartCallDelay::<Runtime>::get(),
                    pallet_subtensor::NetworkMinLockCost::<Runtime>::get().to_u64(),
                    pallet_subtensor::NetworkLastLockCost::<Runtime>::get().to_u64(),
                    pallet_subtensor::NetworkLockReductionInterval::<Runtime>::get(),
                    pallet_subtensor::SubnetOwnerCut::<Runtime>::get(),
                ));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    selector_u32("getGlobalRateLimits()").to_be_bytes().to_vec(),
                )
                .with_static_call(true)
                .execute_returns((
                    pallet_subtensor::NetworkRateLimit::<Runtime>::get(),
                    pallet_subtensor::WeightsVersionKeyRateLimit::<Runtime>::get(),
                    pallet_subtensor::TxRateLimit::<Runtime>::get(),
                    pallet_subtensor::TxDelegateTakeRateLimit::<Runtime>::get(),
                    pallet_subtensor::TxChildkeyTakeRateLimit::<Runtime>::get(),
                    pallet_subtensor::MaxEpochsPerBlock::<Runtime>::get(),
                ));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    selector_u32("getGlobalProtocolConfig()")
                        .to_be_bytes()
                        .to_vec(),
                )
                .with_static_call(true)
                .execute_returns((
                    u8::from(pallet_subtensor::MaxMechanismCount::<Runtime>::get()),
                    pallet_subtensor::CommitRevealWeightsVersion::<Runtime>::get(),
                    pallet_subtensor::NetworkRegistrationStartBlock::<Runtime>::get(),
                    pallet_subtensor::TaoInRefundDeploymentBlock::<Runtime>::get(),
                ));
        });
    }

    #[test]
    fn added_admin_call_preserves_subnet_owner_authorization() {
        new_test_ext().execute_with(|| {
            let owner = addr_from_index(0x5010);
            let non_owner = addr_from_index(0x5011);
            let netuid = setup_owner_subnet(owner);
            let address = addr_from_index(SubnetPrecompile::<Runtime>::INDEX);
            let input = encode_with_selector(
                selector_u32("setBondsPenalty(uint16,uint16)"),
                (TEST_NETUID_U16, 123u16),
            );

            precompiles::<SubnetPrecompile<Runtime>>()
                .prepare_test(owner, address, input.clone())
                .execute_returns(());
            assert_eq!(pallet_subtensor::BondsPenalty::<Runtime>::get(netuid), 123);

            let rejected = execute_precompile(
                &precompiles::<SubnetPrecompile<Runtime>>(),
                address,
                non_owner,
                input,
                U256::zero(),
            );
            assert!(matches!(rejected, Some(Err(_))));
            assert_eq!(pallet_subtensor::BondsPenalty::<Runtime>::get(netuid), 123);
        });
    }
}
