#![cfg_attr(not(feature = "std"), no_std)]

// extern crate alloc;

use frame_system::pallet_prelude::BlockNumberFor;
pub use pallet::*;
// - we could replace it with Vec<(AuthorityId, u64)>, but we would need
//   `sp_consensus_grandpa` for `AuthorityId` anyway
// - we could use a type parameter for `AuthorityId`, but there is
//   no sense for this as GRANDPA's `AuthorityId` is not a parameter -- it's always the same
use sp_consensus_grandpa::AuthorityList;
use sp_runtime::{DispatchResult, RuntimeAppPublic, Vec, traits::Member};

mod benchmarking;
pub mod weights;
pub use weights::WeightInfo;

#[cfg(test)]
pub(crate) mod tests;

#[deny(missing_docs)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::weights::WeightInfo;
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_support::traits::tokens::Balance;
    use frame_support::{dispatch::DispatchResult, pallet_prelude::StorageMap};
    use frame_system::pallet_prelude::*;
    use pallet_evm_chain_id::{self, ChainId};
    use pallet_subtensor::{
        DefaultMaxAllowedUids,
        utils::rate_limiting::{Hyperparameter, TransactionType},
    };
    use sp_runtime::BoundedVec;
    use substrate_fixed::types::{I64F64, I96F32, U64F64};
    use subtensor_runtime_common::{MechId, NetUid, TaoBalance};

    /// The main data structure of the module.
    #[pallet::pallet]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(_);

    /// Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + pallet_subtensor::pallet::Config
        + pallet_evm_chain_id::pallet::Config
    {
        /// Implementation of the AuraInterface
        type Aura: crate::AuraInterface<<Self as Config>::AuthorityId, Self::MaxAuthorities>;

        /// Implementation of [`GrandpaInterface`]
        type Grandpa: crate::GrandpaInterface<Self>;

        /// The identifier type for an authority.
        type AuthorityId: Member
            + Parameter
            + RuntimeAppPublic
            + MaybeSerializeDeserialize
            + MaxEncodedLen;

        /// The maximum number of authorities that the pallet can hold.
        type MaxAuthorities: Get<u32>;

        /// Unit of assets
        type Balance: Balance;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Event emitted when a precompile operation is updated.
        PrecompileUpdated {
            /// The type of precompile operation being updated.
            precompile_id: PrecompileEnum,
            /// Indicates if the precompile operation is enabled or not.
            enabled: bool,
        },
        /// Event emitted when the Yuma3 enable is toggled.
        Yuma3EnableToggled {
            /// The network identifier.
            netuid: NetUid,
            /// Indicates if the Yuma3 enable was enabled or disabled.
            enabled: bool,
        },
        /// Event emitted when Bonds Reset is toggled.
        BondsResetToggled {
            /// The network identifier.
            netuid: NetUid,
            /// Indicates if the Bonds Reset was enabled or disabled.
            enabled: bool,
        },
        /// Event emitted when the burn half-life parameter is set for a subnet.
        BurnHalfLifeSet {
            /// The network identifier.
            netuid: NetUid,
            /// The new burn half-life value.
            burn_half_life: u16,
        },
        /// Event emitted when the burn increase multiplier is set for a subnet.
        BurnIncreaseMultSet {
            /// The network identifier.
            netuid: NetUid,
            /// The new burn increase multiplier.
            burn_increase_mult: U64F64,
        },

        /// Pool-side subnet emission injections and chain buys were enabled or disabled.
        SubnetEmissionEnabledSet {
            /// The network identifier.
            netuid: NetUid,
            /// Whether pool-side emission injections and chain buys are enabled.
            enabled: bool,
        },
    }

    // Errors inform users that something went wrong.
    #[pallet::error]
    pub enum Error<T> {
        /// The subnet does not exist, check the netuid parameter
        SubnetDoesNotExist,
        /// The maximum number of subnet validators must be less than the maximum number of allowed UIDs in the subnet.
        MaxValidatorsLargerThanMaxUIds,
        /// The maximum number of subnet validators must be more than the current number of UIDs already in the subnet.
        MaxAllowedUIdsLessThanCurrentUIds,
        /// The maximum value for bonds moving average is reached
        BondsMovingAverageMaxReached,
        /// Only root can set negative sigmoid steepness values
        NegativeSigmoidSteepness,
        /// Value not in allowed bounds.
        ValueNotInBounds,
        /// The minimum allowed UIDs must be less than the current number of UIDs in the subnet.
        MinAllowedUidsGreaterThanCurrentUids,
        /// The minimum allowed UIDs must be less than the maximum allowed UIDs.
        MinAllowedUidsGreaterThanMaxAllowedUids,
        /// The maximum allowed UIDs must be greater than the minimum allowed UIDs.
        MaxAllowedUidsLessThanMinAllowedUids,
        /// The maximum allowed UIDs must be less than the default maximum allowed UIDs.
        MaxAllowedUidsGreaterThanDefaultMaxAllowedUids,
        /// Bad parameter value
        InvalidValue,
        /// Operation is not permitted on the root network.
        NotPermittedOnRootSubnet,
        /// POW Registration has been deprecated
        POWRegistrationDisabled,
        /// Call is deprecated
        Deprecated,
    }
    /// Enum for specifying the type of precompile operation.
    #[derive(
        Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, Debug, Copy,
    )]
    pub enum PrecompileEnum {
        /// Enum for balance transfer precompile
        BalanceTransfer,
        /// Enum for staking precompile
        Staking,
        /// Enum for subnet precompile
        Subnet,
        /// Enum for metagraph precompile
        Metagraph,
        /// Enum for neuron precompile
        Neuron,
        /// Enum for UID lookup precompile
        UidLookup,
        /// Enum for alpha precompile
        Alpha,
        /// Enum for crowdloan precompile
        Crowdloan,
        /// Proxy precompile
        Proxy,
        /// Leasing precompile
        Leasing,
        /// Address mapping precompile
        AddressMapping,
        /// Voting power precompile
        VotingPower,
    }

    #[pallet::type_value]
    /// Default value for precompile enable
    pub fn DefaultPrecompileEnabled<T: Config>() -> bool {
        true
    }

    #[pallet::storage]
    /// Map PrecompileEnum --> enabled
    pub type PrecompileEnable<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        PrecompileEnum,
        bool,
        ValueQuery,
        DefaultPrecompileEnabled<T>,
    >;

    /// Dispatchable functions allows users to interact with the pallet and invoke state changes.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// The extrinsic sets the new authorities for Aura consensus.
        /// It is only callable by the root account.
        /// The extrinsic will call the Aura pallet to change the authorities.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::swap_authorities(new_authorities.len() as u32))]
        pub fn swap_authorities(
            origin: OriginFor<T>,
            new_authorities: BoundedVec<<T as Config>::AuthorityId, T::MaxAuthorities>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            T::Aura::change_authorities(new_authorities.clone());

            log::debug!("Aura authorities changed: {new_authorities:?}");

            // Return a successful DispatchResultWithPostInfo
            Ok(())
        }

        /// The extrinsic sets the default take for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the default take.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_default_take())]
        pub fn sudo_set_default_take(origin: OriginFor<T>, default_take: u16) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_max_delegate_take(default_take);
            log::debug!("DefaultTakeSet( default_take: {default_take:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the transaction rate limit for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the transaction rate limit.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_tx_rate_limit())]
        pub fn sudo_set_tx_rate_limit(origin: OriginFor<T>, tx_rate_limit: u64) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_tx_rate_limit(tx_rate_limit);
            log::debug!("TxRateLimitSet( tx_rate_limit: {tx_rate_limit:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the serving rate limit for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the serving rate limit.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_serving_rate_limit())]
        pub fn sudo_set_serving_rate_limit(
            origin: OriginFor<T>,
            netuid: NetUid,
            serving_rate_limit: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::ServingRateLimit.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::Pallet::<T>::set_serving_rate_limit(netuid, serving_rate_limit);
            log::debug!("ServingRateLimitSet( serving_rate_limit: {serving_rate_limit:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::ServingRateLimit.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the minimum difficulty for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the minimum difficulty.
        #[pallet::call_index(4)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_difficulty())]
        pub fn sudo_set_min_difficulty(
            origin: OriginFor<T>,
            netuid: NetUid,
            min_difficulty: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_min_difficulty(netuid, min_difficulty);
            log::debug!(
                "MinDifficultySet( netuid: {netuid:?} min_difficulty: {min_difficulty:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the maximum difficulty for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the maximum difficulty.
        #[pallet::call_index(5)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_difficulty())]
        pub fn sudo_set_max_difficulty(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_difficulty: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MaxDifficulty.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_max_difficulty(netuid, max_difficulty);
            log::debug!(
                "MaxDifficultySet( netuid: {netuid:?} max_difficulty: {max_difficulty:?} ) "
            );
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MaxDifficulty.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the weights version key for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the weights version key.
        #[pallet::call_index(6)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_weights_version_key())]
        pub fn sudo_set_weights_version_key(
            origin: OriginFor<T>,
            netuid: NetUid,
            weights_version_key: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin.clone(),
                netuid,
                &[TransactionType::SetWeightsVersionKey],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[TransactionType::SetWeightsVersionKey],
            );

            pallet_subtensor::Pallet::<T>::set_weights_version_key(netuid, weights_version_key);
            log::debug!(
                "WeightsVersionKeySet( netuid: {netuid:?} weights_version_key: {weights_version_key:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the weights set rate limit for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the weights set rate limit.
        #[pallet::call_index(7)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_weights_set_rate_limit())]
        pub fn sudo_set_weights_set_rate_limit(
            origin: OriginFor<T>,
            netuid: NetUid,
            weights_set_rate_limit: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_weights_set_rate_limit(
                netuid,
                weights_set_rate_limit,
            );
            log::debug!(
                "WeightsSetRateLimitSet( netuid: {netuid:?} weights_set_rate_limit: {weights_set_rate_limit:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the adjustment interval for a subnet.
        /// It is only callable by the root account, not changeable by the subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the adjustment interval.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_adjustment_interval())]
        pub fn sudo_set_adjustment_interval(
            origin: OriginFor<T>,
            netuid: NetUid,
            adjustment_interval: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_adjustment_interval(netuid, adjustment_interval);
            log::debug!(
                "AdjustmentIntervalSet( netuid: {netuid:?} adjustment_interval: {adjustment_interval:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the adjustment alpha for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the adjustment alpha.
        #[pallet::call_index(9)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1))
        )]
        pub fn sudo_set_adjustment_alpha(
            origin: OriginFor<T>,
            netuid: NetUid,
            adjustment_alpha: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::AdjustmentAlpha.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_adjustment_alpha(netuid, adjustment_alpha);
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::AdjustmentAlpha.into()],
            );
            log::debug!("AdjustmentAlphaSet( adjustment_alpha: {adjustment_alpha:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the immunity period for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the immunity period.
        #[pallet::call_index(13)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_immunity_period())]
        pub fn sudo_set_immunity_period(
            origin: OriginFor<T>,
            netuid: NetUid,
            immunity_period: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::ImmunityPeriod.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            pallet_subtensor::Pallet::<T>::set_immunity_period(netuid, immunity_period);
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::ImmunityPeriod.into()],
            );
            log::debug!(
                "ImmunityPeriodSet( netuid: {netuid:?} immunity_period: {immunity_period:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the minimum allowed weights for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the minimum allowed weights.
        #[pallet::call_index(14)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_allowed_weights())]
        pub fn sudo_set_min_allowed_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            min_allowed_weights: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MinAllowedWeights.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_min_allowed_weights(netuid, min_allowed_weights);
            log::debug!(
                "MinAllowedWeightSet( netuid: {netuid:?} min_allowed_weights: {min_allowed_weights:?} ) "
            );
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MinAllowedWeights.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the maximum allowed UIDs for a subnet.
        /// It is only callable by the root account and subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the maximum allowed UIDs for a subnet.
        #[pallet::call_index(15)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_allowed_uids())]
        pub fn sudo_set_max_allowed_uids(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_allowed_uids: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MaxAllowedUids.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                max_allowed_uids >= pallet_subtensor::Pallet::<T>::get_min_allowed_uids(netuid),
                Error::<T>::MaxAllowedUidsLessThanMinAllowedUids
            );
            ensure!(
                pallet_subtensor::Pallet::<T>::get_subnetwork_n(netuid) <= max_allowed_uids,
                Error::<T>::MaxAllowedUIdsLessThanCurrentUIds
            );
            ensure!(
                max_allowed_uids <= DefaultMaxAllowedUids::<T>::get(),
                Error::<T>::MaxAllowedUidsGreaterThanDefaultMaxAllowedUids
            );
            // Prevent chain bloat: Require max UIDs to be limited
            let mechanism_count = pallet_subtensor::MechanismCountCurrent::<T>::get(netuid);
            pallet_subtensor::Pallet::<T>::ensure_max_uids_over_all_mechanisms(
                max_allowed_uids,
                mechanism_count.into(),
            )?;
            pallet_subtensor::Pallet::<T>::set_max_allowed_uids(netuid, max_allowed_uids);
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MaxAllowedUids.into()],
            );
            log::debug!(
                "MaxAllowedUidsSet( netuid: {netuid:?} max_allowed_uids: {max_allowed_uids:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the kappa for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the kappa.
        #[pallet::call_index(16)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_kappa())]
        pub fn sudo_set_kappa(origin: OriginFor<T>, netuid: NetUid, kappa: u16) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_kappa(netuid, kappa);
            log::debug!("KappaSet( netuid: {netuid:?} kappa: {kappa:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the rho for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the rho.
        #[pallet::call_index(17)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_rho())]
        pub fn sudo_set_rho(origin: OriginFor<T>, netuid: NetUid, rho: u16) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::Rho.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_rho(netuid, rho);
            log::debug!("RhoSet( netuid: {netuid:?} rho: {rho:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::Rho.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the activity cutoff for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the activity cutoff.
        // #[deprecated(
        //     note = "Please use set_activity_cutoff_factor instead. This extrinsic will be removed soon."
        // )]
        #[pallet::call_index(18)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_activity_cutoff())]
        pub fn sudo_set_activity_cutoff(
            origin: OriginFor<T>,
            netuid: NetUid,
            activity_cutoff: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::ActivityCutoff.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            ensure!(
                activity_cutoff >= pallet_subtensor::MinActivityCutoff::<T>::get(),
                pallet_subtensor::Error::<T>::ActivityCutoffTooLow
            );

            pallet_subtensor::Pallet::<T>::set_activity_cutoff(netuid, activity_cutoff);
            log::debug!(
                "ActivityCutoffSet( netuid: {netuid:?} activity_cutoff: {activity_cutoff:?} ) "
            );
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::ActivityCutoff.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the network registration allowed for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the network registration allowed.
        #[pallet::call_index(19)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_network_registration_allowed())]
        pub fn sudo_set_network_registration_allowed(
            origin: OriginFor<T>,
            netuid: NetUid,
            registration_allowed: bool,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_network_registration_allowed(
                netuid,
                registration_allowed,
            );
            log::debug!(
                "NetworkRegistrationAllowed( registration_allowed: {registration_allowed:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the network PoW registration allowed for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the network PoW registration allowed.
        #[pallet::call_index(20)]
        #[pallet::weight(
			Weight::from_parts(14_000_000, 0)
				.saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1))
		)]
        pub fn sudo_set_network_pow_registration_allowed(
            _origin: OriginFor<T>,
            _netuid: NetUid,
            _registration_allowed: bool,
        ) -> DispatchResult {
            Err(Error::<T>::POWRegistrationDisabled.into())
        }

        /// The extrinsic sets the target registrations per interval for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the target registrations per interval.
        #[pallet::call_index(21)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_target_registrations_per_interval())]
        pub fn sudo_set_target_registrations_per_interval(
            origin: OriginFor<T>,
            netuid: NetUid,
            target_registrations_per_interval: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_target_registrations_per_interval(
                netuid,
                target_registrations_per_interval,
            );
            log::debug!(
                "RegistrationPerIntervalSet( netuid: {netuid:?} target_registrations_per_interval: {target_registrations_per_interval:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the minimum burn for a subnet.
        /// It is only callable by root and subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the minimum burn.
        #[pallet::call_index(22)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_burn())]
        pub fn sudo_set_min_burn(
            origin: OriginFor<T>,
            netuid: NetUid,
            min_burn: TaoBalance,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MinBurn.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                min_burn < T::MinBurnUpperBound::get(),
                Error::<T>::ValueNotInBounds
            );
            // Min burn must be less than max burn
            ensure!(
                min_burn < pallet_subtensor::Pallet::<T>::get_max_burn(netuid),
                Error::<T>::ValueNotInBounds
            );
            pallet_subtensor::Pallet::<T>::set_min_burn(netuid, min_burn);
            log::debug!("MinBurnSet( netuid: {netuid:?} min_burn: {min_burn:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MinBurn.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the maximum burn for a subnet.
        /// It is only callable by root and subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the maximum burn.
        #[pallet::call_index(23)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_burn())]
        pub fn sudo_set_max_burn(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_burn: TaoBalance,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MaxBurn.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                max_burn > T::MaxBurnLowerBound::get(),
                Error::<T>::ValueNotInBounds
            );
            // Max burn must be greater than min burn
            ensure!(
                max_burn > pallet_subtensor::Pallet::<T>::get_min_burn(netuid),
                Error::<T>::ValueNotInBounds
            );
            pallet_subtensor::Pallet::<T>::set_max_burn(netuid, max_burn);
            log::debug!("MaxBurnSet( netuid: {netuid:?} max_burn: {max_burn:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MaxBurn.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the difficulty for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the difficulty.
        #[pallet::call_index(24)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_difficulty())]
        pub fn sudo_set_difficulty(
            origin: OriginFor<T>,
            netuid: NetUid,
            difficulty: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_difficulty(netuid, difficulty);
            log::debug!("DifficultySet( netuid: {netuid:?} difficulty: {difficulty:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the maximum allowed validators for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the maximum allowed validators.
        #[pallet::call_index(25)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_allowed_validators())]
        pub fn sudo_set_max_allowed_validators(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_allowed_validators: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                max_allowed_validators
                    <= pallet_subtensor::Pallet::<T>::get_max_allowed_uids(netuid),
                Error::<T>::MaxValidatorsLargerThanMaxUIds
            );

            pallet_subtensor::Pallet::<T>::set_max_allowed_validators(
                netuid,
                max_allowed_validators,
            );
            log::debug!(
                "MaxAllowedValidatorsSet( netuid: {netuid:?} max_allowed_validators: {max_allowed_validators:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the bonds moving average for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the bonds moving average.
        #[pallet::call_index(26)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_bonds_moving_average())]
        pub fn sudo_set_bonds_moving_average(
            origin: OriginFor<T>,
            netuid: NetUid,
            bonds_moving_average: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::BondsMovingAverage.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            if maybe_owner.is_some() {
                ensure!(
                    bonds_moving_average <= 975000,
                    Error::<T>::BondsMovingAverageMaxReached
                )
            }

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_bonds_moving_average(netuid, bonds_moving_average);
            log::debug!(
                "BondsMovingAverageSet( netuid: {netuid:?} bonds_moving_average: {bonds_moving_average:?} ) "
            );
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::BondsMovingAverage.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the bonds penalty for a subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the bonds penalty.
        #[pallet::call_index(60)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_bonds_penalty())]
        pub fn sudo_set_bonds_penalty(
            origin: OriginFor<T>,
            netuid: NetUid,
            bonds_penalty: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::BondsPenalty.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_bonds_penalty(netuid, bonds_penalty);
            log::debug!("BondsPenalty( netuid: {netuid:?} bonds_penalty: {bonds_penalty:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::BondsPenalty.into()],
            );
            Ok(())
        }

        /// The extrinsic sets the maximum registrations per block for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the maximum registrations per block.
        #[pallet::call_index(27)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_registrations_per_block())]
        pub fn sudo_set_max_registrations_per_block(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_registrations_per_block: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_max_registrations_per_block(
                netuid,
                max_registrations_per_block,
            );
            log::debug!(
                "MaxRegistrationsPerBlock( netuid: {netuid:?} max_registrations_per_block: {max_registrations_per_block:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the subnet owner cut for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the subnet owner cut.
        #[pallet::call_index(28)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_subnet_owner_cut(
            origin: OriginFor<T>,
            subnet_owner_cut: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_subnet_owner_cut(subnet_owner_cut);
            log::debug!("SubnetOwnerCut( subnet_owner_cut: {subnet_owner_cut:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the network rate limit for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the network rate limit.
        #[pallet::call_index(29)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_network_rate_limit(
            origin: OriginFor<T>,
            rate_limit: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_network_rate_limit(rate_limit);
            log::debug!("NetworkRateLimit( rate_limit: {rate_limit:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the tempo for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the tempo.
        #[pallet::call_index(30)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_tempo())]
        pub fn sudo_set_tempo(origin: OriginFor<T>, netuid: NetUid, tempo: u16) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::apply_tempo_with_cycle_reset(netuid, tempo);
            log::debug!("TempoSet( netuid: {netuid:?} tempo: {tempo:?} ) ");
            Ok(())
        }

        /// DEPRECATED
        #[pallet::call_index(33)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_total_issuance())]
        pub fn sudo_set_total_issuance(
            _origin: OriginFor<T>,
            _total_issuance: TaoBalance,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// The extrinsic sets the immunity period for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the immunity period for the network.
        #[pallet::call_index(35)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_network_immunity_period(
            origin: OriginFor<T>,
            immunity_period: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;

            pallet_subtensor::Pallet::<T>::set_network_immunity_period(immunity_period);

            log::debug!("NetworkImmunityPeriod( period: {immunity_period:?} ) ");

            Ok(())
        }

        /// The extrinsic sets the min lock cost for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the min lock cost for the network.
        #[pallet::call_index(36)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_network_min_lock_cost(
            origin: OriginFor<T>,
            lock_cost: TaoBalance,
        ) -> DispatchResult {
            ensure_root(origin)?;

            pallet_subtensor::Pallet::<T>::set_network_min_lock(lock_cost);

            log::debug!("NetworkMinLockCost( lock_cost: {lock_cost:?} ) ");

            Ok(())
        }

        /// The extrinsic sets the subnet limit for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the subnet limit.
        #[pallet::call_index(37)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_subnet_limit(origin: OriginFor<T>, max_subnets: u16) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_max_subnets(max_subnets);
            log::debug!("MaxSubnets ( max_subnets: {max_subnets:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the lock reduction interval for the network.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the lock reduction interval.
        #[pallet::call_index(38)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_lock_reduction_interval(
            origin: OriginFor<T>,
            interval: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;

            pallet_subtensor::Pallet::<T>::set_lock_reduction_interval(interval);

            log::debug!("NetworkLockReductionInterval( interval: {interval:?} ) ");

            Ok(())
        }

        /// The extrinsic sets the recycled RAO for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the recycled RAO.
        #[pallet::call_index(39)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_rao_recycled())]
        pub fn sudo_set_rao_recycled(
            origin: OriginFor<T>,
            netuid: NetUid,
            rao_recycled: TaoBalance,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            pallet_subtensor::Pallet::<T>::set_rao_recycled(netuid, rao_recycled);
            Ok(())
        }

        /// The extrinsic sets the weights min stake.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the weights min stake.
        #[pallet::call_index(42)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_stake_threshold())]
        pub fn sudo_set_stake_threshold(origin: OriginFor<T>, min_stake: u64) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_stake_threshold(min_stake);
            Ok(())
        }

        /// The extrinsic sets the minimum stake required for nominators.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the minimum stake required for nominators.
        #[pallet::call_index(43)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_nominator_min_required_stake())]
        pub fn sudo_set_nominator_min_required_stake(
            origin: OriginFor<T>,
            // The minimum stake required for nominators.
            min_stake: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            let prev_min_stake = pallet_subtensor::Pallet::<T>::get_nominator_min_required_stake();
            log::trace!("Setting minimum stake to: {min_stake}");
            pallet_subtensor::Pallet::<T>::set_nominator_min_required_stake(min_stake);
            if min_stake > prev_min_stake {
                log::trace!("Clearing small nominations if possible");
                pallet_subtensor::Pallet::<T>::clear_small_nominations();
                log::trace!("Small nominations cleared");
            }
            Ok(())
        }

        /// The extrinsic sets the rate limit for delegate take transactions.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the rate limit for delegate take transactions.
        #[pallet::call_index(45)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_tx_delegate_take_rate_limit())]
        pub fn sudo_set_tx_delegate_take_rate_limit(
            origin: OriginFor<T>,
            tx_rate_limit: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_tx_delegate_take_rate_limit(tx_rate_limit);
            log::debug!(
                "TxRateLimitDelegateTakeSet( tx_delegate_take_rate_limit: {tx_rate_limit:?} ) "
            );
            Ok(())
        }

        /// The extrinsic sets the minimum delegate take.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the minimum delegate take.
        #[pallet::call_index(46)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_delegate_take())]
        pub fn sudo_set_min_delegate_take(origin: OriginFor<T>, take: u16) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_min_delegate_take(take);
            log::debug!("TxMinDelegateTakeSet( tx_min_delegate_take: {take:?} ) ");
            Ok(())
        }

        /// The extrinsic sets the minimum childkey take for a subnet.
        /// It is callable by root or the subnet owner.
        /// The subnet minimum can only make the global minimum stricter.
        #[pallet::call_index(93)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_childkey_take_per_subnet())]
        pub fn sudo_set_min_childkey_take_per_subnet(
            origin: OriginFor<T>,
            netuid: NetUid,
            take: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::MinChildkeyTake.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                take >= pallet_subtensor::Pallet::<T>::get_min_childkey_take()
                    && take <= pallet_subtensor::Pallet::<T>::get_max_childkey_take(),
                Error::<T>::InvalidValue
            );

            pallet_subtensor::Pallet::<T>::set_min_childkey_take_for_subnet(netuid, take);
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::MinChildkeyTake.into()],
            );
            log::debug!(
                "MinChildkeyTakePerSubnetSet( netuid: {netuid:?}, min_childkey_take: {take:?} ) "
            );
            Ok(())
        }

        /// The extrinsic enabled/disables commit/reaveal for a given subnet.
        /// It is only callable by the root account or subnet owner.
        /// The extrinsic will call the Subtensor pallet to set the value.
        #[pallet::call_index(49)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_commit_reveal_weights_enabled())]
        pub fn sudo_set_commit_reveal_weights_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::CommitRevealEnabled.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            pallet_subtensor::Pallet::<T>::set_commit_reveal_weights_enabled(netuid, enabled);
            log::debug!("ToggleSetWeightsCommitReveal( netuid: {netuid:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::CommitRevealEnabled.into()],
            );
            Ok(())
        }

        /// Enables or disables Liquid Alpha for a given subnet.
        ///
        /// # Parameters
        /// - `origin`: The origin of the call, which must be the root account or subnet owner.
        /// - `netuid`: The unique identifier for the subnet.
        /// - `enabled`: A boolean flag to enable or disable Liquid Alpha.
        ///
        /// # Weight
        /// This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
        #[pallet::call_index(50)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_liquid_alpha_enabled())]
        pub fn sudo_set_liquid_alpha_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::LiquidAlphaEnabled.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::Pallet::<T>::set_liquid_alpha_enabled(netuid, enabled);
            log::debug!("LiquidAlphaEnableToggled( netuid: {netuid:?}, Enabled: {enabled:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::LiquidAlphaEnabled.into()],
            );
            Ok(())
        }

        /// Sets values for liquid alpha
        #[pallet::call_index(51)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_alpha_values())]
        pub fn sudo_set_alpha_values(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha_low: u16,
            alpha_high: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin.clone(),
                netuid,
                &[Hyperparameter::AlphaValues.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            let res = pallet_subtensor::Pallet::<T>::do_set_alpha_values(
                origin, netuid, alpha_low, alpha_high,
            );
            if res.is_ok() {
                pallet_subtensor::Pallet::<T>::record_owner_rl(
                    maybe_owner,
                    netuid,
                    &[Hyperparameter::AlphaValues.into()],
                );
            }
            res
        }

        /// Sets the duration of the dissolve network schedule.
        ///
        /// This extrinsic allows the root account to set the duration for the dissolve network schedule.
        /// The dissolve network schedule determines how long it takes for a network dissolution operation to complete.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `duration` - The new duration for the dissolve network schedule, in number of blocks.
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(55)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_dissolve_network_schedule_duration())]
        pub fn sudo_set_dissolve_network_schedule_duration(
            origin: OriginFor<T>,
            duration: BlockNumberFor<T>,
        ) -> DispatchResult {
            // Ensure the call is made by the root account
            ensure_root(origin)?;

            // Set the duration of schedule dissolve network
            pallet_subtensor::Pallet::<T>::set_dissolve_network_schedule_duration(duration);

            // Log the change
            log::trace!("DissolveNetworkScheduleDurationSet( duration: {duration:?} )");

            Ok(())
        }

        /// Sets the commit-reveal weights periods for a specific subnet.
        ///
        /// This extrinsic allows the subnet owner or root account to set the duration (in epochs) during which committed weights must be revealed.
        /// The commit-reveal mechanism ensures that users commit weights in advance and reveal them only within a specified period.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the subnet owner or the root account.
        /// * `netuid` - The unique identifier of the subnet for which the periods are being set.
        /// * `periods` - The number of epochs that define the commit-reveal period.
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
        /// * `SubnetDoesNotExist` - If the specified subnet does not exist.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(57)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_commit_reveal_weights_interval())]
        pub fn sudo_set_commit_reveal_weights_interval(
            origin: OriginFor<T>,
            netuid: NetUid,
            interval: u64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::WeightCommitInterval.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            log::debug!("SetWeightCommitInterval( netuid: {netuid:?}, interval: {interval:?} ) ");

            pallet_subtensor::Pallet::<T>::set_reveal_period(netuid, interval)?;
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::WeightCommitInterval.into()],
            );

            Ok(())
        }

        /// Sets the EVM ChainID.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the subnet owner or the root account.
        /// * `chainId` - The u64 chain ID
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(58)]
        #[pallet::weight(Weight::from_parts(27_199_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1_u64))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1_u64)))]
        pub fn sudo_set_evm_chain_id(origin: OriginFor<T>, chain_id: u64) -> DispatchResult {
            // Ensure the call is made by the root account
            ensure_root(origin)?;

            ChainId::<T>::set(chain_id);
            Ok(())
        }

        /// A public interface for `pallet_grandpa::Pallet::schedule_grandpa_change`.
        ///
        /// Schedule a change in the authorities.
        ///
        /// The change will be applied at the end of execution of the block `in_blocks` after the
        /// current block. This value may be 0, in which case the change is applied at the end of
        /// the current block.
        ///
        /// If the `forced` parameter is defined, this indicates that the current set has been
        /// synchronously determined to be offline and that after `in_blocks` the given change
        /// should be applied. The given block number indicates the median last finalized block
        /// number and it should be used as the canon block when starting the new grandpa voter.
        ///
        /// No change should be signaled while any change is pending. Returns an error if a change
        /// is already pending.
        #[pallet::call_index(59)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::schedule_grandpa_change(next_authorities.len() as u32))]
        pub fn schedule_grandpa_change(
            origin: OriginFor<T>,
            // grandpa ID is always the same type, so we don't need to parametrize it via `Config`
            next_authorities: AuthorityList,
            in_blocks: BlockNumberFor<T>,
            forced: Option<BlockNumberFor<T>>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            T::Grandpa::schedule_change(next_authorities, in_blocks, forced)
        }

        /// Enable or disable atomic alpha transfers for a given subnet.
        ///
        /// # Parameters
        /// - `origin`: The origin of the call, which must be the root account or subnet owner.
        /// - `netuid`: The unique identifier for the subnet.
        /// - `enabled`: A boolean flag to enable or disable Liquid Alpha.
        ///
        /// # Weight
        /// This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
        #[pallet::call_index(61)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_toggle_transfer())]
        pub fn sudo_set_toggle_transfer(
            origin: OriginFor<T>,
            netuid: NetUid,
            toggle: bool,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::TransferEnabled.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            let res = pallet_subtensor::Pallet::<T>::toggle_transfer(netuid, toggle);
            if res.is_ok() {
                pallet_subtensor::Pallet::<T>::record_owner_rl(
                    maybe_owner,
                    netuid,
                    &[Hyperparameter::TransferEnabled.into()],
                );
            }
            res
        }

        /// Set the behaviour of the "burn" UID(s) for a given subnet.
        /// If set to `Burn`, the miner emission sent to the burn UID(s) will be burned.
        /// If set to `Recycle`, the miner emission sent to the burn UID(s) will be recycled.
        ///
        /// # Parameters
        /// - `origin`: The origin of the call, which must be the root account or subnet owner.
        /// - `netuid`: The unique identifier for the subnet.
        /// - `recycle_or_burn`: The desired behaviour of the "burn" UID(s) for the subnet.
        ///
        #[pallet::call_index(80)]
        #[pallet::weight(Weight::from_parts(20_000_000, 0)
        .saturating_add(T::DbWeight::get().reads(2_u64))
        .saturating_add(T::DbWeight::get().writes(2_u64)))] // TODO: add benchmarks
        pub fn sudo_set_recycle_or_burn(
            origin: OriginFor<T>,
            netuid: NetUid,
            recycle_or_burn: pallet_subtensor::RecycleOrBurnEnum,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::RecycleOrBurn.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            pallet_subtensor::Pallet::<T>::set_recycle_or_burn(netuid, recycle_or_burn);
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::RecycleOrBurn.into()],
            );

            Ok(())
        }

        /// Toggles the enablement of an EVM precompile.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `precompile_id` - The identifier of the EVM precompile to toggle.
        /// * `enabled` - The new enablement state of the precompile.
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(62)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_toggle_evm_precompile())]
        pub fn sudo_toggle_evm_precompile(
            origin: OriginFor<T>,
            precompile_id: PrecompileEnum,
            enabled: bool,
        ) -> DispatchResult {
            ensure_root(origin)?;
            if PrecompileEnable::<T>::get(precompile_id) != enabled {
                PrecompileEnable::<T>::insert(precompile_id, enabled);
                Self::deposit_event(Event::PrecompileUpdated {
                    precompile_id,
                    enabled,
                });
            }
            Ok(())
        }

        ///
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `alpha` - The new moving alpha value for the SubnetMovingAlpha.
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(63)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_subnet_moving_alpha())]
        pub fn sudo_set_subnet_moving_alpha(origin: OriginFor<T>, alpha: I96F32) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::SubnetMovingAlpha::<T>::set(alpha);

            log::debug!("SubnetMovingAlphaSet( alpha: {alpha:?} )");
            Ok(())
        }

        // Deprecated for sudo_set_sn_owner_hotkey
        // #[pallet::call_index(64)]
        // #[pallet::weight(Weight::from_parts(3_918_000, 0) // TODO: add benchmarks
        // .saturating_add(T::DbWeight::get().writes(1_u64)))]
        // pub fn sudo_set_subnet_owner_hotkey(
        //     origin: OriginFor<T>,
        //     netuid: NetUid,
        //     hotkey: <T as frame_system::Config>::AccountId,
        // ) -> DispatchResult {
        //     pallet_subtensor::Pallet::<T>::do_set_sn_owner_hotkey(origin, netuid, &hotkey)
        // }

        ///
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `ema_alpha_period` - Number of blocks for EMA price to halve
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(65)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_ema_price_halving_period())]
        pub fn sudo_set_ema_price_halving_period(
            origin: OriginFor<T>,
            netuid: NetUid,
            ema_halving: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::EMAPriceHalvingBlocks::<T>::set(netuid, ema_halving);

            log::debug!(
                "EMAPriceHalvingBlocks( netuid: {netuid:?}, ema_halving: {ema_halving:?} )"
            );
            Ok(())
        }

        ///
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `netuid` - The unique identifier for the subnet.
        /// * `steepness` - The Steepness for the alpha sigmoid function. (range is 0-int16::MAX,
        /// negative values are reserved for future use)
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        /// * `SubnetDoesNotExist` - If the specified subnet does not exist.
        /// * `NegativeSigmoidSteepness` - If the steepness is negative and the caller is
        /// root.
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(68)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_alpha_sigmoid_steepness())]
        pub fn sudo_set_alpha_sigmoid_steepness(
            origin: OriginFor<T>,
            netuid: NetUid,
            steepness: i16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin.clone(),
                netuid,
                &[Hyperparameter::AlphaSigmoidSteepness.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );

            let is_root = ensure_root(origin).is_ok();
            ensure!(
                is_root || steepness >= 0,
                Error::<T>::NegativeSigmoidSteepness
            );

            pallet_subtensor::Pallet::<T>::set_alpha_sigmoid_steepness(netuid, steepness);

            log::debug!("AlphaSigmoidSteepnessSet( netuid: {netuid:?}, steepness: {steepness:?} )");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::AlphaSigmoidSteepness.into()],
            );
            Ok(())
        }

        /// Enables or disables Yuma3 for a given subnet.
        ///
        /// # Parameters
        /// - `origin`: The origin of the call, which must be the root account or subnet owner.
        /// - `netuid`: The unique identifier for the subnet.
        /// - `enabled`: A boolean flag to enable or disable Yuma3.
        ///
        /// # Weight
        /// This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
        #[pallet::call_index(69)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_yuma3_enabled())]
        pub fn sudo_set_yuma3_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::Yuma3Enabled.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::Pallet::<T>::set_yuma3_enabled(netuid, enabled);

            Self::deposit_event(Event::Yuma3EnableToggled { netuid, enabled });
            log::debug!("Yuma3EnableToggled( netuid: {netuid:?}, Enabled: {enabled:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::Yuma3Enabled.into()],
            );
            Ok(())
        }

        /// Enables or disables Bonds Reset for a given subnet.
        ///
        /// # Parameters
        /// - `origin`: The origin of the call, which must be the root account or subnet owner.
        /// - `netuid`: The unique identifier for the subnet.
        /// - `enabled`: A boolean flag to enable or disable Bonds Reset.
        ///
        /// # Weight
        /// This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
        #[pallet::call_index(70)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_bonds_reset_enabled())]
        pub fn sudo_set_bonds_reset_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::BondsResetEnabled.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::Pallet::<T>::set_bonds_reset(netuid, enabled);

            Self::deposit_event(Event::BondsResetToggled { netuid, enabled });
            log::debug!("BondsResetToggled( netuid: {netuid:?} bonds_reset: {enabled:?} ) ");
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::BondsResetEnabled.into()],
            );
            Ok(())
        }

        /// Sets or updates the hotkey account associated with the owner of a specific subnet.
        ///
        /// This function allows either the root origin or the current subnet owner to set or update
        /// the hotkey for a given subnet. The subnet must already exist. To prevent abuse, the call is
        /// rate-limited to once per configured interval (default: one week) per subnet.
        ///
        /// # Parameters
        /// - `origin`: The dispatch origin of the call. Must be either root or the current owner of the subnet.
        /// - `netuid`: The unique identifier of the subnet whose owner hotkey is being set.
        /// - `hotkey`: The new hotkey account to associate with the subnet owner.
        ///
        /// # Returns
        /// - `DispatchResult`: Returns `Ok(())` if the hotkey was successfully set, or an appropriate error otherwise.
        ///
        /// # Errors
        /// - `Error::SubnetNotExists`: If the specified subnet does not exist.
        /// - `Error::TxRateLimitExceeded`: If the function is called more frequently than the allowed rate limit.
        ///
        /// # Access Control
        /// Only callable by:
        /// - Root origin, or
        /// - The coldkey account that owns the subnet.
        ///
        /// # Storage
        /// - Updates [`SubnetOwnerHotkey`] for the given `netuid`.
        /// - Reads and updates [`LastRateLimitedBlock`] for rate-limiting.
        /// - Reads [`DefaultSetSNOwnerHotkeyRateLimit`] to determine the interval between allowed updates.
        ///
        /// # Rate Limiting
        /// This function is rate-limited to one call per subnet per interval (e.g., one week).
        #[pallet::call_index(67)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_sn_owner_hotkey())]
        pub fn sudo_set_sn_owner_hotkey(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: <T as frame_system::Config>::AccountId,
        ) -> DispatchResult {
            pallet_subtensor::Pallet::<T>::do_set_sn_owner_hotkey(origin, netuid, &hotkey)
        }

        /// Enables or disables subtoken trading for a given subnet.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the root account.
        /// * `netuid` - The unique identifier of the subnet.
        /// * `subtoken_enabled` - A boolean indicating whether subtoken trading should be enabled or disabled.
        ///
        /// # Errors
        /// * `BadOrigin` - If the caller is not the root account.
        ///
        /// # Weight
        /// Weight is handled by the `#[pallet::weight]` attribute.
        #[pallet::call_index(66)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_subtoken_enabled())]
        pub fn sudo_set_subtoken_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            subtoken_enabled: bool,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::SubtokenEnabled::<T>::set(netuid, subtoken_enabled);

            log::debug!(
                "SubtokenEnabled( netuid: {netuid:?}, subtoken_enabled: {subtoken_enabled:?} )"
            );
            Ok(())
        }

        /// Sets the commit-reveal weights version for all subnets
        #[pallet::call_index(71)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_commit_reveal_version())]
        pub fn sudo_set_commit_reveal_version(
            origin: OriginFor<T>,
            version: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_commit_reveal_weights_version(version);
            Ok(())
        }

        /// Sets the number of immune owner neurons
        #[pallet::call_index(72)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_owner_immune_neuron_limit())]
        pub fn sudo_set_owner_immune_neuron_limit(
            origin: OriginFor<T>,
            netuid: NetUid,
            immune_neurons: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::ImmuneNeuronLimit.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;
            pallet_subtensor::Pallet::<T>::set_owner_immune_neuron_limit(netuid, immune_neurons)?;
            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::ImmuneNeuronLimit.into()],
            );
            Ok(())
        }

        /// Sets the childkey burn for a subnet.
        /// It is only callable by the root account.
        /// The extrinsic will call the Subtensor pallet to set the childkey burn.
        #[pallet::call_index(73)]
        #[pallet::weight(Weight::from_parts(15_650_000, 0)
		.saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1_u64))
		.saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1_u64)))]
        pub fn sudo_set_ck_burn(origin: OriginFor<T>, burn: u64) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_ck_burn(burn);
            log::debug!("CKBurnSet( burn: {burn:?} ) ");
            Ok(())
        }

        /// Sets the admin freeze window length (in blocks) at the end of a tempo.
        /// Only callable by root.
        #[pallet::call_index(74)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_admin_freeze_window())]
        pub fn sudo_set_admin_freeze_window(origin: OriginFor<T>, window: u16) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_admin_freeze_window(window);
            log::debug!("AdminFreezeWindowSet( window: {window:?} ) ");
            Ok(())
        }

        /// Sets the owner hyperparameter rate limit in epochs (global multiplier).
        /// Only callable by root.
        #[pallet::call_index(75)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_owner_hparam_rate_limit())]
        pub fn sudo_set_owner_hparam_rate_limit(
            origin: OriginFor<T>,
            epochs: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_owner_hyperparam_rate_limit(epochs);
            log::debug!("OwnerHyperparamRateLimitSet( epochs: {epochs:?} ) ");
            Ok(())
        }

        /// Sets the desired number of mechanisms in a subnet
        #[pallet::call_index(76)]
        #[pallet::weight(Weight::from_parts(15_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1_u64))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1_u64)))]
        pub fn sudo_set_mechanism_count(
            origin: OriginFor<T>,
            netuid: NetUid,
            mechanism_count: MechId,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[TransactionType::MechanismCountUpdate],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            pallet_subtensor::Pallet::<T>::do_set_mechanism_count(netuid, mechanism_count)?;

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[TransactionType::MechanismCountUpdate],
            );
            Ok(())
        }

        /// Sets the emission split between mechanisms in a subnet
        #[pallet::call_index(77)]
        #[pallet::weight(Weight::from_parts(15_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1_u64))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1_u64)))]
        pub fn sudo_set_mechanism_emission_split(
            origin: OriginFor<T>,
            netuid: NetUid,
            maybe_split: Option<Vec<u16>>,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[TransactionType::MechanismEmission],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            pallet_subtensor::Pallet::<T>::do_set_emission_split(netuid, maybe_split)?;

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[TransactionType::MechanismEmission],
            );
            Ok(())
        }

        /// Trims the maximum number of UIDs for a subnet.
        ///
        /// The trimming is done by sorting the UIDs by emission descending and then trimming
        /// the lowest emitters while preserving temporally and owner immune UIDs. The UIDs are
        /// then compressed to the left and storage is migrated to the new compressed UIDs.
        #[pallet::call_index(78)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_trim_to_max_allowed_uids())]
        pub fn sudo_trim_to_max_allowed_uids(
            origin: OriginFor<T>,
            netuid: NetUid,
            max_n: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin.clone(),
                netuid,
                &[TransactionType::MaxUidsTrimming],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            pallet_subtensor::Pallet::<T>::trim_to_max_allowed_uids(netuid, max_n)?;

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[TransactionType::MaxUidsTrimming],
            );
            Ok(())
        }

        /// The extrinsic sets the minimum allowed UIDs for a subnet.
        /// It is only callable by the root account.
        #[pallet::call_index(79)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_allowed_uids())]
        pub fn sudo_set_min_allowed_uids(
            origin: OriginFor<T>,
            netuid: NetUid,
            min_allowed_uids: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(
                min_allowed_uids < pallet_subtensor::Pallet::<T>::get_max_allowed_uids(netuid),
                Error::<T>::MinAllowedUidsGreaterThanMaxAllowedUids
            );
            ensure!(
                min_allowed_uids < pallet_subtensor::Pallet::<T>::get_subnetwork_n(netuid),
                Error::<T>::MinAllowedUidsGreaterThanCurrentUids
            );

            pallet_subtensor::Pallet::<T>::set_min_allowed_uids(netuid, min_allowed_uids);

            log::debug!(
                "MinAllowedUidsSet( netuid: {netuid:?} min_allowed_uids: {min_allowed_uids:?} ) "
            );
            Ok(())
        }

        /// Sets TAO flow cutoff value (A)
        #[pallet::call_index(81)]
        #[pallet::weight(Weight::from_parts(7_343_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(0))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_tao_flow_cutoff(
            origin: OriginFor<T>,
            flow_cutoff: I64F64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_tao_flow_cutoff(flow_cutoff);
            log::debug!("set_tao_flow_cutoff( {flow_cutoff:?} ) ");
            Ok(())
        }

        /// Sets TAO flow normalization exponent (p)
        #[pallet::call_index(82)]
        #[pallet::weight(Weight::from_parts(7_343_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(0))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_tao_flow_normalization_exponent(
            origin: OriginFor<T>,
            exponent: U64F64,
        ) -> DispatchResult {
            ensure_root(origin)?;

            let one = U64F64::saturating_from_num(1);
            let two = U64F64::saturating_from_num(2);
            ensure!(
                (one <= exponent) && (exponent <= two),
                Error::<T>::InvalidValue
            );

            pallet_subtensor::Pallet::<T>::set_tao_flow_normalization_exponent(exponent);
            log::debug!("set_tao_flow_normalization_exponent( {exponent:?} ) ");
            Ok(())
        }

        /// Sets TAO flow smoothing factor (alpha)
        #[pallet::call_index(83)]
        #[pallet::weight(Weight::from_parts(7_343_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(0))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_tao_flow_smoothing_factor(
            origin: OriginFor<T>,
            smoothing_factor: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_tao_flow_smoothing_factor(smoothing_factor);
            log::debug!("set_tao_flow_smoothing_factor( {smoothing_factor:?} ) ");
            Ok(())
        }

        /// Enables or disables net TAO flow (protocol cost deduction from emission shares).
        /// When enabled, emission shares use net flow = user flow - protocol cost.
        /// When disabled, emission shares use gross user flow only (current behavior).
        #[pallet::call_index(91)]
        #[pallet::weight(Weight::from_parts(7_343_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(0))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_net_tao_flow_enabled(
            origin: OriginFor<T>,
            enabled: bool,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_net_tao_flow_enabled(enabled);
            log::debug!("set_net_tao_flow_enabled( {enabled:?} ) ");
            Ok(())
        }

        /// Sets the global maximum number of mechanisms in a subnet
        #[pallet::call_index(88)]
        #[pallet::weight(Weight::from_parts(15_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(1_u64))
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1_u64)))]
        pub fn sudo_set_max_mechanism_count(
            origin: OriginFor<T>,
            max_mechanism_count: MechId,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::do_set_max_mechanism_count(max_mechanism_count)?;
            Ok(())
        }

        /// Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet
        #[pallet::call_index(84)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_min_non_immune_uids())]
        pub fn sudo_set_min_non_immune_uids(
            origin: OriginFor<T>,
            netuid: NetUid,
            min: u16,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_min_non_immune_uids(netuid, min);
            Ok(())
        }

        /// Sets the delay before a subnet can call start
        #[pallet::call_index(85)]
        #[pallet::weight(Weight::from_parts(14_000_000, 0)
        .saturating_add(<T as frame_system::Config>::DbWeight::get().writes(1)))]
        pub fn sudo_set_start_call_delay(origin: OriginFor<T>, delay: u64) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_start_call_delay(delay);
            log::debug!("StartCallDelay( delay: {delay:?} ) ");
            Ok(())
        }

        /// Sets the announcement delay for coldkey swap.
        #[pallet::call_index(86)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_coldkey_swap_announcement_delay())]
        pub fn sudo_set_coldkey_swap_announcement_delay(
            origin: OriginFor<T>,
            duration: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_coldkey_swap_announcement_delay(duration);
            log::trace!("ColdkeySwapAnnouncementDelaySet( duration: {duration:?} )");
            Ok(())
        }

        /// Sets the coldkey swap reannouncement delay.
        #[pallet::call_index(87)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_coldkey_swap_reannouncement_delay())]
        pub fn sudo_set_coldkey_swap_reannouncement_delay(
            origin: OriginFor<T>,
            duration: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            pallet_subtensor::Pallet::<T>::set_coldkey_swap_reannouncement_delay(duration);
            log::trace!("ColdkeySwapReannouncementDelaySet( duration: {duration:?} )");
            Ok(())
        }

        /// Set BurnHalfLife for a subnet.
        /// It is only callable by root and subnet owner.
        #[pallet::call_index(89)]
        #[pallet::weight((
            Weight::from_parts(25_000_000, 0)
                .saturating_add(T::DbWeight::get().reads(4))
                .saturating_add(T::DbWeight::get().writes(1)),
            DispatchClass::Operational,
            Pays::Yes,
        ))]
        pub fn sudo_set_burn_half_life(
            origin: OriginFor<T>,
            netuid: NetUid,
            burn_half_life: u16,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::BurnHalfLife.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(!netuid.is_root(), Error::<T>::NotPermittedOnRootSubnet);
            ensure!(
                burn_half_life > 0
                    && burn_half_life <= pallet_subtensor::MaxBurnHalfLife::<T>::get(),
                Error::<T>::InvalidValue
            );

            pallet_subtensor::BurnHalfLife::<T>::insert(netuid, burn_half_life);
            Self::deposit_event(Event::BurnHalfLifeSet {
                netuid,
                burn_half_life,
            });

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::BurnHalfLife.into()],
            );

            Ok(())
        }

        /// Set BurnIncreaseMult for a subnet.
        /// It is only callable by root and subnet owner.
        #[pallet::call_index(90)]
        #[pallet::weight((
            Weight::from_parts(25_000_000, 0)
                .saturating_add(T::DbWeight::get().reads(4))
                .saturating_add(T::DbWeight::get().writes(1)),
            DispatchClass::Operational,
            Pays::Yes,
        ))]
        pub fn sudo_set_burn_increase_mult(
            origin: OriginFor<T>,
            netuid: NetUid,
            burn_increase_mult: U64F64,
        ) -> DispatchResult {
            let maybe_owner = pallet_subtensor::Pallet::<T>::ensure_sn_owner_or_root_with_limits(
                origin,
                netuid,
                &[Hyperparameter::BurnIncreaseMult.into()],
            )?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(!netuid.is_root(), Error::<T>::NotPermittedOnRootSubnet);
            ensure!(
                (1..=3).contains(&burn_increase_mult),
                Error::<T>::InvalidValue
            );

            pallet_subtensor::BurnIncreaseMult::<T>::insert(
                netuid,
                U64F64::from_num(burn_increase_mult),
            );
            Self::deposit_event(Event::BurnIncreaseMultSet {
                netuid,
                burn_increase_mult,
            });

            pallet_subtensor::Pallet::<T>::record_owner_rl(
                maybe_owner,
                netuid,
                &[Hyperparameter::BurnIncreaseMult.into()],
            );

            Ok(())
        }

        /// Set whether the subnet owner cut is enabled for a subnet.
        /// It is only callable by root and subnet owner.
        #[pallet::call_index(92)]
        #[pallet::weight((
            Weight::from_parts(25_000_000, 0)
                .saturating_add(T::DbWeight::get().reads(4))
                .saturating_add(T::DbWeight::get().writes(1)),
            DispatchClass::Operational,
            Pays::Yes,
        ))]
        pub fn sudo_set_owner_cut_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            pallet_subtensor::Pallet::<T>::ensure_subnet_owner_or_root(origin, netuid)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(!netuid.is_root(), Error::<T>::NotPermittedOnRootSubnet);

            pallet_subtensor::Pallet::<T>::set_owner_cut_enabled_flag(netuid, enabled);
            log::debug!("OwnerCutEnabledSet( netuid: {netuid:?}, enabled: {enabled:?} ) ");

            Ok(())
        }

        /// Set whether subnet owner cut is auto-locked for a subnet.
        /// It is only callable by root and subnet owner.
        #[pallet::call_index(95)]
        #[pallet::weight((
            Weight::from_parts(25_000_000, 0)
                .saturating_add(T::DbWeight::get().reads(4))
                .saturating_add(T::DbWeight::get().writes(1)),
            DispatchClass::Operational,
            Pays::Yes,
        ))]
        pub fn sudo_set_owner_cut_auto_lock_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            pallet_subtensor::Pallet::<T>::ensure_subnet_owner_or_root(origin, netuid)?;
            pallet_subtensor::Pallet::<T>::ensure_admin_window_open(netuid)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(!netuid.is_root(), Error::<T>::NotPermittedOnRootSubnet);

            pallet_subtensor::Pallet::<T>::set_owner_cut_auto_lock_enabled(netuid, enabled);
            log::debug!("OwnerCutAutoLockEnabledSet( netuid: {netuid:?}, enabled: {enabled:?} ) ");

            Ok(())
        }

        /// Enables or disables subnet pool-side emission for a subnet.
        ///
        /// This does not remove the subnet from emission share calculation and does not
        /// change `alpha_out`, owner cut, root proportion, pending server emission, or
        /// pending validator emission. It only zeros the pool-side `alpha_in`, `tao_in`,
        /// and `excess_tao` chain-buy paths.
        #[pallet::call_index(94)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_subnet_emission_enabled())]
        pub fn sudo_set_subnet_emission_enabled(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                pallet_subtensor::Pallet::<T>::if_subnet_exist(netuid),
                Error::<T>::SubnetDoesNotExist
            );
            ensure!(!netuid.is_root(), Error::<T>::NotPermittedOnRootSubnet);

            pallet_subtensor::SubnetEmissionEnabled::<T>::insert(netuid, enabled);
            Self::deposit_event(Event::SubnetEmissionEnabledSet { netuid, enabled });
            log::debug!("SubnetEmissionEnabledSet( netuid: {netuid:?}, enabled: {enabled:?} )");

            Ok(())
        }

        /// Sets the per-block cap on subnet epochs (dynamic tempo throttle).
        #[pallet::call_index(96)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::sudo_set_max_epochs_per_block())]
        pub fn sudo_set_max_epochs_per_block(
            origin: OriginFor<T>,
            max_epochs_per_block: u8,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(max_epochs_per_block >= 1, Error::<T>::ValueNotInBounds);
            pallet_subtensor::Pallet::<T>::set_max_epochs_per_block(max_epochs_per_block);

            Ok(())
        }
    }
}

impl<T: Config> sp_runtime::BoundToRuntimeAppPublic for Pallet<T> {
    type Public = <T as Config>::AuthorityId;
}

// Interfaces to interact with other pallets
use sp_runtime::BoundedVec;

pub trait AuraInterface<AuthorityId, MaxAuthorities> {
    fn change_authorities(new: BoundedVec<AuthorityId, MaxAuthorities>);
}

impl<A, M> AuraInterface<A, M> for () {
    fn change_authorities(_: BoundedVec<A, M>) {}
}

pub trait GrandpaInterface<Runtime>
where
    Runtime: frame_system::Config,
{
    fn schedule_change(
        next_authorities: AuthorityList,
        in_blocks: BlockNumberFor<Runtime>,
        forced: Option<BlockNumberFor<Runtime>>,
    ) -> DispatchResult;
}

impl<R> GrandpaInterface<R> for ()
where
    R: frame_system::Config,
{
    fn schedule_change(
        _next_authorities: AuthorityList,
        _in_blocks: BlockNumberFor<R>,
        _forced: Option<BlockNumberFor<R>>,
    ) -> DispatchResult {
        Ok(())
    }
}
