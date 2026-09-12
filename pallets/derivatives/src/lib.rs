//! Long and short positions on subnet alpha, borrowed from the subnet's own liquidity pool.
//!
//! One position per `(owner, netuid)`, built with one call: `add(side, deposit, leverage)`.
//! The deposit is TAO. Adding on the position's own side lifts a further slice `phi` of both
//! pool reserves without moving price, swaps one half into the other token, and folds the
//! result into the position. Adding on the other side settles that much of it at the current
//! price, and flips through zero if there is more. `close` settles everything. At settlement
//! the swap is reversed, the borrowed slice plus the interest go back to the pool, and whatever is
//! left of the owner's cushion and proceeds is paid out. Nothing is minted or burned: the pool
//! only ever gets its own liquidity back.
//!
//! Two root-set numbers are the design: the pool lends out at most `pool_share` of itself per
//! side, at `interest_rate` on exposure, the same for both sides, fixed per tranche when it is
//! added and accruing per block. Once a week, on its own block, each position's interest is
//! collected out of its cushion and spent buying alpha from the pool, which is then recycled:
//! the interest reaches the pool as buy pressure, on either side. A position has no term: it
//! lives until its owner closes it, or until its cushion can no longer pay and the chain
//! forfeits it to the pool. Nobody else can touch it.
//!
//! Two rules keep a settlement from being a trade the pool pays for. A position whose pot the
//! pool's own quote says cannot repay its debt is not swapped at all: everything held for it
//! goes back in kind, so there is no market order for anyone to trade against. And liquidity
//! handed back while the spot price is more than [`PARK_THRESHOLD_PERCENT`] from the moving
//! price is parked in the pallet, not re-added at the pushed price; `on_idle` releases it once
//! the spot is back.

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
pub use position::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod position;
mod settle;
#[cfg(test)]
mod tests;
pub mod weights;

use frame_support::{PalletId, pallet_prelude::*, traits::Get, weights::WeightMeter};
use frame_system::pallet_prelude::*;
use sp_runtime::traits::{AccountIdConversion, Hash, Saturating, TrailingZeroInput};
use subtensor_runtime_common::{
    AlphaBalance, DerivativesHook, NetUid, SubnetDissolveHook, TaoBalance, Token,
};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface, Perquintill};

/// Why a position closed.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, Copy, PartialEq, Eq, RuntimeDebug,
)]
pub enum Closer {
    /// The owner closed it.
    Owner,
    /// The owner closed it, but the pool's own quote said the pot could not repay the debt
    /// plus the interest owed. Nothing was swapped: everything the pallet held for it went to
    /// the pool in kind, and the owner was paid nothing.
    Underwater,
    /// Its cushion could no longer pay its interest; the chain forfeited everything it held to
    /// the pool, with no swap.
    Starved,
    /// The subnet was dissolved. The position was cash-settled, with no swap, at the prices
    /// fixed for the whole subnet in [`DissolutionPrice`]; see
    /// [`Pallet::settle_at_dissolution`].
    Dissolution,
}

