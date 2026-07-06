use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use safe_math::FixedExt;
use scale_info::TypeInfo;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::ops::Neg;
use substrate_fixed::transcendental::exp;
use substrate_fixed::types::{I64F64, U64F64};
use subtensor_runtime_common::NetUid;

pub const ONE_YEAR: u64 = 7200 * 365 + 1800;
pub const LOCK_STATE_ZERO_THRESHOLD: u64 = 100;

/// Exponential lock state for a coldkey on a subnet.
#[crate::freeze_struct("1f6be20a66128b8d")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct LockState {
    /// Exponentially decaying locked amount.
    pub locked_mass: AlphaBalance,
    /// Matured decaying score (integral of locked_mass over time).
    pub conviction: U64F64,
    /// Block number of last roll-forward.
    pub last_update: u64,
}

impl LockState {
    pub fn is_zero(&self) -> bool {
        self.locked_mass < AlphaBalance::from(LOCK_STATE_ZERO_THRESHOLD)
            && self.conviction < U64F64::saturating_from_num(LOCK_STATE_ZERO_THRESHOLD)
    }
}

/// Unsigned decrease produced by rolling a lock forward.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RollDelta {
    pub locked_mass_delta: AlphaBalance,
    pub conviction_delta: U64F64,
}

impl RollDelta {
    pub fn zero() -> Self {
        Self {
            locked_mass_delta: AlphaBalance::ZERO,
            conviction_delta: U64F64::saturating_from_num(0),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.locked_mass_delta.is_zero() && self.conviction_delta == U64F64::saturating_from_num(0)
    }
}

/// A struct that incapsulates Lock primitives such as adding, removing,
/// rolling, and updating aggregates.
///
/// This model has one individual lock state, which relates to the stake owner
/// (locking coldkey) lock and 4 aggregates that are maintained in operations.
pub struct ConvictionModel {
    /// Whether this model's individual lock targets the subnet owner hotkey.
    owner_lock: bool,
    /// Whether this model's individual lock uses the non-decaying lock mode.
    perpetual_lock: bool,
    /// Individual stake owner coldkey lock
    individual_lock: LockState,
    individual_lock_dirty: bool,
    /// Perpetual non-owner aggregate
    agg_perpetual_general: LockState,
    agg_perpetual_general_dirty: bool,
    /// Decaying non-owner aggregate
    agg_decaying_general: LockState,
    agg_decaying_general_dirty: bool,
    /// Perpetual owner aggregate
    agg_perpetual_owner: LockState,
    agg_perpetual_owner_dirty: bool,
    /// Decaying owner aggregate
    agg_decaying_owner: LockState,
    agg_decaying_owner_dirty: bool,
}

impl ConvictionModel {
    pub fn new(
        owner_lock: bool,
        perpetual_lock: bool,
        individual_lock: LockState,
        agg_perpetual_general: LockState,
        agg_decaying_general: LockState,
        agg_perpetual_owner: LockState,
        agg_decaying_owner: LockState,
    ) -> Self {
        Self {
            owner_lock,
            perpetual_lock,
            individual_lock,
            individual_lock_dirty: false,
            agg_perpetual_general,
            agg_perpetual_general_dirty: false,
            agg_decaying_general,
            agg_decaying_general_dirty: false,
            agg_perpetual_owner,
            agg_perpetual_owner_dirty: false,
            agg_decaying_owner,
            agg_decaying_owner_dirty: false,
        }
    }

    pub fn individual_lock(&self) -> &LockState {
        &self.individual_lock
    }

    pub fn agg_perpetual_general(&self) -> &LockState {
        &self.agg_perpetual_general
    }

    pub fn agg_decaying_general(&self) -> &LockState {
        &self.agg_decaying_general
    }

    pub fn agg_perpetual_owner(&self) -> &LockState {
        &self.agg_perpetual_owner
    }

    pub fn agg_decaying_owner(&self) -> &LockState {
        &self.agg_decaying_owner
    }

    pub fn aggregate_lock(&self) -> &LockState {
        if self.owner_lock && self.perpetual_lock {
            &self.agg_perpetual_owner
        } else if self.owner_lock {
            &self.agg_decaying_owner
        } else if self.perpetual_lock {
            &self.agg_perpetual_general
        } else {
            &self.agg_decaying_general
        }
    }

    pub fn individual_lock_dirty(&self) -> bool {
        self.individual_lock_dirty
    }

    pub fn agg_perpetual_general_dirty(&self) -> bool {
        self.agg_perpetual_general_dirty
    }

    pub fn agg_decaying_general_dirty(&self) -> bool {
        self.agg_decaying_general_dirty
    }

    pub fn agg_perpetual_owner_dirty(&self) -> bool {
        self.agg_perpetual_owner_dirty
    }

    pub fn agg_decaying_owner_dirty(&self) -> bool {
        self.agg_decaying_owner_dirty
    }

    pub fn merge(&mut self, conv: &ConvictionModel) {
        self.individual_lock = Self::merge_lock(&self.individual_lock, &conv.individual_lock);
        self.individual_lock_dirty = true;
        self.agg_perpetual_general =
            Self::merge_lock(&self.agg_perpetual_general, &conv.agg_perpetual_general);
        self.agg_perpetual_general_dirty = true;
        self.agg_decaying_general =
            Self::merge_lock(&self.agg_decaying_general, &conv.agg_decaying_general);
        self.agg_decaying_general_dirty = true;
        self.agg_perpetual_owner =
            Self::merge_lock(&self.agg_perpetual_owner, &conv.agg_perpetual_owner);
        self.agg_perpetual_owner_dirty = true;
        self.agg_decaying_owner =
            Self::merge_lock(&self.agg_decaying_owner, &conv.agg_decaying_owner);
        self.agg_decaying_owner_dirty = true;
    }

    pub fn set_individual_lock(&mut self, lock: LockState) {
        self.individual_lock = lock;
        self.individual_lock_dirty = true;
    }

    pub fn set_rolled_individual_lock(
        &mut self,
        lock: LockState,
        now: u64,
        unlock_rate: u64,
        maturity_rate: u64,
    ) {
        self.individual_lock = Self::roll_forward_lock(
            lock,
            now,
            unlock_rate,
            maturity_rate,
            self.owner_lock,
            self.perpetual_lock,
        )
        .0;
        self.individual_lock_dirty = true;
    }

    pub fn roll_forward(&mut self, now: u64, unlock_rate: u64, maturity_rate: u64) {
        let (rolled_individual_lock, roll_delta) = Self::roll_forward_lock(
            self.individual_lock.clone(),
            now,
            unlock_rate,
            maturity_rate,
            self.owner_lock,
            self.perpetual_lock,
        );
        self.individual_lock = rolled_individual_lock;
        self.individual_lock_dirty = true;
        if !roll_delta.is_zero() {
            self.apply_roll_delta_to_aggregate(roll_delta, now);
        } else {
            self.roll_forward_aggregate(now, unlock_rate, maturity_rate);
        }
    }

