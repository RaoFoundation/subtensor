//! What `pallet_subtensor` asks of lending (DESIGN-v2 sections 5 and 8): the custody guards, the
//! coldkey-swap re-key, the pallet hotkey and the two hook points whose economics are on hold.
//!
//! `pallet_subtensor` consumes [`LendingInterface`] through its `Config::Lending`; this pallet
//! implements it, and `()` is the implementation of a runtime without lending.

use codec::Encode;
use frame_support::{
    pallet_prelude::{DispatchResult, Weight},
    traits::Get,
    weights::WeightMeter,
};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::traits::AccountIdConversion;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

use crate::{
    Config, Custody, Due, Event, HolderPositions, InterestRetry, LendingPoolInterface,
    LongLiftedTao, Pallet, Positions, ResidualAlpha, SwappedOutAt, SyntheticOutstanding,
    WeightInfo,
    types::{PositionId, Side},
};

/// Seed of the pallet hotkey under the pallet id. A custody seed is a hash, so it never equals
/// this one.
const PALLET_HOTKEY_SEED: [u8; 6] = *b"hotkey";

/// The most positions one coldkey may hold, counting every subnet and side. The open gate
/// enforces it and a coldkey swap only moves positions to a key that holds none, so the
/// re-key of a coldkey swap is bounded without a limit of its own: the swap declares its
/// weight before it knows the holder and already uses most of a normal-class extrinsic.
pub const MAX_POSITIONS_PER_HOLDER: u32 = 32;

/// The lending hooks of `pallet_subtensor`.
pub trait LendingInterface<AccountId> {
    /// Whether `who` is an account of the lending pallet: a position's custody, the pallet
    /// account or the pallet hotkey. No stake path may move TAO or alpha out of one.
    fn is_lending_custody(who: &AccountId) -> bool;
    /// The alpha of `netuid` held by `who` that lending keeps locked, in rao. Everything a
    /// custody account holds is locked; any other account has none.
    fn locked_alpha(who: &AccountId, netuid: NetUid) -> u64;
    /// The alpha lending has minted on `netuid` and not yet burned, in rao: the open shorts'
    /// `SyntheticOutstanding` plus the `ResidualAlpha` defaulted shorts left in circulation.
    /// It is part of `get_alpha_issuance` (SPEC I2, I7) and of no staker's claim.
    fn lending_alpha(netuid: NetUid) -> u64;
    /// `LongLiftedTao` of `netuid`, in rao: pool TAO that is lender credit, not real TAO;
    /// outflows must never take it (Jacob, Nov 27, C1 option (a)); enforced on swap paths in
    /// the longs task.
    fn long_credit(netuid: NetUid) -> TaoBalance;
    /// Whether a coldkey swap from `old` to `new` may be admitted: `false` if `old` holds
    /// positions and `new` holds any. Read-only. The swap refuses such a destination when it
    /// is announced and when it executes, so [`Self::on_coldkey_swap`] always moves every
    /// position and none is left under a key that may be compromised.
    fn coldkey_swap_destination_ok(old: &AccountId, new: &AccountId) -> bool;
    /// The weight of [`Self::coldkey_swap_destination_ok`].
    fn coldkey_swap_admission_weight() -> Weight;
    /// Moves every position of `old` to `new`, both sides, each with its `Due` entry, its
    /// retry count and its custody account. No funds move. It cannot fail, so it never leaves
    /// a coldkey swap part-done. The caller has checked
    /// [`Self::coldkey_swap_destination_ok`] in the same dispatch. Returns the weight used, at
    /// most [`Self::coldkey_swap_weight`].
    fn on_coldkey_swap(old: &AccountId, new: &AccountId) -> Weight;
    /// Upper bound of the weight of [`Self::coldkey_swap_destination_ok`] and
    /// [`Self::on_coldkey_swap`] together.
    fn coldkey_swap_weight() -> Weight;
    /// The hotkey custody alpha is staked on, or `None` in a runtime without lending. Nobody
    /// else may stake to it, register it or name it as a child.
    fn pallet_hotkey() -> Option<AccountId>;
    /// Called once per dissolved subnet, before any of its pot is distributed. The hook takes
    /// the weight it uses from `meter` itself; the caller accounts for none of it.
    fn on_dissolve(netuid: NetUid, meter: &mut WeightMeter);
    /// Alpha added to the conviction bar a challenger must clear to take ownership of `netuid`.
    fn conviction_bar_extra(netuid: NetUid) -> AlphaBalance;
    /// Called when `netuid` is registered (or registered again), once its pool has its first
    /// reserves: lending seeds the subnet's price rings at the pool's price, so every live
    /// subnet has them (SPEC I25). Returns the weight used.
    fn on_subnet_registered(netuid: NetUid) -> Weight;
}