// The pallet macro expands to `expect()` calls in generated storage and error code.
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;
    use crate::weights::WeightInfo;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Pool access plus plain TAO and stake transfers, both implemented by `pallet-subtensor`.
        type Pool: DerivativesPoolInterface<Self::AccountId> + OrderSwapInterface<Self::AccountId>;

        /// Derives the account that custodies every position's TAO and stakes its alpha.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// Highest leverage a short may choose, in percent: `100` is 1x. A short at leverage
        /// `L` costs the pool once the price rises by `1 / L`.
        #[pallet::constant]
        type MaxShortLeverage: Get<u16>;

        /// Highest leverage a long may choose, in percent: `150` is 1.5x. A long at leverage `L`
        /// costs the pool once the price falls by `1 / L`. At 1x a long can never lose the pool
        /// anything, and is nothing a spot buy does not do better. The ceiling must stay below
        /// `1 + sqrt(1 - pool_share)` (1.87x at a 25% share): above it, the largest long the
        /// cap admits can dump alpha it holds outside into its own lifted price and walk away
        /// from the debt for more than the cushion it loses. 1.5x holds for every balancer
        /// weight the pool drifts to and every share up to 25%.
        #[pallet::constant]
        type MaxLongLeverage: Get<u16>;

        /// Smallest deposit one `add` may put up, and the smallest surplus a flip will open.
        #[pallet::constant]
        type MinDeposit: Get<TaoBalance>;

        type WeightInfo: WeightInfo;
    }

    #[pallet::type_value]
    pub fn DefaultParams<T: Config>() -> DerivativesParams {
        DerivativesParams::defaults()
    }

    #[pallet::storage]
    pub type Params<T: Config> = StorageValue<_, DerivativesParams, ValueQuery, DefaultParams<T>>;

    /// One position per `(owner, netuid)`; its side is the sign of its exposure.
    #[pallet::storage]
    pub type Positions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Position<BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Index by subnet so dissolution can find every open position.
    #[pallet::storage]
    pub type OpenByNetuid<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, (), OptionQuery>;

    /// The interest queue: positions by the block their next collection falls on. A position is
    /// listed under its `due` block from the moment it opens until it closes; each collection
    /// moves it one [`INTEREST_PERIOD`] ahead.
    #[pallet::storage]
    pub type Due<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;

    /// The first block whose [`Due`] slot has not been fully collected. Trails the current block
    /// only while a slot holds more positions than one block may collect, or after a stall.
    #[pallet::storage]
    pub type NextDue<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    /// Sum of [`Legs::footprint`] over open positions, in the lent token (TAO for shorts, alpha
    /// for longs). Compared against `pool_share` of the lent reserve at open.
    #[pallet::storage]
    pub type Footprint<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Identity, Side, u64, ValueQuery>;

    /// Hotkey owned by the pallet account; all alpha the pallet holds is staked here. Chosen
    /// and registered in the upgrade block from that block's parent hash (see
    /// [`Pallet::claim_hotkey`]), so nobody can register it ahead of the pallet.
    #[pallet::storage]
    pub type PalletHotkey<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// The prices every position on a dissolving subnet settles at: shorts are charged at the
    /// higher of the pool's spot and moving price, longs credited at the lower. Fixed before
    /// the first one is settled and removed once the last one is.
    ///
    /// Stored rather than re-read because settlement may span several blocks: the pool's
    /// price could drift between them, and the order positions happen to be visited in must
    /// not change what any of them is paid. One pair for all also keeps the settlement
    /// independent of position count; there is no netting of shorts against longs and no
    /// swap, so a subnet with thousands of positions costs one read per position and nothing
    /// more.
    #[pallet::storage]
    pub type DissolutionPrice<T: Config> =
        StorageMap<_, Identity, NetUid, DissolutionPrices, OptionQuery>;

    /// Liquidity a settlement handed back that has not rejoined the pool yet, per subnet, as
    /// `(tao, alpha)`. The TAO sits on the pallet account and the alpha is staked at the
    /// pallet hotkey, both outside the price. A pair lands here when the spot price was more
    /// than [`PARK_THRESHOLD_PERCENT`] from the moving price at the time; `on_idle` re-adds
    /// it once the spot is back within the band, and a dissolution returns it to the reserves
    /// the stakers are paid from before any position is settled.
    #[pallet::storage]
    pub type Parked<T: Config> =
        StorageMap<_, Identity, NetUid, (TaoBalance, AlphaBalance), ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A tranche was added on the position's side (or a new position opened). The `*_added`
        /// fields are this tranche alone; the position's totals are their running sums.
        PositionAdded {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            /// TAO the owner put up for this tranche.
            deposit: TaoBalance,
            /// Exposure as a percentage of the deposit, as the owner chose it for this tranche.
            leverage_percent: u16,
            /// Proceeds held, debt owed, escrow kept by this tranche, each in its own token.
            legs: Legs,
            exposure_added: TaoBalance,
            /// The position's exposure after this add.
            exposure_tao: TaoBalance,
        },
        /// Part of a position was settled at the current price; the rest stays open.
        PositionReduced {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            /// Share of the position that was unwound.
            fraction: Perquintill,
            /// TAO paid to the owner.
            payout: TaoBalance,
            /// Interest paid on the whole position, brought up to date at this block.
            interest_paid: TaoBalance,
            /// Debt the settled part could not repay, in the lent token.
            shortfall: Lent,
            /// The position's exposure after this reduction.
            exposure_tao: TaoBalance,
        },
        PositionClosed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            closed_by: Closer,
            /// TAO paid to the owner.
            payout: TaoBalance,
            /// Interest paid at this settlement: spent buying alpha that was recycled. A starved
            /// position pays what is left of its cushion, in kind, to the pool instead; an
            /// underwater one pays nothing separately, its whole pot goes to the pool.
            interest_paid: TaoBalance,
            /// Debt the position could not repay, in the lent token. Zero for a starved
            /// position: nothing is swapped, so the pool takes what it lent back in kind. The
            /// whole debt for an underwater one: nothing is bought back, the pool is paid in
            /// the other token instead.
            shortfall: Lent,
        },
        ParamsSet {
            params: DerivativesParams,
        },
        /// A dissolving subnet's positions are about to be cash-settled with no swap: shorts
        /// charged at `short`, longs credited at `long`, each `tao / alpha` TAO per alpha and
        /// each the worse for that side of the pool's spot and moving price. Emitted once per
        /// dissolving subnet, in the block the first position is settled; every
        /// `PositionClosed` with `closed_by: Dissolution` that follows used these.
        DissolutionPriced {
            netuid: NetUid,
            short: (TaoBalance, AlphaBalance),
            long: (TaoBalance, AlphaBalance),
        },
        /// A settlement handed liquidity back while the spot price was off the moving price;
        /// the pair waits in [`Parked`] instead of joining the pool at that price.
        LiquidityParked {
            netuid: NetUid,
            tao: TaoBalance,
            alpha: AlphaBalance,
        },
        /// Parked liquidity rejoined the pool: the spot price came back within the band, or
        /// the subnet is dissolving and the pair went to the reserves the stakers are paid from.
        LiquidityReleased {
            netuid: NetUid,
            tao: TaoBalance,
            alpha: AlphaBalance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The subnet does not exist, is not AMM-priced, or has its subtoken disabled.
        SubnetNotDynamic,
        /// No such position.
        NoPosition,
        /// The deposit is below `MinDeposit`.
        DepositTooLow,
        /// Leverage is zero or above the side's maximum (`MaxShortLeverage` or
        /// `MaxLongLeverage`).
        LeverageOutOfRange,
        /// Leverage times deposit rounds to nothing, or the pool would swap it for nothing.
        ZeroExposure,
        /// Open positions of this side would exceed `pool_share` of the lent reserve. A
        /// `pool_share` of zero means adds are paused.
        PoolCapExceeded,
        /// The pallet has not claimed its hotkey yet; no position can be opened.
        PalletHotkeyUnset,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Claim the pallet hotkey before any extrinsic can, and start the interest queue at
        /// this block. One read once it is set.
        fn on_runtime_upgrade() -> Weight {
            if PalletHotkey::<T>::exists() {
                return T::DbWeight::get().reads(1);
            }
            Self::claim_hotkey();
            NextDue::<T>::put(frame_system::Pallet::<T>::block_number());
            T::DbWeight::get().reads_writes(4, 5)
        }

        /// Collect the interest of every position due by this block, up to
        /// [`COLLECTIONS_PER_BLOCK`] of them.
        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            Self::collect_due(now)
        }

        /// Re-add every parked pair whose subnet's spot price is back within
        /// [`PARK_THRESHOLD_PERCENT`] of its moving price, as far as the leftover block weight
        /// allows. A full block only delays a release; nothing is lost by waiting.
        fn on_idle(_now: BlockNumberFor<T>, remaining: Weight) -> Weight {
            Self::release_parked_within(remaining)
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Add `side` exposure on `netuid`: `leverage_percent / 100` times `deposit`, measured
        /// against the pool's TAO reserve. One call covers open, add, reduce, and flip.
        ///
        /// With no position, or one on the same side, `deposit` is taken from the caller's free
        /// balance as cushion, and a tranche is lifted from the pool and folded into the
        /// position. There is no term: the position runs while its cushion pays the weekly
        /// interest.
        ///
        /// With a position on the other side, this settles the matching share of it at the
        /// current price and pays that share of the cushion, less interest and any loss, to the
        /// caller. If the exposure asked for is larger than the position, the whole position is
        /// closed and the rest, if it reaches `MinDeposit`, opens on the new side. Only the
        /// deposit for that rest is taken from the caller.
        ///
        /// The leverage must be above zero and at most the side's maximum (`MaxShortLeverage`
        /// or `MaxLongLeverage`).
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::add())]
        pub fn add(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            deposit: TaoBalance,
            leverage_percent: u16,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_add(owner, netuid, side, deposit, leverage_percent)
        }

        /// Settle the caller's position on `netuid` in full, at the current price. Only the
        /// owner can close a position; the chain forfeits one that can no longer pay its
        /// interest. A position the pool's quote says is underwater is not traded: everything
        /// held for it goes to the pool in kind and the caller is paid nothing.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::close())]
        pub fn close(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_settle(&owner, netuid, Perquintill::one())
        }

        /// Set the two parameters. Root only. A `pool_share` of zero pauses new adds; open
        /// positions keep the rate they were added with and settle as usual.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::sudo_set_params())]
        pub fn sudo_set_params(origin: OriginFor<T>, params: DerivativesParams) -> DispatchResult {
            ensure_root(origin)?;
            Params::<T>::put(params);
            Self::deposit_event(Event::ParamsSet { params });
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Account derived from the pallet's `PalletId`.
        pub fn pallet_account() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        /// The hotkey the pallet stakes through, once claimed.
        pub fn pallet_hotkey() -> Result<T::AccountId, DispatchError> {
            PalletHotkey::<T>::get().ok_or_else(|| Error::<T>::PalletHotkeyUnset.into())
        }

        /// Pick a hotkey nobody could have registered in advance and register it to the pallet
        /// account in the same block.
        ///
        /// Anyone may register any address as a hotkey, and registration is first come first
        /// served, so a hotkey fixed at compile time could be claimed before the upgrade and
        /// later `swap_hotkey`ed together with the pallet's stake. Deriving it from the parent
        /// block hash makes it unknowable until the block that registers it; hooks run before
        /// any extrinsic in that block. The nonce skips the (practically impossible) case of an
        /// address that already exists.
        pub(crate) fn claim_hotkey() {
            let coldkey = Self::pallet_account();
            for nonce in 0u8..=u8::MAX {
                let Some(hotkey) = Self::hotkey_candidate(nonce) else {
                    return;
                };
                if T::Pool::hotkey_exists(&hotkey) {
                    continue;
                }
                if T::Pool::register_pallet_hotkey(&coldkey, &hotkey).is_ok()
                    && T::Pool::pallet_hotkey_registered(&coldkey, &hotkey)
                {
                    PalletHotkey::<T>::put(hotkey);
                }
                return;
            }
        }

        /// `nonce`-th hotkey candidate for this block: a hash of the pallet id and the parent
        /// block hash.
        pub(crate) fn hotkey_candidate(nonce: u8) -> Option<T::AccountId> {
            let parent = frame_system::Pallet::<T>::parent_hash();
            let seed = T::Hashing::hash_of(&(T::PalletId::get(), b"hotkey", parent, nonce));
            T::AccountId::decode(&mut TrailingZeroInput::new(seed.as_ref())).ok()
        }

        /// Whether an owner may open `side` at `leverage_percent`: above zero and at most the
        /// side's maximum.
        pub fn leverage_allowed(side: Side, leverage_percent: u16) -> bool {
            let max = match side {
                Side::Short => T::MaxShortLeverage::get(),
                Side::Long => T::MaxLongLeverage::get(),
            };
            leverage_percent > 0 && leverage_percent <= max
        }
    }

    impl<T: Config> SubnetDissolveHook for Pallet<T> {
        /// Cash-settle every position on `netuid` ahead of the stake payout. This is the first
        /// cleanup phase of a dissolution, run while the pool and the stake maps still exist.
        ///
        /// **Two prices, no swaps.** On first entry the pool's spot and moving prices are
        /// read once, the worse of the two for each side is stored in [`DissolutionPrice`]
        /// and announced in `DissolutionPriced`, and any liquidity parked for the subnet is
        /// returned to the reserves. Every position is then settled on its own by
        /// [`Pallet::settle_at_dissolution`]: a short is charged its alpha debt at the higher
        /// price, a long is credited its alpha at the lower. Nothing is netted between
        /// positions and nothing is traded, so the settlement cannot move the price, cannot
        /// fail for lack of liquidity, and pays each owner the same whatever order the
        /// positions are visited in and however many blocks it takes. A short that pushed the
        /// spot down in the block the subnet dissolved is charged the moving price it could
        /// not push. The balancer is still in storage at this point, so the spot is the one
        /// the pool last showed.
        ///
        /// **Resume contract.** Returns `true` once no position is left, `false` when the
        /// meter ran out first; the caller keeps the phase and calls again next block. Work is
        /// paced at `WeightInfo::close()` per position, plus one unit the first time to fix the
        /// prices and one `WeightInfo::release_parked()` if there was a parked pair. There is no
        /// cap on positions: a subnet with more than a block can hold is settled over as many
        /// blocks as it takes, and nothing here can block the dissolution.
        fn on_subnet_dissolve(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let per_position = T::WeightInfo::close();
            if !meter.can_consume(per_position) {
                return false;
            }
            let prices = DissolutionPrice::<T>::get(netuid).unwrap_or_else(|| {
                meter.consume(per_position);
                let prices = DissolutionPrices::from_spot_and_moving(
                    T::Pool::spot_price(netuid),
                    T::Pool::moving_price(netuid),
                );
                DissolutionPrice::<T>::insert(netuid, prices);
                Self::deposit_event(Event::DissolutionPriced {
                    netuid,
                    short: prices.short,
                    long: prices.long,
                });
                prices
            });
            if Parked::<T>::contains_key(netuid) {
                if !meter.can_consume(T::WeightInfo::release_parked()) {
                    return false;
                }
                meter.consume(T::WeightInfo::release_parked());
                // The pool is dissolving, so the pair lands straight in the reserves the
                // stakers are paid from. A failure here would leave the pair on the pallet;
                // it is logged and the entry dropped so the cleanup can never stall on it.
                if let Err(error) = Self::release_parked(netuid) {
                    log::error!(
                        "derivatives: could not return parked liquidity on {netuid:?}: {error:?}"
                    );
                    Parked::<T>::remove(netuid);
                }
            }
            loop {
                if !meter.can_consume(per_position) {
                    return false;
                }
                let Some(owner) = OpenByNetuid::<T>::iter_key_prefix(netuid).next() else {
                    DissolutionPrice::<T>::remove(netuid);
                    return true;
                };
                meter.consume(per_position);
                Self::settle_at_dissolution(&owner, netuid, prices);
            }
        }
    }

    impl<T: Config> DerivativesHook for Pallet<T> {
        /// The alpha the pool is missing to the pallet: the long-side footprint, which is the
        /// lifted slice plus what the lifted TAO bought, both held until settlement, and any
        /// alpha parked on its way back.
        fn long_alpha_outstanding(netuid: NetUid) -> AlphaBalance {
            AlphaBalance::from(Footprint::<T>::get(netuid, Side::Long))
                .saturating_add(Parked::<T>::get(netuid).1)
        }
    }
}
