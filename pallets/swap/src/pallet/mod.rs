//! FRAME pallet definition for the TAO↔alpha weighted-balancer AMM.
//!
//! Storage names, call indices, and event/error variant order are frozen wire surfaces.
//! Swap execution lives in `impls` / `swap_step`; pool math in `balancer`.

use core::num::NonZeroU64;

use frame_support::{PalletId, pallet_prelude::*, traits::Get};
use frame_system::pallet_prelude::*;
use subtensor_runtime_common::{
    AlphaBalance, BalanceOps, NetUid, SubnetInfo, TaoBalance, TokenReserve,
};

use crate::{pallet::balancer::Balancer, weights::WeightInfo};
pub use pallet::*;
use subtensor_macros::freeze_struct;

mod balancer;
mod hooks;
mod impls;
pub mod migrations;
mod swap_step;
#[cfg(test)]
mod tests;

/// Max length of a `HasMigrationRun` key (`BoundedVec<u8, …>`).
type MigrationKeyMaxLen = ConstU32<128>;

#[allow(clippy::module_inception)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
mod pallet {
    use super::*;
    use frame_system::ensure_root;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Runtime configuration for the swap pallet (reserves, fee bounds, protocol account).
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Subnet existence / mechanism queries (`mechanism == 1` ⇒ dynamic AMM).
        type SubnetInfo: SubnetInfo<Self::AccountId>;

        /// Price-active TAO reserve provider (`SubnetTAO` in the full runtime).
        type TaoReserve: TokenReserve<TaoBalance>;

        /// Price-active alpha reserve provider (`SubnetAlphaIn` in the full runtime).
        type AlphaReserve: TokenReserve<AlphaBalance>;

        /// Coldkey/hotkey balance ops used by deprecated LP paths and fee sinks.
        type BalanceOps: BalanceOps<Self::AccountId>;

        /// PalletId used to derive the protocol-owned account for this swap pallet.
        #[pallet::constant]
        type ProtocolId: Get<PalletId>;

        /// Upper bound for [`FeeRate`] (u16-normalized); root cannot set above this.
        #[pallet::constant]
        type MaxFeeRate: Get<u16>;

        /// Minimum liquidity considered safe for rounding / integer math.
        #[pallet::constant]
        type MinimumLiquidity: Get<u64>;

        /// Floor for TAO and alpha reserves before a swap may execute.
        #[pallet::constant]
        type MinimumReserve: Get<NonZeroU64>;

        /// Extrinsic weight functions (generated / benchmarked).
        type WeightInfo: WeightInfo;

        /// Cross-pallet subnet/hotkey setup for runtime benchmarks.
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

    /// Default [`FeeRate`]: `33 / u16::MAX` ≈ 0.05%.
    #[pallet::type_value]
    pub fn DefaultFeeRate() -> u16 {
        33 // ~0.05 %
    }

    /// Per-subnet swap fee rate, u16-normalized (`rate / u16::MAX` of the input).
    #[pallet::storage]
    pub type FeeRate<T> = StorageMap<_, Twox64Concat, NetUid, u16, ValueQuery, DefaultFeeRate>;

    ////////////////////////////////////////////////////
    // Balancer (PalSwap) maps and variables

    /// Default [`Balancer`]: equal 0.5 / 0.5 base/quote weights.
    #[pallet::type_value]
    pub fn DefaultBalancer() -> Balancer {
        Balancer::default()
    }

    /// Per-subnet weighted-balancer state (stores quote weight; base = 1 − quote).
    #[pallet::storage]
    pub type SwapBalancer<T> =
        StorageMap<_, Twox64Concat, NetUid, Balancer, ValueQuery, DefaultBalancer>;

    /// Whether the balancer pool for `netuid` has been initialized (lazy via swaps / init).
    #[pallet::storage]
    pub type PalSwapInitialized<T> = StorageMap<_, Twox64Concat, NetUid, bool, ValueQuery>;

    /// Materialized TAO that could not become price-active without violating weight bounds.
    #[pallet::storage]
    pub type BalancerTaoReservoir<T> = StorageMap<_, Twox64Concat, NetUid, TaoBalance, ValueQuery>;

    /// Materialized alpha that could not become price-active without violating weight bounds.
    #[pallet::storage]
    pub type BalancerAlphaReservoir<T> =
        StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// Idempotency flags for on-runtime-upgrade migrations (keyed by migration name bytes).
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    /// Leftover alpha scraps from protocol fee claims (legacy; largely unused post-v3 migration).
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

        /// The extrinsic is deprecated
        Deprecated,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Set the per-subnet swap [`FeeRate`] (u16-normalized; e.g. ~196 ≈ 0.3%).
        ///
        /// Root-only. Requires the subnet to exist and `rate <= MaxFeeRate`.
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

/// Deprecated Uniswap-v3 tick index retained only for call/SCALE compatibility of LP stubs.
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

/// Deprecated Uniswap-v3 LP position id retained only for call/SCALE compatibility of LP stubs.
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