impl<AccountId> LendingInterface<AccountId> for () {
    fn is_lending_custody(_who: &AccountId) -> bool {
        false
    }

    fn locked_alpha(_who: &AccountId, _netuid: NetUid) -> u64 {
        0
    }

    fn lending_alpha(_netuid: NetUid) -> u64 {
        0
    }

    fn long_credit(_netuid: NetUid) -> TaoBalance {
        TaoBalance::from(0u64)
    }

    fn coldkey_swap_destination_ok(_old: &AccountId, _new: &AccountId) -> bool {
        true
    }

    fn coldkey_swap_admission_weight() -> Weight {
        Weight::zero()
    }

    fn on_coldkey_swap(_old: &AccountId, _new: &AccountId) -> Weight {
        Weight::zero()
    }

    fn coldkey_swap_weight() -> Weight {
        Weight::zero()
    }

    fn pallet_hotkey() -> Option<AccountId> {
        None
    }

    fn on_dissolve(_netuid: NetUid, _meter: &mut WeightMeter) {}

    fn conviction_bar_extra(_netuid: NetUid) -> AlphaBalance {
        AlphaBalance::from(0u64)
    }

    fn on_subnet_registered(_netuid: NetUid) -> Weight {
        Weight::zero()
    }
}

impl<T: Config> Pallet<T> {
    /// The pallet's own account: the coldkey that owns the pallet hotkey.
    #[must_use]
    pub fn pallet_account() -> T::AccountId {
        T::PalletId::get().into_account_truncating()
    }

    /// The hotkey every custody account stakes its alpha on.
    #[must_use]
    pub fn pallet_hotkey_account() -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating(PALLET_HOTKEY_SEED)
    }

    /// Registers the pallet hotkey under the pallet account. Idempotent: the runtime upgrade,
    /// a genesis build and a test setup may each call it.
    pub fn initialize_custody() -> DispatchResult {
        T::Pool::register_pallet_hotkey(&Self::pallet_account(), &Self::pallet_hotkey_account())
    }

    /// Whether `who` was derived from the pallet id. A pallet-derived account starts with
    /// `modl` and the pallet id, and nobody holds a key for such an address, so the test needs
    /// no storage and has no false negative for a custody account.
    #[must_use]
    pub fn is_pallet_derived(who: &T::AccountId) -> bool {
        let prefix = (*b"modl", T::PalletId::get()).encode();
        who.using_encoded(|bytes| bytes.starts_with(&prefix))
    }

    /// The custody account of the position `id` opened at `opened_at`: the one recorded by a
    /// coldkey swap if the position was re-keyed, else the one derived from its key.
    pub(crate) fn position_custody(
        id: &PositionId<T::AccountId>,
        opened_at: BlockNumberFor<T>,
    ) -> T::AccountId {
        Custody::<T>::get(id).unwrap_or_else(|| Self::custody_of(id.2, &id.0, id.1, opened_at))
    }

    /// The custody account of `who`'s open position on `side` of `netuid`, if there is one.
    #[must_use]
    pub fn custody_account(who: &T::AccountId, netuid: NetUid, side: Side) -> Option<T::AccountId> {
        let id = (who.clone(), netuid, side);
        Positions::<T>::get(&id).map(|pos| Self::position_custody(&id, pos.opened_at))
    }

    /// The re-key of [`LendingInterface::on_coldkey_swap`]: every position of `old` moves to
    /// `new`. It has no failure path: every step is a storage move decided from values
    /// already read, so it is all or nothing with or without a storage layer around it.
    ///
    /// `new` holds no position: the swap checked
    /// [`LendingInterface::coldkey_swap_destination_ok`] in this dispatch. So no key of `new`
    /// is taken and the whole list fits ([`MAX_POSITIONS_PER_HOLDER`] bounds it at open). If
    /// a caller skipped the check, nothing moves.
    ///
    /// The custody account stays the one the position was opened with, recorded in `Custody`
    /// under the new key, so the swap moves no TAO and no alpha and cannot strand any.
    ///
    /// If a moved position was opened in this block, `old` opens nothing more in this block
    /// (`SwappedOutAt`): the open would derive the custody account the moved position uses.
    ///
    /// Returns the measured weight of moving the positions it found (`rekey_positions`); a
    /// swap that moves nothing read the two position lists.
    pub(crate) fn rekey_positions(old: &T::AccountId, new: &T::AccountId) -> Weight {
        let nothing_moved = T::DbWeight::get().reads(2);
        if old == new {
            return nothing_moved;
        }
        let held = HolderPositions::<T>::get(old);
        if held.is_empty() {
            return nothing_moved;
        }
        let admitted = HolderPositions::<T>::decode_len(new).unwrap_or(0) == 0;
        debug_assert!(
            admitted,
            "coldkey swap onto a lending holder was not refused"
        );
        if !admitted {
            return nothing_moved;
        }
        let moved = u32::try_from(held.len()).unwrap_or(MAX_POSITIONS_PER_HOLDER);
        let now = frame_system::Pallet::<T>::block_number();
        let mut opened_now = false;
        for (netuid, side) in held.iter().copied() {
            let from = (old.clone(), netuid, side);
            let to = (new.clone(), netuid, side);
            let Some(pos) = Positions::<T>::take(&from) else {
                continue;
            };
            let custody = Self::position_custody(&from, pos.opened_at);
            Custody::<T>::remove(&from);
            Custody::<T>::insert(&to, custody);
            if Due::<T>::take(pos.charge_at, &from).is_some() {
                Due::<T>::insert(pos.charge_at, &to, ());
            }
            let retries = InterestRetry::<T>::take(&from);
            if retries > 0 {
                InterestRetry::<T>::insert(&to, retries);
            }
            opened_now |= pos.opened_at == now;
            Positions::<T>::insert(&to, pos);
            Self::deposit_event(Event::PositionRekeyed {
                old: old.clone(),
                new: new.clone(),
                netuid,
                side,
            });
        }
        HolderPositions::<T>::remove(old);
        HolderPositions::<T>::insert(new, held);
        if opened_now {
            SwappedOutAt::<T>::insert(old, now);
        }
        debug_assert!(
            !Positions::<T>::iter_keys().any(|(who, _, _)| who == *old),
            "a coldkey swap left a lending position with the old key"
        );
        T::WeightInfo::rekey_positions(moved)
    }
}

