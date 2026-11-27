//! # Canonical pool
//!
//! The one place where tUSD and TAO trade: a constant-product pool priced by the swap
//! pallet's balancer math at fixed 50/50 weights (see [`math`]). Subnet alpha is never paired
//! with tUSD; a dollar reaches alpha only through TAO, and TAO only through this pool.
//!
//! - **Liquidity is open.** Anyone adds TAO and tUSD in the pool's current ratio and receives
//!   shares; a share redeems its part of both reserves at any time, even while the pool is
//!   disabled.
//! - **The fee stays in the pool.** It raises what every share redeems, so LPs earn it.
//! - **The pool ships disabled.** The admin origin enables it; the pause origin can only
//!   disable it again.
//! - **Swaps are capped by depth.** A swap may pay in at most a fraction of the reserve it
//!   pays into, and a refilling window bounds the flow each way. Both bounds are fractions,
//!   so they loosen as the pool deepens.
//! - **A time-weighted price is kept.** Consumers that must not trust the price of the
//!   moment read [`Pallet::twap_price`].
//!
//! ## Invariants
//!
//! 1. The pool account holds at least [`ReserveTao`] TAO and [`ReserveTusd`] tUSD.
//! 2. A swap never lowers `ReserveTao * ReserveTusd`.
//! 3. Adding or removing liquidity never lowers either reserve per share.
//! 4. Once liquidity exists, both reserves stay at or above [`Config::MinReserve`] and
//!    [`TotalShares`] stays above zero.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod math;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
#[cfg(any(feature = "try-runtime", test))]
use frame_support::traits::fungible::Inspect;
use frame_support::{
    PalletId,
    pallet_prelude::*,
    storage::with_storage_layer,
    traits::{fungible::Mutate, tokens::Preservation},
};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_runtime::{Permill, traits::AccountIdConversion};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{TaoBalance, Token, TusdBalance, rails::RateWindow};

pub use math::{PRICE_SCALE, Reserves, Side};
pub use pallet::*;
pub use weights::WeightInfo;

/// The lowest fee the pool may charge: 0.05%.
pub const MIN_FEE: Permill = Permill::from_parts(500);
/// The highest fee the pool may charge: 1%.
pub const MAX_FEE: Permill = Permill::from_parts(10_000);

/// A source of the TAO price in dollars, scaled by [`PRICE_SCALE`].
pub trait TaoUsdOracle {
    /// The current price, or `None` while there is no fresh one.
    fn tao_usd_price() -> Option<u64>;
}

impl TaoUsdOracle for () {
    fn tao_usd_price() -> Option<u64> {
        None
    }
}

/// Bounds on swaps, as fractions of the reserve a swap pays into.
#[freeze_struct("7cee515b32b6ecdc")]
#[derive(
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct SwapCaps {
    /// The most one swap may pay in.
    pub per_swap: Permill,
    /// The most all swaps one way may pay in within a refilling window.
    pub per_window: Permill,
    /// Blocks an exhausted window takes to refill.
    pub window: u32,
}

/// What each direction has paid in lately.
#[freeze_struct("713a5f069d2e2d1d")]
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
pub struct Flow {
    pub tusd_in: RateWindow,
    pub tao_in: RateWindow,
}

/// The price accumulator behind [`Pallet::twap_price`].
///
/// `cumulative` grows by the price times the blocks it stood. The average price between two
/// blocks is the growth of `cumulative` over their distance. Two checkpoints, one averaging
/// period apart, give the pool a reference that is always between one and two periods old.
#[freeze_struct("2506f5b2db9ca6a")]
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
pub struct PriceAccumulator {
    /// Sum of price times blocks, up to `updated_at`.
    pub cumulative: u128,
    /// The block `cumulative` is current to.
    pub updated_at: u32,
    /// The older checkpoint `(block, cumulative)`: the reference of the average.
    pub reference: (u32, u128),
    /// The newer checkpoint, which becomes the reference after one period.
    pub checkpoint: (u32, u128),
}

