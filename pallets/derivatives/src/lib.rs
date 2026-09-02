//! Expiry-bounded long and short positions on subnet alpha, borrowed from the subnet's own
//! liquidity pool.
//!
//! A position lifts a slice `phi` of both pool reserves without moving price, swaps one half
//! into the other token, and holds everything until close. At close the swap is reversed, the
//! borrowed slice plus a borrow fee go back to the pool, and whatever is left of the user's
//! cushion and proceeds is paid back to the user. Nothing is minted or burned: the pool only
//! ever gets its own liquidity back.

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

use frame_support::{BoundedVec, PalletId, pallet_prelude::*, traits::Get, weights::WeightMeter};
use frame_system::pallet_prelude::*;
use sp_runtime::traits::{AccountIdConversion, Saturating, UniqueSaturatedInto, Zero};
use subtensor_runtime_common::{AlphaBalance, NetUid, SubnetDissolveHook, TaoBalance, Token};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface};

/// Who triggered a settlement.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, RuntimeDebug)]
pub enum Closer<AccountId> {
    /// The owner closed early, or anyone closed after expiry.
    Account(AccountId),
    /// The `on_idle` sweep found the position expired.
    Expiry,
    /// The owner rolled the position: settled it and reopened with what came back.
    Roll,
    /// The subnet was dissolved; the position was cancelled at par.
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

        /// Hotkey owned by the pallet account. All alpha the pallet holds is staked here.
        #[pallet::constant]
        type PalletHotkey: Get<Self::AccountId>;

        /// How many positions may be scheduled to expire in one block. Overflow spills to the
        /// next block.
        #[pallet::constant]
        type MaxExpiriesPerBlock: Get<u32>;

