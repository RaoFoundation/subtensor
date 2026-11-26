//! Synthetic-mint shorts and pool-lifted longs.
//!
//! The normative spec is `docs/SPEC.md` and the code design is `docs/DESIGN-v2.md`.
//! This crate currently holds the pure numeric core ([`math`]), the root-settable parameter
//! groups with their pending-apply cycle ([`params`], SPEC 2.1) and the price rings
//! ([`prices`], SPEC P6, P12, P13, section 9). Positions build on them in later modules.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod math;
pub mod migrations;
pub mod params;
pub mod pool;
pub mod prices;
pub mod weights;

#[cfg(any(test, feature = "try-runtime"))]
mod try_state;

#[cfg(test)]
mod tests;

pub use pallet::*;
pub use pool::LendingPoolInterface;
pub use weights::WeightInfo;

// The pallet macros expand to `expect` on storage and error metadata.
#[allow(clippy::expect_used)]
#[frame_support::pallet]
pub mod pallet {
    use frame_support::{PalletId, pallet_prelude::*};
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::UniqueSaturatedInto;
    use subtensor_runtime_common::NetUid;

    use crate::{
        LendingPoolInterface, WeightInfo,
        math::fixed::Price,
        migrations,
        params::{DeregParams, LendingParams, LongParams, ShortParams},
        prices::{RankState, TwapState},
    };

    /// Storage layout version; v1 is the initialisation in [`migrations`].
    const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Read access to subnet pools, implemented by `pallet_subtensor`.
        type Pool: LendingPoolInterface<Self::AccountId>;

        /// Weight information for the calls and hooks of this pallet.
        type WeightInfo: WeightInfo;

