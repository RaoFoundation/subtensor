//! Expiry-bounded long and short positions on subnet alpha, borrowed from the subnet's own
//! liquidity pool.
//!
//! One position per `(owner, netuid)`, built with one call: `add(side, amount, leverage)`.
//! Adding on the position's own side lifts a further slice `phi` of both pool reserves without
//! moving price, swaps one half into the other token, and folds the result into the position.
//! Adding on the other side settles that much of it at the current price, and flips through
//! zero if there is more. `close` settles everything. At settlement the swap is reversed, the
//! borrowed slice plus the borrow fee go back to the pool, and whatever is left of the owner's
//! cushion and proceeds is paid out. Nothing is minted or burned: the pool only ever gets its
//! own liquidity back.

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
use sp_runtime::traits::{AccountIdConversion, Hash, Saturating, TrailingZeroInput, Zero};
use subtensor_runtime_common::{AlphaBalance, NetUid, SubnetDissolveHook, TaoBalance, Token};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface, Perquintill};

/// Who triggered a settlement.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, RuntimeDebug)]
pub enum Closer<AccountId> {
    /// The owner closed or reduced it, or anyone closed it after expiry.
    Account(AccountId),
    /// The `on_idle` sweep found the position expired.
    Expiry,
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
        BoundedVec<(T::AccountId, NetUid), T::MaxExpiriesPerBlock>,
        ValueQuery,
    >;

