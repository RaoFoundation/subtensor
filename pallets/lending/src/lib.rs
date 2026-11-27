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
pub mod positions;
pub mod prices;
pub mod types;
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
        positions::Grow,
        prices::{RankState, TwapState},
        types::{Position, PositionId, Side},
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

    /// Every open position, keyed by holder, subnet and side (DESIGN-v2 section 3).
    #[pallet::storage]
    pub type Positions<T> = StorageMap<
        _,
        Blake2_128Concat,
        PositionId<<T as frame_system::Config>::AccountId>,
        Position<BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// `SyntheticOutstanding`: the short book `Q`, synthetic alpha owed back (SPEC I1).
    #[pallet::storage]
    pub type SyntheticOutstanding<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// `LongLockedAlpha`: the alpha held in custody by all longs of the subnet (SPEC I12).
    #[pallet::storage]
    pub type LongLockedAlpha<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// `LongLiftedTao`: the debt owed to the pool by all longs of the subnet (SPEC I12).
    #[pallet::storage]
    pub type LongLiftedTao<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// Open longs per subnet, bounded by `DeregParams::max_long_positions_per_subnet` (SPEC 7.4).
    #[pallet::storage]
    pub type LongPositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;

    /// Net minted alpha of the block it is tagged with, per subnet (SPEC P8). Read as zero in
    /// any other block.
    #[pallet::storage]
    pub type BlockNetMinted<T> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, i128), OptionQuery>;

    /// Net lifted TAO of the block it is tagged with, per subnet (SPEC L8). Read as zero in any
    /// other block.
    #[pallet::storage]
    pub type BlockNetLifted<T> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, i128), OptionQuery>;

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
        /// A short or long position was opened.
        PositionOpened {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            size: u64,
            value: u64,
            collateral: u64,
        },
        /// A short or long position grew.
        PositionIncreased {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            size: u64,
            value: u64,
            collateral: u64,
        },
        /// Alpha was closed out of a position; `value` is the TAO paid by the buy-back (short)
        /// or the TAO received for the sale (long). `full` is true when the position closed.
        PositionClosed {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            alpha: u64,
            value: u64,
            full: bool,
        },
        /// Collateral was added to a position.
        CollateralAdded {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            amount: u64,
        },
        /// Collateral was withdrawn from a position.
        CollateralWithdrawn {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            amount: u64,
        },
        /// Interest was charged from a position's collateral to the pool (SPEC P15).
        InterestCharged {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            amount: u64,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// A parameter group violates a SPEC 2.1 bound or cross-check; nothing was written.
        InvalidParams,
        /// The subnet has no TWAP ring, or no rank ring of the current generation (SPEC I25).
        /// There is no fallback price.
        PriceRingMissing,
        /// Shorts are off (SPEC P1).
        Disabled,
        /// Longs are off (SPEC L1).
        LongsDisabled,
        /// A size, amount or quoted value is zero, or a close asks for more than is held.
        InvalidAmount,
        /// The holder already has a position on this side of the subnet.
        PositionExists,
        /// The holder has no position on this side of the subnet.
        NoPosition,
        /// The holder has a position on the other side of the subnet (one side per holder).
        OppositeSideOpen,
        /// The subnet's TAO reserve is below `min_pool_tao` (SPEC P18).
        PoolTooShallow,
        /// The mint would take alpha issuance past 21,000,000 (SPEC P23, I21).
        AlphaSupplyCap,
        /// The block's net minted alpha would exceed `open_limit_bps` of the pool (SPEC P8).
        OpenImpactTooHigh,
        /// The position's notional is below `min_notional` (SPEC P10, L10).
        BelowMinNotional,
        /// The holder's free TAO cannot cover the collateral and deposit.
        InsufficientBalance,
        /// The quote moved past the caller's bound: the price is worse than `min_out` or the
        /// buy-back costs more than `max_cost`.
        SlippageExceeded,
        /// The short's collateral and proceeds fall below its coverage need (SPEC P2).
        CoverageNotMet,
        /// The long's collateral falls below its coverage need (SPEC L2).
        LongCoverageNotMet,
        /// The block's aggregate lift would exceed `lift_cap_bps` of the pool (SPEC L5).
        LiftCapExceeded,
        /// The block's net lifted TAO would exceed `open_limit_bps` of the pool (SPEC L8).
        LongOpenLimit,
        /// The subnet already has `max_long_positions_per_subnet` longs (SPEC 7.4).
        TooManyLongPositions,
        /// The position's collateral and proceeds cannot pay the buy-back (short close).
        InsufficientFunds,
        /// The sale plus the collateral cannot repay the long's debt (SPEC 7.1.4).
        CloseInsolvent,
        /// The withdrawal would take collateral below the coverage need (SPEC P20, L12).
        WithdrawBreaksCoverage,
        /// The withdrawal would leave less than the coverage need plus the interest buffer.
        WithdrawBufferBreached,
        /// A fixed-point amount overflowed; the operation is refused.
        Overflow,
        /// The pool moved a different amount than the quote the pallet gated on.
        QuoteMismatch,
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

    // Call indices 0..=19 are reserved for the user calls (DESIGN-v2 section 6); 5 and 11 are
    // the poke calls, not part of this task.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Opens a short: mints `q` alpha, sells it, and pays the proceeds to custody (SPEC P1-P23).
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::open_position())]
        pub fn open_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            q: u64,
            collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_grow(
                &who,
                Grow {
                    netuid,
                    side: Side::Short,
                    size: q,
                    collateral,
                    min_out: min_tao_out,
                    opening: true,
                },
            )
        }

        /// Adds `q` to an open short (SPEC P1-P23).
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::increase_position())]
        pub fn increase_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            q: u64,
            collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_grow(
                &who,
                Grow {
                    netuid,
                    side: Side::Short,
                    size: q,
                    collateral,
                    min_out: min_tao_out,
                    opening: false,
                },
            )
        }

        /// Buys back up to `alpha` of a short, retiring it (SPEC 1.4). Pays at most `max_tao_in`.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::close_position())]
        pub fn close_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha: u64,
            max_tao_in: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_close(&who, netuid, Side::Short, alpha, max_tao_in)
        }

        /// Adds collateral to a short (SPEC P19).
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::add_collateral())]
        pub fn add_collateral(origin: OriginFor<T>, netuid: NetUid, amount: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_add_collateral(&who, netuid, Side::Short, amount)
        }

        /// Withdraws collateral from a short if coverage and the interest buffer still hold
        /// (SPEC P2, P20).
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::withdraw_collateral())]
        pub fn withdraw_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            amount: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_withdraw_collateral(&who, netuid, Side::Short, amount)
        }

        /// Opens a long: lifts `lift` TAO from the pool and buys alpha that custody locks
        /// (SPEC L1-L12). Receives at least `min_alpha_out`.
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::open_position())]
        pub fn open_long(
            origin: OriginFor<T>,
            netuid: NetUid,
            lift: u64,
            collateral: u64,
            min_alpha_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_grow(
                &who,
                Grow {
                    netuid,
                    side: Side::Long,
                    size: lift,
                    collateral,
                    min_out: min_alpha_out,
                    opening: true,
                },
            )
        }

        /// Lifts `lift` more TAO into an open long (SPEC L1-L12).
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::increase_position())]
        pub fn increase_long(
            origin: OriginFor<T>,
            netuid: NetUid,
            lift: u64,
            collateral: u64,
            min_alpha_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_grow(
                &who,
                Grow {
                    netuid,
                    side: Side::Long,
                    size: lift,
                    collateral,
                    min_out: min_alpha_out,
                    opening: false,
                },
            )
        }

        /// Sells up to `alpha` of a long's locked alpha and repays the debt (SPEC 7.1.4).
        /// Receives at least `min_tao_out` for the sale.
        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::close_position())]
        pub fn close_long(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_close(&who, netuid, Side::Long, alpha, min_tao_out)
        }

        /// Adds collateral to a long (SPEC L11).
        #[pallet::call_index(9)]
        #[pallet::weight(T::WeightInfo::add_collateral())]
        pub fn add_long_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            amount: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_add_collateral(&who, netuid, Side::Long, amount)
        }

        /// Withdraws collateral from a long if coverage and the interest buffer still hold
        /// (SPEC L2, L12).
        #[pallet::call_index(10)]
        #[pallet::weight(T::WeightInfo::withdraw_collateral())]
        pub fn withdraw_long_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            amount: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_withdraw_collateral(&who, netuid, Side::Long, amount)
        }

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
