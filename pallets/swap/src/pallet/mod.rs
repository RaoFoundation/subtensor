use core::num::NonZeroU64;

use frame_support::{PalletId, pallet_macros::import_section, pallet_prelude::*, traits::Get};
use frame_system::pallet_prelude::*;
use subtensor_runtime_common::{
    AlphaBalance, BalanceOps, NetUid, SubnetInfo, TaoBalance, TokenReserve,
};

use crate::{pallet::balancer::Balancer, weights::WeightInfo};
pub use pallet::*;
use subtensor_macros::freeze_struct;
pub use superellipse::Superellipse;

mod balancer;
mod hooks;
mod impls;
#[cfg(test)]
mod migration_tests;
pub mod migrations;
pub(crate) mod superellipse;
mod swap_step;
#[cfg(test)]
mod tests;

// Define a maximum length for the migration key
type MigrationKeyMaxLen = ConstU32<128>;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);
pub const SLIPPAGE_REFERENCE_TAO: u64 = 500_000_000_000;

#[allow(clippy::module_inception)]
#[import_section(hooks::hooks)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
mod pallet {
    use super::*;
    use frame_system::ensure_root;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Implementor of
        /// [`SubnetInfo`](subtensor_swap_interface::SubnetInfo).
        type SubnetInfo: SubnetInfo<Self::AccountId>;

        /// Tao reserves info.
        type TaoReserve: TokenReserve<TaoBalance>;

        /// Alpha reserves info.
        type AlphaReserve: TokenReserve<AlphaBalance>;

        /// Implementor of
        /// [`BalanceOps`](subtensor_swap_interface::BalanceOps).
        type BalanceOps: BalanceOps<Self::AccountId>;

        /// This type is used to derive protocol accoun ID.
        #[pallet::constant]
        type ProtocolId: Get<PalletId>;

        /// The maximum fee rate that can be set
        #[pallet::constant]
        type MaxFeeRate: Get<u16>;

        /// Minimum liquidity that is safe for rounding and integer math.
        #[pallet::constant]
        type MinimumLiquidity: Get<u64>;

        /// Minimum reserve for tao and alpha
        #[pallet::constant]
        type MinimumReserve: Get<NonZeroU64>;