        type WeightInfo: WeightInfo;
    }

    #[pallet::type_value]
    pub fn DefaultParams<T: Config>() -> DerivativesParams<BlockNumberFor<T>> {
        DerivativesParams::defaults()
    }

    #[pallet::storage]
    pub type Params<T: Config> =
        StorageValue<_, DerivativesParams<BlockNumberFor<T>>, ValueQuery, DefaultParams<T>>;

    /// One position per `(owner, netuid, side)`.
    #[pallet::storage]
    pub type Positions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        (NetUid, Side),
        Position<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Index by subnet so dissolution can find every open position.
    #[pallet::storage]
    pub type OpenByNetuid<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        (T::AccountId, Side),
        (),
        OptionQuery,
    >;

    /// Sum of [`Legs::footprint`] over open positions, in the lent token (TAO for shorts, alpha
    /// for longs). Compared against `max_pool_share` of the lent reserve at open.
    #[pallet::storage]
    pub type Footprint<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Identity, Side, u64, ValueQuery>;

    /// Positions that stop being owner-only at this block. Drained by `on_idle`.
    #[pallet::storage]
    pub type Expiring<T: Config> = StorageMap<
        _,
        Identity,
        BlockNumberFor<T>,
        BoundedVec<(T::AccountId, NetUid, Side), T::MaxExpiriesPerBlock>,
        ValueQuery,
    >;

    /// First block of `Expiring` not yet swept. Zero means "not started".
    #[pallet::storage]
    pub type NextSweep<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        PositionOpened {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            cushion: Deposit<T::AccountId>,
            /// Proceeds held, debt owed, escrow kept, each in its own token.
            legs: Legs,
            exposure_tao: TaoBalance,
            /// Borrow fee per day, fixed for the life of the position.
            fee_per_day: TaoBalance,
            expires_at: BlockNumberFor<T>,
        },
        PositionClosed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            closed_by: Closer<T::AccountId>,
            tao_to_owner: TaoBalance,
            alpha_to_owner: AlphaBalance,
            fee_paid: TaoBalance,
            /// Debt the position could not repay, in the lent token.
            shortfall: Lent,
        },
        /// A scheduled settlement failed. The position stays open and can still be closed
        /// permissionlessly. `retry_at` is the block of the next automatic attempt, or `None`
        /// when the retries are used up.
        SettleFailed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            error: DispatchError,
            retry_at: Option<BlockNumberFor<T>>,
        },
        ParamsSet {
            params: DerivativesParams<BlockNumberFor<T>>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Opening this side is switched off.
        SideDisabled,
        /// The subnet does not exist, is not AMM-priced, or has its subtoken disabled.
        SubnetNotDynamic,
        /// The caller already has a position of this side on this subnet.
        PositionExists,
        /// No such position.
        NoPosition,
        /// The cushion is worth less than `min_deposit_tao`.
        DepositTooLow,
        /// Leverage times deposit would take the whole reserve.
        ExposureTooLarge,
        /// Leverage times deposit rounds to nothing.
        ZeroExposure,
        /// Open positions of this side would exceed `max_pool_share` of the lent reserve.
        PoolCapExceeded,
        /// Only the owner may close before `expires_at`.
        NotExpired,
        /// Too many positions already expire in the next blocks.
        ExpiryQueueFull,
        /// The pool swap returned nothing for a non-zero input.
        SwapReturnedZero,
        /// `leverage_percent`, `max_pool_share`, or `lifetime_blocks` is zero.
        InvalidParams,
        /// A roll top-up must be in the token the cushion comes back in, and for alpha on the
        /// same hotkey.
        TopUpMismatch,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Claim `PalletHotkey` for the pallet account before any extrinsic can. Idempotent:
        /// one read once the hotkey exists. Doing this lazily in `open` would let anyone
        /// register the hotkey first and later `swap_hotkey` the pallet's stake away.
        fn on_runtime_upgrade() -> Weight {
            let _ =
                T::Pool::register_pallet_hotkey(&Self::pallet_account(), &T::PalletHotkey::get());
            T::DbWeight::get().reads_writes(1, 3)
        }

        fn on_idle(now: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining_weight);
            Self::sweep_expired(now, &mut meter);
            meter.consumed()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Open a `side` position on `netuid` backed by `deposit`.
        ///
        /// Exposure is `leverage_percent` of the deposit, measured in the deposit's own token
        /// against the matching reserve. The position stays open until the owner closes it or
        /// `lifetime_blocks` pass, after which anyone may close it.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::open())]
        pub fn open(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            deposit: Deposit<T::AccountId>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_open(owner, netuid, side, deposit)
        }

        /// Settle `owner`'s `side` position on `netuid`. The owner may close at any time; anyone
        /// else only once the position has expired. To stay in the trade past expiry, `roll`.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::close())]
        pub fn close(
            origin: OriginFor<T>,
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            let position =
                Positions::<T>::get(&owner, (netuid, side)).ok_or(Error::<T>::NoPosition)?;
            if caller != owner {
                ensure!(
                    frame_system::Pallet::<T>::block_number() >= position.expires_at,
                    Error::<T>::NotExpired
                );
            }
            Self::do_settle(&owner, netuid, side, Closer::Account(caller)).map(|_| ())
        }

        /// Settle the caller's `side` position on `netuid` at the current price and, in the same
        /// transaction, open a fresh one with what came back as the cushion. Owner only.
        ///
        /// The new position gets today's entry price and a full `lifetime_blocks`. The cushion
        /// comes back in its own token and is reopened in that token; TAO profit on an alpha
        /// cushion stays with the owner. `top_up` adds to the new cushion and must be in the
        /// same token (same hotkey for alpha). Fails, leaving the position open, if what comes
        /// back is below `min_deposit_tao` or the pool cap is reached; `close` instead.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::roll())]
        pub fn roll(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            top_up: Option<Deposit<T::AccountId>>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_roll(owner, netuid, side, top_up)
        }

        /// Replace every parameter at once. Root only. Rejects a zero `leverage_percent`,
        /// `max_pool_share`, or `lifetime_blocks`.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::sudo_set_params())]
        pub fn sudo_set_params(
            origin: OriginFor<T>,
            params: DerivativesParams<BlockNumberFor<T>>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(params.is_valid(), Error::<T>::InvalidParams);
            Params::<T>::put(params.clone());
            Self::deposit_event(Event::ParamsSet { params });
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Account derived from the pallet's `PalletId`.
        pub fn pallet_account() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        /// Settle everything that expired up to `now`, as far as `meter` allows.
        pub(crate) fn sweep_expired(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            let step_cost = T::DbWeight::get().reads_writes(2, 2);
            // A failed settle is rescheduled: one position write plus up to
            // `MAX_EXPIRY_SHIFT` queue probes.
            let settle_cost = T::WeightInfo::close().saturating_add(
                T::DbWeight::get()
                    .reads_writes(u64::from(settle::MAX_EXPIRY_SHIFT).saturating_add(1), 2),
            );

            let mut cursor = NextSweep::<T>::get();
            if cursor.is_zero() {
                // First run on a chain that already has history: nothing can have been scheduled
                // before now, so skip the empty prefix instead of reading it block by block.
                cursor = now;
            }

            while cursor <= now {
                if !meter.can_consume(step_cost) {
                    break;
                }
                meter.consume(step_cost);

                let mut due = Expiring::<T>::take(cursor);
                while let Some((owner, netuid, side)) = due.pop() {
                    if !meter.can_consume(settle_cost) {
                        // Put the unfinished tail back and resume here next block.
                        due.try_push((owner, netuid, side)).ok();
                        Expiring::<T>::insert(cursor, due);
                        NextSweep::<T>::put(cursor);
                        return;
                    }
                    meter.consume(settle_cost);
                    if let Err(error) = Self::do_settle(&owner, netuid, side, Closer::Expiry) {
                        let retry_at = Self::reschedule_failed(&owner, netuid, side, now);
                        Self::deposit_event(Event::SettleFailed {
                            owner,
                            netuid,
                            side,
                            error,
                            retry_at,
                        });
                    }
                }
                cursor.saturating_inc();
            }
            NextSweep::<T>::put(cursor);
        }
    }

    impl<T: Config> SubnetDissolveHook for Pallet<T> {
        /// Cancel every position on `netuid` at par: the borrowed slice goes back to the pool
        /// in kind, the cushion goes back to its owner in kind, no fee, no profit or loss.
        /// Settling through swaps is not possible here because the subnet's TAO has already
        /// been taken out of `TotalStake` and its stake maps are about to be converted.
        fn on_subnet_dissolve(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let per_position = T::WeightInfo::close();
            loop {
                if !meter.can_consume(per_position) {
                    return false;
                }
                let Some((owner, side)) = OpenByNetuid::<T>::iter_key_prefix(netuid).next() else {
                    return true;
                };
                meter.consume(per_position);
                Self::unwind(&owner, netuid, side);
            }
        }
    }
}