    pub fn roll_forward_aggregate(&mut self, now: u64, unlock_rate: u64, maturity_rate: u64) {
        let owner_lock = self.owner_lock;
        let perpetual_lock = self.perpetual_lock;
        let (aggregate, aggregate_dirty) = self.aggregate_mut();
        *aggregate = Self::roll_forward_lock(
            aggregate.clone(),
            now,
            unlock_rate,
            maturity_rate,
            owner_lock,
            perpetual_lock,
        )
        .0;
        *aggregate_dirty = true;
    }

    pub fn add_to_aggregate(&mut self, added: &LockState) {
        let (aggregate, aggregate_dirty) = self.aggregate_mut();
        *aggregate = Self::merge_lock(aggregate, added);
        *aggregate_dirty = true;
    }

    pub fn reduce_aggregate(&mut self, locked_mass: AlphaBalance, conviction: U64F64) {
        let (aggregate, aggregate_dirty) = self.aggregate_mut();
        *aggregate = Self::reduce_lock(aggregate, locked_mass, conviction);
        *aggregate_dirty = true;
    }

    fn apply_roll_delta_to_aggregate(&mut self, roll_delta: RollDelta, now: u64) {
        let (aggregate, aggregate_dirty) = self.aggregate_mut();
        *aggregate = Self::reduce_lock(
            aggregate,
            roll_delta.locked_mass_delta,
            roll_delta.conviction_delta,
        );
        aggregate.last_update = now;
        *aggregate_dirty = true;
    }

    pub fn reduce(&mut self, locked_mass: AlphaBalance, conviction: U64F64) {
        self.individual_lock = Self::reduce_lock(&self.individual_lock, locked_mass, conviction);
        self.individual_lock_dirty = true;

        let (aggregate, aggregate_dirty) = self.aggregate_mut();
        *aggregate = Self::reduce_lock(aggregate, locked_mass, conviction);
        *aggregate_dirty = true;
    }

    pub fn force_reduce_individual(&mut self, amount: AlphaBalance, now: u64) {
        let rolled = self.individual_lock.clone();
        let new_locked_mass = rolled.locked_mass.saturating_sub(amount);
        let locked_mass_diff = rolled.locked_mass.saturating_sub(new_locked_mass);

        let conviction_diff = if new_locked_mass.is_zero() {
            self.individual_lock = LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::saturating_from_num(0),
                last_update: now,
            };
            rolled.conviction
        } else {
            let removed_proportion = U64F64::saturating_from_num(u64::from(amount))
                .safe_div(U64F64::saturating_from_num(u64::from(rolled.locked_mass)));
            let new_conviction = rolled
                .conviction
                .saturating_mul(U64F64::saturating_from_num(1).saturating_sub(removed_proportion));
            self.individual_lock = LockState {
                locked_mass: new_locked_mass,
                conviction: new_conviction,
                last_update: now,
            };
            rolled.conviction.saturating_sub(new_conviction)
        };
        self.individual_lock_dirty = true;

        self.reduce_aggregate(locked_mass_diff, conviction_diff);
    }

    fn aggregate_mut(&mut self) -> (&mut LockState, &mut bool) {
        if self.owner_lock && self.perpetual_lock {
            (
                &mut self.agg_perpetual_owner,
                &mut self.agg_perpetual_owner_dirty,
            )
        } else if self.owner_lock {
            (
                &mut self.agg_decaying_owner,
                &mut self.agg_decaying_owner_dirty,
            )
        } else if self.perpetual_lock {
            (
                &mut self.agg_perpetual_general,
                &mut self.agg_perpetual_general_dirty,
            )
        } else {
            (
                &mut self.agg_decaying_general,
                &mut self.agg_decaying_general_dirty,
            )
        }
    }

    fn merge_lock(lhs: &LockState, rhs: &LockState) -> LockState {
        LockState {
            locked_mass: lhs.locked_mass.saturating_add(rhs.locked_mass),
            conviction: lhs.conviction.saturating_add(rhs.conviction),
            last_update: lhs.last_update.max(rhs.last_update),
        }
    }

    fn reduce_lock(lock: &LockState, locked_mass: AlphaBalance, conviction: U64F64) -> LockState {
        LockState {
            locked_mass: lock.locked_mass.saturating_sub(locked_mass),
            conviction: lock.conviction.saturating_sub(conviction),
            last_update: lock.last_update,
        }
    }

    pub fn exp_decay(dt: u64, tau: u64) -> U64F64 {
        if tau == 0 || dt == 0 {
            if dt == 0 {
                return U64F64::saturating_from_num(1);
            }
            return U64F64::saturating_from_num(0);
        }
        let min_ratio = I64F64::saturating_from_num(-40);
        let neg_ratio = I64F64::saturating_from_num((dt as i128).neg())
            .checked_div(I64F64::saturating_from_num(tau))
            .unwrap_or(min_ratio);
        let clamped = neg_ratio.max(min_ratio);
        let decay: I64F64 = exp(clamped).unwrap_or(I64F64::saturating_from_num(0));
        if decay < I64F64::saturating_from_num(0) {
            U64F64::saturating_from_num(0)
        } else {
            U64F64::saturating_from_num(decay)
        }
    }

    fn calculate_decayed_mass_and_conviction(
        locked_mass: AlphaBalance,
        conviction: U64F64,
        dt: u64,
        unlock_rate: u64,
        maturity_rate: u64,
        perpetual_lock: bool,
    ) -> (AlphaBalance, U64F64) {
        let unlock_decay = Self::exp_decay(dt, unlock_rate);
        let maturity_decay = Self::exp_decay(dt, maturity_rate);
        let mass_fixed = U64F64::saturating_from_num(locked_mass);
        let new_locked_mass = if perpetual_lock {
            locked_mass
        } else {
            unlock_decay
                .saturating_mul(mass_fixed)
                .saturating_to_num::<u64>()
                .into()
        };

        let conviction_from_existing = maturity_decay.saturating_mul(conviction);
        let conviction_from_mass = if perpetual_lock {
            mass_fixed.saturating_mul(U64F64::saturating_from_num(1).saturating_sub(maturity_decay))
        } else if unlock_rate == maturity_rate {
            let dt_fixed = U64F64::saturating_from_num(dt);
            let maturity_rate_fixed = U64F64::saturating_from_num(maturity_rate);
            mass_fixed.saturating_mul(
                dt_fixed
                    .safe_div(maturity_rate_fixed)
                    .saturating_mul(maturity_decay),
            )
        } else if unlock_rate == 0 || maturity_rate == 0 {
            U64F64::saturating_from_num(0)
        } else {
            let tau_x = I64F64::saturating_from_num(unlock_rate);
            let tau_delta = I64F64::saturating_from_num(
                (unlock_rate as i128).saturating_sub(maturity_rate as i128),
            );
            let decay_delta = I64F64::saturating_from_num(unlock_decay)
                .saturating_sub(I64F64::saturating_from_num(maturity_decay));
            let gamma = tau_x
                .saturating_mul(decay_delta)
                .checked_div(tau_delta)
                .unwrap_or(I64F64::saturating_from_num(0));
            if gamma <= I64F64::saturating_from_num(0) {
                U64F64::saturating_from_num(0)
            } else {
                mass_fixed.saturating_mul(U64F64::saturating_from_num(gamma))
            }
        };
        let new_conviction = conviction_from_existing.saturating_add(conviction_from_mass);
        (new_locked_mass, new_conviction)
    }

