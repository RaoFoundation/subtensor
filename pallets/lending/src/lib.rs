//! Synthetic-mint shorts and pool-lifted longs.
//!
//! The normative spec is `docs/SPEC.md` and the code design is `docs/DESIGN-v2.md`.
//! This crate currently holds the pure numeric core ([`math`]), the root-settable parameter
//! groups with their pending-apply cycle ([`params`], SPEC 2.1) and the price rings
//! ([`prices`], SPEC P6, P12, P13, section 9), the position operations of both sides
//! ([`positions`]) and the interest queue ([`interest`], SPEC P15).
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod dissolution;
pub mod hooks;
pub mod interest;
pub mod long_weights;
pub mod math;
pub mod migrations;
pub mod params;
pub mod pool;
pub mod positions;
pub mod prices;
pub mod types;
pub mod views;
pub mod weights;

// Always built: the integration tests of the pool adapter run the invariants too.
mod try_state;

#[cfg(test)]
mod tests;

pub use hooks::LendingInterface;
pub use pallet::*;
#[cfg(feature = "runtime-benchmarks")]
pub use pool::BenchmarkHelper;
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
        dissolution::{Dissolving, Settlement},
        long_weights,
        math::fixed::Price,
        migrations,
        params::{DeregParams, LendingParams, LongParams, ShortParams},
        positions::Grow,
        prices::{RankState, TwapState},
        types::{InterestTotal, Position, PositionId, Side},
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

        /// The pools' pricing function: every quote the pallet gates on, and the result it
        /// requires each swap leg of `Pool` to return. The runtime sets the swap pallet's
        /// Balancer math; the model and the tests use `ConstantProduct`.
        type Curve: crate::math::curve::Curve;

        /// Weight information for the calls and hooks of this pallet.
        type WeightInfo: WeightInfo;

        /// Seed of the pallet's custody accounts.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// Ceiling for `DeregParams::max_long_positions_per_subnet` (SPEC 2.1, 7.4): 1,024.
        /// A runtime constant because the prune weight is benchmarked against it.
        #[pallet::constant]
        type MaxLongPositionsCeiling: Get<u32>;

        /// Scheduled interest charges `on_initialize` runs per block (SPEC P15); the rest of a
        /// larger backlog carries over to the following blocks.
        #[pallet::constant]
        type MaxChargesPerBlock: Get<u32>;

        /// Builds the pools the benchmarks run against.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: crate::BenchmarkHelper<Self::AccountId>;
    }

    /// Shorts on (SPEC P1). False until root enables lending.
    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;

    /// Longs on (SPEC L1).
    #[pallet::storage]
    pub type LongsEnabled<T> = StorageValue<_, bool, ValueQuery>;

    /// Named one-off migrations that have run; the value is always `true`. The initialisation
    /// is guarded by this and not by the `StorageVersion`: FRAME sets the storage version of a
    /// pallet that is new to a chain before any migration runs, so a version guard would skip
    /// it on the very upgrade that adds the pallet.
    #[pallet::storage]
    pub type HasMigrationRun<T> =
        StorageMap<_, Identity, BoundedVec<u8, ConstU32<32>>, bool, ValueQuery>;

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

    /// `LendingParams` written by `set_lending_params` with the block from which they are live
    /// (`apply_at`, SPEC 2.1). A call that tightens the live group sets `apply_at` to its block
    /// plus the `rate_delay_blocks` then in force; any other call sets the next block.
    /// `on_initialize` of `apply_at` makes the group live and clears the entry; a later call
    /// replaces it.
    #[pallet::storage]
    pub type PendingLendingParams<T> =
        StorageValue<_, (LendingParams, BlockNumberFor<T>), OptionQuery>;

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

    /// The open positions by subnet and side: one key per entry of [`Positions`] and no other
    /// (`try_state`). An open adds the key, a full close, a default and a dissolution settlement
    /// remove it, and a coldkey swap moves it, each in the same step as the position. The
    /// settlement of a dissolved subnet walks the `(netuid, Short)` prefix, so it reads that
    /// subnet's shorts and no other position (`dissolution`).
    #[pallet::storage]
    pub type PositionsByNetuid<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,
            NMapKey<Identity, Side>,
            NMapKey<Blake2_128Concat, <T as frame_system::Config>::AccountId>,
        ),
        (),
        OptionQuery,
    >;

    /// The custody account of a position a coldkey swap moved to a new holder. A position
    /// without an entry keeps the account derived from its own key (`custody_of`).
    #[pallet::storage]
    pub type Custody<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        PositionId<<T as frame_system::Config>::AccountId>,
        <T as frame_system::Config>::AccountId,
        OptionQuery,
    >;

    /// The open positions of a coldkey, on every subnet and side: at most
    /// `hooks::MAX_POSITIONS_PER_HOLDER`. The open gate enforces the cap, so a coldkey swap
    /// moves a bounded number of positions without reading any other holder's.
    #[pallet::storage]
    pub type HolderPositions<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        <T as frame_system::Config>::AccountId,
        BoundedVec<(NetUid, Side), ConstU32<{ crate::hooks::MAX_POSITIONS_PER_HOLDER }>>,
        ValueQuery,
    >;

    /// The block in which a coldkey swap moved away a position this coldkey had opened in
    /// that same block. The coldkey opens nothing more in that block: a new position would
    /// derive the custody account the moved one still uses. Cleared by its next open.
    #[pallet::storage]
    pub type SwappedOutAt<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        <T as frame_system::Config>::AccountId,
        BlockNumberFor<T>,
        OptionQuery,
    >;

    /// `SyntheticOutstanding`: the short book `Q`, synthetic alpha owed back (SPEC I1).
    #[pallet::storage]
    pub type SyntheticOutstanding<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// `ResidualAlpha`: synthetic alpha that defaulted shorts left in circulation (SPEC section
    /// 1 item 4, I2). A default moves the short's `q` here from `SyntheticOutstanding`; nothing
    /// is minted or burned. It stays part of the subnet's alpha issuance and of no staker's
    /// claim.
    #[pallet::storage]
    pub type ResidualAlpha<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// The lending record of each subnet being dissolved, from its flag until the owner refund
    /// has read it (see [`crate::dissolution`]).
    #[pallet::storage]
    pub type DissolvingSubnets<T> = StorageMap<_, Identity, NetUid, Dissolving, OptionQuery>;

    /// How far the settlement of a dissolving subnet's shorts has got: its stage, the block the
    /// shorts are accrued to, the dereg rate once it is read and the cursor of the walk over
    /// [`PositionsByNetuid`]. Written by the first settlement step and removed with the
    /// subnet's record (`on_dissolve_done`).
    #[pallet::storage]
    pub type DissolutionSettlements<T: Config> = StorageMap<
        _,
        Identity,
        NetUid,
        Settlement<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// The shorts of a dissolving subnet that the settlement walk did not settle and a later
    /// cleanup step tries again, with the number of failed attempts: a short whose accrual or
    /// settlement failed (`ShortSettlementFailed`), and a short a coldkey swap re-keyed while
    /// the walk was running (zero attempts). The entry leaves with the position.
    #[pallet::storage]
    pub type SettlementRetry<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        <T as frame_system::Config>::AccountId,
        u32,
        OptionQuery,
    >;

    /// `LongLockedAlpha`: the alpha held in custody by all longs of the subnet (SPEC I12).
    #[pallet::storage]
    pub type LongLockedAlpha<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// `LongLiftedTao`: the debt owed to the pool by all longs of the subnet (SPEC I12).
    #[pallet::storage]
    pub type LongLiftedTao<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// Open longs per subnet, bounded by `DeregParams::max_long_positions_per_subnet` (SPEC 7.4).
    #[pallet::storage]
    pub type LongPositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;

    /// Open positions per subnet, both sides (SPEC 7.5).
    #[pallet::storage]
    pub type PositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;

    /// Open positions on all subnets, both sides (SPEC 7.5).
    #[pallet::storage]
    pub type TotalPositions<T> = StorageValue<_, u32, ValueQuery>;

    /// Scheduled interest charges by block (SPEC P15). Every open position has exactly one
    /// entry, at its `charge_at` (SPEC I5).
    #[pallet::storage]
    pub type Due<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        PositionId<<T as frame_system::Config>::AccountId>,
        (),
        OptionQuery,
    >;

    /// Earliest `Due` bucket not yet drained; no `Due` entry is scheduled before it.
    #[pallet::storage]
    pub type NextDue<T> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;

    /// Consecutive scheduled charges of a position that failed and were rescheduled
    /// `RETRY_DELAY_BLOCKS` later. Cleared by the next charge that succeeds and on close.
    #[pallet::storage]
    pub type InterestRetry<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        PositionId<<T as frame_system::Config>::AccountId>,
        u32,
        ValueQuery,
    >;

    /// Cumulative interest per subnet (SPEC I19): the rao taken from collateral and the rao the
    /// pool's TAO reserve rose by across those credits. The two are equal.
    #[pallet::storage]
    pub type InterestTotals<T> = StorageMap<_, Identity, NetUid, InterestTotal, ValueQuery>;

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
        /// `set_lending_params` tightened the live group (SPEC 2.1): `new` becomes live at block
        /// `apply_at`, and until then `old` stays in force.
        LendingParamsScheduled {
            old: LendingParams,
            new: LendingParams,
            apply_at: BlockNumberFor<T>,
        },
        /// A pending `ShortParams` became live.
        ShortParamsSet { old: ShortParams, new: ShortParams },
        /// A pending `LongParams` became live.
        LongParamsSet { old: LongParams, new: LongParams },
        /// A pending `DeregParams` became live.
        DeregParamsSet { old: DeregParams, new: DeregParams },
        /// A coldkey swap moved a position, with its interest schedule and its custody
        /// account, from `old` to `new`.
        PositionRekeyed {
            old: T::AccountId,
            new: T::AccountId,
            netuid: NetUid,
            side: Side,
        },
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
        /// Interest was charged from a position's collateral to the pool's TAO reserve
        /// (SPEC P4, P15). `rate_bps` is the yearly rate the accrual read from the position
        /// (SPEC section 1 item 2, I23).
        InterestCharged {
            who: T::AccountId,
            netuid: NetUid,
            side: Side,
            to_pool: u64,
            rate_bps: u32,
        },
        /// A short defaulted at a scheduled charge that found its collateral exhausted with
        /// interest still due (SPEC section 1 item 4). Its `residual_alpha` (`q`) moved from
        /// `SyntheticOutstanding` to `ResidualAlpha`, `proceeds_to_pool` (what its custody
        /// still held apart from the deposit) was credited to the pool's TAO reserve, its
        /// deposit was burned and the unpaid interest was forgiven.
        ShortDefaulted {
            who: T::AccountId,
            netuid: NetUid,
            residual_alpha: u64,
            proceeds_to_pool: u64,
        },
        /// A short was settled when its subnet was dissolved (SPEC section 7.4 phase E). It paid
        /// `paid` (`min(q * price, C + P)`) from its collateral and proceeds into the subnet's pot,
        /// at the dereg rate `price`; the rest of its custody went back to the holder.
        ShortSettledOnDissolution {
            who: T::AccountId,
            netuid: NetUid,
            price: Price,
            paid: u64,
        },
        /// The settlement of a short at the dissolution of its subnet failed with `error`. The
        /// short is as it was, with its custody, and is queued in `SettlementRetry`: every later
        /// cleanup step of the subnet tries it again and reports it again if it fails again.
        ShortSettlementFailed {
            who: T::AccountId,
            netuid: NetUid,
            error: DispatchError,
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
        /// A fixed-point amount overflowed; the operation is refused.
        Overflow,
        /// The pool moved a different amount than the quote the pallet gated on.
        QuoteMismatch,
        /// The coldkey already holds `hooks::MAX_POSITIONS_PER_HOLDER` positions.
        TooManyPositions,
        /// A coldkey swap moved away a position this coldkey opened in this block; it opens
        /// again from the next block.
        SwappedOutThisBlock,
        /// The subnet is not in the emission set: shorts open and grow only on a subnet that
        /// currently receives emission. Closing and adding collateral stay allowed.
        SubnetNotInEmission,
        /// The subnet is being dissolved: from its dissolution flag on, its shorts are settled by
        /// the dissolution alone (SPEC P12), so no short opens, grows, closes or takes collateral.
        DissolutionInProgress,
    }

    /// Nothing to configure: a chain that starts with this pallet gets the state the runtime
    /// upgrade gives an existing one, with lending and longs disabled.
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        #[serde(skip)]
        pub _phantom: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            migrations::initialize_at_genesis::<T>();
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Applies the pending lending parameters that are due and the other pending parameters
        /// first, then samples prices for every live subnet,
        /// whether lending is enabled or not (SPEC 2.1, section 9 item 4), then runs the
        /// interest charges that are due, up to `MaxChargesPerBlock` (SPEC P15).
        ///
        /// The weight is the measured hook of this block's kind (one that only steps `T_ema`,
        /// or one that samples the rings), plus the measured parts that ran on top of it: the
        /// parameter apply if a group was pending, the charges and the `Due` buckets read.
        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            let subnets = T::Pool::live_subnets();
            let live = u32::try_from(subnets.len()).unwrap_or(u32::MAX);
            let lending = Self::apply_pending_lending_params(now, &subnets);
            let applied = Self::apply_pending_params(&subnets);
            let sampled = Self::sample_prices(now.unique_saturated_into(), &subnets);
            let collected = Self::collect_due(now);
            let mut weight = if sampled {
                T::WeightInfo::on_initialize_sampling(live)
            } else {
                T::WeightInfo::on_initialize_ema(live)
            };
            // The apply is measured with all four groups pending, so it covers a block that
            // applies the lending group or any other. A block with nothing pending pays only the
            // pending-parameter reads that the hooks above are measured with.
            if lending || applied {
                weight = weight.saturating_add(T::WeightInfo::apply_pending_params(live));
            }
            // A default is part of its charge: `collect_due` is measured with every charge
            // defaulting its short, so the defaults of a block add nothing to it.
            weight
                .saturating_add(T::WeightInfo::collect_due(collected.charges))
                .saturating_add(T::WeightInfo::collect_due_buckets(collected.buckets))
        }

        fn on_runtime_upgrade() -> Weight {
            migrations::migrate_to_v1::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    // Call indices 0..=19 are reserved for the user calls (DESIGN-v2 section 6). Indices 4, 5,
    // 10 and 11 are retired: v2.1 has no collateral withdrawal and no call that acts on another
    // holder's position. A short defaults only in its own scheduled charge (`interest`).
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Opens a short: mints `q` alpha, sells it, and pays the proceeds to custody (SPEC P1-P23).
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::open_short(T::Pool::subnet_limit()))]
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
        #[pallet::weight(T::WeightInfo::increase_short(T::Pool::subnet_limit()))]
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
        /// Weighed as the costlier of a full close and a partial one.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::close_short().max(T::WeightInfo::close_short_partial()))]
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

        // Call index 4 is retired: it was `withdraw_collateral`. Collateral leaves a position
        // only on close (approved change beyond SPEC-v2.1), and the index is not reused.

        /// Opens a long: lifts `lift` TAO from the pool and buys alpha that custody locks
        /// (SPEC L1-L12). Receives at least `min_alpha_out`.
        // HOLD(longs): placeholder weight, not a benchmark.
        #[pallet::call_index(6)]
        #[pallet::weight(long_weights::open_long::<T>())]
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
        // HOLD(longs): placeholder weight, not a benchmark.
        #[pallet::call_index(7)]
        #[pallet::weight(long_weights::increase_long::<T>())]
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
        // HOLD(longs): placeholder weight, not a benchmark.
        #[pallet::call_index(8)]
        #[pallet::weight(long_weights::close_long::<T>())]
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
        // HOLD(longs): placeholder weight, not a benchmark.
        #[pallet::call_index(9)]
        #[pallet::weight(long_weights::add_long_collateral::<T>())]
        pub fn add_long_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            amount: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_add_collateral(&who, netuid, Side::Long, amount)
        }

        // Call index 10 is retired: it was `withdraw_long_collateral`. The index is not reused.

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

        /// Replaces `LendingParams` (SPEC 2.1). A call that tightens the live group (a higher
        /// `apr_bps` or `rate_cap_bps`, any change of `rate_k` or of `twap_samples` or
        /// `twap_interval_blocks`, or a shorter `rate_delay_blocks`) is live only at the block
        /// set by the `rate_delay_blocks` in force at the call, and emits
        /// `LendingParamsScheduled`; any other call, a longer delay included, is live from the
        /// next block.
        #[pallet::call_index(22)]
        #[pallet::weight(T::WeightInfo::set_lending_params())]
        pub fn set_lending_params(origin: OriginFor<T>, params: LendingParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_lending_params(params)
        }

        /// Replaces `ShortParams` from the next block (SPEC 2.1).
        #[pallet::call_index(23)]
        #[pallet::weight(T::WeightInfo::set_short_params())]
        pub fn set_short_params(origin: OriginFor<T>, params: ShortParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_short_params(params)
        }

        /// Replaces `LongParams` from the next block (SPEC 2.1).
        #[pallet::call_index(24)]
        #[pallet::weight(T::WeightInfo::set_long_params())]
        pub fn set_long_params(origin: OriginFor<T>, params: LongParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_long_params(params)
        }

        /// Replaces `DeregParams` from the next block (SPEC 2.1).
        #[pallet::call_index(25)]
        #[pallet::weight(T::WeightInfo::set_dereg_params())]
        pub fn set_dereg_params(origin: OriginFor<T>, params: DeregParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::queue_dereg_params(params)
        }
    }
}