        /// Runtime-provided compute envelope for one bounded curve calibration
        /// or extraction. Composed from already measured swap operation weights;
        /// caller-owned dispatches must charge this in addition to storage reads.
        type CalibrationWeight: Get<Weight>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// Helper for setting up cross-pallet state needed by benchmarks.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: BenchmarkHelper<Self::AccountId>;
    }

    /// Benchmark setup helper — the runtime wires this to set state in other pallets.
    #[cfg(feature = "runtime-benchmarks")]
    pub trait BenchmarkHelper<AccountId> {
        fn setup_subnet(netuid: NetUid);
        fn register_hotkey(hotkey: &AccountId, coldkey: &AccountId);
    }

    #[cfg(feature = "runtime-benchmarks")]
    impl<AccountId> BenchmarkHelper<AccountId> for () {
        fn setup_subnet(_netuid: NetUid) {}
        fn register_hotkey(_hotkey: &AccountId, _coldkey: &AccountId) {}
    }

    /// Default fee rate if not set
    #[pallet::type_value]
    pub fn DefaultFeeRate() -> u16 {
        33 // ~0.05 %
    }

    /// The fee rate applied to swaps per subnet, normalized value between 0 and u16::MAX
    #[pallet::storage]
    pub type FeeRate<T> = StorageMap<_, Twox64Concat, NetUid, u16, ValueQuery, DefaultFeeRate>;

    ////////////////////////////////////////////////////
    // Balancer (PalSwap) maps and variables

    /// Default reserve weight
    #[pallet::type_value]
    pub fn DefaultBalancer() -> Balancer {
        Balancer::default()
    }

    /// Archived Balancer parameters, retained for migration and legacy queries.
    #[pallet::storage]
    pub type SwapBalancer<T> =
        StorageMap<_, Twox64Concat, NetUid, Balancer, ValueQuery, DefaultBalancer>;

    /// Fixed exponent-two translated superellipse used by live dynamic pools.
    #[pallet::storage]
    pub type SwapSuperellipse<T> = StorageMap<_, Twox64Concat, NetUid, Superellipse, OptionQuery>;

    #[pallet::type_value]
    pub fn DefaultMinimumSellImpactBps() -> u16 {
        100
    }

    /// Minimum ending spot-price fall at calibration for the fixed 500-TAO
    /// opening-price-equivalent alpha sale. Parameters stay fixed after that.
    #[pallet::storage]
    pub type MinimumSellImpactBps<T> =
        StorageMap<_, Twox64Concat, NetUid, u16, ValueQuery, DefaultMinimumSellImpactBps>;

    /// The complete reference sale cannot execute on this pool's safe curve.
    #[pallet::storage]
    pub type SlippageReferenceLimited<T> = StorageMap<_, Twox64Concat, NetUid, bool, ValueQuery>;

    /// Cumulative active balances extracted into separate lending custody.
    /// This independently witnesses active-plus-extracted migration conservation.
    #[pallet::storage]
    pub type ExtractedReserves<T> =
        StorageMap<_, Twox64Concat, NetUid, (AlphaBalance, TaoBalance), ValueQuery>;

    /// Storage to determine whether balancer swap was initialized for a specific subnet.
    #[pallet::storage]
    pub type PalSwapInitialized<T> = StorageMap<_, Twox64Concat, NetUid, bool, ValueQuery>;

    /// TAO protocol liquidity that could not be injected without exceeding balancer weight bounds.
    #[pallet::storage]
    pub type BalancerTaoReservoir<T> = StorageMap<_, Twox64Concat, NetUid, TaoBalance, ValueQuery>;

    /// Alpha protocol liquidity that could not be injected without exceeding balancer weight bounds.
    #[pallet::storage]
    pub type BalancerAlphaReservoir<T> =
        StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// --- Storage for migration run status
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    /// Alpha reservoir for scraps of protocol claimed fees.
    #[pallet::storage]
    pub type ScrapReservoirAlpha<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Event emitted when the fee rate has been updated for a subnet
        FeeRateSet { netuid: NetUid, rate: u16 },
    }

    #[pallet::error]
    #[derive(PartialEq)]
    pub enum Error<T> {
        /// The fee rate is too high
        FeeRateTooHigh,

        /// The provided amount is insufficient for the swap.
        InsufficientInputAmount,

        /// The provided liquidity is insufficient for the operation.
        InsufficientLiquidity,

        /// The operation would exceed the price limit.
        PriceLimitExceeded,

        /// The caller does not have enough balance for the operation.
        InsufficientBalance,

        /// The provided tick range is invalid.
        InvalidTickRange,

        /// Provided liquidity parameter is invalid (likely too small)
        InvalidLiquidityValue,

        /// Reserves too low for operation.
        ReservesTooLow,

        /// The subnet does not exist.
        MechanismDoesNotExist,

        /// The subnet does not have subtoken enabled
        SubtokenDisabled,

        /// Swap reserves are too imbalanced
        ReservesOutOfBalance,

        /// Swap input is too large relative to input-side liquidity
        SwapInputTooLarge,

        /// The requested calibration is outside (0, 10000) basis points.
        InvalidSlippageTarget,

        /// The extrinsic is deprecated
        Deprecated,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Set the fee rate for swaps on a specific subnet (normalized value).
        /// For example, 0.3% is approximately 196.
        ///
        /// Only callable by the admin origin
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_fee_rate())]
        pub fn set_fee_rate(origin: OriginFor<T>, netuid: NetUid, rate: u16) -> DispatchResult {
            ensure_root(origin)?;

            // Ensure that the subnet exists.
            ensure!(
                T::SubnetInfo::exists(netuid.into()),
                Error::<T>::MechanismDoesNotExist
            );

            ensure!(rate <= T::MaxFeeRate::get(), Error::<T>::FeeRateTooHigh);

            FeeRate::<T>::insert(netuid, rate);

            Self::deposit_event(Event::FeeRateSet { netuid, rate });

            Ok(())
        }

        /// DEPRECATED
        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::toggle_user_liquidity())]
        pub fn toggle_user_liquidity(
            _origin: OriginFor<T>,
            _netuid: NetUid,
            _enable: bool,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// DEPRECATED
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::add_liquidity())]
        pub fn add_liquidity(
            _origin: OriginFor<T>,
            _hotkey: T::AccountId,
            _netuid: NetUid,
            _tick_low: TickIndex,
            _tick_high: TickIndex,
            _liquidity: u64,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// DEPRECATED
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::remove_liquidity())]
        pub fn remove_liquidity(
            _origin: OriginFor<T>,
            _hotkey: T::AccountId,
            _netuid: NetUid,
            _position_id: PositionId,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// DEPRECATED
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::modify_position())]
        #[deprecated(note = "Deprecated, user liquidity is permanently disabled")]
        pub fn modify_position(
            _origin: OriginFor<T>,
            _hotkey: T::AccountId,
            _netuid: NetUid,
            _position_id: PositionId,
            _liquidity_delta: i64,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// DEPRECATED
        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::disable_lp())]
        #[deprecated(note = "Deprecated, user liquidity is permanently disabled")]
        pub fn disable_lp(_origin: OriginFor<T>) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }
    }
}

/// Struct representing a tick index, DEPRECATED
#[freeze_struct("7c280c2b3bbbb33e")]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    Decode,
    Encode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub struct TickIndex(i32);

/// Struct representing a liquidity position ID, DEPRECATED
#[freeze_struct("e695cd6455c3f0cb")]
#[derive(
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct PositionId(u128);