    pub fn roll_forward_lock(
        lock: LockState,
        now: u64,
        unlock_rate: u64,
        maturity_rate: u64,
        owner_lock: bool,
        perpetual_lock: bool,
    ) -> (LockState, RollDelta) {
        let previous_locked_mass = lock.locked_mass;
        let previous_conviction = lock.conviction;
        let mut rolled = if now > lock.last_update {
            let dt = now.saturating_sub(lock.last_update);
            let (new_locked_mass, new_conviction) = Self::calculate_decayed_mass_and_conviction(
                lock.locked_mass,
                lock.conviction,
                dt,
                unlock_rate,
                maturity_rate,
                perpetual_lock,
            );

            LockState {
                locked_mass: new_locked_mass,
                conviction: new_conviction,
                last_update: now,
            }
        } else {
            lock
        };

        if owner_lock {
            rolled.conviction = U64F64::saturating_from_num(u64::from(rolled.locked_mass));
        }

        if rolled.is_zero() {
            rolled.locked_mass = AlphaBalance::ZERO;
            rolled.conviction = U64F64::saturating_from_num(0);
        }

        let roll_delta = RollDelta {
            locked_mass_delta: previous_locked_mass.saturating_sub(rolled.locked_mass),
            conviction_delta: previous_conviction.saturating_sub(rolled.conviction),
        };

        (rolled, roll_delta)
    }
}

impl<T: Config> Pallet<T> {
    pub fn add_locking_coldkey(hotkey: &T::AccountId, netuid: NetUid, coldkey: &T::AccountId) {
        LockingColdkeys::<T>::insert((netuid, hotkey, coldkey), ());
    }

    pub fn maybe_remove_locking_coldkey(
        hotkey: &T::AccountId,
        netuid: NetUid,
        coldkey: &T::AccountId,
    ) {
        LockingColdkeys::<T>::remove((netuid, hotkey, coldkey));
    }

    pub fn account_rejects_locked_alpha(coldkey: &T::AccountId) -> bool {
        AccountFlags::<T>::get(coldkey) & crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA != 1
    }

    pub fn set_accept_locked_alpha(coldkey: &T::AccountId, enabled: bool) {
        AccountFlags::<T>::mutate_exists(coldkey, |maybe_flags| {
            let mut flags = maybe_flags.unwrap_or_default();
            if enabled {
                flags |= crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA;
            } else {
                flags &= !crate::ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA;
            }
            *maybe_flags = if flags == 0 { None } else { Some(flags) };
        });
    }

    pub fn ensure_can_receive_locked_alpha(
        coldkey: &T::AccountId,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let rejects_locked_alpha = Self::account_rejects_locked_alpha(coldkey);
        Self::ensure_can_receive_locked_alpha_with_flag(rejects_locked_alpha, amount)
    }