impl<T: Config> LendingInterface<T::AccountId> for Pallet<T> {
    fn is_lending_custody(who: &T::AccountId) -> bool {
        Self::is_pallet_derived(who)
    }

    fn locked_alpha(who: &T::AccountId, netuid: NetUid) -> u64 {
        if Self::is_pallet_derived(who) {
            T::Pool::held_alpha(who, netuid)
        } else {
            0
        }
    }

    fn lending_alpha(netuid: NetUid) -> u64 {
        SyntheticOutstanding::<T>::get(netuid).saturating_add(ResidualAlpha::<T>::get(netuid))
    }

    fn long_credit(netuid: NetUid) -> TaoBalance {
        TaoBalance::from(LongLiftedTao::<T>::get(netuid))
    }

    fn coldkey_swap_destination_ok(old: &T::AccountId, new: &T::AccountId) -> bool {
        old == new
            || HolderPositions::<T>::decode_len(old).unwrap_or(0) == 0
            || HolderPositions::<T>::decode_len(new).unwrap_or(0) == 0
    }

    /// Both holders' position lists, as measured.
    fn coldkey_swap_admission_weight() -> Weight {
        T::WeightInfo::coldkey_swap_destination_ok()
    }

    fn on_coldkey_swap(old: &T::AccountId, new: &T::AccountId) -> Weight {
        Self::rekey_positions(old, new)
    }

    /// The admission check and the measured re-key of the most positions the old key can
    /// hold, [`MAX_POSITIONS_PER_HOLDER`]: each with its `Due` entry, retry count and custody
    /// record, then the two lists written back and the same-block marker.
    fn coldkey_swap_weight() -> Weight {
        Self::coldkey_swap_admission_weight()
            .saturating_add(T::WeightInfo::rekey_positions(MAX_POSITIONS_PER_HOLDER))
    }

    fn pallet_hotkey() -> Option<T::AccountId> {
        Some(Self::pallet_hotkey_account())
    }

    // HOLD(2.8/2.9): payout split pending spec decision
    // TODO(ResidualAlpha dissolution): when a subnet dissolves, its `ResidualAlpha` (the alpha
    // defaulted shorts left in circulation, SPEC section 1 item 4) must be settled here
    // together with the rest of the payout split, and its entry removed. Not implemented: the
    // payout is pending the spec decision above, so a dissolved subnet keeps its entry.
    fn on_dissolve(_netuid: NetUid, _meter: &mut WeightMeter) {}

    // HOLD(2.6): pending spec decision
    fn conviction_bar_extra(_netuid: NetUid) -> AlphaBalance {
        AlphaBalance::from(0u64)
    }

    /// Seeds the five price items of the subnet (`prices::seed_subnet`), overwriting what a
    /// dissolved subnet of the same netuid left. Reads the two ring sizes, the generation and
    /// the pool (reserves, weight, price).
    fn on_subnet_registered(netuid: NetUid) -> Weight {
        Self::on_subnet_registered(netuid);
        T::DbWeight::get().reads_writes(8, 5)
    }
}