    /// First block of `Expiring` not yet swept. Zero means "not started".
    #[pallet::storage]
    pub type NextSweep<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    /// Hotkey owned by the pallet account; all alpha the pallet holds is staked here. Chosen
    /// and registered in the upgrade block from that block's parent hash (see
    /// [`Pallet::claim_hotkey`]), so nobody can register it ahead of the pallet.
    #[pallet::storage]
    pub type PalletHotkey<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// Per-subnet pause switches and cap, replacing the global ones where set. Root-settable
    /// with [`Pallet::sudo_set_subnet_override`]; the lever for one misbehaving pool.
    #[pallet::storage]
    pub type SubnetOverrides<T: Config> =
        StorageMap<_, Identity, NetUid, SubnetOverride, OptionQuery>;

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
            cushion_added: TaoBalance,
            /// Exposure as a percentage of the cushion, as the owner chose it for this tranche.
            leverage_percent: u16,
            /// Proceeds held, debt owed, escrow kept by this tranche, each in its own token.
            legs_added: Legs,
            exposure_added: TaoBalance,
            /// Borrow fee per day this tranche adds, fixed for as long as it is held.
            fee_per_day_added: TaoBalance,
            /// The position's exposure after this add.
            exposure_tao: TaoBalance,
            /// Unchanged by adds after the first.
            expires_at: BlockNumberFor<T>,
        },
        /// Part of a position was settled at the current price; the rest stays open.
        PositionReduced {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            /// Share of the position that was unwound.
            fraction: Perquintill,
            tao_to_owner: TaoBalance,
            /// Fee paid on the whole position, brought up to date at this block.
            fee_paid: TaoBalance,
            /// Debt the settled part could not repay, in the lent token.
            shortfall: Lent,
            /// The position's exposure after this reduction.
            exposure_tao: TaoBalance,
        },
        PositionClosed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            closed_by: Closer<T::AccountId>,
            tao_to_owner: TaoBalance,
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
            error: DispatchError,
            retry_at: Option<BlockNumberFor<T>>,
        },
        ParamsSet {
            params: DerivativesParams<BlockNumberFor<T>>,
        },
        /// `None` means the subnet is back on the global parameters.
        SubnetOverrideSet {
            netuid: NetUid,
            override_: Option<SubnetOverride>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Opening this side is switched off, globally or on this subnet.
        SideDisabled,
        /// The subnet does not exist, is not AMM-priced, or has its subtoken disabled.
        SubnetNotDynamic,
        /// No such position.
        NoPosition,
        /// The position has expired: it can be closed, but nothing can be added to it.
        Expired,
        /// The tranche's cushion is worth less than `min_deposit_tao`.
        DepositTooLow,
        /// Leverage is zero or above the side's maximum (`max_short_leverage_percent` or
        /// `max_long_leverage_percent`).
        LeverageOutOfRange,
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
        /// A maximum leverage, `max_pool_share`, or `lifetime_blocks` is zero.
        InvalidParams,
        /// The pallet has not claimed its hotkey yet; no position can be opened.
        PalletHotkeyUnset,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Claim the pallet hotkey before any extrinsic can. One read once it is set.
        fn on_runtime_upgrade() -> Weight {
            if PalletHotkey::<T>::exists() {
                return T::DbWeight::get().reads(1);
            }
            Self::claim_hotkey();
            T::DbWeight::get().reads_writes(4, 4)
        }

        fn on_idle(now: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining_weight);
            Self::sweep_expired(now, &mut meter);
            meter.consumed()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Add `side` exposure on `netuid`: `leverage_percent / 100` times `amount`, measured
        /// against the pool's TAO reserve. One call covers open, add, reduce, and flip.
        ///
        /// With no position, or one on the same side, `amount` TAO is taken from the caller's
        /// free balance as cushion and a tranche is lifted from the pool and folded into the
        /// position. One day of the tranche's fee is booked up front. Nothing can be added to an
        /// expired position.
        ///
        /// With a position on the other side, this settles the matching share of it at the
        /// current price and pays that share of the cushion, less fee and any loss, to the
        /// caller. If the exposure asked for is larger than the position, the whole position is
        /// closed and the rest, if it reaches `min_deposit_tao`, opens on the new side. Only the
        /// cushion for that rest is taken from the caller.
        ///
        /// The leverage must be above zero and at most the side's maximum
        /// (`max_short_leverage_percent` or `max_long_leverage_percent`).
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::add())]
        pub fn add(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            amount: TaoBalance,
            leverage_percent: u16,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_add(owner, netuid, side, amount, leverage_percent)
        }

        /// Settle `owner`'s position on `netuid` in full. The owner may close at any time;
        /// anyone else only once the position has expired.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::close())]
        pub fn close(origin: OriginFor<T>, owner: T::AccountId, netuid: NetUid) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            let position = Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::NoPosition)?;
            if caller != owner {
                ensure!(
                    frame_system::Pallet::<T>::block_number() >= position.expires_at,
                    Error::<T>::NotExpired
                );
            }
            Self::do_settle(&owner, netuid, Perquintill::one(), Closer::Account(caller)).map(|_| ())
        }

        /// Replace every parameter at once. Root only. Rejects a zero maximum leverage,
        /// `max_pool_share`, or `lifetime_blocks`. Open positions keep the fee and lifetime
        /// they were opened with; a later add is checked against the new values.
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

        /// Pause a side or change the pool-share cap on one subnet. Root only. `None` removes
        /// the override. Affects opens only; positions already open settle as usual.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::sudo_set_subnet_override())]
        pub fn sudo_set_subnet_override(
            origin: OriginFor<T>,
            netuid: NetUid,
            override_: Option<SubnetOverride>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            match override_ {
                Some(value) => {
                    ensure!(value.is_valid(), Error::<T>::InvalidParams);
                    SubnetOverrides::<T>::insert(netuid, value);
                }
                None => SubnetOverrides::<T>::remove(netuid),
            }
            Self::deposit_event(Event::SubnetOverrideSet { netuid, override_ });
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
                while let Some((owner, netuid)) = due.pop() {
                    if !meter.can_consume(settle_cost) {
                        // Put the unfinished tail back and resume here next block.
                        due.try_push((owner, netuid)).ok();
                        Expiring::<T>::insert(cursor, due);
                        NextSweep::<T>::put(cursor);
                        return;
                    }
                    meter.consume(settle_cost);
                    if let Err(error) =
                        Self::do_settle(&owner, netuid, Perquintill::one(), Closer::Expiry)
                    {
                        let retry_at = Self::reschedule_failed(&owner, netuid, now);
                        Self::deposit_event(Event::SettleFailed {
                            owner,
                            netuid,
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
        /// in kind, the cushion goes back to its owner, no fee, no profit or loss.
        /// Settling through swaps is not possible here because the subnet's TAO has already
        /// been taken out of `TotalStake` and its stake maps are about to be converted.
        fn on_subnet_dissolve(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let per_position = T::WeightInfo::close();
            loop {
                if !meter.can_consume(per_position) {
                    return false;
                }
                let Some(owner) = OpenByNetuid::<T>::iter_key_prefix(netuid).next() else {
                    return true;
                };
                meter.consume(per_position);
                Self::unwind(&owner, netuid);
            }
        }
    }
}