    fn ensure_can_receive_locked_alpha_with_flag(
        rejects_locked_alpha: bool,
        amount: AlphaBalance,
    ) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        ensure!(!rejects_locked_alpha, Error::<T>::AccountRejectsLockedAlpha);
        Ok(())
    }

    pub fn insert_lock_state(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        lock_state: LockState,
    ) {
        if lock_state.is_zero() {
            Self::maybe_remove_locking_coldkey(hotkey, netuid, coldkey);
            // If there is no record previously, this is a no-op
            Lock::<T>::remove((coldkey, netuid, hotkey));
        } else {
            Self::add_locking_coldkey(hotkey, netuid, coldkey);
            Lock::<T>::insert((coldkey, netuid, hotkey), lock_state);
        }
    }

    pub fn insert_hotkey_lock_state(netuid: NetUid, hotkey: &T::AccountId, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            HotkeyLock::<T>::insert(netuid, hotkey, lock_state);
        } else {
            HotkeyLock::<T>::remove(netuid, hotkey);
        }
    }

    pub fn insert_decaying_hotkey_lock_state(
        netuid: NetUid,
        hotkey: &T::AccountId,
        lock_state: LockState,
    ) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            DecayingHotkeyLock::<T>::insert(netuid, hotkey, lock_state);
        } else {
            DecayingHotkeyLock::<T>::remove(netuid, hotkey);
        }
    }

    pub fn insert_owner_lock_state(netuid: NetUid, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            OwnerLock::<T>::insert(netuid, lock_state);
        } else {
            OwnerLock::<T>::remove(netuid);
        }
    }

    pub fn insert_decaying_owner_lock_state(netuid: NetUid, lock_state: LockState) {
        if !lock_state.locked_mass.is_zero()
            || lock_state.conviction > U64F64::saturating_from_num(0)
        {
            DecayingOwnerLock::<T>::insert(netuid, lock_state);
        } else {
            DecayingOwnerLock::<T>::remove(netuid);
        }
    }

    pub(crate) fn is_subnet_owner_hotkey(netuid: NetUid, hotkey: &T::AccountId) -> bool {
        hotkey == &SubnetOwnerHotkey::<T>::get(netuid)
    }

    pub(crate) fn is_perpetual_lock(coldkey: &T::AccountId, netuid: NetUid) -> bool {
        DecayingLock::<T>::get(coldkey, netuid) == Some(false)
    }

    fn empty_lock(now: u64) -> LockState {
        LockState {
            locked_mass: AlphaBalance::ZERO,
            conviction: U64F64::saturating_from_num(0),
            last_update: now,
        }
    }

    pub(crate) fn read_conviction_model_for_hotkey(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        now: u64,
    ) -> ConvictionModel {
        ConvictionModel::new(
            Self::is_subnet_owner_hotkey(netuid, hotkey),
            Self::is_perpetual_lock(coldkey, netuid),
            Lock::<T>::get((coldkey, netuid, hotkey)).unwrap_or_else(|| Self::empty_lock(now)),
            HotkeyLock::<T>::get(netuid, hotkey).unwrap_or_else(|| Self::empty_lock(now)),
            DecayingHotkeyLock::<T>::get(netuid, hotkey).unwrap_or_else(|| Self::empty_lock(now)),
            OwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now)),
            DecayingOwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now)),
        )
    }

    fn read_conviction_model(
        coldkey: &T::AccountId,
        netuid: NetUid,
        now: u64,
    ) -> Option<(T::AccountId, ConvictionModel)> {
        Lock::<T>::iter_prefix((coldkey, netuid))
            .next()
            .map(|(hotkey, _lock)| {
                let model = Self::read_conviction_model_for_hotkey(coldkey, netuid, &hotkey, now);
                (hotkey, model)
            })
    }

    pub(crate) fn save_conviction_model(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        model: ConvictionModel,
    ) {
        if model.individual_lock_dirty() {
            Self::insert_lock_state(coldkey, netuid, hotkey, model.individual_lock().clone());
        }
        if model.agg_perpetual_general_dirty() {
            Self::insert_hotkey_lock_state(netuid, hotkey, model.agg_perpetual_general().clone());
        }
        if model.agg_decaying_general_dirty() {
            Self::insert_decaying_hotkey_lock_state(
                netuid,
                hotkey,
                model.agg_decaying_general().clone(),
            );
        }
        if model.agg_perpetual_owner_dirty() {
            Self::insert_owner_lock_state(netuid, model.agg_perpetual_owner().clone());
        }
        if model.agg_decaying_owner_dirty() {
            Self::insert_decaying_owner_lock_state(netuid, model.agg_decaying_owner().clone());
        }
    }

    pub fn do_set_perpetual_lock(
        coldkey: &T::AccountId,
        netuid: NetUid,
        enabled: bool,
    ) -> DispatchResult {
        let now = Self::get_current_block_as_u64();
        let current_enabled = Self::is_perpetual_lock(coldkey, netuid);

        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            let rolled = model.individual_lock().clone();
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);

            if current_enabled != enabled {
                Self::reduce_aggregate_lock(
                    coldkey,
                    &hotkey,
                    netuid,
                    rolled.locked_mass,
                    rolled.conviction,
                );
            }
        }

        if enabled {
            DecayingLock::<T>::insert(coldkey, netuid, false);
        } else {
            DecayingLock::<T>::remove(coldkey, netuid);
        }

        if current_enabled != enabled
            && let Some((hotkey, model)) = Self::read_conviction_model(coldkey, netuid, now)
        {
            Self::add_aggregate_lock(coldkey, &hotkey, netuid, model.individual_lock().clone());
        }
        Self::deposit_event(Event::PerpetualLockUpdated {
            coldkey: coldkey.clone(),
            netuid,
            enabled,
        });
        Ok(())
    }

    /// Returns the sum of raw alpha shares for a coldkey across all hotkeys on a given subnet.
    pub fn total_coldkey_alpha_on_subnet(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .map(|hotkey| {
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, coldkey, netuid)
            })
            .fold(AlphaBalance::ZERO, |acc, stake| acc.saturating_add(stake))
    }

    /// Returns the current locked amount for a coldkey on a subnet.
    pub fn get_current_locked(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now)
            .map(|(_hotkey, mut model)| {
                model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
                model.individual_lock().locked_mass
            })
            .unwrap_or(AlphaBalance::ZERO)
    }

    /// Returns the current conviction for a coldkey on a subnet (rolled forward to now).
    pub fn get_conviction(coldkey: &T::AccountId, netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now)
            .map(|(_hotkey, mut model)| {
                model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
                model.individual_lock().conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0))
    }

    /// Returns the current lock for a coldkey on a subnet, rolled forward to now.
    pub fn get_coldkey_lock(coldkey: &T::AccountId, netuid: NetUid) -> Option<LockState> {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now).map(|(_hotkey, mut model)| {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.individual_lock().clone()
        })
    }

    /// (total_stake, locked_mass, available_to_unstake) for a coldkey on one subnet.
    ///
    /// The lock is subnet-wide: it blocks unstaking from any hotkey on that subnet,
    /// not from a single hotkey position.
    pub fn stake_availability(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> (AlphaBalance, AlphaBalance, AlphaBalance) {
        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let locked = Self::get_current_locked(coldkey, netuid);
        let available = total.saturating_sub(locked);
        (total, locked, available)
    }

    /// Alpha the coldkey can still unstake on this subnet right now.
    pub fn available_to_unstake(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        let (_, _, available) = Self::stake_availability(coldkey, netuid);
        available
    }

    /// Ensures that the amount can be unstaked
    pub fn ensure_available_to_unstake(
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let alpha_available = Self::available_to_unstake(coldkey, netuid);
        ensure!(alpha_available >= amount, Error::<T>::StakeUnavailable);
        Ok(())
    }

    /// Locks stake for a coldkey on a subnet to a specific hotkey.
    /// If no lock exists, creates one. If one exists, the hotkey must match.
    /// Top-up adds to locked_mass after rolling forward.
    pub fn do_lock_stake(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        amount: AlphaBalance,
    ) -> dispatch::DispatchResult {
        ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let now = Self::get_current_block_as_u64();

        let mut model = match Self::read_conviction_model(coldkey, netuid, now) {
            Some((existing_hotkey, model)) => {
                ensure!(*hotkey == existing_hotkey, Error::<T>::LockHotkeyMismatch);
                model
            }
            None => Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now),
        };
        model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());

        if model.individual_lock().locked_mass.is_zero()
            && model.individual_lock().conviction == U64F64::saturating_from_num(0)
        {
            ensure!(total >= amount, Error::<T>::InsufficientStakeForLock);

            model.set_rolled_individual_lock(
                LockState {
                    locked_mass: amount,
                    conviction: U64F64::saturating_from_num(0),
                    last_update: now,
                },
                now,
                UnlockRate::<T>::get(),
                MaturityRate::<T>::get(),
            );
        } else {
            let mut lock = model.individual_lock().clone();
            lock.locked_mass = lock.locked_mass.saturating_add(amount);
            ensure!(
                total >= lock.locked_mass,
                Error::<T>::InsufficientStakeForLock
            );
            model.set_rolled_individual_lock(
                lock,
                now,
                UnlockRate::<T>::get(),
                MaturityRate::<T>::get(),
            );
        }

        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.add_to_aggregate(&LockState {
            locked_mass: amount,
            conviction: U64F64::saturating_from_num(0),
            last_update: now,
        });
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        Self::save_conviction_model(coldkey, netuid, hotkey, model);

        Self::deposit_event(Event::StakeLocked {
            coldkey: coldkey.clone(),
            hotkey: hotkey.clone(),
            netuid,
            amount,
        });

        Ok(())
    }

    /// Reduces the coldkey lock by a specified alpha amount and the coldkey conviction
    /// proportionally.
    pub fn force_reduce_lock(coldkey: &T::AccountId, netuid: NetUid, amount: AlphaBalance) {
        let now = Self::get_current_block_as_u64();
        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.force_reduce_individual(amount, now);
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);
        }
    }

    /// Rolls the lock forward to now and persists it if the locked mass is zero. This is used when we want to
    /// update the lock when a user stakes or unstakes.
    pub fn cleanup_lock_if_zero(coldkey: &T::AccountId, netuid: NetUid) {
        let now = Self::get_current_block_as_u64();

        // Cleanup locks for the specific coldkey and hotkey
        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);
        }
    }

    /// Update the total lock for a hotkey on a subnet or create one if
    /// it doesn't exist.
    ///
    /// Roll the existing hotkey lock forward to now, then add the
    /// latest conviction and locked mass.
    pub fn upsert_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        let now = Self::get_current_block_as_u64();
        Self::add_aggregate_lock(
            coldkey,
            hotkey,
            netuid,
            LockState {
                locked_mass: amount,
                conviction: U64F64::saturating_from_num(0),
                last_update: now,
            },
        );
    }

    /// Merges an already-existing lock state into the aggregate lock bucket.
    ///
    /// This is used when lock state moves between keys, such as lock moves, stake
    /// transfers, or coldkey swaps. Unlike `upsert_aggregate_lock`, this preserves
    /// both locked mass and conviction from the moved lock because that conviction
    /// was already earned before the aggregate bucket changed.
    ///
    /// Locks to the subnet owner hotkey are merged into `OwnerLock`; all other
    /// locks are merged into the destination hotkey's perpetual or decaying bucket.
    fn add_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        added: LockState,
    ) {
        let now = Self::get_current_block_as_u64();
        let mut model = Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.add_to_aggregate(&added);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        Self::save_conviction_model(coldkey, netuid, hotkey, model);
    }

    /// Reduces locked mass and conviction from exactly one aggregate bucket.
    fn reduce_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
        conviction: U64F64,
    ) {
        let now = Self::get_current_block_as_u64();
        let mut model = Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.reduce_aggregate(amount, conviction);
        Self::save_conviction_model(coldkey, netuid, hotkey, model);
    }

    /// Returns the total conviction for a hotkey on a subnet,
    /// summed over all coldkeys that have locked to this hotkey.
    pub fn hotkey_conviction(hotkey: &T::AccountId, netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let perpetual_conviction = HotkeyLock::<T>::get(netuid, hotkey)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    true,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_conviction = DecayingHotkeyLock::<T>::get(netuid, hotkey)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    false,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let hotkey_conviction = perpetual_conviction.saturating_add(decaying_conviction);
        if hotkey == &SubnetOwnerHotkey::<T>::get(netuid) {
            let owner_conviction = OwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        true,
                    )
                    .0
                    .conviction
                })
                .unwrap_or_else(|| U64F64::saturating_from_num(0));
            let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        false,
                    )
                    .0
                    .conviction
                })
                .unwrap_or_else(|| U64F64::saturating_from_num(0));
            hotkey_conviction
                .saturating_add(owner_conviction)
                .saturating_add(decaying_owner_conviction)
        } else {
            hotkey_conviction
        }
    }

    /// Returns total rolled aggregate conviction across all hotkey and owner locks on a subnet.
    pub fn get_total_conviction(netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let hotkey_conviction = HotkeyLock::<T>::iter_prefix(netuid)
            .map(|(_hotkey, lock)| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    true,
                )
                .0
                .conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let decaying_hotkey_conviction = DecayingHotkeyLock::<T>::iter_prefix(netuid)
            .map(|(_hotkey, lock)| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    false,
                )
                .0
                .conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let owner_conviction = OwnerLock::<T>::get(netuid)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    true,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    false,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));

        hotkey_conviction
            .saturating_add(decaying_hotkey_conviction)
            .saturating_add(owner_conviction)
            .saturating_add(decaying_owner_conviction)
    }

    /// Finds the hotkey with the highest conviction on a given subnet.
    pub fn subnet_king(netuid: NetUid) -> Option<T::AccountId> {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let mut scores: BTreeMap<T::AccountId, U64F64> = BTreeMap::new();

        HotkeyLock::<T>::iter_prefix(netuid).for_each(|(hotkey, lock)| {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                true,
            );
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        });
        DecayingHotkeyLock::<T>::iter_prefix(netuid).for_each(|(hotkey, lock)| {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                false,
            );
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        });
        if let Some(lock) = OwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                true,
            );
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        }
        if let Some(lock) = DecayingOwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                false,
            );
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        }

        scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal))
            .map(|(hotkey, _)| hotkey)
    }

    /// Reassigns subnet ownership to the current lock-conviction leader when the subnet
    /// is mature enough and enough conviction has accumulated.
    ///
    /// Ownership can change only after the subnet is at least [`ONE_YEAR`] old and the
    /// total rolled aggregate conviction on the subnet is at least 10% of `SubnetAlphaOut`.
    /// If those gates pass, the hotkey with the highest rolled aggregate conviction
    /// becomes the subnet owner hotkey, and that hotkey's owning coldkey becomes the
    /// subnet owner coldkey. The new owner hotkey's conviction is then progressed to
    /// its current locked mass so the new owner starts with full owner conviction.
    pub fn change_subnet_owner_if_needed(netuid: NetUid) {
        // No outstanding alpha means there is no meaningful 10% conviction threshold.
        let subnet_alpha_out = SubnetAlphaOut::<T>::get(netuid);
        if subnet_alpha_out.is_zero() {
            return;
        }

        // Ownership can only be reassigned after the subnet has aged for one year.
        let now = Self::get_current_block_as_u64();
        let registered_at = NetworkRegisteredAt::<T>::get(netuid);
        if now < registered_at.saturating_add(ONE_YEAR) {
            return;
        }

        // Require total rolled aggregate conviction to be at least 10% of subnet alpha out.
        let total_conviction = Self::get_total_conviction(netuid);
        if total_conviction.saturating_mul(U64F64::saturating_from_num(10))
            < U64F64::saturating_from_num(u64::from(subnet_alpha_out))
        {
            return;
        }

        // Pick the hotkey with the highest rolled aggregate conviction.
        let Some(king_hotkey) = Self::subnet_king(netuid) else {
            return;
        };

        // The king hotkey must resolve to a real coldkey owner.
        let new_owner_coldkey = Self::get_owning_coldkey_for_hotkey(&king_hotkey);
        if new_owner_coldkey == DefaultAccount::<T>::get() {
            return;
        }

        // If the winning hotkey already belongs to the current owner, nothing changes.
        let current_owner_coldkey = SubnetOwner::<T>::get(netuid);
        if new_owner_coldkey == current_owner_coldkey {
            return;
        }
        let old_owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();

        // Register new owner as a neuron if not yet registered.
        if Self::get_uid_for_net_and_hotkey(netuid, &king_hotkey).is_err()
            && Self::register_neuron(netuid, &king_hotkey).is_err()
        {
            return;
        }

        // Move aggregate buckets using the hotkey's new role.
        if let Some(owner_lock) = OwnerLock::<T>::take(netuid) {
            let moved_owner_lock = ConvictionModel::roll_forward_lock(
                owner_lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                true,
            );
            let current = HotkeyLock::<T>::get(netuid, &old_owner_hotkey)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        true,
                    )
                    .0
                })
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_hotkey_lock_state(
                netuid,
                &old_owner_hotkey,
                LockState {
                    locked_mass: current
                        .locked_mass
                        .saturating_add(moved_owner_lock.0.locked_mass),
                    conviction: current
                        .conviction
                        .saturating_add(moved_owner_lock.0.conviction),
                    last_update: now,
                },
            );
        }
        if let Some(owner_lock) = DecayingOwnerLock::<T>::take(netuid) {
            let moved_owner_lock = ConvictionModel::roll_forward_lock(
                owner_lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                false,
            );
            let current = DecayingHotkeyLock::<T>::get(netuid, &old_owner_hotkey)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        false,
                    )
                    .0
                })
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_decaying_hotkey_lock_state(
                netuid,
                &old_owner_hotkey,
                LockState {
                    locked_mass: current
                        .locked_mass
                        .saturating_add(moved_owner_lock.0.locked_mass),
                    conviction: current
                        .conviction
                        .saturating_add(moved_owner_lock.0.conviction),
                    last_update: now,
                },
            );
        }
        if let Some(king_lock) = HotkeyLock::<T>::take(netuid, &king_hotkey) {
            let moved_king_lock = ConvictionModel::roll_forward_lock(
                king_lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                true,
            );
            let current = OwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        true,
                    )
                    .0
                })
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_owner_lock_state(
                netuid,
                ConvictionModel::roll_forward_lock(
                    LockState {
                        locked_mass: current
                            .locked_mass
                            .saturating_add(moved_king_lock.0.locked_mass),
                        conviction: current
                            .conviction
                            .saturating_add(moved_king_lock.0.conviction),
                        last_update: now,
                    },
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    true,
                )
                .0,
            );
        }
        if let Some(king_lock) = DecayingHotkeyLock::<T>::take(netuid, &king_hotkey) {
            let moved_king_lock = ConvictionModel::roll_forward_lock(
                king_lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                false,
            );
            let current = DecayingOwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        false,
                    )
                    .0
                })
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_decaying_owner_lock_state(
                netuid,
                ConvictionModel::roll_forward_lock(
                    LockState {
                        locked_mass: current
                            .locked_mass
                            .saturating_add(moved_king_lock.0.locked_mass),
                        conviction: current
                            .conviction
                            .saturating_add(moved_king_lock.0.conviction),
                        last_update: now,
                    },
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    false,
                )
                .0,
            );
        }

        // Reassign subnet owner coldkey and owner hotkey.
        SubnetOwner::<T>::insert(netuid, new_owner_coldkey.clone());
        SubnetOwnerHotkey::<T>::insert(netuid, king_hotkey.clone());
        Self::deposit_event(Event::SubnetOwnerChanged {
            netuid,
            old_coldkey: current_owner_coldkey,
            new_coldkey: new_owner_coldkey,
        });
    }

    /// Ensure the coldkey does not have an active lock on any subnets.
    pub fn ensure_no_active_locks(coldkey: &T::AccountId) -> Result<(), Error<T>> {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();

        for ((netuid, hotkey), lock) in Lock::<T>::iter_prefix((coldkey,)) {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                Self::is_perpetual_lock(coldkey, netuid),
            );
            if rolled.0.locked_mass > AlphaBalance::ZERO {
                return Err(Error::<T>::ActiveLockExists);
            }
        }

        Ok(())
    }

    /// Transfers the lock from one coldkey to another for all subnets. This is used when a
    /// user swaps their coldkey and we want to preserve their locks.
    ///
    /// The hotkey and netuid remain the same, only the coldkey changes.
    ///
    /// The new coldkey must have no active locks, so we can transfer the locks
    /// "as is" without rolling them forward and the
    /// HotkeyLock map does not change (because it only contains totals, not individual coldkey locks).
    pub fn swap_coldkey_locks(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        Self::ensure_no_active_locks(new_coldkey)?;

        let mut locks_to_transfer: Vec<(NetUid, T::AccountId, LockState)> = Vec::new();
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let new_coldkey_rejects_locked_alpha = Self::account_rejects_locked_alpha(new_coldkey);
        let decaying_locks_to_transfer: Vec<(NetUid, bool)> =
            DecayingLock::<T>::iter_prefix(old_coldkey).collect();

        // Gather locks for old coldkey
        for ((netuid, hotkey), lock) in Lock::<T>::iter_prefix((old_coldkey,)) {
            locks_to_transfer.push((netuid, hotkey, lock));
        }

        let mut rolled_locks_to_transfer: Vec<(NetUid, T::AccountId, LockState, bool)> = Vec::new();
        for (netuid, hotkey, lock) in locks_to_transfer {
            let perpetual_lock = decaying_locks_to_transfer
                .iter()
                .any(|(decaying_netuid, decaying)| *decaying_netuid == netuid && !*decaying);
            let (old_lock, _) = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                perpetual_lock,
            );
            Self::ensure_can_receive_locked_alpha_with_flag(
                new_coldkey_rejects_locked_alpha,
                old_lock.locked_mass,
            )?;
            rolled_locks_to_transfer.push((netuid, hotkey, old_lock, perpetual_lock));
        }

        // Remove old locks and reduce old aggregate buckets before moving the
        // perpetual-lock flags; aggregate selection depends on the old flag.
        for (netuid, hotkey, old_lock, _) in rolled_locks_to_transfer.iter() {
            Lock::<T>::remove((old_coldkey.clone(), *netuid, hotkey.clone()));
            Self::maybe_remove_locking_coldkey(hotkey, *netuid, old_coldkey);
            Self::reduce_aggregate_lock(
                old_coldkey,
                hotkey,
                *netuid,
                old_lock.locked_mass,
                old_lock.conviction,
            );
        }

        for (netuid, _) in decaying_locks_to_transfer {
            if let Some(decaying) = DecayingLock::<T>::take(old_coldkey, netuid) {
                DecayingLock::<T>::insert(new_coldkey, netuid, decaying);
            }
        }

        let flags = AccountFlags::<T>::get(old_coldkey);
        AccountFlags::<T>::remove(old_coldkey);
        if flags != 0 {
            AccountFlags::<T>::insert(new_coldkey, flags);
        } else {
            AccountFlags::<T>::remove(new_coldkey);
        }

        // Insert locks for the new coldkey and add to the destination aggregate
        // buckets after the flags have moved.
        for (netuid, hotkey, old_lock, perpetual_lock) in rolled_locks_to_transfer {
            let new_lock = ConvictionModel::roll_forward_lock(
                old_lock.clone(),
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                perpetual_lock,
            )
            .0;
            Self::insert_lock_state(new_coldkey, netuid, &hotkey, new_lock.clone());
            Self::add_aggregate_lock(new_coldkey, &hotkey, netuid, new_lock);
        }

        Ok(())
    }

    /// Swap all locks made to the old_hotkey to new_hotkey on all netuids
    ///
    /// There is no need to roll the locks, they can be just copied "as is":
    /// The lock relation between coldkeys and hotkey is 1:1, so if old hotkey has a
    /// coldkey locking to it, then the same coldkey cannot lock to the new hotkey.
    /// And in reverse: If a coldkey is locking to the new hotkey, it will not appear
    /// in the transfer list because it does not lock to the old hotkey.
    ///
    /// Conviction is not reset because the hotkey ownership does not change, it's still
    /// the same hotkey owner who will own the new hotkey.
    pub fn swap_hotkey_locks(old_hotkey: &T::AccountId, new_hotkey: &T::AccountId) -> (u64, u64) {
        Self::swap_hotkey_locks_for_netuids(old_hotkey, new_hotkey, Self::get_all_subnet_netuids())
    }

    /// Swap locks made to the old_hotkey to new_hotkey on one netuid.
    pub fn swap_hotkey_locks_on_subnet(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> (u64, u64) {
        Self::swap_hotkey_locks_for_netuids(old_hotkey, new_hotkey, vec![netuid])
    }

    fn swap_hotkey_locks_for_netuids(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuids: Vec<NetUid>,
    ) -> (u64, u64) {
        let mut locks_to_transfer: Vec<(T::AccountId, NetUid, LockState)> = Vec::new();
        let mut netuids_to_transfer: Vec<(NetUid, bool, bool)> = Vec::new();
        let mut reads: u64 = 0;
        let mut writes: u64 = 0;

        for netuid in netuids.iter().copied() {
            let old_is_owner_hotkey = Self::is_subnet_owner_hotkey(netuid, old_hotkey);
            let new_is_owner_hotkey = Self::is_subnet_owner_hotkey(netuid, new_hotkey);
            let has_hotkey_lock = HotkeyLock::<T>::contains_key(netuid, old_hotkey);
            let has_decaying_hotkey_lock =
                DecayingHotkeyLock::<T>::contains_key(netuid, old_hotkey);
            let has_owner_lock = old_is_owner_hotkey && OwnerLock::<T>::contains_key(netuid);
            let has_decaying_owner_lock =
                old_is_owner_hotkey && DecayingOwnerLock::<T>::contains_key(netuid);

            if old_is_owner_hotkey
                || new_is_owner_hotkey
                || has_hotkey_lock
                || has_decaying_hotkey_lock
                || has_owner_lock
                || has_decaying_owner_lock
            {
                netuids_to_transfer.push((
                    netuid,
                    old_is_owner_hotkey,
                    old_is_owner_hotkey || new_is_owner_hotkey,
                ));
            }
            reads = reads.saturating_add(5);
        }

        // Build a concrete transfer list from the hotkey-to-coldkey index.
        // The index can contain stale coldkeys, so only locks that still exist
        // are carried forward; missing locks are pruned from the index.
        for (netuid, _, _) in &netuids_to_transfer {
            for (coldkey, _) in LockingColdkeys::<T>::iter_prefix((*netuid, old_hotkey)) {
                if let Some(lock) = Lock::<T>::get((coldkey.clone(), *netuid, old_hotkey.clone())) {
                    locks_to_transfer.push((coldkey, *netuid, lock));
                } else {
                    Self::maybe_remove_locking_coldkey(old_hotkey, *netuid, &coldkey);
                    writes = writes.saturating_add(1);
                }
                reads = reads.saturating_add(1);
            }
        }

        for (coldkey, netuid, lock) in locks_to_transfer {
            let now = Self::get_current_block_as_u64();
            let unlock_rate = UnlockRate::<T>::get();
            let maturity_rate = MaturityRate::<T>::get();
            let old_owner_lock = netuids_to_transfer
                .iter()
                .any(|(rebuild_netuid, is_owner, _)| *rebuild_netuid == netuid && *is_owner);
            let new_owner_lock = netuids_to_transfer
                .iter()
                .any(|(rebuild_netuid, _, is_owner)| *rebuild_netuid == netuid && *is_owner);
            let perpetual_lock = Self::is_perpetual_lock(&coldkey, netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                old_owner_lock,
                perpetual_lock,
            )
            .0;
            let moved = ConvictionModel::roll_forward_lock(
                rolled,
                now,
                unlock_rate,
                maturity_rate,
                new_owner_lock,
                perpetual_lock,
            )
            .0;
            Lock::<T>::remove((coldkey.clone(), netuid, old_hotkey.clone()));
            Self::maybe_remove_locking_coldkey(old_hotkey, netuid, &coldkey);
            Self::insert_lock_state(&coldkey, netuid, new_hotkey, moved);
            writes = writes.saturating_add(2);
        }

        for (netuid, old_was_owner, new_is_owner) in netuids_to_transfer {
            let now = Self::get_current_block_as_u64();
            let unlock_rate = UnlockRate::<T>::get();
            let maturity_rate = MaturityRate::<T>::get();
            let moved_perpetual_lock = if old_was_owner {
                OwnerLock::<T>::take(netuid).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        true,
                    )
                    .0
                })
            } else {
                HotkeyLock::<T>::take(netuid, old_hotkey).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        true,
                    )
                    .0
                })
            };
            let moved_decaying_lock = if old_was_owner {
                DecayingOwnerLock::<T>::take(netuid).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        false,
                    )
                    .0
                })
            } else {
                DecayingHotkeyLock::<T>::take(netuid, old_hotkey).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        false,
                    )
                    .0
                })
            };

            if let Some(lock) = moved_perpetual_lock {
                if new_is_owner {
                    Self::insert_owner_lock_state(
                        netuid,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            true,
                            true,
                        )
                        .0,
                    );
                } else {
                    Self::insert_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            false,
                            true,
                        )
                        .0,
                    );
                }
            }
            if let Some(lock) = moved_decaying_lock {
                if new_is_owner {
                    Self::insert_decaying_owner_lock_state(
                        netuid,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            true,
                            false,
                        )
                        .0,
                    );
                } else {
                    Self::insert_decaying_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            false,
                            false,
                        )
                        .0,
                    );
                }
            }
            writes = writes.saturating_add(6);
        }
        (reads, writes)
    }

    /// Moves lock from one hotkey to another and clears conviction
    ///
    /// The lock is rolled forward to the current block before switching the
    /// associated hotkey so that the lock stays mathematically correct and
    /// preserves current decayed locked mass.
    ///
    /// The conviction is reset to zero if the destination and source hotkeys
    /// are owned by different coldkeys, otherwise it is preserved.
    pub fn do_move_lock(
        coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> DispatchResult {
        ensure!(
            Self::hotkey_account_exists(destination_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        let now = Self::get_current_block_as_u64();

        match Self::read_conviction_model(coldkey, netuid, now) {
            Some((origin_hotkey, mut model)) => {
                let unlock_rate = UnlockRate::<T>::get();
                let maturity_rate = MaturityRate::<T>::get();
                model.roll_forward(now, unlock_rate, maturity_rate);
                let mut lock = model.individual_lock().clone();
                let removed = lock.clone();

                if Self::get_owning_coldkey_for_hotkey(&origin_hotkey)
                    != Self::get_owning_coldkey_for_hotkey(destination_hotkey)
                {
                    lock.conviction = U64F64::saturating_from_num(0);
                }
                lock = ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    Self::is_subnet_owner_hotkey(netuid, destination_hotkey),
                    Self::is_perpetual_lock(coldkey, netuid),
                )
                .0;

                Lock::<T>::remove((coldkey.clone(), netuid, origin_hotkey.clone()));
                Self::maybe_remove_locking_coldkey(&origin_hotkey, netuid, coldkey);
                Self::insert_lock_state(coldkey, netuid, destination_hotkey, lock.clone());
                Self::reduce_aggregate_lock(
                    coldkey,
                    &origin_hotkey,
                    netuid,
                    removed.locked_mass,
                    removed.conviction,
                );
                Self::add_aggregate_lock(coldkey, destination_hotkey, netuid, lock);

                Self::deposit_event(Event::LockMoved {
                    coldkey: coldkey.clone(),
                    origin_hotkey,
                    destination_hotkey: destination_hotkey.clone(),
                    netuid,
                });
                Ok(())
            }
            None => Err(Error::<T>::NoExistingLock.into()),
        }
    }

    pub fn auto_lock_owner_cut(netuid: NetUid, amount: AlphaBalance) {
        if !OwnerCutAutoLockEnabled::<T>::get(netuid) {
            return;
        }

        let subnet_owner_coldkey = Self::get_subnet_owner(netuid);

        // Determine the lock hotkey. If no locks exist, assign subnet owner's hotkey, otherwise
        // auto-lock to existing lock hotkey
        let lock_hotkey = if let Some((existing_hotkey, _model)) = Self::read_conviction_model(
            &subnet_owner_coldkey,
            netuid,
            Self::get_current_block_as_u64(),
        ) {
            existing_hotkey
        } else {
            SubnetOwnerHotkey::<T>::get(netuid)
        };

        // Ignore the result. It may only fail if amount is zero, which is OK to ignore because nothing
        // needs to happen in that case
        let _ = Self::do_lock_stake(&subnet_owner_coldkey, netuid, &lock_hotkey, amount);
    }

    /// When locked stake is transfered, the lock should follow the stake
    ///
    /// First, this function rolls the lock forward and checks if amount is over available
    /// stake and if it is, the stake that's over the available amount on the destination
    /// coldkey is locked in the same way as the original stake: If original stake is locked to
    /// a hotkey, it remains locked to the same hotkey. Conviction is moved proportionally to
    /// the moved locked amount of alpha. For example, if 20% of locked alpha is moved, then
    /// also 20% of conviction is moved.
    pub fn transfer_lock(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let now = Self::get_current_block_as_u64();

        // If no actual transfer happens, this is ok
        if origin_coldkey == destination_coldkey || amount.is_zero() {
            return Ok(());
        }

        // Read total alpha of the coldkey on this netuid. Do not check if total alpha is
        // lower than amount transferred, this is responsibility of a higher level, this
        // function needs to act protectively.
        let total_alpha = Self::total_coldkey_alpha_on_subnet(origin_coldkey, netuid);
        let mut remaining_to_transfer = amount;

        // Read the locks for source and destination coldkey (if exist) and roll forward
        let Some((source_hotkey, mut source_model)) =
            Self::read_conviction_model(origin_coldkey, netuid, now)
        else {
            return Ok(());
        };

        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        source_model.roll_forward(now, unlock_rate, maturity_rate);
        let mut source_lock = source_model.individual_lock().clone();
        let maybe_destination_lock = Self::read_conviction_model(destination_coldkey, netuid, now)
            .map(|(hotkey, mut model)| {
                model.roll_forward(now, unlock_rate, maturity_rate);
                (hotkey, model.individual_lock().clone())
            });

        let mut destination_hotkey = maybe_destination_lock
            .as_ref()
            .map(|(hotkey, _)| hotkey.clone())
            .unwrap_or_else(|| source_hotkey.clone());
        let mut destination_lock = maybe_destination_lock
            .as_ref()
            .map(|(_, lock)| lock.clone())
            .unwrap_or(LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::saturating_from_num(0),
                last_update: now,
            });

        // Calculate available stake by subtracting locked_mass from total alpha.
        let unavailable = source_lock.locked_mass;
        let available_stake = total_alpha.saturating_sub(unavailable);

        // Reduce remaining_to_transfer by min(remaining_to_transfer, available stake)
        let available_transfer = remaining_to_transfer.min(available_stake);
        remaining_to_transfer = remaining_to_transfer.saturating_sub(available_transfer);

        // If result is non-zero, check the hotkey match between source and destination coldkey locks
        // (if destination coldkey lock exists). If no match, error out with LockHotkeyMismatch, otherwise,
        // reduce remaining_to_transfer by min(remaining_to_transfer, locked_mass), reduce locked_mass on
        // the source coldkey by the same amount, increase locked_mass on the destination coldkey by the
        // same amount, reduce conviction on the source coldkey proportionally, and increase conviction
        // on the destination coldkey proportionally.
        let mut locked_transfer = AlphaBalance::ZERO;
        let mut conviction_transfer = U64F64::saturating_from_num(0);
        if !remaining_to_transfer.is_zero() {
            if let Some((existing_hotkey, _)) = maybe_destination_lock.as_ref() {
                ensure!(
                    existing_hotkey == &source_hotkey,
                    Error::<T>::LockHotkeyMismatch
                );
                destination_hotkey = existing_hotkey.clone();
            }

            locked_transfer = remaining_to_transfer.min(source_lock.locked_mass);
            conviction_transfer = if locked_transfer.is_zero() || source_lock.locked_mass.is_zero()
            {
                U64F64::saturating_from_num(0)
            } else {
                let locked_transfer = U64F64::saturating_from_num(locked_transfer.to_u64());
                let source_locked = U64F64::saturating_from_num(source_lock.locked_mass.to_u64());
                let transferred_proportion = locked_transfer.safe_div(source_locked);
                source_lock
                    .conviction
                    .saturating_mul(transferred_proportion)
            };

            source_lock.locked_mass = source_lock.locked_mass.saturating_sub(locked_transfer);
            source_lock.conviction = source_lock.conviction.saturating_sub(conviction_transfer);
            destination_lock.locked_mass =
                destination_lock.locked_mass.saturating_add(locked_transfer);
            destination_lock.conviction = destination_lock
                .conviction
                .saturating_add(conviction_transfer);
        }
        Self::ensure_can_receive_locked_alpha(destination_coldkey, locked_transfer)?;

        source_lock = ConvictionModel::roll_forward_lock(
            source_lock,
            now,
            unlock_rate,
            maturity_rate,
            Self::is_subnet_owner_hotkey(netuid, &source_hotkey),
            Self::is_perpetual_lock(origin_coldkey, netuid),
        )
        .0;
        destination_lock = ConvictionModel::roll_forward_lock(
            destination_lock,
            now,
            unlock_rate,
            maturity_rate,
            Self::is_subnet_owner_hotkey(netuid, &destination_hotkey),
            Self::is_perpetual_lock(destination_coldkey, netuid),
        )
        .0;

        // Upsert updated locks (only once per this fn) even if there were no updates because
        // of roll-forward
        Self::insert_lock_state(origin_coldkey, netuid, &source_hotkey, source_lock);
        Self::insert_lock_state(
            destination_coldkey,
            netuid,
            &destination_hotkey,
            destination_lock,
        );
        if !locked_transfer.is_zero() {
            Self::reduce_aggregate_lock(
                origin_coldkey,
                &source_hotkey,
                netuid,
                locked_transfer,
                conviction_transfer,
            );
            Self::add_aggregate_lock(
                destination_coldkey,
                &destination_hotkey,
                netuid,
                LockState {
                    locked_mass: locked_transfer,
                    conviction: conviction_transfer,
                    last_update: now,
                },
            );
        }

        Ok(())
    }

    /// Destroys all lock maps for network dissolution
    pub fn destroy_lock_maps(netuid: NetUid) {
        // LockingColdkeys: (netuid, hotkey, coldkey)
        // Lock: (coldkey, netuid, hotkey)
        {
            let to_rm: sp_std::vec::Vec<((T::AccountId, T::AccountId), ())> =
                LockingColdkeys::<T>::iter_prefix((netuid,)).collect();

            for ((hot, cold), _) in to_rm {
                Lock::<T>::remove((cold, netuid, hot));
            }
            let _ = LockingColdkeys::<T>::clear_prefix((netuid,), u32::MAX, None);
        }

        // HotkeyLock: (netuid, hotkey) → LockState
        let _ = HotkeyLock::<T>::clear_prefix(netuid, u32::MAX, None);

        // DecayingHotkeyLock: (netuid, hotkey)
        let _ = DecayingHotkeyLock::<T>::clear_prefix(netuid, u32::MAX, None);

        // OwnerLock / DecayingOwnerLock: (netuid)
        OwnerLock::<T>::remove(netuid);
        DecayingOwnerLock::<T>::remove(netuid);

        // DecayingLock: (coldkey, netuid)
        {
            let to_rm: sp_std::vec::Vec<T::AccountId> = DecayingLock::<T>::iter()
                .filter_map(|(cold, n, _)| if n == netuid { Some(cold) } else { None })
                .collect();

            for cold in to_rm {
                DecayingLock::<T>::remove(cold, netuid);
            }
        }
    }
}