// The FRAME macros expand to `expect` on infallible codec paths.
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_tusd::Config {
        /// TAO.
        type Currency: Mutate<Self::AccountId, Balance = TaoBalance>;

        /// Enables the pool and sets its fee and caps.
        type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// May disable the pool, and nothing else.
        type PauseOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// The reference the first deposit is priced against.
        type PriceOracle: TaoUsdOracle;

        /// Derives the account that holds the reserves.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// How far the price a first deposit sets may be from the oracle's.
        #[pallet::constant]
        type MaxInitialPriceDeviation: Get<Permill>;

        /// The least either reserve may hold once liquidity exists. At least the existential
        /// deposit of TAO and the minimum balance of tUSD, so the pool account lives on.
        #[pallet::constant]
        type MinReserve: Get<u64>;

        /// Blocks the time-weighted price averages over, at least.
        #[pallet::constant]
        type TwapPeriod: Get<u32>;

        /// Weights of this pallet's calls.
        type WeightInfo: WeightInfo;
    }

    /// The fee a new pool charges: 0.3%.
    #[pallet::type_value]
    pub fn DefaultFee() -> Permill {
        Permill::from_parts(3_000)
    }

    /// The caps a new pool starts with: 2% of the paying reserve per swap, and 20% each way
    /// per 300 blocks (an hour).
    #[pallet::type_value]
    pub fn DefaultSwapCaps() -> SwapCaps {
        SwapCaps {
            per_swap: Permill::from_percent(2),
            per_window: Permill::from_percent(20),
            window: 300,
        }
    }

    /// ITEM --> bool | Whether swaps and deposits are open. Withdrawals always are.
    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;

    /// ITEM --> TAO | The pool's TAO reserve.
    #[pallet::storage]
    pub type ReserveTao<T> = StorageValue<_, TaoBalance, ValueQuery>;

    /// ITEM --> tUSD | The pool's tUSD reserve.
    #[pallet::storage]
    pub type ReserveTusd<T> = StorageValue<_, TusdBalance, ValueQuery>;

    /// ITEM --> shares | All LP shares, the locked minimum included.
    #[pallet::storage]
    pub type TotalShares<T> = StorageValue<_, u128, ValueQuery>;

    /// MAP ( account ) --> shares | The LP shares each account holds.
    #[pallet::storage]
    pub type Shares<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, u128, ValueQuery>;

    /// ITEM --> Permill | The part of a swap's input that stays in the pool.
    #[pallet::storage]
    pub type Fee<T> = StorageValue<_, Permill, ValueQuery, DefaultFee>;

    /// ITEM --> SwapCaps | The bounds on swaps.
    #[pallet::storage]
    pub type Caps<T> = StorageValue<_, SwapCaps, ValueQuery, DefaultSwapCaps>;

    /// ITEM --> Flow | What each direction has paid in lately, against `Caps.per_window`.
    #[pallet::storage]
    pub type RecentFlow<T> = StorageValue<_, Flow, ValueQuery>;

    /// ITEM --> PriceAccumulator | The time-weighted price accumulator.
    #[pallet::storage]
    pub type Accumulator<T> = StorageValue<_, PriceAccumulator, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// The pool was enabled or disabled.
        EnabledSet { enabled: bool },
        /// The swap fee changed.
        FeeSet { fee: Permill },
        /// The swap caps changed.
        SwapCapsSet { caps: SwapCaps },
        /// An account added liquidity.
        LiquidityAdded {
            who: T::AccountId,
            tao: TaoBalance,
            tusd: TusdBalance,
            shares: u128,
        },
        /// An account removed liquidity.
        LiquidityRemoved {
            who: T::AccountId,
            tao: TaoBalance,
            tusd: TusdBalance,
            shares: u128,
        },
        /// An account swapped tUSD for TAO. `fee` is the part of `tusd_in` the pool kept.
        TusdSwappedForTao {
            who: T::AccountId,
            tusd_in: TusdBalance,
            tao_out: TaoBalance,
            fee: TusdBalance,
        },
        /// An account swapped TAO for tUSD. `fee` is the part of `tao_in` the pool kept.
        TaoSwappedForTusd {
            who: T::AccountId,
            tao_in: TaoBalance,
            tusd_out: TusdBalance,
            fee: TaoBalance,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The pool is disabled.
        PoolDisabled,
        /// The pool has no liquidity.
        NoLiquidity,
        /// The amounts are too small to mint or redeem anything.
        AmountTooLow,
        /// The result is worse than the caller's minimum.
        SlippageExceeded,
        /// The swap pays in more than `Caps.per_swap` of the reserve.
        SwapTooLarge,
        /// Swaps this way have used up the window; it refills with time.
        SwapRateLimited,
        /// The operation would leave a reserve below the minimum.
        ReserveTooLow,
        /// The account does not hold that many shares.
        InsufficientShares,
        /// There is no fresh oracle price to check the first deposit against.
        OracleUnavailable,
        /// The first deposit sets a price too far from the oracle's.
        InitialPriceOutOfRange,
        /// The fee is outside `MIN_FEE..=MAX_FEE`.
        FeeOutOfRange,
        /// A cap of zero, or a window of zero blocks.
        InvalidCaps,
        /// Arithmetic overflowed.
        Overflow,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Adds liquidity: takes at most `max_tao` and `max_tusd` from the signer in the pool's
        /// current ratio and mints shares, at least `min_shares` of them.
        ///
        /// The first deposit sets the price. It must be within
        /// [`Config::MaxInitialPriceDeviation`] of the oracle's price, and it leaves
        /// [`math::MINIMUM_LIQUIDITY`] shares locked in the pool.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::add_liquidity())]
        pub fn add_liquidity(
            origin: OriginFor<T>,
            max_tao: TaoBalance,
            max_tusd: TusdBalance,
            min_shares: u128,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_add_liquidity(&who, max_tao, max_tusd, min_shares)
        }

        /// Removes liquidity: burns `shares` of the signer's and pays out their part of both
        /// reserves, at least `min_tao` and `min_tusd`. Open even while the pool is disabled.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::remove_liquidity())]
        pub fn remove_liquidity(
            origin: OriginFor<T>,
            shares: u128,
            min_tao: TaoBalance,
            min_tusd: TusdBalance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_remove_liquidity(&who, shares, min_tao, min_tusd)
        }

        /// Swaps `amount_in` of the signer's tUSD for at least `min_out` TAO.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::swap_tusd_for_tao())]
        pub fn swap_tusd_for_tao(
            origin: OriginFor<T>,
            amount_in: TusdBalance,
            min_out: TaoBalance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_swap_tusd_for_tao(&who, amount_in, min_out).map(|_| ())
        }

        /// Swaps `amount_in` of the signer's TAO for at least `min_out` tUSD.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::swap_tao_for_tusd())]
        pub fn swap_tao_for_tusd(
            origin: OriginFor<T>,
            amount_in: TaoBalance,
            min_out: TusdBalance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_swap_tao_for_tusd(&who, amount_in, min_out).map(|_| ())
        }

        /// Enables or disables swaps and deposits. The pause origin may only disable.
        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::set_enabled())]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            let may_pause =
                !enabled && <T as Config>::PauseOrigin::try_origin(origin.clone()).is_ok();
            if !may_pause {
                <T as Config>::AdminOrigin::ensure_origin(origin)?;
            }
            Enabled::<T>::put(enabled);
            Self::deposit_event(Event::EnabledSet { enabled });
            Ok(())
        }

        /// Sets the swap fee, within [`MIN_FEE`] and [`MAX_FEE`].
        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::set_fee())]
        pub fn set_fee(origin: OriginFor<T>, fee: Permill) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                (MIN_FEE..=MAX_FEE).contains(&fee),
                Error::<T>::FeeOutOfRange
            );
            Fee::<T>::put(fee);
            Self::deposit_event(Event::FeeSet { fee });
            Ok(())
        }

        /// Sets the swap caps.
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::set_swap_caps())]
        pub fn set_swap_caps(origin: OriginFor<T>, caps: SwapCaps) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                !caps.per_swap.is_zero() && !caps.per_window.is_zero() && caps.window != 0,
                Error::<T>::InvalidCaps
            );
            Caps::<T>::put(caps);
            Self::deposit_event(Event::SwapCapsSet { caps });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// The account that holds the reserves.
    pub fn account_id() -> T::AccountId {
        <T as Config>::PalletId::get().into_account_truncating()
    }

    /// The pool's reserves.
    pub fn reserves() -> Reserves {
        Reserves {
            tao: ReserveTao::<T>::get().to_u64(),
            tusd: ReserveTusd::<T>::get().to_u64(),
        }
    }

    /// The tUSD price of one TAO right now, scaled by [`PRICE_SCALE`]. `None` without liquidity.
    pub fn spot_price() -> Option<u64> {
        (TotalShares::<T>::get() != 0).then(|| math::price(Self::reserves()))
    }

    /// The tUSD price of one TAO averaged over at least [`Config::TwapPeriod`] blocks, scaled
    /// by [`PRICE_SCALE`]. `None` until the pool has that much history.
    ///
    /// The price of a block enters the average only through what stood at its end, so a swap
    /// and its reversal inside one block leave no trace.
    pub fn twap_price() -> Option<u64> {
        if TotalShares::<T>::get() == 0 {
            return None;
        }
        let now = Self::now();
        let accumulator = Self::accrued(Accumulator::<T>::get(), now);
        let (since, cumulative_then) = accumulator.reference;
        let blocks = now.checked_sub(since)?;
        if blocks < T::TwapPeriod::get().max(1) {
            return None;
        }
        let average = accumulator
            .cumulative
            .checked_sub(cumulative_then)?
            .checked_div(u128::from(blocks))?;
        u64::try_from(average).ok()
    }

    /// What `amount_in` tUSD buys in TAO right now, caps aside.
    pub fn quote_tusd_for_tao(amount_in: TusdBalance) -> Option<TaoBalance> {
        math::quote(
            Self::reserves(),
            Side::TusdIn,
            amount_in.to_u64(),
            Fee::<T>::get(),
        )
        .map(|quote| quote.amount_out.into())
    }

    /// What `amount_in` TAO buys in tUSD right now, caps aside.
    pub fn quote_tao_for_tusd(amount_in: TaoBalance) -> Option<TusdBalance> {
        math::quote(
            Self::reserves(),
            Side::TaoIn,
            amount_in.to_u64(),
            Fee::<T>::get(),
        )
        .map(|quote| quote.amount_out.into())
    }

    /// See [`Pallet::add_liquidity`].
    pub fn do_add_liquidity(
        who: &T::AccountId,
        max_tao: TaoBalance,
        max_tusd: TusdBalance,
        min_shares: u128,
    ) -> DispatchResult {
        ensure!(Enabled::<T>::get(), Error::<T>::PoolDisabled);
        let reserves = Self::reserves();
        let total_shares = TotalShares::<T>::get();
        let first = total_shares == 0;
        let (minted, locked, cost) = if first {
            let cost = Reserves {
                tao: max_tao.to_u64(),
                tusd: max_tusd.to_u64(),
            };
            Self::ensure_initial_price(cost)?;
            let shares = math::initial_shares(cost.tao, cost.tusd);
            let minted = shares
                .checked_sub(math::MINIMUM_LIQUIDITY)
                .ok_or(Error::<T>::AmountTooLow)?;
            (minted, math::MINIMUM_LIQUIDITY, cost)
        } else {
            let (shares, cost) = math::shares_for_deposit(
                reserves,
                total_shares,
                max_tao.to_u64(),
                max_tusd.to_u64(),
            )
            .ok_or(Error::<T>::Overflow)?;
            (shares, 0, cost)
        };
        ensure!(minted != 0, Error::<T>::AmountTooLow);
        ensure!(minted >= min_shares, Error::<T>::SlippageExceeded);
        let after = Reserves {
            tao: reserves
                .tao
                .checked_add(cost.tao)
                .ok_or(Error::<T>::Overflow)?,
            tusd: reserves
                .tusd
                .checked_add(cost.tusd)
                .ok_or(Error::<T>::Overflow)?,
        };
        Self::ensure_min_reserves(after)?;
        let total_shares = total_shares
            .checked_add(minted)
            .and_then(|total| total.checked_add(locked))
            .ok_or(Error::<T>::Overflow)?;

        with_storage_layer(|| {
            Self::accrue_price();
            let pool = Self::account_id();
            <T as Config>::Currency::transfer(who, &pool, cost.tao.into(), Preservation::Preserve)?;
            pallet_tusd::Pallet::<T>::do_transfer(who, &pool, cost.tusd.into())?;
            Self::set_reserves(after);
            TotalShares::<T>::put(total_shares);
            Shares::<T>::mutate(who, |shares| *shares = shares.saturating_add(minted));
            if first {
                // The first deposit starts the price history.
                let now = Self::now();
                Accumulator::<T>::put(PriceAccumulator {
                    cumulative: 0,
                    updated_at: now,
                    reference: (now, 0),
                    checkpoint: (now, 0),
                });
            }
            Ok::<(), DispatchError>(())
        })?;
        Self::deposit_event(Event::LiquidityAdded {
            who: who.clone(),
            tao: cost.tao.into(),
            tusd: cost.tusd.into(),
            shares: minted,
        });
        Ok(())
    }

    /// See [`Pallet::remove_liquidity`].
    pub fn do_remove_liquidity(
        who: &T::AccountId,
        shares: u128,
        min_tao: TaoBalance,
        min_tusd: TusdBalance,
    ) -> DispatchResult {
        let held = Shares::<T>::get(who);
        let total_shares = TotalShares::<T>::get();
        ensure!(shares != 0, Error::<T>::AmountTooLow);
        ensure!(shares <= held, Error::<T>::InsufficientShares);
        let reserves = Self::reserves();
        let paid = math::redemption(reserves, total_shares, shares).ok_or(Error::<T>::Overflow)?;
        ensure!(
            paid.tao >= min_tao.to_u64() && paid.tusd >= min_tusd.to_u64(),
            Error::<T>::SlippageExceeded
        );
        let after = Reserves {
            tao: reserves.tao.saturating_sub(paid.tao),
            tusd: reserves.tusd.saturating_sub(paid.tusd),
        };
        Self::ensure_min_reserves(after)?;

        with_storage_layer(|| {
            Self::accrue_price();
            let pool = Self::account_id();
            <T as Config>::Currency::transfer(&pool, who, paid.tao.into(), Preservation::Preserve)?;
            pallet_tusd::Pallet::<T>::do_transfer(&pool, who, paid.tusd.into())?;
            Self::set_reserves(after);
            TotalShares::<T>::put(total_shares.saturating_sub(shares));
            let rest = held.saturating_sub(shares);
            if rest == 0 {
                Shares::<T>::remove(who);
            } else {
                Shares::<T>::insert(who, rest);
            }
            Ok::<(), DispatchError>(())
        })?;
        Self::deposit_event(Event::LiquidityRemoved {
            who: who.clone(),
            tao: paid.tao.into(),
            tusd: paid.tusd.into(),
            shares,
        });
        Ok(())
    }

    /// See [`Pallet::swap_tusd_for_tao`]. Returns the TAO paid out.
    pub fn do_swap_tusd_for_tao(
        who: &T::AccountId,
        amount_in: TusdBalance,
        min_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let quote = Self::swap(who, Side::TusdIn, amount_in.to_u64(), min_out.to_u64())?;
        Self::deposit_event(Event::TusdSwappedForTao {
            who: who.clone(),
            tusd_in: amount_in,
            tao_out: quote.amount_out.into(),
            fee: quote.fee.into(),
        });
        Ok(quote.amount_out.into())
    }

    /// See [`Pallet::swap_tao_for_tusd`]. Returns the tUSD paid out.
    pub fn do_swap_tao_for_tusd(
        who: &T::AccountId,
        amount_in: TaoBalance,
        min_out: TusdBalance,
    ) -> Result<TusdBalance, DispatchError> {
        let quote = Self::swap(who, Side::TaoIn, amount_in.to_u64(), min_out.to_u64())?;
        Self::deposit_event(Event::TaoSwappedForTusd {
            who: who.clone(),
            tao_in: amount_in,
            tusd_out: quote.amount_out.into(),
            fee: quote.fee.into(),
        });
        Ok(quote.amount_out.into())
    }

    fn swap(
        who: &T::AccountId,
        side: Side,
        amount_in: u64,
        min_out: u64,
    ) -> Result<math::SwapQuote, DispatchError> {
        ensure!(Enabled::<T>::get(), Error::<T>::PoolDisabled);
        ensure!(TotalShares::<T>::get() != 0, Error::<T>::NoLiquidity);
        let reserves = Self::reserves();
        let (reserve_in, reserve_out) = reserves.in_out(side);
        let caps = Caps::<T>::get();
        ensure!(
            amount_in <= caps.per_swap.mul_floor(reserve_in),
            Error::<T>::SwapTooLarge
        );
        let quote = math::quote(reserves, side, amount_in, Fee::<T>::get())
            .ok_or(Error::<T>::AmountTooLow)?;
        ensure!(quote.amount_out >= min_out, Error::<T>::SlippageExceeded);
        let reserve_in = reserve_in
            .checked_add(amount_in)
            .ok_or(Error::<T>::Overflow)?;
        let reserve_out = reserve_out.saturating_sub(quote.amount_out);
        let after = match side {
            Side::TusdIn => Reserves {
                tao: reserve_out,
                tusd: reserve_in,
            },
            Side::TaoIn => Reserves {
                tao: reserve_in,
                tusd: reserve_out,
            },
        };
        Self::ensure_min_reserves(after)?;

        with_storage_layer(|| {
            Self::consume_flow(side, reserves, caps, amount_in)?;
            Self::accrue_price();
            let pool = Self::account_id();
            match side {
                Side::TusdIn => {
                    pallet_tusd::Pallet::<T>::do_transfer(who, &pool, amount_in.into())?;
                    <T as Config>::Currency::transfer(
                        &pool,
                        who,
                        quote.amount_out.into(),
                        Preservation::Preserve,
                    )?;
                }
                Side::TaoIn => {
                    <T as Config>::Currency::transfer(
                        who,
                        &pool,
                        amount_in.into(),
                        Preservation::Preserve,
                    )?;
                    pallet_tusd::Pallet::<T>::do_transfer(&pool, who, quote.amount_out.into())?;
                }
            }
            Self::set_reserves(after);
            Ok::<(), DispatchError>(())
        })?;
        Ok(quote)
    }

    /// Counts `amount_in` against the window of `side`, whose limit follows the reserve.
    fn consume_flow(
        side: Side,
        reserves: Reserves,
        caps: SwapCaps,
        amount_in: u64,
    ) -> DispatchResult {
        let now = Self::now();
        let limit = caps.per_window.mul_floor(reserves.in_out(side).0);
        RecentFlow::<T>::try_mutate(|flow| {
            let window = match side {
                Side::TusdIn => &mut flow.tusd_in,
                Side::TaoIn => &mut flow.tao_in,
            };
            *window = window.retargeted(now, limit, caps.window);
            ensure!(
                window.try_consume(now, amount_in),
                Error::<T>::SwapRateLimited
            );
            Ok(())
        })
    }

    /// The first deposit must price TAO close to the oracle: nobody may open the pool at a
    /// price of their choosing.
    fn ensure_initial_price(deposit: Reserves) -> DispatchResult {
        let oracle = T::PriceOracle::tao_usd_price().ok_or(Error::<T>::OracleUnavailable)?;
        let price = math::price(deposit);
        let tolerance = T::MaxInitialPriceDeviation::get().mul_floor(oracle);
        ensure!(
            price.abs_diff(oracle) <= tolerance,
            Error::<T>::InitialPriceOutOfRange
        );
        Ok(())
    }

    fn ensure_min_reserves(reserves: Reserves) -> DispatchResult {
        let minimum = T::MinReserve::get();
        ensure!(
            reserves.tao >= minimum && reserves.tusd >= minimum,
            Error::<T>::ReserveTooLow
        );
        Ok(())
    }

    fn set_reserves(reserves: Reserves) {
        ReserveTao::<T>::put(TaoBalance::from(reserves.tao));
        ReserveTusd::<T>::put(TusdBalance::from(reserves.tusd));
    }

    /// Brings the accumulator up to this block at the price that stood until now. Called
    /// before every change of the reserves.
    fn accrue_price() {
        if TotalShares::<T>::get() != 0 {
            let accumulator = Self::accrued(Accumulator::<T>::get(), Self::now());
            Accumulator::<T>::put(accumulator);
        }
    }

    /// `accumulator` brought up to `now` at the current price.
    fn accrued(mut accumulator: PriceAccumulator, now: u32) -> PriceAccumulator {
        let blocks = now.saturating_sub(accumulator.updated_at);
        if blocks == 0 {
            return accumulator;
        }
        accumulator.cumulative = accumulator.cumulative.saturating_add(
            u128::from(math::price(Self::reserves())).saturating_mul(u128::from(blocks)),
        );
        accumulator.updated_at = now;
        if now.saturating_sub(accumulator.checkpoint.0) >= T::TwapPeriod::get().max(1) {
            accumulator.reference = accumulator.checkpoint;
            accumulator.checkpoint = (now, accumulator.cumulative);
        }
        accumulator
    }

    fn now() -> u32 {
        frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// Checks that the pool account covers the reserves and that live liquidity is sound.
    #[cfg(any(feature = "try-runtime", test))]
    pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
        let pool = Self::account_id();
        let reserves = Self::reserves();
        ensure!(
            <T as Config>::Currency::balance(&pool).to_u64() >= reserves.tao,
            "pool account holds less TAO than the reserve"
        );
        ensure!(
            pallet_tusd::Pallet::<T>::balance(&pool).to_u64() >= reserves.tusd,
            "pool account holds less tUSD than the reserve"
        );
        if TotalShares::<T>::get() == 0 {
            ensure!(
                reserves.tao == 0 && reserves.tusd == 0,
                "reserves without shares"
            );
        } else {
            ensure!(
                TotalShares::<T>::get() >= math::MINIMUM_LIQUIDITY,
                "the locked shares are gone"
            );
            ensure!(
                Self::ensure_min_reserves(reserves).is_ok(),
                "a reserve is below the minimum"
            );
        }
        Ok(())
    }
}