        /// Seed of the pallet's custody accounts.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// Ceiling for `DeregParams::max_long_positions_per_subnet` (SPEC 2.1, 7.4): 1,024.
        /// A runtime constant because the prune weight is benchmarked against it.
        #[pallet::constant]
        type MaxLongPositionsCeiling: Get<u32>;
    }

    /// Shorts on (SPEC P1). False until root enables lending.
    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;

    /// Longs on (SPEC L1).
    #[pallet::storage]
    pub type LongsEnabled<T> = StorageValue<_, bool, ValueQuery>;

    /// Live parameters shared by both sides (SPEC 2.1).
    #[pallet::storage]
    pub type LendingParamsStore<T> = StorageValue<_, LendingParams, ValueQuery>;

    /// Live short-only parameters (SPEC 2.1).
    #[pallet::storage]
    pub type ShortParamsStore<T> = StorageValue<_, ShortParams, ValueQuery>;

    /// Live long-only parameters (SPEC 2.1).
    #[pallet::storage]
    pub type LongParamsStore<T> = StorageValue<_, LongParams, ValueQuery>;

    /// Live dereg parameters (SPEC 2.1).
    #[pallet::storage]
    pub type DeregParamsStore<T> = StorageValue<_, DeregParams, ValueQuery>;

    /// `LendingParams` written by `set_lending_params`, applied in the next `on_initialize`.
    #[pallet::storage]
    pub type PendingLendingParams<T> = StorageValue<_, LendingParams, OptionQuery>;

    /// `ShortParams` written by `set_short_params`, applied in the next `on_initialize`.
    #[pallet::storage]
    pub type PendingShortParams<T> = StorageValue<_, ShortParams, OptionQuery>;

    /// `LongParams` written by `set_long_params`, applied in the next `on_initialize`.
    #[pallet::storage]
    pub type PendingLongParams<T> = StorageValue<_, LongParams, OptionQuery>;

    /// `DeregParams` written by `set_dereg_params`, applied in the next `on_initialize`.
    #[pallet::storage]
    pub type PendingDeregParams<T> = StorageValue<_, DeregParams, OptionQuery>;

    /// TWAP samples per subnet, read as a lower median (SPEC P6).
    #[pallet::storage]
    pub type TwapRing<T> = StorageMap<_, Identity, NetUid, TwapState, OptionQuery>;

    /// Daily means and the open day behind the dereg-ranking SMA (SPEC P13).
    #[pallet::storage]
    pub type RankRing<T> = StorageMap<_, Identity, NetUid, RankState, OptionQuery>;

    /// Published ranking SMA: the mean of the subnet's `RankRing` days (SPEC P13).
    #[pallet::storage]
    pub type RankPrice<T> = StorageMap<_, Identity, NetUid, Price, OptionQuery>;

    /// Current rank-ring generation; bumped by every re-seed of all rings (SPEC section 9).
    #[pallet::storage]
    pub type RankGeneration<T> = StorageValue<_, u32, ValueQuery>;

    /// Generation each subnet's `RankRing` was seeded under (SPEC I25).
    #[pallet::storage]
    pub type RankRingGeneration<T> = StorageMap<_, Identity, NetUid, u32, OptionQuery>;

    /// `T_ema`: EMA of pool TAO in rao, used only in the dereg rate r (SPEC P12, 8.7).
    #[pallet::storage]
    pub type PoolTaoEma<T> = StorageMap<_, Identity, NetUid, u64, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(crate) fn deposit_event)]
    pub enum Event<T: Config> {
        /// `Enabled` was set by root.
        EnabledSet { enabled: bool },
        /// `LongsEnabled` was set by root.
        LongsEnabledSet { enabled: bool },
        /// A pending `LendingParams` became live.
        LendingParamsSet {
            old: LendingParams,
            new: LendingParams,
        },
        /// A pending `ShortParams` became live.
        ShortParamsSet { old: ShortParams, new: ShortParams },
        /// A pending `LongParams` became live.
        LongParamsSet { old: LongParams, new: LongParams },
        /// A pending `DeregParams` became live.
        DeregParamsSet { old: DeregParams, new: DeregParams },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// A parameter group violates a SPEC 2.1 bound or cross-check; nothing was written.
        InvalidParams,
        /// The subnet has no TWAP ring, or no rank ring of the current generation (SPEC I25).
        /// There is no fallback price.
        PriceRingMissing,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Applies pending parameters first, then samples prices for every live subnet,
        /// whether lending is enabled or not (SPEC 2.1, section 9 item 4).
        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            let subnets = T::Pool::live_subnets();
            Self::apply_pending_params(&subnets);
            Self::sample_prices(now.unique_saturated_into(), &subnets);
            T::WeightInfo::on_initialize(u32::try_from(subnets.len()).unwrap_or(u32::MAX))
        }

        fn on_runtime_upgrade() -> Weight {
            migrations::migrate_to_v1::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    // Call indices 0..=19 are reserved for the user calls (DESIGN-v2 section 6).
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Turns lending on or off (SPEC P1). Turning it on re-seeds every rank ring from the
        /// emission moving price in this call (SPEC section 9 item 3, I25).
        #[pallet::call_index(20)]
        #[pallet::weight(T::WeightInfo::set_enabled(T::Pool::subnet_limit()))]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_set_enabled(enabled);
            Ok(())
        }

        /// Turns longs on or off (SPEC L1).
        #[pallet::call_index(21)]
        #[pallet::weight(T::WeightInfo::set_longs_enabled())]
        pub fn set_longs_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            LongsEnabled::<T>::put(enabled);
            Self::deposit_event(Event::LongsEnabledSet { enabled });
            Ok(())
        }

        /// Replaces `LendingParams` from the next block (SPEC 2.1).
        #[pallet::call_index(22)]
        #[pallet::weight(T::WeightInfo::set_params())]
        pub fn set_lending_params(origin: OriginFor<T>, params: LendingParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_lending_params(params)
        }

        /// Replaces `ShortParams` from the next block (SPEC 2.1).
        #[pallet::call_index(23)]
        #[pallet::weight(T::WeightInfo::set_params())]
        pub fn set_short_params(origin: OriginFor<T>, params: ShortParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_short_params(params)
        }

        /// Replaces `LongParams` from the next block (SPEC 2.1).
        #[pallet::call_index(24)]
        #[pallet::weight(T::WeightInfo::set_params())]
        pub fn set_long_params(origin: OriginFor<T>, params: LongParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_long_params(params)
        }

        /// Replaces `DeregParams` from the next block (SPEC 2.1).
        #[pallet::call_index(25)]
        #[pallet::weight(T::WeightInfo::set_params())]
        pub fn set_dereg_params(origin: OriginFor<T>, params: DeregParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_dereg_params(params)
        }
    }
}
