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
    /// Its cushion could no longer pay its interest; the chain forfeited everything it held to
    /// the pool, with no swap.
    Starved,
    /// The subnet was dissolved; the position was cash-settled at the dissolution price.
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

        /// Highest leverage a long may choose, in percent: `200` is 2x. A long at leverage `L`
        /// costs the pool once the price falls by `1 / L`. At 1x a long can never lose the pool
        /// anything, and is nothing a spot buy does not do better.
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

    /// The spot price, as `(tao, alpha)`, that every position on a dissolving subnet settles
    /// at. Fixed before the first one is settled and removed once the last one is.
    #[pallet::storage]
    pub type DissolutionPrice<T: Config> =
        StorageMap<_, Identity, NetUid, (TaoBalance, AlphaBalance), OptionQuery>;

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
            /// position pays what is left of its cushion, in kind, to the pool instead.
            interest_paid: TaoBalance,
            /// Debt the position could not repay, in the lent token. Zero for a starved
            /// position: nothing is swapped, so the pool takes what it lent back in kind.
            shortfall: Lent,
        },
        ParamsSet {
            params: DerivativesParams,
        },
        /// A dissolving subnet's positions are about to be cash-settled at its spot price,
        /// `tao / alpha` TAO per alpha, with no swap.
        DissolutionPriced {
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
        /// interest.
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
        /// Cash-settle every position on `netuid` at the spot price of the block dissolution
        /// began, ahead of the stake payout. The price is fixed once, before the first position
        /// is settled, and every position settles at it with no swap: a short is charged its
        /// alpha debt at that price, a long is credited its alpha at it. A short that moved the
        /// price down keeps that move; nothing climbs the curve back. The balancer is still in
        /// storage at this point, so the price is the one the pool last showed.
        fn on_subnet_dissolve(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let per_position = T::WeightInfo::close();
            if !meter.can_consume(per_position) {
                return false;
            }
            let price = DissolutionPrice::<T>::get(netuid).unwrap_or_else(|| {
                meter.consume(per_position);
                let price = T::Pool::spot_price(netuid);
                DissolutionPrice::<T>::insert(netuid, price);
                Self::deposit_event(Event::DissolutionPriced {
                    netuid,
                    tao: price.0,
                    alpha: price.1,
                });
                price
            });
            loop {
                if !meter.can_consume(per_position) {
                    return false;
                }
                let Some(owner) = OpenByNetuid::<T>::iter_key_prefix(netuid).next() else {
                    DissolutionPrice::<T>::remove(netuid);
                    return true;
                };
                meter.consume(per_position);
                Self::settle_at_dissolution(&owner, netuid, price);
            }
        }
    }

    impl<T: Config> DerivativesHook for Pallet<T> {
        /// The long-side footprint is exactly the alpha the pool is missing: the lifted slice
        /// plus what the lifted TAO bought, both held by the pallet until settlement.
        fn long_alpha_outstanding(netuid: NetUid) -> AlphaBalance {
            AlphaBalance::from(Footprint::<T>::get(netuid, Side::Long))
        }
    }
}
